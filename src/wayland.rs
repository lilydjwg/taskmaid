use std::{rc::Rc, cell::{Cell, RefCell}};
use std::collections::HashMap;
use tokio::sync::mpsc::{Sender, Receiver};

use wayland_client::{Display, EventQueue, GlobalManager, Main, GlobalEvent, Interface};
use wayland_client::protocol::wl_output;
use wayland_protocols::unstable::xdg_output::v1::client::{
  zxdg_output_manager_v1,
  zxdg_output_v1,
};
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

  // (Main<WlOutput>, global@id)
  let outputs = Rc::new(RefCell::new(Vec::new()));
  let outputs2 = outputs.clone();

  // wl_output@id => zxdg_output_v1.name
  let output_name_map = Rc::new(RefCell::new(HashMap::new()));
  let output_name_map2 = output_name_map.clone();

  let xdg_output: Rc<RefCell<Option<Main<zxdg_output_manager_v1::ZxdgOutputManagerV1>>>> = Rc::new(RefCell::new(None));
  let xdg_output2 = xdg_output.clone();

  let tx2 = tx.clone();

  let globals = GlobalManager::new_with_cb(
    &attached_display,
    move |event, registry, _| match event {
      GlobalEvent::New { id, interface, version } if interface == wl_output::WlOutput::NAME => {
        debug!("got a new output: {}", id);
        assert!(version >= 2);
        let output = registry.bind::<wl_output::WlOutput>(version, id);
        let oid = output.as_ref().id();
        if let Some(xdg_output) = &*xdg_output2.borrow() {
          let output_name_map3 = output_name_map2.clone();
          let tx3 = tx2.clone();
          xdg_output.get_xdg_output(&output).quick_assign(move |_, event, _|
            if let zxdg_output_v1::Event::Name { name } = event {
              send_event(&tx3, toplevel::Event::NewOutput(name.clone()));
              output_name_map3.borrow_mut().insert(oid, name);
            }
          );
        }
        outputs2.borrow_mut().push((output, id));
      }
      GlobalEvent::Removed { id, interface } if interface == wl_output::WlOutput::NAME => {
        debug!("an output has been removed: {}", id);
        let mut o2 = outputs2.borrow_mut();
        let (idx, _) = o2.iter().enumerate().find(|(_, (_, gid))| *gid == id).unwrap();
        let (output, _) = o2.remove(idx);
        let oid = output.as_ref().id();
        output.release();
        output_name_map2.borrow_mut().remove(&oid);
      }
      _ => { }
    }
  );

  event_queue.sync_roundtrip(&mut (), |_, _, _| unreachable!()).unwrap();

  *xdg_output.borrow_mut() = Some(globals
    .instantiate_exact::<zxdg_output_manager_v1::ZxdgOutputManagerV1>(3)
    .expect("Compositor does not support xdg_output"));

  for output in &*outputs.borrow() {
    let id = output.0.as_ref().id();
    let output_name_map4 = output_name_map.clone();
    xdg_output.borrow().as_ref().unwrap().get_xdg_output(&*output.0).quick_assign(move |_, event, _|
      if let zxdg_output_v1::Event::Name { name } = event {
        output_name_map4.borrow_mut().insert(id, name);
      }
    );
  }

  let foreign_toplevel = globals
    .instantiate_exact::<ZwlrForeignToplevelManagerV1>(3)
    .expect("Compositor does not support wlr-foreign-toplevel-management");

  let finished = Rc::new(Cell::new(false));
  let finished2 = finished.clone();

  foreign_toplevel.quick_assign(move |_, event, _| match event {
    zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } => {
      let output_name_map5 = output_name_map.clone();
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
          let borrow = output_name_map5.borrow();
          let name = borrow.get(&output_id).map(|x| x.as_ref()).unwrap_or("unknown");
          debug!("toplevel@{} entered output {}", id, name);
          send_event(&tx, toplevel::Event::OutputName(id, name.into()));
        }
        Event::OutputLeave { .. } => {
          // if we have already bound to the new output, we will miss its output_leave events
          // at least we can record that they are left.
          debug!("toplevel@{} left output", id);
          send_event(&tx, toplevel::Event::OutputName(id, String::new()));
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
