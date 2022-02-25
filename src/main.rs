#![feature(async_closure)]

use std::sync::{Arc, RwLock};

use eyre::Result;
use tracing_subscriber::EnvFilter;
use futures::future;
use tokio::sync::mpsc::channel;

mod toplevel;
mod topmaid;
mod dbus;
mod wayland;
mod util;

use topmaid::TopMaid;

fn main() -> Result<()> {
  // default RUST_SPANTRACE=0
  color_eyre::config::HookBuilder::new()
    .capture_span_trace_by_default(false)
    .install()?;

  // default RUST_LOG=warn
  let filter = EnvFilter::try_from_default_env()
    .unwrap_or_else(|_| EnvFilter::from("warn"));
  let fmt = tracing_subscriber::fmt::fmt()
    .with_writer(std::io::stderr)
    .with_env_filter(filter);
  if !atty::is(atty::Stream::Stderr) {
    fmt.without_time().init();
  } else {
    fmt.init();
  }

  // keep it large since one toplevel may generate several events
  // and we receive all of them at startup
  let (toplevel_tx, toplevel_rx) = channel(10240);
  let (action_tx, action_rx) = channel(10);
  let (dbus_tx, dbus_rx) = channel(10);
  let fu1 = wayland::run(toplevel_tx, action_rx);
  let maid = Arc::new(RwLock::new(TopMaid::new(dbus_tx, action_tx)));
  let fu2 = TopMaid::run(Arc::clone(&maid), toplevel_rx);
  let fu3 = dbus::dbus_run(maid, dbus_rx);

  let fu = async || {
    let _ = future::join(future::join(fu1, fu2), fu3).await;
  };
  let rt = tokio::runtime::Builder::new_current_thread()
    .enable_all()
    .build()
    .unwrap();
  rt.block_on(fu());
  Ok(())
}
