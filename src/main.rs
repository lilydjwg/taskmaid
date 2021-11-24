use std::{rc::Rc, cell::{Cell, RefCell}};
use std::sync::{Arc, RwLock};
use std::collections::HashMap;

use wayland_client::{Display, GlobalManager, Main, global_filter};
use wayland_client::protocol::wl_output;
use wayland_protocols::unstable::xdg_output::v1::client::{
  zxdg_output_manager_v1,
  zxdg_output_v1,
};
use wayland_protocols::wlr::unstable::foreign_toplevel::v1::client::{
  zwlr_foreign_toplevel_handle_v1::Event,
  zwlr_foreign_toplevel_manager_v1::{ZwlrForeignToplevelManagerV1, self},
};

use eyre::Result;
use tracing::{debug, warn};
use tracing_subscriber::EnvFilter;

mod toplevel;

fn main() -> Result<()> {
  if std::env::var("RUST_LOG").is_err() {
    std::env::set_var("RUST_LOG", "warn")
  }
  if std::env::var("RUST_SPANTRACE").is_err() {
    std::env::set_var("RUST_SPANTRACE", "0");
  }
  color_eyre::install()?;
  let fmt = tracing_subscriber::fmt::fmt()
    .with_writer(std::io::stderr)
    .with_env_filter(EnvFilter::from_default_env());
  if !atty::is(atty::Stream::Stderr) {
    fmt.without_time().init();
  } else {
    fmt.init();
  }

  run();
  Ok(())
}

fn run() {
  let display = Display::connect_to_env().unwrap();
  let mut event_queue = display.create_event_queue();
  let attached_display = (*display).clone().attach(event_queue.token());

  let outputs = Rc::new(RefCell::new(Vec::new()));
  let outputs2 = outputs.clone();
  let globals = GlobalManager::new_with_cb(
    &attached_display,
    global_filter!(
      [wl_output::WlOutput, 2, move |output: Main<wl_output::WlOutput>, _: DispatchData| {
        outputs2.borrow_mut().push(output);
      }]
    )
  );

  event_queue.sync_roundtrip(&mut (), |_, _, _| unreachable!()).unwrap();

  let xdg_output = globals
    .instantiate_exact::<zxdg_output_manager_v1::ZxdgOutputManagerV1>(3)
    .expect("Compositor does not support xdg_output");

  let output_name_map = Rc::new(RefCell::new(HashMap::new()));
  for output in &*outputs.borrow() {
    let id = output.as_ref().id();
    let output_name_map2 = output_name_map.clone();
    xdg_output.get_xdg_output(output).quick_assign(move |_, event, _|
      if let zxdg_output_v1::Event::Name { name } = event {
        output_name_map2.borrow_mut().insert(id, name);
      }
    );
  }

  let foreign_toplevel = globals
    .instantiate_exact::<ZwlrForeignToplevelManagerV1>(3)
    .expect("Compositor does not support wlr-foreign-toplevel-management");

  let finished = Rc::new(Cell::new(false));
  let finished2 = finished.clone();

  let toplevels = Arc::new(RwLock::new(Vec::new()));
  let toplevels2 = Arc::clone(&toplevels);
  foreign_toplevel.quick_assign(move |_, event, _| match event {
    zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } => {
      let output_name_map3 = output_name_map.clone();
      let id = toplevel.as_ref().id();
      debug!("got a toplevel id {}", id);
      let t = Arc::new(RwLock::new(toplevel::Toplevel::new(id)));
      let toplevels3 = Arc::clone(&toplevels2);
      toplevels2.write().unwrap().push(t.clone());

      toplevel.quick_assign(move |_, event, _| match event {
        Event::Title { title } => {
          debug!("toplevel@{} has title {}", id, title);
          t.write().unwrap().title = Some(title);
        }
        Event::AppId { app_id } => {
          debug!("toplevel@{} has app_id {}", id, app_id);
          t.write().unwrap().app_id = Some(app_id);
        }
        Event::State { state } => {
          let state = toplevel::State::from_bytes(&state);
          debug!("toplevel@{} has state {:?}", id, state);
          t.write().unwrap().state = state;
        }
        Event::OutputEnter { output } => {
          let output_id = output.as_ref().id();
          let borrow = output_name_map3.borrow();
          let name = borrow.get(&output_id).map(|x| x.as_ref()).unwrap_or("unknown");
          debug!("toplevel@{} entered output {}", id, name);
          t.write().unwrap().output_name = Some(name.into());
        }
        Event::Closed => {
          debug!("{:?} has been closed", t.read().unwrap());
          let mut ts = toplevels3.write().unwrap();
          if let Some((idx, _)) = ts.iter().enumerate()
            .map(|(i, x)| (i, x.read().unwrap().id))
            .find(|(_, tid)| *tid == id) {
            ts.swap_remove(idx);
          }
        }
        _ => { }
      });
    },
    zwlr_foreign_toplevel_manager_v1::Event::Finished => {
      warn!("finished?");
      finished2.set(true);
    },
    _ => unreachable!(),
  });

  let toplevels4 = Arc::clone(&toplevels);
  fn print_toplevels(ts: &[Arc<RwLock<toplevel::Toplevel>>]) {
    for t in ts {
      let t = t.read().unwrap();
      if t.state.contains(&toplevel::State::Active) {
        println!("Active: {:?}", t);
      }
      if t.state.contains(&toplevel::State::Minimized) {
        println!("Minimized: {:?}", t);
      }
    }
  }
  std::thread::spawn(move || {
    use std::time::Duration;
    std::thread::sleep(Duration::from_millis(100));
    print_toplevels(&*toplevels4.read().unwrap());
    loop {
      std::thread::sleep(Duration::from_secs(10));
      print_toplevels(&*toplevels4.read().unwrap());
    }
  });

  while !finished.get() {
    event_queue.dispatch(&mut (), |_, _, _| { /* we ignore unfiltered messages */ }).unwrap();
  }
}
