use interprocess::local_socket::traits::Listener as _;
use interprocess::local_socket::{Listener, ListenerOptions, ToFsName};
use interprocess::os::windows::local_socket::{ListenerOptionsExt, NamedPipe};
use interprocess::os::windows::security_descriptor::SecurityDescriptor;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use crate::cmd::ActivityCmd;
use tracing::{error, info, warn};

use super::ipc_utils::{handle_stream, IpcFacilitator};

#[derive(Clone)]
pub struct IpcConnector {
    socket: Arc<Mutex<Listener>>,
    did_handshake: bool,
    pub client_id: String,
    pub pid: u64,
    pub nonce: String,

    event_sender: mpsc::Sender<ActivityCmd>,
}

impl IpcFacilitator for IpcConnector {
    fn handshake(&self) -> bool {
        self.did_handshake
    }

    fn set_handshake(&mut self, handshake: bool) {
        self.did_handshake = handshake;
    }

    fn client_id(&self) -> &str {
        &self.client_id
    }

    fn set_client_id(&mut self, client_id: String) {
        self.client_id = client_id;
    }

    fn pid(&self) -> u64 {
        self.pid
    }

    fn set_pid(&mut self, pid: u64) {
        self.pid = pid;
    }

    fn nonce(&self) -> &str {
        &self.nonce
    }

    fn set_nonce(&mut self, nonce: String) {
        self.nonce = nonce;
    }

    /**
     * Create a new thread that will recieve messages from the socket
     */
    fn start(&mut self) {
        let socket_arc = Arc::clone(&self.socket);
        let event_sender = self.event_sender.clone();

        std::thread::spawn(move || {
            loop {
                // Never hold the guard across the accept loop: lock only long
                // enough to accept one connection, mirroring the unix backend.
                // (`accept` borrows nothing, so the guard MUST be scoped to the
                // call by hand — the borrow checker cannot enforce it here.)
                let stream = {
                    let socket_guard = match socket_arc.lock() {
                        Ok(guard) => guard,
                        Err(poisoned) => {
                            warn!("[IPC] Listener lock poisoned, recovering");
                            poisoned.into_inner()
                        }
                    };
                    socket_guard.accept()
                };

                match stream {
                    Ok(mut stream) => {
                        info!("[IPC] Incoming stream...");

                        // Fresh per-connection state (see unix backend).
                        let mut conn = IpcConnector {
                            socket: Arc::clone(&socket_arc),
                            did_handshake: false,
                            client_id: String::new(),
                            pid: 0,
                            nonce: String::new(),
                            event_sender: event_sender.clone(),
                        };
                        std::thread::spawn(move || handle_stream(&mut conn, &mut stream));
                    }
                    Err(err) => {
                        error!("[IPC] Accept error: {}", err);
                        break;
                    }
                }
            }
        });
    }

    fn event_sender(&mut self) -> &mut mpsc::Sender<ActivityCmd> {
        &mut self.event_sender
    }
}

impl IpcConnector {
    /**
     * Create a socket and return a new IpcConnector.
     *
     * Fatal when no pipe can be bound (see unix backend).
     */
    pub fn new(event_sender: mpsc::Sender<ActivityCmd>) -> Self {
        let socket = match Self::create_socket(None) {
            Ok(bound) => bound,
            Err(err) => {
                error!("[IPC] Failed to create socket: {}, exiting", err);
                std::process::exit(1);
            }
        };

        Self {
            socket: Arc::new(Mutex::new(socket)),
            did_handshake: false,
            client_id: String::new(),
            pid: 0,
            nonce: String::new(),
            event_sender,
        }
    }

    /// See the unix backend: `io::Error`, never aborts.
    fn create_socket(tries: Option<u8>) -> std::io::Result<Listener> {
        // Define the path to the named pipe
        let pipe_path = r"\\.\pipe\discord-ipc";

        // Append tried number to name if applicable
        let tries = match tries {
            Some(tries) => tries,
            None => 0,
        };
        let pipe_path = format!("{pipe_path}-{tries}");

        let name = pipe_path
            .clone()
            .to_fs_name::<NamedPipe>()
            .map_err(|err| std::io::Error::new(err.kind(), format!("invalid pipe name: {err}")))?;
        let listener = ListenerOptions::new()
            .name(name)
            .security_descriptor(SecurityDescriptor::default());

        let socket = match listener.create_sync() {
            Ok(socket) => socket,
            Err(err) => {
                warn!("[IPC] Failed to create IPC socket: {}", err);

                if tries < 9 {
                    return Self::create_socket(Some(tries + 1));
                }
                return Err(err);
            }
        };

        info!("[IPC] Created IPC socket: {}", pipe_path);

        Ok(socket)
    }
}
