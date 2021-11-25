use std::sync::{Arc, Mutex, RwLock};

use futures::future;
use dbus_tokio::connection;
use dbus::channel::MatchingReceiver;
use dbus::message::MatchRule;
use dbus::nonblock::SyncConnection;
use dbus_crossroads::{MethodErr, Crossroads, IfaceToken, IfaceBuilder};
use eyre::Result;

use super::topmaid::TopMaid;

fn register_iface(cr: Arc<Mutex<Crossroads>>, _conn: Arc<SyncConnection>) -> IfaceToken<Arc<RwLock<TopMaid>>> {
  cr.lock().unwrap().register("me.lilydjwg.taskmaid", |b: &mut IfaceBuilder<Arc<RwLock<TopMaid>>>| {
    b.property("active")
      .emits_changed_true()
      .get(|_, maid| maid.read().unwrap().get_active().ok_or_else(||MethodErr::failed("no toplevel active")));
    b.method("List", (), ("reply",), move |_, maid, _: ()| {
      Ok((maid.read().unwrap().list(),))
    });
  })
}

pub async fn dbus_run(maid: Arc<RwLock<TopMaid>>) -> Result<(), Box<dyn std::error::Error>> {
  let (resource, c) = connection::new_session_sync()?;

  let _handle = tokio::spawn(async {
    let err = resource.await;
    panic!("Lost connection to D-Bus: {}", err);
  });

  let cr = Arc::new(Mutex::new(Crossroads::new()));
  let token = register_iface(Arc::clone(&cr), c.clone());
  cr.lock().unwrap().insert("/taskmaid", &[token], maid);

  c.request_name("me.lilydjwg.taskmaid", false, true, false).await?;

  c.start_receive(MatchRule::new_method_call(), Box::new(move |msg, conn| {
    cr.lock().unwrap().handle_message(msg, conn).unwrap();
    true
  }));

  future::pending::<()>().await;
  unreachable!()
}
