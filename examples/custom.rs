use rsrpc_core::RPCConfig;

pub fn main() -> Result<(), Box<dyn std::error::Error>> {

  // Create new client initialized with a custom game
  let custom_games_json = r#"
  [
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
    }
  ]"#;

  let mut client = rsrpc_core::RPCServer::from_json_str(custom_games_json, RPCConfig::default())?;
  client.start()?;

  // let 'er run forever
  std::thread::park();
  Ok(())
}
