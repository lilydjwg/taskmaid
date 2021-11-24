use std::sync::mpsc::Receiver;
use std::collections::HashMap;

use super::toplevel::Toplevel;
use super::event::Event;

pub fn run(rx: Receiver<Event>) {
  let mut woman = TopWoman::new();
  woman.run(rx);
}

struct TopWoman {
  toplevels: HashMap<u32, Toplevel>,
}

impl TopWoman {
  fn new() -> Self {
    Self {
      toplevels: HashMap::new(),
    }
  }

  fn run(&mut self, rx: Receiver<Event>) {
    loop {
      let event = rx.recv().unwrap();
      if let Event::Finished = event {
        break;
      }
      self.handle_event(event);
    }
  }

  fn handle_event(&mut self, event: Event) {
    match event {
      Event::New(id) => {
        self.toplevels.insert(id, Toplevel::new(id));
      }
      Event::Title(id, title) => {
        if let Some(t) = self.toplevels.get_mut(&id) {
          t.title = Some(title);
        }
      }
      Event::AppId(id, app_id) => {
        if let Some(t) = self.toplevels.get_mut(&id) {
          t.app_id = Some(app_id);
        }
      }
      Event::OutputName(id, output_name) => {
        if let Some(t) = self.toplevels.get_mut(&id) {
          t.output_name = Some(output_name);
        }
      }
      Event::State(id, state) => {
        if let Some(t) = self.toplevels.get_mut(&id) {
          t.state = state;
        }
      }
      Event::Closed(id) => {
        self.toplevels.remove(&id);
      }
      Event::Finished => {
        unreachable!();
      }
    }
  }
}
