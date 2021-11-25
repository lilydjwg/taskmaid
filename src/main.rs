use eyre::Result;
use tracing_subscriber::EnvFilter;

mod toplevel;
mod event;
mod topmaid;
mod dbus;
mod wayland;

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
  let fu = wayland::run(finished, event_queue);
  let fu2 = topmaid::run(rx);

  let rt = tokio::runtime::Builder::new_current_thread()
    .enable_all()
    .build()
    .unwrap();
  rt.block_on(futures::future::join(fu, fu2));
  Ok(())
}
