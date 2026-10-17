use std::{
    collections::HashMap,
    io,
    sync::{mpsc, Arc, Mutex},
};

use serde::Deserialize;
use simple_websockets::{Event, EventHub, Message, Responder};

use crate::{
    cmd::{ActivityCmd, ActivityCmdArgs},
    server::utils::CONNECTION_RESPONSE,
    url_params::get_url_params,
};
use tracing::{debug, error, info, trace, warn};

type ActivityResponder = (Option<ActivityCmd>, Responder);

/// Minimal pre-parse probe: only the `cmd` field is deserialized so unknown
/// or malformed payloads are rejected before paying for a full `ActivityCmd`
/// parse.
#[derive(Deserialize)]
struct CmdProbe {
    cmd: String,
}

const VALID_ORIGINS: [&str; 3] = [
    "https://discord.com",
    "https://canary.discord.com",
    "https://ptb.discord.com",
];

#[derive(Clone)]
pub struct WebsocketConnector {
    server: Arc<Mutex<Option<EventHub>>>,
    pub clients: Arc<Mutex<HashMap<u64, ActivityResponder>>>,

    event_sender: mpsc::Sender<ActivityCmd>,
}

impl WebsocketConnector {
    /// Try starting the websocket server on ports 6463..6472.
    ///
    /// # Errors
    /// Returns an `io::Error` with kind `AddrInUse` when every port is taken
    /// (or the listener thread fails to spawn).
    pub fn new(event_sender: mpsc::Sender<ActivityCmd>) -> io::Result<Self> {
        // Try starting websocket server on ports 6463 - 6472
        for port in 6463..6472 {
            match simple_websockets::launch(port) {
                Ok(server) => {
                    info!("Server started on port {}", port);
                    return Ok(Self {
                        server: Arc::new(Mutex::new(Some(server))),
                        clients: Arc::new(Mutex::new(HashMap::new())),
                        event_sender,
                    });
                }
                Err(_) => {
                    warn!("Failed to start server on port {}", port);
                }
            }
        }

        error!("Failed to start server on any port");
        Err(io::Error::new(
            io::ErrorKind::AddrInUse,
            "websocket server: all ports 6463..6472 in use",
        ))
    }

    pub fn start(&mut self, set_activity: bool, secondary_events: bool) {
        let server = match self.server.lock() {
            Ok(mut guard) => match guard.take() {
                Some(server) => server,
                None => {
                    warn!("Server already started");
                    return;
                }
            },
            Err(err) => {
                error!("Server lock poisoned: {}", err);
                return;
            }
        };
        let clients = self.clients.clone();
        let event_sender = self.event_sender.clone();

        std::thread::spawn(move || {
            loop {
                trace!("Polling for events...");

                match server.poll_event() {
                    Event::Connect(client_id, responder) => {
                        let connection = responder.connection_details();
                        let url_params = get_url_params(&connection.uri);
                        let version = url_params.v.as_deref().unwrap_or("0");
                        let encoding = url_params.encoding.as_deref().unwrap_or("json");

                        info!("Client {} connected", client_id);

                        if version != "1" || encoding != "json" {
                            warn!("Invalid connection from client {}", client_id);
                            continue;
                        }

                        if !responder.send(Message::Text(CONNECTION_RESPONSE.to_string())) {
                            error!("Failed to send READY to client {}", client_id);
                            continue;
                        }

                        match clients.lock() {
                            Ok(mut guard) => {
                                guard.insert(client_id, (None, responder));
                            }
                            Err(err) => {
                                error!("Clients lock poisoned: {}", err);
                            }
                        }
                    }
                    Event::Disconnect(client_id) => {
                        info!("Client {} disconnected", client_id);
                        let removed = match clients.lock() {
                            Ok(mut guard) => guard.remove(&client_id),
                            Err(err) => {
                                error!("Clients lock poisoned: {}", err);
                                continue;
                            }
                        };

                        match removed {
                            Some(responder) => {
                                handle_disconnect(client_id, &event_sender, &responder)
                            }
                            None => warn!("Disconnect from unknown client {}, ignoring", client_id),
                        }
                    }
                    Event::Message(client_id, message) => {
                        debug!("Received message from client {}: {:?}", client_id, message);

                        let text = match message {
                            Message::Text(text) => text,
                            _ => continue,
                        };

                        // Snapshot the responder under a short lock, then work unlocked.
                        let responder = match clients.lock() {
                            Ok(guard) => guard.get(&client_id).map(|(_, r)| r.clone()),
                            Err(err) => {
                                error!("Clients lock poisoned: {}", err);
                                continue;
                            }
                        };
                        let responder = match responder {
                            Some(responder) => responder,
                            None => {
                                warn!("Message from unknown client {}, ignoring", client_id);
                                continue;
                            }
                        };

                        // Origin check before ANY parse (headers are free).
                        if let Some(origin) = responder.connection_details().headers.get("origin") {
                            let value = origin.to_str().unwrap_or_default();
                            if !VALID_ORIGINS.contains(&value) {
                                warn!("Invalid origin from client {}", client_id);
                                continue;
                            }
                        }

                        // Cheap reject: probe `{cmd}` before the full deserialize.
                        let probe = match serde_json::from_str::<CmdProbe>(&text) {
                            Ok(probe) => probe,
                            Err(err) => {
                                warn!("Invalid message from client {}: {}", client_id, err);
                                continue;
                            }
                        };
                        match probe.cmd.as_str() {
                            "INVITE_BROWSER" | "SET_ACTIVITY" => {}
                            "DEEP_LINK" => {
                                warn!("Deep link unimplemented. PRs are open!");
                                continue;
                            }
                            other => {
                                warn!("Unknown command: {}", other);
                                continue;
                            }
                        }

                        // If not ActivityCmd, ignore
                        let event: ActivityCmd = match serde_json::from_str(&text) {
                            Ok(event) => event,
                            Err(err) => {
                                warn!("Invalid message from client {}: {}", client_id, err);
                                continue;
                            }
                        };

                        match event.cmd.as_str() {
                            "INVITE_BROWSER" => {
                                if !secondary_events {
                                    continue;
                                }

                                handle_invite(&event, &event_sender, &responder)
                            }
                            "SET_ACTIVITY" => {
                                if !set_activity {
                                    continue;
                                }

                                handle_set_activity(&event, &event_sender, &clients, client_id)
                            }
                            // Re-checked after the full parse; the probe above already
                            // filtered anything else, so these are unreachable in practice.
                            "DEEP_LINK" => {
                                warn!("Deep link unimplemented. PRs are open!");
                            }
                            _ => {
                                warn!("Unknown command: {}", event.cmd);
                            }
                        }
                    }
                }
            }
        });
    }
}

fn handle_invite(
    event: &ActivityCmd,
    event_sender: &mpsc::Sender<ActivityCmd>,
    responder: &Responder,
) {
    // Wire shape of the echo `data`: `{ "code": "<invite code>",
    // "pid": "<sender pid>" }`, echoed straight back from `args` (absent
    // fields are omitted, never null).
    let mut data = HashMap::new();
    if let Some(args) = event.args.as_ref() {
        if let Some(code) = args.code.as_ref() {
            data.insert("code".to_string(), code.clone());
        }
        if let Some(pid) = args.pid {
            data.insert("pid".to_string(), pid.to_string());
        }
    }

    // Let's just assume this went well I don't care
    let response = ActivityCmd {
        application_id: event.application_id.clone(),
        cmd: event.cmd.clone(),
        args: None,
        data: Some(data),
        evt: None,
        nonce: event.nonce.clone(),
    };

    // Send the event away!
    if let Err(err) = event_sender.send(event.clone()) {
        error!("Failed to forward invite event: {}", err);
    }

    // Respond
    match serde_json::to_string(&response) {
        Ok(text) => {
            if !responder.send(Message::Text(text)) {
                error!("Failed to send invite response");
            }
        }
        Err(err) => error!("Failed to serialize invite response: {}", err),
    }
}

fn handle_set_activity(
    event: &ActivityCmd,
    event_sender: &mpsc::Sender<ActivityCmd>,
    clients: &Arc<Mutex<HashMap<u64, ActivityResponder>>>,
    client_id: u64,
) {
    // Record the last activity for the client under a short lock.
    match clients.lock() {
        Ok(mut guard) => {
            if let Some(responder) = guard.get_mut(&client_id) {
                responder.0 = Some(event.clone());
            } else {
                warn!(
                    "SET_ACTIVITY from unknown client {}, ignoring store",
                    client_id
                );
            }
        }
        Err(err) => error!("Clients lock poisoned: {}", err),
    }

    if let Err(err) = event_sender.send(event.clone()) {
        error!("Failed to forward activity event: {}", err);
    }
}

fn handle_disconnect(
    _client_id: u64,
    event_sender: &mpsc::Sender<ActivityCmd>,
    responder: &ActivityResponder,
) {
    if let Some(activity_cmd) = &responder.0 {
        // Send empty activity
        let pid = activity_cmd
            .args
            .as_ref()
            .and_then(|args| args.pid)
            .unwrap_or_default();
        let activity_cmd = ActivityCmd {
            application_id: activity_cmd.application_id.clone(),
            cmd: "SET_ACTIVITY".to_string(),
            data: None,
            evt: None,
            args: Some(ActivityCmdArgs {
                pid: Some(pid),
                activity: None,
                code: None,
            }),
            nonce: activity_cmd.nonce.clone(),
        };

        if let Err(err) = event_sender.send(activity_cmd) {
            error!("Failed to forward disconnect event: {}", err);
        }
    }
}
