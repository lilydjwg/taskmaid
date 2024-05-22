use std::collections::HashMap;
use std::os::fd::AsFd;

use tokio::sync::mpsc::{Sender, Receiver};

use wayland_client::{Connection, QueueHandle, Dispatch, Proxy, event_created_child};
use wayland_client::protocol::{wl_registry, wl_output, wl_callback};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
  zwlr_foreign_toplevel_handle_v1::{Event, ZwlrForeignToplevelHandleV1, self},
  zwlr_foreign_toplevel_manager_v1::{ZwlrForeignToplevelManagerV1, self},
};
use tracing::{debug, warn};
use tokio::io::unix::AsyncFd;

use super::toplevel;
use super::topmaid::Action;
use super::util::send_event;

pub async fn run(
  toplevel_tx: Sender<toplevel::Event>,
  mut action_rx: Receiver<Action>,
) {
  let mut state = State::new(toplevel_tx);

  let conn = Connection::connect_to_env().unwrap();
  let display = conn.display();
  let mut event_queue = conn.new_event_queue();
  let qh = event_queue.handle();
  let _registry = display.get_registry(&qh, ());
  display.sync(&qh, ());
  let afd = AsyncFd::new(conn.as_fd()).unwrap();

  while !state.finished {
    event_queue.flush().unwrap();
    let read_guard = event_queue.prepare_read().unwrap();

    debug!("waiting to read from wayland server...");
    tokio::select! {
      guard = afd.readable() => {
        guard.unwrap().clear_ready();
        read_guard.read().unwrap();
        event_queue.dispatch_pending(&mut state).unwrap();
      }
      action = action_rx.recv() => match action.unwrap() {
        Action::Close(id) => state.close(id)
      }
    }
  }
}

struct State {
  toplevels: HashMap<u32, ZwlrForeignToplevelHandleV1>,
  outputs: Vec<(u32, wl_output::WlOutput)>,
  manager: Option<ZwlrForeignToplevelManagerV1>,
  tx: Sender<toplevel::Event>,
  finished: bool,
}

impl State {
  fn new(tx: Sender<toplevel::Event>) -> Self {
    Self {
      toplevels: HashMap::new(),
      outputs: Vec::new(),
      manager: None,
      tx,
      finished: false,
    }
  }

  fn close(&self, id: u32) {
    debug!("closing {}", id);
    if let Some(t) = self.toplevels.get(&id) {
      t.close();
    }
  }
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
  fn event(
    s: &mut Self,
    registry: &wl_registry::WlRegistry,
    event: wl_registry::Event,
    _: &(),
    _: &Connection,
    qh: &QueueHandle<State>,
  ) {
    if let wl_registry::Event::Global { name, interface, version } = event {
      match &interface[..] {
        "wl_output" => {
          debug!("got a new output: {}", name);
          assert!(version >= 4);
          let output = registry.bind::<wl_output::WlOutput, _, _>(name, 4, qh, ());
          s.outputs.push((name, output));
        }
        "zwlr_foreign_toplevel_manager_v1" => {
          assert!(version >= 3);
          s.manager = Some(registry.bind::<ZwlrForeignToplevelManagerV1, _, _>(name, 3, qh, ()));
        }
        _ => {}
      }
    } else if let wl_registry::Event::GlobalRemove { name } = event {
      debug!("an output has been removed: {}", name);
      let (idx, (_, o)) = s.outputs.iter().enumerate().find(|&(_, (oname, _))| *oname == name).unwrap();
      o.release();
      s.outputs.remove(idx);
      send_event(&s.tx, toplevel::Event::OutputRemoved(name));
    }
  }
}


impl Dispatch<wl_callback::WlCallback, ()> for State {
  fn event(
    s: &mut Self,
    _: &wl_callback::WlCallback,
    _event: wl_callback::Event,
    _: &(),
    _: &Connection,
    _qh: &QueueHandle<State>,
  ) {
    if s.manager.is_none() {
      panic!("Compositor does not support wlr-foreign-toplevel-management");
    }
  }
}

impl Dispatch<wl_output::WlOutput, ()> for State {
  fn event(
    s: &mut Self,
    o: &wl_output::WlOutput,
    event: wl_output::Event,
    _: &(),
    _: &Connection,
    _qh: &QueueHandle<Self>,
  ) {
    if let wl_output::Event::Name { name } = event {
      let oid = o.id().protocol_id();
      send_event(&s.tx, toplevel::Event::OutputNew(oid, name));
    }
  }
}

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for State {
  fn event(
    s: &mut Self,
    _: &ZwlrForeignToplevelManagerV1,
    event: zwlr_foreign_toplevel_manager_v1::Event,
    _: &(),
    _: &Connection,
    _qh: &QueueHandle<Self>,
  ) {
    match event {
      zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } => {
        let id = toplevel.id().protocol_id();
        debug!("got a toplevel id {}", id);
        send_event(&s.tx, toplevel::Event::New(id));
        s.toplevels.insert(id, toplevel);
      },
      zwlr_foreign_toplevel_manager_v1::Event::Finished => {
        warn!("finished?");
        s.finished = true;
      },
      _ => unreachable!(),
    }
  }

  event_created_child!(State, ZwlrForeignToplevelManagerV1, [
    zwlr_foreign_toplevel_manager_v1::EVT_TOPLEVEL_OPCODE => (ZwlrForeignToplevelHandleV1, ()),
  ]);
}


impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for State {
  fn event(
    s: &mut Self,
    toplevel: &ZwlrForeignToplevelHandleV1,
    event: zwlr_foreign_toplevel_handle_v1::Event,
    _: &(),
    _: &Connection,
    _qh: &QueueHandle<Self>,
  ) {
    let id = toplevel.id().protocol_id();
    match event {
      Event::Title { title } => {
        debug!("toplevel@{} has title {}", id, title);
        send_event(&s.tx, toplevel::Event::Title(id, title));
      }
      Event::AppId { app_id } => {
        debug!("toplevel@{} has app_id {}", id, app_id);
        send_event(&s.tx, toplevel::Event::AppId(id, app_id));
      }
      Event::State { state } => {
        let state = toplevel::State::from_bytes(&state);
        debug!("toplevel@{} has state {:?}", id, state);
        if state.contains(&toplevel::State::Minimized) {
          toplevel.unset_minimized();
        }
        send_event(&s.tx, toplevel::Event::State(id, state));
      }
      Event::OutputEnter { output } => {
        let output_id = output.id().protocol_id();
        debug!("toplevel@{} entered output {}", id, output_id);
        send_event(&s.tx, toplevel::Event::Output(id, Some(output_id)));
      }
      Event::OutputLeave { output } => {
        // if we have already bound to the new output, we will miss its output_leave events
        // at least we can record that they are left.
        debug!("toplevel@{} left output {}", id, output.id().protocol_id());
        send_event(&s.tx, toplevel::Event::Output(id, None));
      }
      Event::Closed => {
        debug!("{} has been closed", id);
        send_event(&s.tx, toplevel::Event::Closed(id));
        toplevel.destroy();
        s.toplevels.remove(&id);
      }
      Event::Done => {
        debug!("{}'s info is now stable", id);
        send_event(&s.tx, toplevel::Event::Done(id));
      }
      _ => { }
    }
  }
}

