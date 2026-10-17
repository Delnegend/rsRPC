use detection::DetectableActivity;
use std::{
    path::Path,
    sync::{mpsc, Arc, Mutex, MutexGuard},
};

pub mod cmd;
pub mod detection;
pub mod logger;
pub mod plugins;
mod server;
pub mod url_params;
pub use tracing;

/// Callback invoked after each process scan completes.
///
/// Prefer the crate-root [`ProcessScanState`] re-export in new code.
pub type ProcessCallback = Box<dyn FnMut(ProcessScanState) + Send + Sync>;

pub use plugins::{
    GameEnrichment, GameStatusPlugin, LeagueOfLegendsPlugin, PluginRegistry, VrChatPlugin,
};
#[doc(hidden)]
pub use server::gsi::{gsi_payload_to_activity, GsiMap, GsiPayload, GsiServer, GsiTeam};
pub use server::ipc_utils::{encode, encode_into, PacketType, MAX_FRAME};
#[doc(hidden)]
pub use server::process::{ProcessEventListeners, ProcessScanState, ProcessServer};
pub use server::{
    client_connector::ClientConnector, ipc::IpcConnector, ipc_utils::IpcFacilitator,
    websocket::WebsocketConnector,
};
#[doc(hidden)]
pub use url_params::{get_url_params, UrlParams};

#[derive(Clone, Debug)]
pub struct RPCConfig {
    pub enable_process_scanner: bool,
    pub enable_ipc_connector: bool,
    pub enable_websocket_connector: bool,
    pub enable_secondary_events: bool,
    pub enable_gsi: bool,
    pub gsi_port: u16,
    pub port: u16,
}

impl Default for RPCConfig {
    fn default() -> Self {
        Self {
            enable_process_scanner: true,
            enable_ipc_connector: true,
            enable_websocket_connector: true,
            enable_secondary_events: true,
            enable_gsi: false,
            gsi_port: 3000,
            port: 1337,
        }
    }
}

/// Errors returned by [`RPCServer`] construction and runtime mutation.
///
/// Ordering contract: build the server with [`RPCServer::from_json_str`] or
/// [`RPCServer::from_file`], optionally register
/// [`RPCServer::on_process_scan_complete`], then call [`RPCServer::start`].
/// Mutators that need a running server fail with [`RpcError::NotStarted`]
/// before [`RPCServer::start`]; registering the scan callback after
/// [`RPCServer::start`] fails with [`RpcError::AlreadyStarted`].
///
/// ```
/// # fn main() -> Result<(), rsrpc::RpcError> {
/// use rsrpc::{RPCConfig, RPCServer, RpcError};
///
/// // Mutators before start() report NotStarted.
/// let mut server = RPCServer::from_json_str("[]", RPCConfig::default())?;
/// assert!(matches!(
///   server.append_detectables(vec![]),
///   Err(RpcError::NotStarted)
/// ));
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
#[non_exhaustive]
pub enum RpcError {
    /// Reading the detectable list (or another file) failed.
    Io(std::io::Error),
    /// The detectable list was not valid JSON.
    Json(serde_json::Error),
    /// The server has not been started yet.
    NotStarted,
    /// The server has already been started.
    AlreadyStarted,
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RpcError::Io(err) => write!(f, "RPC I/O error: {err}"),
            RpcError::Json(err) => write!(f, "RPC JSON error: {err}"),
            RpcError::NotStarted => write!(f, "RPC server has not been started"),
            RpcError::AlreadyStarted => write!(f, "RPC server has already been started"),
        }
    }
}

impl std::error::Error for RpcError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RpcError::Io(err) => Some(err),
            RpcError::Json(err) => Some(err),
            RpcError::NotStarted | RpcError::AlreadyStarted => None,
        }
    }
}

impl From<std::io::Error> for RpcError {
    fn from(err: std::io::Error) -> Self {
        RpcError::Io(err)
    }
}

impl From<serde_json::Error> for RpcError {
    fn from(err: serde_json::Error) -> Self {
        RpcError::Json(err)
    }
}

#[derive(Clone)]
struct Connectors {
    process_server: Arc<Mutex<ProcessServer>>,
    client_connector: Arc<Mutex<ClientConnector>>,
    ipc_connector: Arc<Mutex<IpcConnector>>,
    ws_connector: Arc<Mutex<WebsocketConnector>>,
}

pub struct RPCServer {
    detectable: Arc<Mutex<Vec<Arc<DetectableActivity>>>>,
    connectors: Option<Connectors>,
    config: RPCConfig,

    on_process_scan_complete: Option<Arc<Mutex<ProcessCallback>>>,
}

impl RPCServer {
    /// Create a new `RPCServer` from a JSON detectable-games list.
    ///
    /// # Errors
    ///
    /// Returns [`RpcError::Json`] when `detectable` is not valid JSON.
    ///
    /// ```
    /// # fn main() -> Result<(), rsrpc::RpcError> {
    /// use rsrpc::{RPCConfig, RPCServer};
    ///
    /// let server = RPCServer::from_json_str("[]", RPCConfig::default())?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn from_json_str(detectable: impl AsRef<str>, config: RPCConfig) -> Result<Self, RpcError> {
        let detectable: Vec<DetectableActivity> = serde_json::from_str(detectable.as_ref())?;
        let detectable: Vec<Arc<DetectableActivity>> =
            detectable.into_iter().map(Arc::new).collect();

        Ok(Self {
            detectable: Arc::new(Mutex::new(detectable)),

            // Default to empty servers
            connectors: None,
            config,

            // Event listeners
            on_process_scan_complete: None,
        })
    }

    /// Create a new `RPCServer` and read the detectable games list from file.
    ///
    /// # Errors
    ///
    /// Returns [`RpcError::Io`] when the file cannot be read and
    /// [`RpcError::Json`] when its contents are not valid JSON.
    ///
    /// ```no_run
    /// # fn main() -> Result<(), rsrpc::RpcError> {
    /// use rsrpc::{RPCConfig, RPCServer};
    ///
    /// let server = RPCServer::from_file("./detectable.json", RPCConfig::default())?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn from_file(file: impl AsRef<Path>, config: RPCConfig) -> Result<Self, RpcError> {
        let detectable = std::fs::read_to_string(file.as_ref())?;
        Self::from_json_str(detectable.as_str(), config)
    }

    /// Add new detectable processes on-the-fly. This should be run AFTER start().
    ///
    /// # Errors
    ///
    /// Returns [`RpcError::NotStarted`] when called before [`RPCServer::start`].
    ///
    /// ```
    /// # fn main() -> Result<(), rsrpc::RpcError> {
    /// use rsrpc::{RPCConfig, RPCServer, RpcError};
    ///
    /// let mut server = RPCServer::from_json_str("[]", RPCConfig::default())?;
    /// assert!(matches!(
    ///   server.append_detectables(vec![]),
    ///   Err(RpcError::NotStarted)
    /// ));
    /// # Ok(())
    /// # }
    /// ```
    pub fn append_detectables(
        &mut self,
        detectable: Vec<DetectableActivity>,
    ) -> Result<(), RpcError> {
        let connectors = self.connectors()?;
        lock_or_poison(&connectors.process_server)?.append_detectables(detectable);
        Ok(())
    }

    /// Remove a detectable process by name.
    ///
    /// # Errors
    ///
    /// Returns [`RpcError::NotStarted`] when called before [`RPCServer::start`].
    ///
    /// ```
    /// # fn main() -> Result<(), rsrpc::RpcError> {
    /// use rsrpc::{RPCConfig, RPCServer, RpcError};
    ///
    /// let mut server = RPCServer::from_json_str("[]", RPCConfig::default())?;
    /// assert!(matches!(
    ///   server.remove_detectable_by_name("Some Game"),
    ///   Err(RpcError::NotStarted)
    /// ));
    /// # Ok(())
    /// # }
    /// ```
    pub fn remove_detectable_by_name(&mut self, name: impl Into<String>) -> Result<(), RpcError> {
        let connectors = self.connectors()?;
        lock_or_poison(&connectors.process_server)?.remove_detectable_by_name(name.into());
        Ok(())
    }

    /// Manually trigger a scan for processes. This should be run AFTER start().
    ///
    /// # Errors
    ///
    /// Returns [`RpcError::NotStarted`] when called before [`RPCServer::start`].
    ///
    /// ```
    /// # fn main() -> Result<(), rsrpc::RpcError> {
    /// use rsrpc::{RPCConfig, RPCServer, RpcError};
    ///
    /// let mut server = RPCServer::from_json_str("[]", RPCConfig::default())?;
    /// assert!(matches!(
    ///   server.scan_for_processes(),
    ///   Err(RpcError::NotStarted)
    /// ));
    /// # Ok(())
    /// # }
    /// ```
    pub fn scan_for_processes(&mut self) -> Result<(), RpcError> {
        let connectors = self.connectors()?;
        let process_server = lock_or_poison(&connectors.process_server)?;

        if let Err(err) = process_server.scan_for_processes() {
            tracing::error!("[RPC Server] Error while scanning processes: {}", err);
        }

        Ok(())
    }

    /// Register the callback invoked after each process scan.
    /// Must be called BEFORE [`RPCServer::start`].
    ///
    /// # Errors
    ///
    /// Returns [`RpcError::AlreadyStarted`] when called after
    /// [`RPCServer::start`].
    ///
    /// ```
    /// # fn main() -> Result<(), rsrpc::RpcError> {
    /// use rsrpc::{RPCConfig, RPCServer};
    ///
    /// let mut server = RPCServer::from_json_str("[]", RPCConfig::default())?;
    /// server.on_process_scan_complete(|_state| {})?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn on_process_scan_complete(
        &mut self,
        callback: impl FnMut(ProcessScanState) + Send + Sync + 'static,
    ) -> Result<(), RpcError> {
        if self.connectors.is_some() {
            tracing::warn!("[RPC Server] Cannot set on_process_scan_complete, connectors are already initialized");
            return Err(RpcError::AlreadyStarted);
        }

        self.on_process_scan_complete = Some(Arc::new(Mutex::new(Box::new(callback))));
        Ok(())
    }

    fn connectors(&mut self) -> Result<&mut Connectors, RpcError> {
        if self.connectors.is_none() {
            tracing::error!("[RPC Server] Connectors are not initialized, call start() first");
            return Err(RpcError::NotStarted);
        }

        if let Some(connectors) = self.connectors.as_mut() {
            Ok(connectors)
        } else {
            Err(RpcError::NotStarted)
        }
    }

    /// Start the process detector, client connector, and related threads.
    ///
    /// A poisoned internal lock is recovered instead of aborting; the
    /// recovered state is used as-is.
    ///
    /// ```
    /// # fn main() -> Result<(), rsrpc::RpcError> {
    /// use rsrpc::{RPCConfig, RPCServer};
    ///
    /// let mut server = RPCServer::from_json_str("[]", RPCConfig::default())?;
    /// server.on_process_scan_complete(|_state| {})?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn start(&mut self) {
        let (proc_event_sender, proc_event_receiver) = mpsc::channel();
        let (ipc_event_sender, ipc_event_receiver) = mpsc::channel();
        let (ws_event_sender, ws_event_reciever) = mpsc::channel();

        let detectable = match lock_or_poison(&self.detectable) {
            Ok(guard) => guard.to_vec(),
            Err(_) => return,
        };

        let client_connector = match ClientConnector::new(
            self.config.port,
            server::utils::CONNECTION_RESPONSE.to_string(),
            ipc_event_receiver,
            proc_event_receiver,
            ws_event_reciever,
        ) {
            Ok(connector) => connector,
            Err(err) => {
                tracing::error!("[RPC Server] Client connector failed: {}", err);
                return;
            }
        };
        let ws_connector = match WebsocketConnector::new(ws_event_sender) {
            Ok(connector) => connector,
            Err(err) => {
                tracing::error!("[RPC Server] Websocket connector failed: {}", err);
                return;
            }
        };
        let connectors = Connectors {
            process_server: Arc::new(Mutex::new(ProcessServer::new(
                detectable,
                proc_event_sender,
                ProcessEventListeners {
                    on_process_scan_complete: self.on_process_scan_complete.clone(),
                },
            ))),
            client_connector: Arc::new(Mutex::new(client_connector)),
            ipc_connector: Arc::new(Mutex::new(IpcConnector::new(ipc_event_sender.clone()))),
            ws_connector: Arc::new(Mutex::new(ws_connector)),
        };

        let port = match lock_or_poison(&connectors.client_connector) {
            Ok(guard) => guard.port,
            Err(_) => return,
        };
        tracing::info!("[RPC Server] Starting client connector on port {}...", port);
        if let Ok(mut guard) = lock_or_poison(&connectors.client_connector) {
            guard.start();
        }

        let config = self.config.clone();

        if config.enable_ipc_connector {
            tracing::info!("[RPC Server] Starting IPC connector...");
            if let Ok(mut guard) = lock_or_poison(&connectors.ipc_connector) {
                guard.start();
            }
        }

        if config.enable_process_scanner {
            tracing::info!("[RPC Server] Starting process server...");
            if let Ok(guard) = lock_or_poison(&connectors.process_server) {
                guard.start();
            }
        }

        if config.enable_websocket_connector || config.enable_secondary_events {
            tracing::info!("[RPC Server] Starting websocket connector...");
            if let Ok(mut guard) = lock_or_poison(&connectors.ws_connector) {
                guard.start(
                    config.enable_websocket_connector,
                    config.enable_secondary_events,
                );
            }
        }

        if config.enable_gsi {
            tracing::info!(
                "[RPC Server] Starting GSI listener on port {}...",
                config.gsi_port
            );
            let gsi_server = server::gsi::GsiServer::new(config.gsi_port);
            if let Err(err) = gsi_server.start(ipc_event_sender) {
                tracing::error!("[RPC Server] Failed to start GSI listener: {}", err);
            }
        }

        tracing::info!("[RPC Server] Done! Watching for activity...");
        self.connectors = Some(connectors);
    }
}

/// Lock a mutex, recovering the inner value when a previous holder failed
/// while holding it.
///
/// The poisoned state is logged and recovered via `into_inner`;
/// this function always returns `Ok`.
fn lock_or_poison<T>(mutex: &Mutex<T>) -> Result<MutexGuard<'_, T>, RpcError> {
    match mutex.lock() {
        Ok(guard) => Ok(guard),
        Err(poisoned) => {
            tracing::warn!("[RPC Server] Recovering from poisoned mutex");
            Ok(poisoned.into_inner())
        }
    }
}
