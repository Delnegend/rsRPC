use rsrpc::{detection::DetectableActivity, logger::set_logs_enabled, RPCConfig};
use std::sync::{Arc, Mutex};

pub fn main() -> Result<(), Box<dyn std::error::Error>> {
  // When running as a binary, enable logs
  set_logs_enabled(true);

  // Create new client and stuff
  let client = Arc::new(Mutex::new(
    rsrpc::RPCServer::from_json_str("[]", RPCConfig::default())?,
  ));

  // We can use this cloned Arc of the client to append new detectables
  let append_client = client.clone();

  // In a seperate thread, append a new detectable game
  std::thread::spawn(move || {
    let new_game: Result<DetectableActivity, _> = serde_json::from_str(
      r#"
    {
      "bot_public": true,
      "bot_require_code_grant": false,
      "description": "",
      "executables": [
        {
          "is_launcher": false,
          "name": "my_awesome_game.exe",
          "os": "win32"
        }
      ],
      "flags": 0,
      "hook": true,
      "id": "1337",
      "name": "Awesome Game!!",
      "type": 1
    }"#,
    );
    let Ok(new_game) = new_game else {
      return;
    };

    // This function takes a vec, as you might want to add more than one game (say, if you stored configurable lists of games in a seperate file)
    if let Ok(mut guard) = append_client.lock() {
      let _ = guard.append_detectables(vec![new_game]);
    }

    // Remove a custom game via its name
    if let Ok(mut guard) = append_client.lock() {
      let _ = guard.remove_detectable_by_name("Awesome Game!!");
    }
  });

  // Starts the other threads (process detector, client connector, etc)
  if let Ok(mut guard) = client.lock() {
    guard.start();
  }

  // let 'er run forever
  std::thread::park();
  Ok(())
}
