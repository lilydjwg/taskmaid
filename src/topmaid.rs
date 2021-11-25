use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use tokio::sync::mpsc::{Sender, Receiver};
use tracing::debug;

use super::toplevel::{Toplevel, State, Event};
use super::util::send_event;

pub struct TopMaid {
  toplevels: HashMap<u32, Toplevel>,
  active_changed: bool,
  last_active_toplevel: u32,
  dbus_tx: Sender<Signal>,
  action_tx: Sender<Action>,
  no_active: bool,
}

impl TopMaid {
  pub fn new(dbus_tx: Sender<Signal>, action_tx: Sender<Action>) -> Self {
    Self {
      dbus_tx,
      action_tx,
      toplevels: HashMap::new(),
      active_changed: false,
      last_active_toplevel: 0,
      no_active: true,
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
        if id == self.last_active_toplevel {
          self.active_changed = true;
        }
      }
      Event::AppId(id, app_id) => {
        if let Some(t) = self.toplevels.get_mut(&id) {
          t.app_id = Some(app_id);
        }
        if id == self.last_active_toplevel {
          self.active_changed = true;
        }
      }
      Event::OutputName(id, output_name) => {
        if let Some(t) = self.toplevels.get_mut(&id) {
          t.output_name = Some(output_name);
        }
        if id == self.last_active_toplevel {
          self.active_changed = true;
        }
      }
      Event::State(id, state) => {
        if state.contains(&State::Active) {
          self.last_active_toplevel = id;
          self.active_changed = true;
          self.no_active = false;
        } else if id == self.last_active_toplevel {
          self.no_active = true;
          self.active_changed = true;
        }
        if let Some(t) = self.toplevels.get_mut(&id) {
          t.state = state;
        }
      }
      Event::Closed(id) => {
        if let Some(t) = self.toplevels.remove(&id) {
          if id == self.last_active_toplevel {
            debug!("active toplevel closed");
            self.no_active = true;
            let a = ActiveInfo {
              title: String::new(),
              app_id: String::new(),
              output_name: t.output_name.unwrap_or_default(),
            };
            let _ = self.dbus_tx.send(Signal::ActiveChanged(a)).await;
            self.active_changed = false;
          }
        }
      }
      Event::Done(id) => {
        if id == self.last_active_toplevel && self.active_changed {
          if let Some(a) = self.get_active() {
            debug!("active changed to {:?}", a);
            let _ = self.dbus_tx.send(Signal::ActiveChanged(a)).await;
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
    self.toplevels.get(&self.last_active_toplevel).map(|t| {
      if self.no_active {
        // signal that no active toplevel should be shown on this output
        ActiveInfo {
          title: String::new(),
          app_id: String::new(),
          output_name: t.output_name.clone().unwrap_or_default(),
        }
      } else {
        ActiveInfo::from_toplevel(t)
      }
    })
  }

  pub fn close_active(&self) {
    debug!("closing active toplevel ({})", self.last_active_toplevel);
    send_event(&self.action_tx, Action::Close(self.last_active_toplevel));
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

#[derive(Debug)]
pub enum Action {
  Close(u32),
}
