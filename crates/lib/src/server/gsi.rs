use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::thread;

use serde::Deserialize;
use serde_json::Value;

use crate::cmd::{Activity, ActivityCmd, ActivityCmdArgs, Assets};

/// Payload received from Valve Game State Integration (GSI).
#[derive(Deserialize, Debug, Default)]
pub struct GsiPayload {
    pub provider: Option<GsiProvider>,
    pub map: Option<GsiMap>,
    pub player: Option<GsiPlayer>,
    pub round: Option<Value>,
}

#[derive(Deserialize, Debug, Default)]
pub struct GsiProvider {
    pub name: Option<String>,
    pub appid: Option<u64>,
}

#[derive(Deserialize, Debug, Default)]
pub struct GsiMap {
    pub mode: Option<String>,
    pub name: Option<String>,
    pub phase: Option<String>,
    pub round: Option<u64>,
    pub team_ct: Option<GsiTeam>,
    pub team_t: Option<GsiTeam>,
}

#[derive(Deserialize, Debug, Default)]
pub struct GsiTeam {
    pub score: Option<u64>,
}

#[derive(Deserialize, Debug, Default)]
pub struct GsiPlayer {
    pub name: Option<String>,
    pub activity: Option<String>,
    pub match_stats: Option<GsiMatchStats>,
}

#[derive(Deserialize, Debug, Default)]
pub struct GsiMatchStats {
    pub kills: Option<u64>,
    pub deaths: Option<u64>,
    pub assists: Option<u64>,
    pub score: Option<u64>,
}

/// Convert a GSI payload to an enriched [`ActivityCmd`].
pub fn gsi_payload_to_activity(payload: &GsiPayload) -> Option<ActivityCmd> {
    let app_id = payload
        .provider
        .as_ref()
        .and_then(|p| p.appid)
        .map(|id| id.to_string())
        .unwrap_or_else(|| "730".to_string()); // Default to CS2 appid (730)

    let map_name = payload
        .map
        .as_ref()
        .and_then(|m| m.name.as_deref())
        .unwrap_or("In Menu");

    let mode = payload
        .map
        .as_ref()
        .and_then(|m| m.mode.as_deref())
        .unwrap_or("Competitive");

    let ct_score = payload
        .map
        .as_ref()
        .and_then(|m| m.team_ct.as_ref())
        .and_then(|t| t.score)
        .unwrap_or(0);

    let t_score = payload
        .map
        .as_ref()
        .and_then(|m| m.team_t.as_ref())
        .and_then(|t| t.score)
        .unwrap_or(0);

    let details = if map_name == "In Menu" {
        "In Main Menu".to_string()
    } else {
        format!("{mode} on {map_name}")
    };

    let state = if map_name == "In Menu" {
        "Lobby".to_string()
    } else {
        format!("CT {ct_score} - {t_score} T")
    };

    let large_image = if map_name != "In Menu" {
        Some(map_name.to_lowercase().replace('/', "_"))
    } else {
        Some("cs2_logo".to_string())
    };

    Some(ActivityCmd {
        application_id: Some(app_id.clone()),
        cmd: "SET_ACTIVITY".to_string(),
        data: None,
        evt: None,
        args: Some(ActivityCmdArgs {
            pid: None,
            activity: Some(Activity {
                id: Some(app_id.clone()),
                name: Some("Counter-Strike 2".to_string()),
                application_id: Some(app_id),
                details: Some(details),
                state: Some(state),
                assets: Some(Assets {
                    large_image,
                    large_text: Some(map_name.to_string()),
                    small_image: None,
                    small_text: None,
                }),
                r#type: 0,
                url: None,
                created_at: None,
                session_id: None,
                platform: None,
                supported_platforms: None,
                timestamps: None,
                sync_id: None,
                instance: None,
                flags: None,
                emoji: None,
                party: None,
                secrets: None,
                metadata: None,
                buttons: None,
            }),
            code: None,
        }),
        nonce: Value::String("gsi".to_string()),
    })
}

/// Lightweight HTTP server listening for Game State Integration POSTs.
pub struct GsiServer {
    port: u16,
    running: Arc<AtomicBool>,
}

impl GsiServer {
    pub fn new(port: u16) -> Self {
        Self {
            port,
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Start the GSI server and forward parsed activities to `event_sender`.
    pub fn start(&self, event_sender: Sender<ActivityCmd>) -> std::io::Result<()> {
        let listener = TcpListener::bind(("127.0.0.1", self.port))?;
        let running = self.running.clone();
        running.store(true, Ordering::SeqCst);

        thread::spawn(move || {
            while running.load(Ordering::Relaxed) {
                if let Ok((stream, _)) = listener.accept() {
                    let sender = event_sender.clone();
                    thread::spawn(move || {
                        let _ = handle_gsi_stream(stream, &sender);
                    });
                }
            }
        });

        tracing::info!("Listening on 127.0.0.1:{}", self.port);
        Ok(())
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

fn handle_gsi_stream(mut stream: TcpStream, sender: &Sender<ActivityCmd>) -> std::io::Result<()> {
    let mut reader = BufReader::new(&mut stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;

    if !request_line.starts_with("POST") {
        let _ = stream.write_all(b"HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\n\r\n");
        return Ok(());
    }

    let mut content_length: usize = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        if line == "\r\n" || line.is_empty() {
            break;
        }
        if let Some(val) = line.strip_prefix("Content-Length: ") {
            content_length = val.trim().parse::<usize>().unwrap_or(0);
        } else if let Some(val) = line.strip_prefix("content-length: ") {
            content_length = val.trim().parse::<usize>().unwrap_or(0);
        }
    }

    if content_length > 0 && content_length <= 512 * 1024 {
        let mut body = vec![0u8; content_length];
        reader.read_exact(&mut body)?;
        if let Ok(gsi_payload) = serde_json::from_slice::<GsiPayload>(&body) {
            if let Some(cmd) = gsi_payload_to_activity(&gsi_payload) {
                let _ = sender.send(cmd);
            }
        }
    }

    let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
    Ok(())
}
