use super::toplevel::State;

pub enum Event {
  New(u32),
  Title(u32, String),
  AppId(u32, String),
  State(u32, Vec<State>),
  OutputName(u32, String),
  Closed(u32),
  Finished,
}
