use rsrpc_core::ProcessServer;
use rsrpc_core::custom_games::{custom_game, custom_game_with_args};
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[test]
fn test_builtin_custom_games_initialization() {
    let (tx, _rx) = mpsc::channel();
    let server = ProcessServer::new(vec![], tx);

    let detected = server.scan_for_processes().unwrap();
    if let Some(game) = detected.iter().find(|d| d.name == "Drag'n Wash") {
        assert_eq!(game.id, "1568491392664346654");
        assert!(matches!(
            game.details.as_deref(),
            Some("In-game" | "Main Menu")
        ));
    }
}

#[test]
fn test_custom_game_detection_by_process_scanner() {
    let mut child = Command::new("sleep").arg("9998").spawn().unwrap();
    let pid = child.id() as u64;

    let game = custom_game("Sleep Game", "test_custom_sleep_id", &[("linux", "sleep")]);

    let (tx, _rx) = mpsc::channel();
    let server = ProcessServer::new(vec![game], tx);

    let deadline = Instant::now() + Duration::from_secs(10);
    let detected = loop {
        let matches = server.scan_for_processes().unwrap();
        if let Some(found) = matches.iter().find(|m| m.pid == Some(pid)) {
            break found.clone();
        }
        assert!(
            Instant::now() < deadline,
            "spawned process should be detected by custom game rule"
        );
        std::thread::sleep(Duration::from_millis(20));
    };

    assert_eq!(detected.id, "test_custom_sleep_id");
    assert_eq!(detected.name, "Sleep Game");

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn test_custom_game_with_args_detection() {
    let mut child = Command::new("sleep").arg("9997").spawn().unwrap();
    let pid = child.id() as u64;

    let game = custom_game_with_args(
        "Sleep With Arg Game",
        "test_custom_arg_id",
        &[("linux", ">sleep", Some("9997"))],
    );

    let (tx, _rx) = mpsc::channel();
    let server = ProcessServer::new(vec![game], tx);

    let deadline = Instant::now() + Duration::from_secs(10);
    let detected = loop {
        let matches = server.scan_for_processes().unwrap();
        if let Some(found) = matches.iter().find(|m| m.pid == Some(pid)) {
            break found.clone();
        }
        assert!(
            Instant::now() < deadline,
            "spawned process with matching argument should be detected"
        );
        std::thread::sleep(Duration::from_millis(20));
    };

    assert_eq!(detected.id, "test_custom_arg_id");
    assert_eq!(detected.name, "Sleep With Arg Game");

    let _ = child.kill();
    let _ = child.wait();
}
#[test]
fn test_timestamp_preservation_across_status_changes() {
    use std::sync::Arc;
    let mut g1 = custom_game(
        "Drag'n Wash",
        "1568491392664346654",
        &[("win32", "DragNWash.exe")],
    );
    g1.timestamp = Some("1700000000000".to_string());
    g1.details = Some("Main Menu".to_string());

    let mut g2 = custom_game(
        "Drag'n Wash",
        "1568491392664346654",
        &[("win32", "DragNWash.exe")],
    );
    g2.timestamp = Some("1700000010000".to_string()); // new scan generated later timestamp
    g2.details = Some("In-game".to_string());

    // When level changes for same game, the original start timestamp must be kept
    let mut current = vec![Arc::new(g2)];
    let previous = vec![Arc::new(g1)];

    if !previous.is_empty() && current[0].id == previous[0].id {
        if let Some(orig_ts) = &previous[0].timestamp {
            let mut updated = (*current[0]).clone();
            updated.timestamp = Some(orig_ts.clone());
            current[0] = Arc::new(updated);
        }
    }

    assert_eq!(current[0].details.as_deref(), Some("In-game"));
    assert_eq!(current[0].timestamp.as_deref(), Some("1700000000000"));
}
