use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use tokio::sync::mpsc::{Sender, Receiver};
use tracing::debug;

use super::toplevel::{Toplevel, State, Event};

pub struct TopMaid {
  toplevels: HashMap<u32, Toplevel>,
  active_changed: bool,
  active_toplevel: u32,
  signal_tx: Sender<Signal>,
}

impl TopMaid {
  pub fn new(signal_tx: Sender<Signal>) -> Self {
    Self {
      signal_tx,
      toplevels: HashMap::new(),
      active_changed: false,
      active_toplevel: 0,
    }
  }

  pub async fn run(maid: Arc<RwLock<Self>>, mut rx: Receiver<Event>) {
    while let Some(event) = rx.recv().await {
      maid.write().unwrap().handle_event(event).await;
    }
  }

  async fn handle_event(&mut self, event: Event) {
    match event {
      Event::New(id) => {
        self.toplevels.insert(id, Toplevel::new(id));
      }
      Event::Title(id, title) => {
        if let Some(t) = self.toplevels.get_mut(&id) {
          t.title = Some(title);
        }
        if id == self.active_toplevel {
          self.active_changed = true;
        }
      }
      Event::AppId(id, app_id) => {
        if let Some(t) = self.toplevels.get_mut(&id) {
          t.app_id = Some(app_id);
        }
        if id == self.active_toplevel {
          self.active_changed = true;
        }
      }
      Event::OutputName(id, output_name) => {
        if let Some(t) = self.toplevels.get_mut(&id) {
          t.output_name = Some(output_name);
        }
        if id == self.active_toplevel {
          self.active_changed = true;
        }
      }
      Event::State(id, state) => {
        if state.contains(&State::Active) {
          self.active_toplevel = id;
          self.active_changed = true;
        }
        if let Some(t) = self.toplevels.get_mut(&id) {
          t.state = state;
        }
      }
      Event::Closed(id) => {
        self.toplevels.remove(&id);
      }
      Event::Done(id) => {
        if id == self.active_toplevel && self.active_changed {
          if let Some(a) = self.get_active() {
            debug!("active changed to {:?}", a);
            let _ = self.signal_tx.send(Signal::ActiveChanged(a)).await;
          }
          self.active_changed = false;
        }
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

  pub fn get_active(&self) -> Option<ActiveInfo> {
    self.toplevels.get(&self.active_toplevel).map(ActiveInfo::from_toplevel)
  }
}

#[derive(Debug)]
pub struct ActiveInfo {
  pub title: String,
  pub app_id: String,
  pub output_name: String,
}

impl ActiveInfo {
  fn from_toplevel(t: &Toplevel) -> Self {
    Self {
      title: t.title.as_ref().map(|x| x.to_owned()).unwrap_or_default(),
      app_id: t.app_id.as_ref().map(|x| x.to_owned()).unwrap_or_default(),
      output_name: t.output_name.as_ref().map(|x| x.to_owned()).unwrap_or_default(),
    }
  }
}

pub enum Signal {
  ActiveChanged(ActiveInfo),
}
