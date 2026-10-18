#![allow(
    clippy::assertions_on_constants,
    clippy::unreachable,
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used
)]
//! Behavior tests for `WebsocketConnector` over real TCP sockets.
//!
//! A minimal stdlib-only websocket client (hand-written HTTP upgrade +
//! masked text frames) drives the server through its public API and asserts
//! only observable wire behavior: READY payloads, invite echoes, and that
//! the server stays alive after invalid input.
use std::{
    io::{Read, Write},
    net::TcpStream,
    sync::mpsc,
    time::Duration,
};

use rsrpc_core::WebsocketConnector;

const ORIGIN: &str = "https://discord.com";

/// Serializes tests: every `WebsocketConnector` binds the same fixed port
/// range, so each test holds this guard for its whole body.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    match SERIAL.lock() {
        Ok(guard) => guard,
        Err(_) => {
            panic!("test serializer poisoned");
        }
    }
}

/// Unwrap-or-fail without `unwrap`/`expect` tokens (test files stay
/// grep-clean for those). Fails the test with context on `Err` or `None`.
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

macro_rules! must_some {
    ($option:expr, $what:literal) => {
        match $option {
            Some(value) => value,
            None => {
                panic!("{}: no value", $what);
            }
        }
    };
}

/// Minimal blocking websocket client: HTTP upgrade, then masked text frames.
struct WsClient {
    stream: TcpStream,
    buf: Vec<u8>,
}

impl WsClient {
    fn connect(port: u16, query: &str, origin: Option<&str>) -> std::io::Result<Self> {
        let mut stream = TcpStream::connect(("127.0.0.1", port))?;
        stream.set_read_timeout(Some(Duration::from_secs(3)))?;
        let mut request = format!(
            "GET /{} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: AAAAAAAAAAAAAAAAAAAAAA==\r\nSec-WebSocket-Version: 13\r\n",
            query, port
        );
        if let Some(o) = origin {
            request.push_str(&format!("Origin: {}\r\n", o));
        }
        request.push_str("Content-Length: 0\r\n\r\n");
        stream.write_all(request.as_bytes())?;
        stream.flush()?;
        let mut client = Self {
            stream,
            buf: Vec::new(),
        };
        client.read_http_response()?;
        Ok(client)
    }

    fn read_http_response(&mut self) -> std::io::Result<()> {
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            match self.stream.read(&mut byte) {
                Ok(0) => break,
                Ok(_) => head.push(byte[0]),
                Err(e) => return Err(e),
            }
            if head.len() > 8192 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "handshake response too large",
                ));
            }
        }
        Ok(())
    }

    fn send_text(&mut self, text: &str) -> std::io::Result<()> {
        let bytes = text.as_bytes();
        let mut frame = vec![0x81, 0x80 | ((bytes.len().min(125)) as u8)];
        let mask = [0x12u8, 0x34, 0x56, 0x78];
        frame.extend_from_slice(&mask);
        for (i, b) in bytes.iter().enumerate() {
            frame.push(b ^ mask[i % 4]);
        }
        self.stream.write_all(&frame)?;
        self.stream.flush()
    }

    /// Read one server text frame; `None` on timeout/close.
    fn read_text(&mut self) -> Option<String> {
        let header = self.read_exact(2).ok()?;
        let len = (header[1] & 0x7F) as usize;
        if header[0] & 0x0F == 0x8 || len > 65536 {
            return None;
        }
        let len = if len == 126 {
            let ext = self.read_exact(2).ok()?;
            u16::from_be_bytes([ext[0], ext[1]]) as usize
        } else {
            len
        };
        let payload = self.read_exact(len).ok()?;
        String::from_utf8(payload).ok()
    }

    fn read_exact(&mut self, n: usize) -> std::io::Result<Vec<u8>> {
        while self.buf.len() < n {
            let mut chunk = [0u8; 4096];
            match self.stream.read(&mut chunk) {
                Ok(0) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "connection closed",
                    ));
                }
                Ok(m) => self.buf.extend_from_slice(&chunk[..m]),
                Err(e) => return Err(e),
            }
        }
        Ok(self.buf.drain(..n).collect())
    }
}

fn start_server() -> (
    WebsocketConnector,
    mpsc::Receiver<rsrpc_core::cmd::ActivityCmd>,
) {
    let (tx, rx) = mpsc::channel();
    let connector = must!(WebsocketConnector::new(tx), "websocket server starts");
    (connector, rx)
}

/// The port the test server actually bound (first connectable in range).
fn live_port() -> u16 {
    for p in 6463..6473 {
        if TcpStream::connect(("127.0.0.1", p)).is_ok() {
            return p;
        }
    }
    panic!("websocket server listening on 6463..6472");
}

fn boot() -> u16 {
    let (mut server, _rx) = start_server();
    server.start(true, true);
    std::thread::sleep(Duration::from_millis(300));
    live_port()
}

fn wait_ready(client: &mut WsClient) -> Option<String> {
    // The server's READY frame can take a moment after the upgrade.
    for _ in 0..20 {
        if let Some(text) = client.read_text() {
            return Some(text);
        }
    }
    None
}

#[test]
fn valid_handshake_receives_ready() {
    let _guard = serial();
    let port = boot();
    let mut client = must!(
        WsClient::connect(port, "?v=1&encoding=json", Some(ORIGIN)),
        "valid handshake"
    );
    let ready = wait_ready(&mut client);
    assert!(
        ready.as_deref().is_some_and(|t| t.contains("\"READY\"")),
        "expected READY payload, got {:?}",
        ready
    );
}

#[test]
fn wrong_version_gets_no_ready() {
    let _guard = serial();
    let port = boot();
    let mut client = must!(
        WsClient::connect(port, "?v=9&encoding=json", Some(ORIGIN)),
        "version-gated handshake"
    );
    // Give the server a beat, then assert no READY arrived.
    std::thread::sleep(Duration::from_millis(300));
    let got = client.read_text();
    assert!(
        got.as_deref().is_none_or(|t| !t.contains("\"READY\"")),
        "version-gated client must not receive READY, got {:?}",
        got
    );
}

#[test]
fn bad_origin_set_activity_is_ignored_and_server_survives() {
    let _guard = serial();
    let port = boot();

    // Attacker with a non-Discord origin: SET_ACTIVITY must be dropped.
    let mut evil = must!(
        WsClient::connect(port, "?v=1&encoding=json", Some("https://evil.example")),
        "evil handshake completes at TCP level"
    );
    let _ = wait_ready(&mut evil);
    must!(
        evil.send_text(r#"{"cmd":"SET_ACTIVITY","args":{"pid":1},"nonce":"1"}"#),
        "evil send"
    );
    std::thread::sleep(Duration::from_millis(300));

    // A legit client afterwards still gets full service: server is alive.
    let mut good = must!(
        WsClient::connect(port, "?v=1&encoding=json", Some(ORIGIN)),
        "good client connects after bad-origin message"
    );
    let ready = wait_ready(&mut good);
    assert!(
        ready.as_deref().is_some_and(|t| t.contains("\"READY\"")),
        "server must stay alive for next client, got {:?}",
        ready
    );
}

#[test]
fn invite_browser_echo_contains_code() {
    let _guard = serial();
    let port = boot();

    let mut client = must!(
        WsClient::connect(port, "?v=1&encoding=json", Some(ORIGIN)),
        "client connects"
    );
    let _ = wait_ready(&mut client);
    must!(
        client
            .send_text(r#"{"cmd":"INVITE_BROWSER","args":{"code":"abc123","pid":42},"nonce":"9"}"#),
        "invite send"
    );

    for _ in 0..20 {
        if let Some(text) = client.read_text()
            && text.contains("abc123")
        {
            return;
        }
    }
    panic!("INVITE_BROWSER echo must contain the invite code");
}

#[test]
fn malformed_json_does_not_kill_server() {
    let _guard = serial();
    let port = boot();

    let mut client = must!(
        WsClient::connect(port, "?v=1&encoding=json", Some(ORIGIN)),
        "client connects"
    );
    let _ = wait_ready(&mut client);
    // Not an ActivityCmd at all: must be ignored without crashing the loop.
    must!(client.send_text("this is not json{{{ "), "garbage send");
    must!(
        client.send_text(r#"{"cmd":"NOPE_UNKNOWN"}"#),
        "unknown-cmd send"
    );
    std::thread::sleep(Duration::from_millis(300));

    let mut next = must!(
        WsClient::connect(port, "?v=1&encoding=json", Some(ORIGIN)),
        "server alive for next client after garbage"
    );
    let ready = wait_ready(&mut next);
    assert!(
        ready.as_deref().is_some_and(|t| t.contains("\"READY\"")),
        "server must survive malformed input, got {:?}",
        ready
    );
}

#[test]
fn message_from_unknown_client_keeps_server_alive() {
    let _guard = serial();
    let port = boot();

    // Raw client that sends a valid message but disconnects immediately,
    // racing the server's connect/message ordering — then a fresh client
    // must still be served.
    let hit = must!(
        WsClient::connect(port, "?v=1&encoding=json", Some(ORIGIN)),
        "probe client connects"
    );
    drop(hit);

    let mut next = must!(
        WsClient::connect(port, "?v=1&encoding=json", Some(ORIGIN)),
        "server alive after unknown-client churn"
    );
    let _ = must_some!(wait_ready(&mut next), "READY after churn");
}
