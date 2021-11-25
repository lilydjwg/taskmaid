use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use tokio::sync::mpsc::Receiver;

use super::toplevel::{Toplevel, State};
use super::event::Event;

pub async fn run(rx: Receiver<Event>) {
  let maid = Arc::new(RwLock::new(TopMaid::new()));
  TopMaid::run(maid, rx).await;
}

pub struct TopMaid {
  toplevels: HashMap<u32, Toplevel>,
}

impl TopMaid {
  fn new() -> Self {
    Self {
      toplevels: HashMap::new(),
    }
  }

  async fn run(maid: Arc<RwLock<Self>>, mut rx: Receiver<Event>) {
    while let Some(event) = rx.recv().await {
      maid.write().unwrap().handle_event(event);
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
    }
  }

  pub fn list(&self) -> Vec<(u32, String, String, String, Vec<u32>)> {
    self.toplevels.values().map(|t| {
      let st = t.state.iter().map(|st| *st as u32).collect();
      (t.id,
       t.title.as_ref().map(|x| x.to_owned()).unwrap_or_default(),
       t.app_id.as_ref().map(|x| x.to_owned()).unwrap_or_default(),
       t.output_name.as_ref().map(|x| x.to_owned()).unwrap_or_default(),
       st,
      )
    }).collect()
  }

  pub fn get_active(&self) -> Option<(String, String, String)> {
    self.toplevels.values().find(|t| t.state.contains(&State::Active))
      .map(|t| (
          t.title.as_ref().map(|x| x.to_owned()).unwrap_or_default(),
          t.app_id.as_ref().map(|x| x.to_owned()).unwrap_or_default(),
          t.output_name.as_ref().map(|x| x.to_owned()).unwrap_or_default(),
      ))
  }
}
