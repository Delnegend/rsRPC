#![allow(
    clippy::unreachable,
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used
)]
use rsrpc::cmd::{Activity, ActivityCmd, ActivityCmdArgs, TimeoutValue, Timestamps};
use rsrpc::{RPCConfig, RPCServer, RpcError};
use serde_json::{Value, json};

fn empty_server() -> RPCServer {
    match RPCServer::from_json_str("[]", RPCConfig::default()) {
        Ok(server) => server,
        Err(_) => unreachable!(),
    }
}

#[test]
fn invalid_json_reports_json_error() {
    match RPCServer::from_json_str("{not valid json", RPCConfig::default()) {
        Err(RpcError::Json(_)) => {}
        _ => unreachable!(),
    }
}

#[test]
fn missing_file_reports_io_error() {
    match RPCServer::from_file("/definitely/not/here/detectable.json", RPCConfig::default()) {
        Err(RpcError::Io(_)) => {}
        _ => unreachable!(),
    }
}

#[test]
fn valid_file_round_trips() {
    let path = std::env::temp_dir().join(format!(
        "rsrpc-api-errors-valid-{}.json",
        std::process::id()
    ));
    std::fs::write(&path, "[]").unwrap();
    let result = RPCServer::from_file(&path, RPCConfig::default());
    std::fs::remove_file(&path).unwrap();
    assert!(result.is_ok());
}

#[test]
fn mutators_before_start_report_not_started() {
    let mut server = empty_server();

    assert!(matches!(
        server.append_detectables(vec![]),
        Err(RpcError::NotStarted)
    ));
    assert!(matches!(
        server.remove_detectable_by_name("Some Game"),
        Err(RpcError::NotStarted)
    ));
    assert!(matches!(
        server.scan_for_processes(),
        Err(RpcError::NotStarted)
    ));
}

#[test]
fn registering_callback_before_start_succeeds() {
    let mut server = empty_server();
    assert!(matches!(
        server.on_process_scan_complete(|_state| {}),
        Ok(())
    ));
}

#[test]
fn registering_callback_after_start_reports_already_started() {
    let mut server = match RPCServer::from_json_str(
        "[]",
        RPCConfig {
            port: 0,
            enable_process_scanner: false,
            enable_ipc_connector: false,
            enable_websocket_connector: false,
            enable_secondary_events: false,
            ..Default::default()
        },
    ) {
        Ok(server) => server,
        Err(_) => unreachable!(),
    };
    server.start();

    assert!(matches!(
        server.on_process_scan_complete(|_state| {}),
        Err(RpcError::AlreadyStarted)
    ));
}

#[test]
fn fix_normalizes_seconds_timestamps_to_millis() {
    let mut cmd = ActivityCmd {
        args: Some(ActivityCmdArgs {
            pid: None,
            activity: Some(Activity {
                id: None,
                name: Some("Game".to_string()),
                buttons: None,
                r#type: 0,
                url: None,
                created_at: None,
                session_id: None,
                platform: None,
                supported_platforms: None,
                timestamps: Some(Timestamps {
                    // A small value is seconds; fix() must scale it to milliseconds.
                    start: Some(TimeoutValue::new(10)),
                    end: Some(TimeoutValue::new(20)),
                }),
                application_id: None,
                details: None,
                state: None,
                sync_id: None,
                instance: None,
                flags: None,
                emoji: None,
                party: None,
                assets: None,
                secrets: None,
                metadata: None,
            }),
            code: None,
        }),
        ..ActivityCmd::empty()
    };

    cmd.fix();

    let serialized = serde_json::to_value(&cmd).unwrap();
    assert_eq!(
        serialized["args"]["activity"]["timestamps"]["start"],
        json!(10_000)
    );
    assert_eq!(
        serialized["args"]["activity"]["timestamps"]["end"],
        json!(20_000)
    );
}

#[test]
fn fix_maps_object_buttons_to_labels_and_urls() {
    let mut cmd = ActivityCmd {
        args: Some(ActivityCmdArgs {
            pid: None,
            activity: Some(Activity {
                id: None,
                name: Some("Game".to_string()),
                buttons: Some(vec![json!({"label": "Play", "url": "https://example.com"})]),
                r#type: 0,
                url: None,
                created_at: None,
                session_id: None,
                platform: None,
                supported_platforms: None,
                timestamps: None,
                application_id: None,
                details: None,
                state: None,
                sync_id: None,
                instance: None,
                flags: None,
                emoji: None,
                party: None,
                assets: None,
                secrets: None,
                metadata: None,
            }),
            code: None,
        }),
        ..ActivityCmd::empty()
    };

    cmd.fix();

    let serialized = serde_json::to_value(&cmd).unwrap();
    assert_eq!(serialized["args"]["activity"]["buttons"], json!(["Play"]));
    assert_eq!(
        serialized["args"]["activity"]["metadata"]["button_urls"],
        json!(["https://example.com"])
    );
}

#[test]
fn fix_sets_flags_when_instance_without_flags() {
    let mut cmd = ActivityCmd {
        args: Some(ActivityCmdArgs {
            pid: None,
            activity: Some(Activity {
                id: None,
                name: Some("Game".to_string()),
                buttons: None,
                r#type: 0,
                url: None,
                created_at: None,
                session_id: None,
                platform: None,
                supported_platforms: None,
                timestamps: None,
                application_id: None,
                details: None,
                state: None,
                sync_id: None,
                instance: Some(true),
                flags: None,
                emoji: None,
                party: None,
                assets: None,
                secrets: None,
                metadata: None,
            }),
            code: None,
        }),
        ..ActivityCmd::empty()
    };

    cmd.fix();

    let serialized = serde_json::to_value(&cmd).unwrap();
    assert_eq!(serialized["args"]["activity"]["flags"], json!(1));
}

#[test]
fn nonce_accessor_returns_string_value_unquoted() {
    let mut cmd = ActivityCmd::empty();
    cmd.nonce = Value::String("abc".to_string());
    assert_eq!(cmd.nonce_str(), "abc");

    cmd.nonce = json!(42);
    assert_eq!(cmd.nonce_str(), "42");
}
