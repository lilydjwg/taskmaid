use std::sync::{Arc, Mutex, RwLock};

use dbus_tokio::connection;
use dbus::channel::{Sender, MatchingReceiver};
use dbus::message::MatchRule;
use dbus_crossroads::{MethodErr, Crossroads, IfaceBuilder};
use eyre::Result;
use tokio::sync::mpsc::Receiver;
use tracing::{debug, error};

use super::topmaid::{TopMaid, Signal};

pub async fn dbus_run(
  maid: Arc<RwLock<TopMaid>>,
  mut rx: Receiver<Signal>,
) -> Result<(), Box<dyn std::error::Error>> {
  let (resource, c) = connection::new_session_sync()?;

  let _handle = tokio::spawn(async {
    let err = resource.await;
    panic!("Lost connection to D-Bus: {}", err);
  });

  let cr = Arc::new(Mutex::new(Crossroads::new()));
  let mut active_changed = None;
  let token = cr.lock().unwrap().register(
    "me.lilydjwg.taskmaid", |b: &mut IfaceBuilder<Arc<RwLock<TopMaid>>>| {
    let cb = b.property("active")
      .get(|_, maid| {
        maid.read().unwrap().get_active()
          .map(|a| (a.title, a.app_id, a.output_name))
          .ok_or_else(||MethodErr::failed("no toplevel active"))
      })
      .changed_msg_fn();
    b.method("List", (), ("reply",), move |_, maid, _: ()| {
      Ok((maid.read().unwrap().list(),))
    });
    b.method("CloseActive", (), (), move |_, maid, _: ()| {
      maid.read().unwrap().close_active();
      Ok(())
    });
    active_changed = Some(cb);
  });
  cr.lock().unwrap().insert("/taskmaid", &[token], maid);

  c.request_name("me.lilydjwg.taskmaid", false, true, false).await?;

  c.start_receive(MatchRule::new_method_call(), Box::new(move |msg, conn| {
    cr.lock().unwrap().handle_message(msg, conn).unwrap();
    true
  }));

  while let Some(sig) = rx.recv().await {
    match sig {
      Signal::ActiveChanged(a) => {
        debug!("active toplevel changed to {:?}", a);
        if let Some(f) = &active_changed {
          if let Some(msg) = f(&"/taskmaid".into(), &(a.title, a.app_id, a.output_name)) {
            if let Err(()) = c.send(msg) {
              error!("failed to send out D-Bus signal.");
            }
          }
        }
      }
    };
  }
  unreachable!()
}
