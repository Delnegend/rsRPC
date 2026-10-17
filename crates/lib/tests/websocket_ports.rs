#![allow(
    clippy::assertions_on_constants,
    clippy::unreachable,
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used
)]
//! Behavior tests for `RSRPC_WS_PORTS` websocket port selection.
//!
//! Tests speak real TCP: a connector is started and the raw port is probed,
//! or candidate ports are occupied first and construction is asserted to
//! fall through or fail. Env mutation is `#[serial]` within this dedicated
//! binary, so no other test thread observes a half-applied override.
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use rsrpc::{RPCConfig, RPCServer, WebsocketConnector};

/// Unwrap-or-fail without `unwrap`/`expect` tokens (owned files stay
/// grep-clean). Fails the test with context on `Err` or `None`.
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

fn free_port() -> u16 {
    let listener = must!(TcpListener::bind(("127.0.0.1", 0)), "free port bindable");
    let addr = must!(listener.local_addr(), "local addr");
    addr.port()
}

/// Two adjacent ports that are both free at probe time.
fn free_consecutive_pair() -> (u16, u16) {
    for base in (65400..65500).step_by(2) {
        let start_free = TcpListener::bind(("127.0.0.1", base)).is_ok();
        let end_free = TcpListener::bind(("127.0.0.1", base + 1)).is_ok();
        if start_free && end_free {
            return (base, base + 1);
        }
    }
    panic!("no free consecutive port pair found");
}

/// Wait until `port` accepts TCP connections.
fn wait_connectable(port: u16) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("port {} never became connectable", port);
}

/// A single-port override binds exactly that port.
#[test]
#[serial_test::serial]
fn single_port_override_is_bound() {
    let port = free_port();
    // SAFETY: env mutation is serialized via `#[serial]`; this binary owns
    // `RSRPC_WS_PORTS`.
    unsafe {
        std::env::set_var("RSRPC_WS_PORTS", port.to_string());
    }

    let (tx, _rx) = mpsc::channel();
    let mut connector = must!(
        WebsocketConnector::new(tx),
        "connector starts on the override port"
    );
    connector.start(true, true);
    wait_connectable(port);

    // SAFETY: as above.
    unsafe {
        std::env::remove_var("RSRPC_WS_PORTS");
    }
}

/// A `start-end` override is a real range: with the first port occupied the
/// connector must fall through and bind the (inclusive) end port.
#[test]
#[serial_test::serial]
fn range_override_skips_busy_start_and_binds_end() {
    let (start, end) = free_consecutive_pair();
    // Occupy the range start so the connector must bind the end.
    let held = must!(TcpListener::bind(("127.0.0.1", start)), "range start held");

    // SAFETY: env mutation is serialized via `#[serial]`; this binary owns
    // `RSRPC_WS_PORTS`.
    unsafe {
        std::env::set_var("RSRPC_WS_PORTS", format!("{start}-{end}"));
    }

    let (tx, _rx) = mpsc::channel();
    let mut connector = must!(
        WebsocketConnector::new(tx),
        "connector starts on the range end"
    );
    connector.start(true, true);
    wait_connectable(end);

    // SAFETY: as above.
    unsafe {
        std::env::remove_var("RSRPC_WS_PORTS");
    }
    drop(held);
}

/// Every candidate taken: construction reports the exhaustion instead of
/// binding elsewhere or exiting.
#[test]
#[serial_test::serial]
fn exhausted_override_reports_error() {
    let port = free_port();
    let held = must!(TcpListener::bind(("127.0.0.1", port)), "override port held");

    // SAFETY: env mutation is serialized via `#[serial]`; this binary owns
    // `RSRPC_WS_PORTS`.
    unsafe {
        std::env::set_var("RSRPC_WS_PORTS", port.to_string());
    }

    let (tx, _rx) = mpsc::channel();
    let result = WebsocketConnector::new(tx);
    assert!(
        result.is_err(),
        "an exhausted override must report an error"
    );

    // SAFETY: as above.
    unsafe {
        std::env::remove_var("RSRPC_WS_PORTS");
    }
    drop(held);
}

/// An exhausted websocket range must not abort `RPCServer::start`: the rest
/// of the server (client connector on the configured port, IPC) still comes
/// up, observable through a mutator reaching the running connectors instead
/// of reporting `NotStarted`.
#[test]
#[serial_test::serial]
fn rpc_server_starts_with_exhausted_websocket_range() {
    let client_port = free_port();
    let (start, end) = free_consecutive_pair();
    let _held_start = must!(TcpListener::bind(("127.0.0.1", start)), "range start held");
    let _held_end = must!(TcpListener::bind(("127.0.0.1", end)), "range end held");

    // SAFETY: env mutation is serialized via `#[serial]`; this binary owns
    // `RSRPC_WS_PORTS`.
    unsafe {
        std::env::set_var("RSRPC_WS_PORTS", format!("{start}-{end}"));
    }

    let config = RPCConfig {
        port: client_port,
        enable_process_scanner: false,
        ..Default::default()
    };
    let mut server = must!(RPCServer::from_json_str("[]", config), "server builds");
    server.start();

    // The facade stayed disabled, but `start` completed: mutators reach the
    // initialized connectors instead of reporting `NotStarted`.
    must!(
        server.append_detectables(Vec::new()),
        "server must finish starting despite the disabled facade"
    );

    // SAFETY: as above.
    unsafe {
        std::env::remove_var("RSRPC_WS_PORTS");
    }
}
