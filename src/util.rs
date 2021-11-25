use std::fmt::Debug;

use tracing::error;
use tokio::sync::mpsc::{Sender, error::TrySendError};

pub fn send_event<T: Debug>(tx: &Sender<T>, t: T) {
  if let Err(err) = tx.try_send(t) {
    match err {
      TrySendError::Full(t) => {
        error!("too many events to process! Event object discarded: {:?}", t);
      }
      TrySendError::Closed(_) => {
        panic!("channel closed unexpectedly");
      }
    }
  }
}

