use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::mpsc;
use std::time::Duration;

use rsrpc::{gsi_payload_to_activity, GsiMap, GsiPayload, GsiServer, GsiTeam};

#[test]
fn gsi_payload_mapping_to_activity() {
  let payload = GsiPayload {
    map: Some(GsiMap {
      mode: Some("Competitive".to_string()),
      name: Some("de_dust2".to_string()),
      phase: Some("live".to_string()),
      round: Some(12),
      team_ct: Some(GsiTeam { score: Some(8) }),
      team_t: Some(GsiTeam { score: Some(4) }),
    }),
    ..Default::default()
  };

  let activity = gsi_payload_to_activity(&payload).expect("should map GsiPayload to ActivityCmd");
  let args = activity.args.expect("ActivityCmd should have args");
  let act = args.activity.expect("ActivityCmdArgs should have activity");

  assert_eq!(act.details.as_deref(), Some("Competitive on de_dust2"));
  assert_eq!(act.state.as_deref(), Some("CT 8 - 4 T"));
  assert_eq!(act.name.as_deref(), Some("Counter-Strike 2"));
  let assets = act.assets.expect("should populate assets");
  assert_eq!(assets.large_image.as_deref(), Some("de_dust2"));
}

#[test]
fn gsi_server_http_post_dispatches_activity() {
  let port = 19876;
  let (tx, rx) = mpsc::channel();
  let server = GsiServer::new(port);
  server.start(tx).expect("GsiServer should start on port");

  // Send a real HTTP POST request to the GSI server
  let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("should connect to GSI server");
  let body = r#"{"provider":{"name":"Counter-Strike 2","appid":730},"map":{"mode":"Premier","name":"de_inferno","team_ct":{"score":11},"team_t":{"score":9}}}"#;
  let request = format!(
    "POST /gsi HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: {}\r\nContent-Type: application/json\r\n\r\n{body}",
    body.len()
  );

  stream.write_all(request.as_bytes()).expect("write request");
  let mut response = [0u8; 128];
  let n = stream.read(&mut response).expect("read response");
  let resp_str = String::from_utf8_lossy(&response[..n]);
  assert!(
    resp_str.starts_with("HTTP/1.1 200 OK"),
    "server should reply 200 OK"
  );

  // Assert the activity was dispatched through the channel
  let received = rx
    .recv_timeout(Duration::from_secs(2))
    .expect("channel should receive parsed GSI activity within 2s");
  let args = received.args.expect("received activity should have args");
  let act = args.activity.expect("should contain activity");

  assert_eq!(act.details.as_deref(), Some("Premier on de_inferno"));
  assert_eq!(act.state.as_deref(), Some("CT 11 - 9 T"));

  server.stop();
}
