use std::{rc::Rc, cell::{Cell, RefCell}};
use std::collections::HashMap;
use tokio::sync::mpsc::{Sender, Receiver};

use wayland_client::{Display, EventQueue, GlobalManager, Main, GlobalEvent, Interface};
use wayland_client::protocol::wl_output;
use wayland_protocols::wlr::unstable::foreign_toplevel::v1::client::{
  zwlr_foreign_toplevel_handle_v1::{Event, ZwlrForeignToplevelHandleV1},
  zwlr_foreign_toplevel_manager_v1::{ZwlrForeignToplevelManagerV1, self},
};
use tracing::{debug, warn};
use tokio::io::unix::AsyncFd;

use super::toplevel;
use super::topmaid::Action;
use super::util::send_event;

struct Toplevels {
  toplevels: HashMap<u32, Main<ZwlrForeignToplevelHandleV1>>,
}

impl Toplevels {
  fn new() -> Self {
    Self {
      toplevels: HashMap::new(),
    }
  }

  fn close(&self, id: u32) {
    debug!("closing {}", id);
    if let Some(t) = self.toplevels.get(&id) {
      t.close();
    }
  }
}

pub async fn run(
  toplevel_tx: Sender<toplevel::Event>,
  mut action_rx: Receiver<Action>,
) {
  let toplevels = Rc::new(RefCell::new(Toplevels::new()));
  let (finished, mut event_queue) = setup(toplevel_tx, toplevels.clone());
  let afd = AsyncFd::new(event_queue.display().get_connection_fd()).unwrap();

  while !finished.get() {
    event_queue.sync_roundtrip(&mut (), |_, _, _| { /* we ignore unfiltered messages */ }).unwrap();
    debug!("waiting to read from wayland server...");
    tokio::select! {
      guard = afd.readable() => guard.unwrap().clear_ready(),
      action = action_rx.recv() => match action.unwrap() {
        Action::Close(id) => toplevels.borrow().close(id)
      },
    }
  }
}

fn setup(
  tx: Sender<toplevel::Event>,
  toplevels: Rc<RefCell<Toplevels>>,
) -> (Rc<Cell<bool>>, EventQueue) {
  let display = Display::connect_to_env().unwrap();
  let mut event_queue = display.create_event_queue();
  let attached_display = (*display).clone().attach(event_queue.token());

  // (Main<WlOutput>, wl_registry.name)
  let outputs = Rc::new(RefCell::new(Vec::new()));

  let tx2 = tx.clone();

  let globals = GlobalManager::new_with_cb(
    &attached_display,
    move |event, registry, _| match event {
      GlobalEvent::New { id, interface, version } if interface == wl_output::WlOutput::NAME => {
        assert!(version >= 4);
        let output = registry.bind::<wl_output::WlOutput>(4, id);
        let oid = output.as_ref().id();
        debug!("got a new output: {}", oid);
        let tx3 = tx2.clone();
        output.quick_assign(move |_, event, _|
          if let wl_output::Event::Name { name } = event {
            send_event(&tx3, toplevel::Event::OutputNew(oid, name));
          }
        );
        outputs.borrow_mut().push((output, id));
      }
      GlobalEvent::Removed { id, interface } if interface == wl_output::WlOutput::NAME => {
        let mut o2 = outputs.borrow_mut();
        let (idx, _) = o2.iter().enumerate().find(|(_, (_, gid))| *gid == id).unwrap();
        let (output, _) = o2.remove(idx);
        let oid = output.as_ref().id();
        output.release();
        debug!("an output has been removed: {}", oid);
        send_event(&tx2, toplevel::Event::OutputRemoved(oid));
      }
      _ => { }
    }
  );

  event_queue.sync_roundtrip(&mut (), |_, _, _| unreachable!()).unwrap();

  let foreign_toplevel = globals
    .instantiate_exact::<ZwlrForeignToplevelManagerV1>(3)
    .expect("Compositor does not support wlr-foreign-toplevel-management");

  let finished = Rc::new(Cell::new(false));
  let finished2 = finished.clone();

  foreign_toplevel.quick_assign(move |_, event, _| match event {
    zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } => {
      let id = toplevel.as_ref().id();
      debug!("got a toplevel id {}", id);
      send_event(&tx, toplevel::Event::New(id));

      let tx = tx.clone();
      let toplevels2 = toplevels.clone();
      toplevel.quick_assign(move |toplevel, event, _| match event {
        Event::Title { title } => {
          debug!("toplevel@{} has title {}", id, title);
          send_event(&tx, toplevel::Event::Title(id, title));
        }
        Event::AppId { app_id } => {
          debug!("toplevel@{} has app_id {}", id, app_id);
          send_event(&tx, toplevel::Event::AppId(id, app_id));
        }
        Event::State { state } => {
          let state = toplevel::State::from_bytes(&state);
          debug!("toplevel@{} has state {:?}", id, state);
          if state.contains(&toplevel::State::Minimized) {
            toplevel.unset_minimized();
          }
          send_event(&tx, toplevel::Event::State(id, state));
        }
        Event::OutputEnter { output } => {
          let output_id = output.as_ref().id();
          debug!("toplevel@{} entered output {}", id, output_id);
          send_event(&tx, toplevel::Event::Output(id, Some(output_id)));
        }
        Event::OutputLeave { output } => {
          // if we have already bound to the new output, we will miss its output_leave events
          // at least we can record that they are left.
          debug!("toplevel@{} left output {}", id, output.as_ref().id());
          send_event(&tx, toplevel::Event::Output(id, None));
        }
        Event::Closed => {
          debug!("{} has been closed", id);
          send_event(&tx, toplevel::Event::Closed(id));
          toplevel.destroy();
          toplevels2.borrow_mut().toplevels.remove(&id);
        }
        Event::Done => {
          debug!("{}'s info is now stable", id);
          send_event(&tx, toplevel::Event::Done(id));
        }
        _ => { }
      });

      toplevels.borrow_mut().toplevels.insert(id, toplevel);
    },
    zwlr_foreign_toplevel_manager_v1::Event::Finished => {
      warn!("finished?");
      finished2.set(true);
    },
    _ => unreachable!(),
  });

  (finished, event_queue)
}
