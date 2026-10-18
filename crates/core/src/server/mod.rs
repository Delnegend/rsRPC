pub(crate) mod client_connector;
pub(crate) mod gsi;
pub(crate) mod ipc_utils;
pub(crate) mod process;
pub(crate) mod utils;
pub(crate) mod websocket;

#[cfg(target_os = "windows")]
pub(crate) mod ipc_win;

#[cfg(not(target_os = "windows"))]
pub(crate) mod ipc_unix;
#[cfg(target_os = "windows")]
mod platform {
    pub(crate) use super::ipc_win as ipc;
}

#[cfg(not(target_os = "windows"))]
mod platform {
    pub(crate) use super::ipc_unix as ipc;
}

pub(crate) use platform::ipc;
