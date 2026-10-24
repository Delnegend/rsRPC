use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use serde::Serialize;
use serde_with::skip_serializing_none;
use simple_websockets::{Event, EventHub, Message, Responder};

use crate::cmd::{Activity, ActivityCmd, ActivityPayload};
use crate::detection::NULL_ACTIVITY_ID;
use crate::middleware::MiddlewarePipeline;
use tracing::{debug, error, info, warn};

use super::process::ProcessDetectedEvent;

fn empty_activity(pid: u64, socket_id: String) -> String {
    format!(
        r#"
    {{
      "activity": null,
      "pid": {pid},
      "socketId": "{socket_id}"
    }}
  "#
    )
}

#[derive(Clone)]
pub struct ClientConnector {
    pub port: u16,
    server: Arc<Mutex<Option<EventHub>>>,
    pub clients: Arc<Mutex<HashMap<u64, Responder>>>,
    data_on_connect: String,

    pub last_pid: Option<u64>,
    pub active_socket: Option<String>,
    pub last_payload: Arc<Mutex<Option<String>>>,
    pub middleware: MiddlewarePipeline,

    pub ipc_event_rec: Arc<Mutex<Option<std::sync::mpsc::Receiver<ActivityCmd>>>>,
    pub proc_event_rec: Arc<Mutex<Option<std::sync::mpsc::Receiver<ProcessDetectedEvent>>>>,
    pub ws_event_rec: Arc<Mutex<Option<std::sync::mpsc::Receiver<ActivityCmd>>>>,
}

#[skip_serializing_none]
#[derive(Serialize)]
struct ProcessActivity {
    pub application_id: String,
    pub name: String,
    pub timestamps: ProcessTimestamps,
    pub r#type: u32,
    pub metadata: HashMap<String, String>,
    pub flags: u32,
    pub details: Option<String>,
    pub state: Option<String>,
}

#[derive(Serialize)]
struct ProcessTimestamps {
    pub start: String,
}

#[derive(Serialize)]
struct ProcessPayload {
    pub activity: ProcessActivity,
    pub pid: u64,
    #[serde(rename = "socketId")]
    pub socket_id: String,
}

impl ClientConnector {
    /// Create a client connector listening on `port`.
    ///
    /// # Errors
    /// Returns an `io::Error` with kind `AddrInUse` when the websocket
    /// server cannot be launched (e.g. the port is already taken).
    pub fn new(
        port: u16,
        data_on_connect: String,
        ipc_event_rec: std::sync::mpsc::Receiver<ActivityCmd>,
        proc_event_rec: std::sync::mpsc::Receiver<ProcessDetectedEvent>,
        ws_event_rec: std::sync::mpsc::Receiver<ActivityCmd>,
    ) -> std::io::Result<ClientConnector> {
        let server = simple_websockets::launch(port).map_err(|_| {
            error!("Failed to launch websocket server, port may already be in use");
            std::io::Error::new(
                std::io::ErrorKind::AddrInUse,
                format!("client connector: port {} already in use", port),
            )
        })?;
        Ok(ClientConnector {
            server: Arc::new(Mutex::new(Some(server))),
            clients: Arc::new(Mutex::new(HashMap::new())),
            data_on_connect,
            port,

            last_pid: None,
            active_socket: None,
            last_payload: Arc::new(Mutex::new(None)),
            middleware: MiddlewarePipeline::new(),

            ipc_event_rec: Arc::new(Mutex::new(Some(ipc_event_rec))),
            proc_event_rec: Arc::new(Mutex::new(Some(proc_event_rec))),
            ws_event_rec: Arc::new(Mutex::new(Some(ws_event_rec))),
        })
    }

    pub fn start(&mut self) {
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
        let clients_clone = self.clients.clone();
        let data_on_connect = self.data_on_connect.clone();
        let last_payload_clone = self.last_payload.clone();

        std::thread::spawn(move || {
            loop {
                match server.poll_event() {
                    Event::Connect(client_id, responder) => {
                        info!("Client {} connected", client_id);

                        if !responder.send(Message::Text(data_on_connect.clone())) {
                            error!("Failed to send connect data to client {}", client_id);
                        }

                        if let Ok(guard) = last_payload_clone.lock()
                            && let Some(payload) = guard.as_ref()
                        {
                            info!("Resending cached activity payload to client {}", client_id);
                            if !responder.send(Message::Text(payload.clone())) {
                                error!("Failed to send cached payload to client {}", client_id);
                            }
                        }

                        match clients_clone.lock() {
                            Ok(mut guard) => {
                                guard.insert(client_id, responder);
                            }
                            Err(err) => error!("Clients lock poisoned: {}", err),
                        }
                    }
                    Event::Disconnect(client_id) => match clients_clone.lock() {
                        Ok(mut guard) => {
                            guard.remove(&client_id);
                        }
                        Err(err) => error!("Clients lock poisoned: {}", err),
                    },
                    Event::Message(client_id, message) => {
                        debug!("Received message from client {}: {:?}", client_id, message);
                        let responder = match clients_clone.lock() {
                            Ok(guard) => guard.get(&client_id).cloned(),
                            Err(err) => {
                                error!("Clients lock poisoned: {}", err);
                                continue;
                            }
                        };
                        if let Some(responder) = responder {
                            if !responder.send(message) {
                                error!("Failed to echo message to client {}", client_id);
                            }
                        } else {
                            warn!("Message from unknown client {}, ignoring", client_id);
                        }
                    }
                }
            }
        });

        let (ipc_event_rec, proc_event_rec, ws_event_rec) = match (
            self.ipc_event_rec.lock(),
            self.proc_event_rec.lock(),
            self.ws_event_rec.lock(),
        ) {
            (Ok(mut ipc), Ok(mut proc), Ok(mut ws)) => match (ipc.take(), proc.take(), ws.take()) {
                (Some(ipc), Some(proc), Some(ws)) => (ipc, proc, ws),
                _ => {
                    warn!("Receivers already taken");
                    return;
                }
            },
            _ => {
                error!("Receiver lock poisoned");
                return;
            }
        };

        let mut ipc_clone = self.clone();
        let mut proc_clone = self.clone();
        let mut ws_clone = self.clone();

        std::thread::spawn(move || {
            while let Ok(mut ipc_activity) = ipc_event_rec.recv() {
                ipc_activity.fix();

                let mut args = match ipc_activity.args {
                    Some(args) => args,
                    None => {
                        warn!("Invalid activity command, skipping");
                        continue;
                    }
                };

                if args.activity.is_none() {
                    let pid = args.pid.unwrap_or_default();
                    let payload = empty_activity(pid, pid.to_string());

                    info!("Sending empty payload");

                    ipc_clone.send_empty_payload(payload);

                    continue;
                }

                let activity = args.activity.as_mut();

                if let Some(activity) = activity {
                    activity.application_id = ipc_activity.application_id;
                    ipc_clone.middleware.process(activity);
                    let payload = ActivityPayload {
                        activity: Some(activity.clone()),
                        pid: args.pid,
                        socket_id: Some(args.pid.unwrap_or(0).to_string()),
                    };

                    match serde_json::to_string(&payload) {
                        Ok(payload) => {
                            info!("Sending payload for IPC activity: {:?}", payload);
                            ipc_clone.send_activity_payload(payload);
                        }
                        Err(err) => {
                            error!("Error serializing IPC activity: {}", err)
                        }
                    };
                } else {
                    warn!("Invalid activity command, skipping");
                }
            }
        });

        std::thread::spawn(move || {
            while let Ok(proc_event) = proc_event_rec.recv() {
                let proc_activity = proc_event.activity;

                if proc_activity.id == NULL_ACTIVITY_ID {
                    let active_socket = match proc_clone.active_socket.clone() {
                        Some(socket) => socket,
                        None => continue,
                    };

                    info!("Sending empty payload");

                    let payload =
                        empty_activity(proc_clone.last_pid.unwrap_or_default(), active_socket);

                    proc_clone.send_empty_payload(payload);

                    proc_clone.active_socket = None;

                    continue;
                }

                if proc_clone.active_socket != Some(proc_activity.id.clone()) {
                    if let Some(active_socket) = proc_clone.active_socket.clone() {
                        info!("Sending empty payload");

                        let payload =
                            empty_activity(proc_clone.last_pid.unwrap_or_default(), active_socket);

                        proc_clone.send_empty_payload(payload);
                    }
                } else {
                    debug!("Already sent payload for activity: {}", proc_activity.name);
                    continue;
                }

                let mut act = Activity {
                    details: proc_activity.details.clone(),
                    state: proc_activity.state.clone(),
                    ..Default::default()
                };
                proc_clone.middleware.process(&mut act);

                let payload_struct = ProcessPayload {
                    activity: ProcessActivity {
                        application_id: proc_activity.id.clone(),
                        name: proc_activity.name.clone(),
                        timestamps: ProcessTimestamps {
                            start: proc_activity
                                .timestamp
                                .as_ref()
                                .cloned()
                                .unwrap_or_else(|| "0".to_string()),
                        },
                        r#type: 0,
                        metadata: HashMap::new(),
                        flags: 0,
                        details: act.details,
                        state: act.state,
                    },
                    pid: proc_activity.pid.unwrap_or_default(),
                    socket_id: proc_activity.id.clone(),
                };

                proc_clone.last_pid = proc_activity.pid;
                proc_clone.active_socket = Some(proc_activity.id.clone());

                info!("Sending payload for activity: {}", proc_activity.name);

                match serde_json::to_string(&payload_struct) {
                    Ok(payload) => proc_clone.send_activity_payload(payload),
                    Err(err) => error!("Error serializing process activity: {}", err),
                }
            }
        });

        std::thread::spawn(move || {
            while let Ok(mut ws_event) = ws_event_rec.recv() {
                if ws_event.cmd != "SET_ACTIVITY" {
                    // Forward non-activity events directly.
                    let payload = serde_json::to_string(&ws_event).unwrap_or("".to_string());
                    info!("Sending payload for WS event");
                    ws_clone.send_data(payload);

                    continue;
                }

                ws_event.fix();

                let mut args = match ws_event.args {
                    Some(args) => args,
                    None => {
                        warn!("Invalid activity command, skipping");
                        continue;
                    }
                };

                if args.activity.is_none() {
                    let pid = args.pid.unwrap_or_default();
                    let payload = empty_activity(pid, pid.to_string());

                    info!("Sending empty payload");

                    ws_clone.send_empty_payload(payload);

                    continue;
                }

                let activity = args.activity.as_mut();

                if let Some(activity) = activity {
                    activity.application_id = ws_event.application_id;
                    ws_clone.middleware.process(activity);
                    let payload = ActivityPayload {
                        activity: Some(activity.clone()),
                        pid: args.pid,
                        socket_id: Some(args.pid.unwrap_or(0).to_string()),
                    };

                    match serde_json::to_string(&payload) {
                        Ok(payload) => {
                            info!("Sending payload for WS activity: {:?}", payload);
                            ws_clone.send_activity_payload(payload);
                        }
                        Err(err) => {
                            error!("Error serializing WS activity: {}", err)
                        }
                    };
                } else {
                    warn!("Invalid activity command, skipping");
                }
            }
        });
    }
    /// Send an active activity payload to all connected clients and cache it
    /// for newly connecting clients.
    pub fn send_activity_payload(&mut self, payload: String) {
        if let Ok(mut guard) = self.last_payload.lock() {
            *guard = Some(payload.clone());
        }
        self.send_data(payload);
    }

    /// Send an empty activity payload to clear presence and clear the cached
    /// activity payload.
    pub fn send_empty_payload(&mut self, payload: String) {
        if let Ok(mut guard) = self.last_payload.lock() {
            *guard = None;
        }
        self.send_data(payload);
    }

    /// Fan-out `data` to every connected client. Responders are snapshotted
    /// under a short lock and messaged after the guard is dropped so a slow
    /// reader can never hold the clients lock; a lone client gets `data`
    /// moved instead of cloned.
    pub fn send_data(&mut self, data: String) {
        let responders: Vec<Responder> = match self.clients.lock() {
            Ok(guard) => guard.values().cloned().collect(),
            Err(err) => {
                error!("Clients lock poisoned: {}", err);
                return;
            }
        };
        // Guard is dropped here; all sends below run unlocked.
        let mut iter = responders.into_iter();
        match (iter.next(), iter.next()) {
            (None, _) => {}
            (Some(only), None) => {
                if !only.send(Message::Text(data)) {
                    error!("Failed to send data to client");
                }
            }
            (Some(first), Some(second)) => {
                if !first.send(Message::Text(data.clone())) {
                    error!("Failed to send data to client");
                }
                if !second.send(Message::Text(data.clone())) {
                    error!("Failed to send data to client");
                }
                for responder in iter {
                    if !responder.send(Message::Text(data.clone())) {
                        error!("Failed to send data to client");
                    }
                }
            }
        }
    }
}

impl Drop for ClientConnector {
    fn drop(&mut self) {
        if let Ok(mut server) = self.server.lock() {
            drop(server.take());
        }
    }
}
