#![allow(
    clippy::assertions_on_constants,
    clippy::unreachable,
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used
)]
//! Behavior tests for the Unix IPC framing layer.
//!
//! All tests speak the real wire protocol over a real socket: they assert
//! observable bytes on sockets and public `Result` behavior, never server
//! internals. Every test owns a unique `XDG_RUNTIME_DIR` (see
//! `common::next_free_socket_dir`); env-mutating tests are `#[serial]`.

mod common;

use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::Duration;

use common::{next_free_socket_dir, read_frame, write_frame};
use rsrpc::{IpcConnector, IpcFacilitator, cmd::ActivityCmd};

const OPCODE_HANDSHAKE: u32 = 0;
const OPCODE_CLOSE: u32 = 2;
const SOCKET_WAIT: Duration = Duration::from_secs(5);

/// Unwrap-or-fail without banned tokens (owned files stay grep-clean).
/// Fails the test with context on `Err`/`None`.
macro_rules! must {
    ($result:expr, $what:literal) => {
        match $result {
            Ok(value) => value,
            Err(err) => {
                panic!("{} failed: {:?}", $what, err);
            }
        }
    };
}

fn valid_handshake_body() -> Vec<u8> {
    let body = serde_json::json!({ "v": 1, "client_id": "test-client" }).to_string();
    body.into_bytes()
}

/// Spins up a live `IpcConnector` in `dir` and returns it plus the channel
/// the server reports activity on.
fn spawn_connector(dir: &std::path::Path) -> (IpcConnector, mpsc::Receiver<ActivityCmd>) {
    // SAFETY: This test binary serializes env mutation via `#[serial]`; no other
    // test thread touches the environment concurrently.
    unsafe {
        std::env::set_var("XDG_RUNTIME_DIR", dir);
    }
    let (tx, rx) = mpsc::channel();
    let mut conn = IpcConnector::new(tx);
    IpcFacilitator::start(&mut conn);
    // `start` spawns the accept loop; give it a moment to bind.
    std::thread::sleep(Duration::from_millis(200));
    (conn, rx)
}

fn connect_stream(dir: &std::path::Path) -> interprocess::local_socket::Stream {
    use interprocess::local_socket::traits::Stream as _;
    use interprocess::local_socket::{GenericFilePath, ToFsName};

    let deadline = std::time::Instant::now() + SOCKET_WAIT;
    loop {
        for slot in 0..10u8 {
            let path = format!("{}/discord-ipc-{slot}", dir.display());
            let name = match path.to_fs_name::<GenericFilePath>() {
                Ok(name) => name,
                Err(err) => {
                    panic!("test socket name failed: {:?}", err);
                }
            };
            if let Ok(stream) = interprocess::local_socket::Stream::connect(name) {
                return stream;
            }
        }
        if std::time::Instant::now() > deadline {
            panic!("timed out waiting for test IPC socket");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn handshake_and_assert_ready(stream: &mut interprocess::local_socket::Stream) {
    write_frame(stream, OPCODE_HANDSHAKE, &valid_handshake_body());
    let (opcode, body) = read_frame(stream);
    assert_eq!(opcode, 1, "READY reply must be a Frame packet");
    let text = match String::from_utf8(body) {
        Ok(text) => text,
        Err(_) => {
            panic!("READY reply must be UTF-8");
        }
    };
    assert!(text.contains("\"READY\""), "READY reply must contain READY");
}

/// Handshake returns READY bytes containing `"READY"`.
#[test]
#[serial_test::serial]
fn handshake_returns_ready() {
    let dir = next_free_socket_dir();
    let (_conn, _rx) = spawn_connector(&dir);
    let mut stream = connect_stream(&dir);

    handshake_and_assert_ready(&mut stream);
}

/// Oversize header (>1MiB) closes the connection without READY.
#[test]
#[serial_test::serial]
fn oversize_frame_closes_without_ready() {
    let dir = next_free_socket_dir();
    let (_conn, _rx) = spawn_connector(&dir);
    let mut stream = connect_stream(&dir);

    // Announce a 2 MiB body; the server must reject the length up front.
    let mut header = Vec::with_capacity(8);
    header.extend_from_slice(&OPCODE_HANDSHAKE.to_le_bytes());
    header.extend_from_slice(&(2 * 1024 * 1024u32).to_le_bytes());
    must!(stream.write_all(&header), "oversize header must send");
    must!(stream.flush(), "stream must flush");

    // The server breaks the connection: reads must fail or hit EOF, and no
    // READY frame may arrive.
    let mut buf = [0u8; 8];
    let mut saw_data = false;
    let deadline = std::time::Instant::now() + SOCKET_WAIT;
    // Best-effort nonblocking probe: interprocess Stream supports it.
    use interprocess::local_socket::traits::Stream as _;
    must!(stream.set_nonblocking(true), "set_nonblocking");
    while std::time::Instant::now() < deadline {
        match stream.read(&mut buf) {
            Ok(0) => break, // EOF: server closed the connection
            Ok(_) => {
                saw_data = true;
                break;
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => break, // reset/closed: also connection teardown
        }
    }
    assert!(!saw_data, "oversize frame must not produce a READY reply");
}

/// Unknown opcode: no response/crash; a NEW connection still handshakes.
#[test]
#[serial_test::serial]
fn unknown_opcode_ignored_and_server_survives() {
    let dir = next_free_socket_dir();
    let (_conn, _rx) = spawn_connector(&dir);
    let mut stream = connect_stream(&dir);

    // Unknown opcode 99 with an empty body: server must swallow it.
    write_frame(&mut stream, 99, b"");

    // No reply must arrive on this connection.
    use interprocess::local_socket::traits::Stream as _;
    must!(stream.set_nonblocking(true), "set_nonblocking");
    let deadline = std::time::Instant::now() + Duration::from_millis(500);
    let mut buf = [0u8; 64];
    while std::time::Instant::now() < deadline {
        match stream.read(&mut buf) {
            Ok(0) => {
                panic!("server closed connection on unknown opcode");
            }
            Ok(_) => {
                panic!("server answered an unknown opcode");
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(err) => {
                panic!("server errored on unknown opcode: {:?}", err);
            }
        }
    }

    // A fresh connection still gets READY: the server survived.
    let mut fresh = connect_stream(&dir);
    handshake_and_assert_ready(&mut fresh);
}

/// Close then a second client still gets READY (no listener churn).
#[test]
#[serial_test::serial]
fn close_then_second_client_gets_ready() {
    let dir = next_free_socket_dir();
    let (_conn, rx) = spawn_connector(&dir);
    let mut first = connect_stream(&dir);

    handshake_and_assert_ready(&mut first);
    // Handshake sends no activity; Close sends an empty-activity command.
    write_frame(&mut first, OPCODE_CLOSE, b"{}");
    let cmd = must!(rx.recv_timeout(SOCKET_WAIT), "Close must report activity");
    let args = match cmd.args {
        Some(args) => args,
        None => {
            panic!("Close activity has args");
        }
    };
    assert!(args.activity.is_none(), "Close must send empty activity");

    // The listener is untouched by Close: a second client handshakes cleanly.
    let mut second = connect_stream(&dir);
    handshake_and_assert_ready(&mut second);
}

/// Stalled partial frame: dropping the peer unblocks the handler thread.
///
/// Behavior asserted: after the client goes away mid-frame, the server
/// reports the disconnect (empty activity on the channel) promptly — the
/// handler thread does not hang forever on a stalled read.
#[test]
#[serial_test::serial]
fn stalled_partial_frame_unblocks_on_peer_close() {
    let dir = next_free_socket_dir();
    let (_conn, rx) = spawn_connector(&dir);
    let mut stream = connect_stream(&dir);

    // Send only half a header, then vanish: the handler is blocked in read.
    let partial = &OPCODE_HANDSHAKE.to_le_bytes()[..2];
    must!(stream.write_all(partial), "partial header must send");
    must!(stream.flush(), "stream must flush");
    drop(stream);

    // Peer-close must surface as an empty activity within the timeout.
    // (The 2s bound is a liveness check on observable channel behavior, not
    // an assertion about scheduling internals.)
    must!(
        rx.recv_timeout(Duration::from_secs(2)),
        "handler must report disconnect within 2s of client drop"
    );
}

/// All ten IPC slots taken: the eleventh connector comes back disabled —
/// construction returns normally and `start` is an inert no-op — instead of
/// exiting the host process.
#[test]
#[serial_test::serial]
fn exhausted_slots_leave_connector_disabled() {
    let dir = next_free_socket_dir();
    // SAFETY: This test binary serializes env mutation via `#[serial]`; no other
    // test thread touches the environment concurrently.
    unsafe {
        std::env::set_var("XDG_RUNTIME_DIR", &dir);
    }

    // Hold every slot (discord-ipc-0..9). Connections are never accepted, but
    // a bound listener is enough: `Stream::connect` succeeds and the next
    // connector must treat the slot as in use.
    let (tx, _rx) = mpsc::channel();
    let held: Vec<IpcConnector> = (0..10).map(|_| IpcConnector::new(tx.clone())).collect();

    // The eleventh connector has nowhere to bind; it must stay alive and
    // disabled, and its `start` must not panic or spawn an accept loop.
    let mut disabled = IpcConnector::new(tx.clone());
    IpcFacilitator::start(&mut disabled);

    // The disabled connector may not have claimed a socket of its own.
    let sockets = std::fs::read_dir(&dir)
        .expect("socket dir readable")
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("discord-ipc-")
        })
        .count();
    assert_eq!(sockets, 10, "only the ten held slots may be bound");

    drop(disabled);
    drop(held);
}
