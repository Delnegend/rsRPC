use interprocess::local_socket::traits::{Listener as _, Stream as _};
use interprocess::local_socket::{GenericFilePath, Listener, ListenerOptions, Stream, ToFsName};
use std::env;
use std::io::ErrorKind;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use crate::cmd::ActivityCmd;
use tracing::{debug, error, info, warn};

use super::ipc_utils::{handle_stream, IpcFacilitator};

/// First set env dir wins; falls back to `/tmp`.
fn env_dir(key: &str) -> String {
    env::var(key).unwrap_or_default()
}

pub(crate) fn get_socket_path() -> String {
    let xdg_runtime_dir = env_dir("XDG_RUNTIME_DIR");
    let tmpdir = env_dir("TMPDIR");
    let tmp = env_dir("TMP");
    let temp = env_dir("TEMP");
    let tmp_dir = if !xdg_runtime_dir.is_empty() {
        xdg_runtime_dir
    } else if !tmpdir.is_empty() {
        tmpdir
    } else if !tmp.is_empty() {
        tmp
    } else if !temp.is_empty() {
        temp
    } else {
        "/tmp".to_string()
    };

    // Append a / to the temp dir if it doesn't have one
    let tmp_dir = if tmp_dir.ends_with('/') {
        tmp_dir
    } else {
        format!("{tmp_dir}/")
    };

    format!("{tmp_dir}discord-ipc")
}

struct BoundListener {
    socket: Listener,
    path: String,
}

impl Drop for BoundListener {
    fn drop(&mut self) {
        debug!("Cleaning up socket: {}", self.path);
        let _ = std::fs::remove_file(&self.path);
    }
}

#[derive(Clone)]
pub struct IpcConnector {
    socket: Arc<Mutex<BoundListener>>,
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

        thread::spawn(move || {
            loop {
                // Scope the guard to the accept() call only: a blocking accept
                // holds the mutex while no connection is arriving, and any handler
                // thread that needs the listener must not stall behind it.
                let stream = {
                    let socket_guard = match socket_arc.lock() {
                        Ok(guard) => guard,
                        Err(poisoned) => {
                            warn!("Listener lock poisoned, recovering");
                            poisoned.into_inner()
                        }
                    };
                    socket_guard.socket.accept()
                };

                match stream {
                    Ok(mut stream) => {
                        info!("Incoming stream...");

                        // Fresh per-connection state: handshake/client/pid/nonce always
                        // start empty. Nothing leaks from a previous connection, and no
                        // snapshot of the parent's state is shared.
                        let mut conn = IpcConnector {
                            socket: Arc::clone(&socket_arc),
                            did_handshake: false,
                            client_id: String::new(),
                            pid: 0,
                            nonce: String::new(),
                            event_sender: event_sender.clone(),
                        };
                        thread::spawn(move || handle_stream(&mut conn, &mut stream));
                    }
                    Err(err) => {
                        error!("Accept error: {}", err);
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
     * Fatal when no socket can be bound: without a listener the bridge is
     * deaf, so exiting loudly beats serving nothing.
     */
    pub fn new(event_sender: mpsc::Sender<ActivityCmd>) -> Self {
        let (socket, path) = match Self::create_socket(None) {
            Ok(bound) => bound,
            Err(err) => {
                error!("Failed to create socket: {}, exiting", err);
                std::process::exit(1);
            }
        };

        Self {
            socket: Arc::new(Mutex::new(BoundListener { socket, path })),
            did_handshake: false,
            client_id: String::new(),
            pid: 0,
            nonce: String::new(),
            event_sender,
        }
    }

    /**
     * ACTUALLY create a socket, and return the handle.
     *
     * Returns an `io::Error` (NOT a domain error type) so this backend stays
     * independent of other slices' error enums; callers map it as needed.
     */
    fn create_socket(tries: Option<u8>) -> std::io::Result<(Listener, String)> {
        let socket_path = get_socket_path();
        let tries = tries.unwrap_or_default();
        let socket_path = format!("{socket_path}-{tries}");

        info!("Creating socket: {}", socket_path);

        // `to_fs_name` failures (bad path chars) surface as `io::Error` with a
        // descriptive message instead of aborting.
        let name = socket_path
            .clone()
            .to_fs_name::<GenericFilePath>()
            .map_err(|err| {
                std::io::Error::new(err.kind(), format!("invalid socket path: {err}"))
            })?;
        let listener_options = ListenerOptions::new().name(name.clone());

        let socket = match listener_options.create_sync() {
            Ok(socket) => socket,
            Err(err) => {
                if err.kind() == ErrorKind::AddrInUse {
                    info!(
                        "Socket {} already in use, checking if stale...",
                        socket_path
                    );
                    match Stream::connect(name) {
                        Ok(_) => {
                            warn!("Socket {} is in use by another process", socket_path);
                        }
                        Err(_) => {
                            info!("Socket {} is stale, removing and retrying...", socket_path);
                            let _ = std::fs::remove_file(&socket_path);
                            let name = socket_path
                                .clone()
                                .to_fs_name::<GenericFilePath>()
                                .map_err(|err| {
                                    std::io::Error::new(
                                        err.kind(),
                                        format!("invalid socket path: {err}"),
                                    )
                                })?;
                            let listener_options = ListenerOptions::new().name(name);
                            if let Ok(socket) = listener_options.create_sync() {
                                info!("Created IPC socket after cleaning stale: {}", socket_path);
                                return Ok((socket, socket_path));
                            }
                        }
                    }
                }

                warn!("Failed to create IPC socket: {}", err);

                if tries < 9 {
                    return Self::create_socket(Some(tries + 1));
                }
                return Err(err);
            }
        };

        info!("Created IPC socket: {}", socket_path);

        Ok((socket, socket_path))
    }
}
