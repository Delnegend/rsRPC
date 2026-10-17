#![allow(
    clippy::assertions_on_constants,
    clippy::unreachable,
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used
)]
//! Behavior tests for `ClientConnector` payload caching on newly connecting clients.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::mpsc;
use std::time::Duration;

use rsrpc::cmd::{Activity, ActivityCmd, ActivityCmdArgs};
use rsrpc::ClientConnector;

struct WsClient {
    stream: TcpStream,
    buf: Vec<u8>,
}

impl WsClient {
    fn connect(port: u16) -> std::io::Result<Self> {
        let mut stream = TcpStream::connect(("127.0.0.1", port))?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let request = format!(
      "GET / HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: AAAAAAAAAAAAAAAAAAAAAA==\r\nSec-WebSocket-Version: 13\r\nContent-Length: 0\r\n\r\n",
      port
    );
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
                        "eof",
                    ))
                }
                Ok(n_read) => self.buf.extend_from_slice(&chunk[..n_read]),
                Err(e) => return Err(e),
            }
        }
        Ok(self.buf.drain(..n).collect())
    }
}

fn find_free_port() -> u16 {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("free port bindable");
    listener.local_addr().expect("local addr").port()
}

#[test]
fn new_client_receives_cached_activity_and_empty_clears_cache() {
    let port = find_free_port();
    let (ipc_tx, ipc_rx) = mpsc::channel();
    let (_proc_tx, proc_rx) = mpsc::channel();
    let (_ws_tx, ws_rx) = mpsc::channel();

    let ready_msg = "{\"cmd\":\"DISPATCH\",\"evt\":\"READY\"}".to_string();
    let mut connector = ClientConnector::new(port, ready_msg.clone(), ipc_rx, proc_rx, ws_rx)
        .expect("connector starts");
    connector.start();

    std::thread::sleep(Duration::from_millis(50));

    // 1. Send an IPC activity when NO clients are connected yet.
    let cmd = ActivityCmd {
        cmd: "SET_ACTIVITY".to_string(),
        args: Some(ActivityCmdArgs {
            pid: Some(340),
            activity: Some(Activity {
                details: Some("Rainstorm".to_string()),
                state: Some("Stage 1 - Titanic Plains".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        }),
        nonce: serde_json::Value::Null,
        application_id: Some("992086428240580720".to_string()),
        data: None,
        evt: None,
    };
    ipc_tx.send(cmd).expect("send activity");

    // Allow receiver thread to process and cache the payload.
    std::thread::sleep(Duration::from_millis(100));

    // 2. Connect a client AFTER the activity arrived.
    let mut client1 = WsClient::connect(port).expect("client 1 connects");

    // First frame MUST be READY.
    let frame1 = client1.read_text().expect("first frame received");
    assert_eq!(frame1, ready_msg);

    // Second frame MUST be the cached activity payload.
    let frame2 = client1.read_text().expect("cached payload received");
    assert!(
        frame2.contains("Titanic Plains"),
        "expected cached payload to contain Titanic Plains, got: {frame2}"
    );
    assert!(
        frame2.contains("Rainstorm"),
        "expected cached payload to contain Rainstorm, got: {frame2}"
    );

    // 3. Clear activity by sending empty payload.
    let empty_cmd = ActivityCmd {
        cmd: "SET_ACTIVITY".to_string(),
        args: Some(ActivityCmdArgs {
            pid: Some(340),
            activity: None,
            ..Default::default()
        }),
        nonce: serde_json::Value::Null,
        application_id: Some("992086428240580720".to_string()),
        data: None,
        evt: None,
    };
    ipc_tx.send(empty_cmd).expect("send empty cmd");

    // Client 1 should receive the empty payload broadcast.
    let frame3 = client1.read_text().expect("empty payload broadcast");
    assert!(
        frame3.contains("\"activity\":null") || frame3.contains("\"activity\": null"),
        "expected empty activity, got: {frame3}"
    );

    // 4. Connect a second client AFTER activity was cleared.
    let mut client2 = WsClient::connect(port).expect("client 2 connects");

    // Client 2 gets READY.
    let c2_frame1 = client2.read_text().expect("client 2 gets ready");
    assert_eq!(c2_frame1, ready_msg);

    // Client 2 must NOT receive any cached activity since it was cleared.
    let c2_frame2 = client2.read_text();
    assert!(
        c2_frame2.is_none(),
        "expected no cached payload after clear, got: {c2_frame2:?}"
    );
}
