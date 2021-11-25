use std::sync::{Arc, RwLock};

use eyre::Result;
use tracing_subscriber::EnvFilter;
use futures::future;

mod toplevel;
mod event;
mod topmaid;
mod dbus;
mod wayland;

use topmaid::TopMaid;

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

  let (finished, rx, event_queue) = wayland::setup();
  let fu1 = wayland::run(finished, event_queue);
  let maid = Arc::new(RwLock::new(TopMaid::new()));
  let fu2 = TopMaid::run(Arc::clone(&maid), rx);
  let fu3 = dbus::dbus_run(maid);
  let fu = future::join(future::join(fu1, fu2), fu3);

  let rt = tokio::runtime::Builder::new_current_thread()
    .enable_all()
    .build()
    .unwrap();
  rt.block_on(fu);
  Ok(())
}
