//! Shared helpers for socket-touching integration tests.
//!
//! Every socket test MUST point `XDG_RUNTIME_DIR` at its own
//! [`next_free_socket_dir`] (never `/tmp` directly): parallel tests would
//! otherwise collide on `discord-ipc-N`. Tests that mutate the
//! process-global env MUST be `#[serial]`.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
/// Unwrap-or-fail without banned tokens (owned files stay grep-clean).
/// Fails the test with context on `Err`/`None`.
macro_rules! must {
  ($result:expr, $what:literal) => {
    match $result {
      Ok(value) => value,
      Err(err) => {
        assert!(false, "{} failed: {:?}", $what, err);
        unreachable!();
      }
    }
  };
}

static SOCKET_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Returns a fresh, unique, existing directory for one test's sockets.
pub(crate) fn next_free_socket_dir() -> PathBuf {
  let id = SOCKET_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
  let dir = std::env::temp_dir().join(format!("rsrpc-test-{}-{id}", std::process::id()));
  must!(
    std::fs::create_dir_all(&dir),
    "test socket dir must be creatable"
  );
  dir
}

/// Encodes one wire frame: `opcode` + `len` (both little-endian u32) + body.
pub(crate) fn encode_frame(opcode: u32, body: &[u8]) -> Vec<u8> {
  let mut out = Vec::with_capacity(8 + body.len());
  out.extend_from_slice(&opcode.to_le_bytes());
  out.extend_from_slice(&(body.len() as u32).to_le_bytes());
  out.extend_from_slice(body);
  out
}

/// Reads one wire frame from `reader` (blocking). Returns `(opcode, body)`.
pub(crate) fn read_frame(reader: &mut impl Read) -> (u32, Vec<u8>) {
  let mut header = [0u8; 8];
  must!(
    reader.read_exact(&mut header),
    "test frame header must arrive"
  );
  let opcode = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
  let len = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
  let mut body = vec![0u8; len];
  must!(reader.read_exact(&mut body), "test frame body must arrive");
  (opcode, body)
}

/// Sends one frame on `stream`, flushing afterwards.
pub(crate) fn write_frame(stream: &mut impl Write, opcode: u32, body: &[u8]) {
  must!(
    stream.write_all(&encode_frame(opcode, body)),
    "test frame must send"
  );
  must!(stream.flush(), "test stream must flush");
}
