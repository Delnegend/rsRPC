use std::process::Command;
use std::sync::mpsc;

use rsrpc_core::detection::DetectableActivity;
use rsrpc_core::plugins::{GameEnrichment, GameStatusPlugin, LeagueOfLegendsPlugin, VrChatPlugin};
use rsrpc_core::{ProcessEventListeners, ProcessServer};

fn empty_server() -> ProcessServer {
    let (tx, _rx) = mpsc::channel();
    ProcessServer::new(vec![], tx, ProcessEventListeners::default())
}

struct MockGamePlugin {
    target: String,
    details: String,
    state: String,
}

impl GameStatusPlugin for MockGamePlugin {
    fn target_id(&self) -> &str {
        &self.target
    }

    fn extract_status(&self, _pid: u64, _process_name: &str) -> Option<GameEnrichment> {
        Some(GameEnrichment {
            details: Some(self.details.clone()),
            state: Some(self.state.clone()),
            large_image: Some("mock_large".to_string()),
            large_text: Some("Mock Game".to_string()),
            small_image: None,
            small_text: None,
        })
    }
}

#[test]
fn plugin_enriches_detected_game_activity() {
    use std::time::{Duration, Instant};
    // Spawn a real test process to match against
    let mut child = Command::new("sleep").arg("9998").spawn().unwrap();
    let pid = child.id() as u64;

    let da: DetectableActivity = serde_json::from_str(
    r#"{"name":"sleep","id":"game_sleep_app_id","hook":false,"executables":[{"is_launcher":false,"name":"sleep","os":"linux"}]}"#,
  )
  .unwrap();

    let mut s = empty_server();
    s.append_detectables(vec![da]);

    // Register custom telemetry plugin for this game
    s.register_plugin(MockGamePlugin {
        target: "game_sleep_app_id".to_string(),
        details: "In Matchmaking Lobby".to_string(),
        state: "Ranked 5v5 (Searching...)".to_string(),
    });

    // `spawn` can return while the child's execve is still settling — its
    // cmdline briefly reads as empty or as the parent's — so a single
    // immediate scan can miss it under load. Poll instead.
    let deadline = Instant::now() + Duration::from_secs(10);
    let detected = loop {
        let matches = s.scan_for_processes().unwrap();
        if let Some(found) = matches.iter().find(|m| m.pid == Some(pid)) {
            break found.clone();
        }
        assert!(
            Instant::now() < deadline,
            "spawned game process should be detected"
        );
        std::thread::sleep(Duration::from_millis(20));
    };

    assert_eq!(
        detected.details.as_deref(),
        Some("In Matchmaking Lobby"),
        "plugin should enrich activity details"
    );
    assert_eq!(
        detected.state.as_deref(),
        Some("Ranked 5v5 (Searching...)"),
        "plugin should enrich activity state"
    );

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn built_in_league_and_vrchat_plugins() {
    let lol = LeagueOfLegendsPlugin;
    let status = lol.extract_status(1234, "leagueclient.exe").unwrap();
    assert_eq!(status.details.as_deref(), Some("In Summoner's Rift"));
    assert_eq!(status.state.as_deref(), Some("Playing Solo/Duo"));

    let vrc = VrChatPlugin {
        world_name: Some("Furry Hideout".to_string()),
    };
    let vrc_status = vrc.extract_status(5678, "vrchat.exe").unwrap();
    assert_eq!(
        vrc_status.details.as_deref(),
        Some("Exploring Furry Hideout")
    );
    assert_eq!(vrc_status.state.as_deref(), Some("In World (Invite+)"));
}
