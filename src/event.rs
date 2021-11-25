use super::toplevel::State;

#[derive(Debug)]
pub enum Event {
  New(u32),
  Title(u32, String),
  AppId(u32, String),
  State(u32, Vec<State>),
  OutputName(u32, String),
  /// it's time to generate D-Bus signals
  Done(u32),
  Closed(u32),
}
