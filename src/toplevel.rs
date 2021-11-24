#[derive(Debug)]
pub struct Toplevel {
  pub id: u32,
  pub title: Option<String>,
  pub app_id: Option<String>,
  pub output_name: Option<String>,
  pub state: Vec<State>,
}

impl Toplevel {
  pub fn new(id: u32) -> Self {
    Self {
      id,
      title: None,
      app_id: None,
      output_name: None,
      state: vec![],
    }
  }
}

use std::io::Cursor;
use byteorder::{NativeEndian, ReadBytesExt};

#[derive(Debug, PartialEq, Eq)]
pub enum State {
  Maximized,
  Minimized,
  Active,
  Fullscreen,
}

impl State {
  pub fn from_bytes(bytes: &[u8]) -> Vec<State> {
    bytes.chunks(4).map(|buf| {
      let mut r = Cursor::new(buf);
      let a = r.read_u32::<NativeEndian>().unwrap();
      State::from_u32(a)
    }).collect()
  }

  fn from_u32(a: u32) -> State {
    match a {
      0 => State::Maximized,
      1 => State::Minimized,
      2 => State::Active,
      3 => State::Fullscreen,
      _ => panic!("unknown state: {}", a),
    }
  }
}
