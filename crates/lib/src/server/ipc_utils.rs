use std::{
    io::{BufReader, Read, Write},
    sync::{mpsc, LazyLock},
};

use interprocess::local_socket::Stream;
use serde_json::Value;

use crate::{
    cmd::{ActivityCmd, ActivityCmdArgs},
    server::utils,
};
use tracing::{debug, error, info, warn};

/// Hard cap on a single IPC frame body. Anything larger is rejected before
/// allocation (length check happens on the 4-byte header), so a malicious
/// peer cannot OOM the server by announcing a gigabyte payload.
pub const MAX_FRAME: u32 = 1024 * 1024;

/// Pre-encoded handshake reply. Built once at startup; every handshake
/// writes these exact bytes. Tests assert the wire bytes contain `"READY"`.
static READY_FRAME: LazyLock<Vec<u8>> =
    LazyLock::new(|| encode(PacketType::Frame, utils::CONNECTION_RESPONSE));

pub trait IpcFacilitator {
    fn handshake(&self) -> bool;
    fn set_handshake(&mut self, handshake: bool);

    fn client_id(&self) -> &str;
    fn set_client_id(&mut self, client_id: String);

    fn pid(&self) -> u64;
    fn set_pid(&mut self, pid: u64);

    fn nonce(&self) -> &str;
    fn set_nonce(&mut self, nonce: String);

    fn start(&mut self);

    fn event_sender(&mut self) -> &mut mpsc::Sender<ActivityCmd>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PacketType {
    Handshake,
    Frame,
    Close,
    Ping,
    Pong,
}

impl PacketType {
    /// Maps a wire opcode to its packet type.
    ///
    /// # Errors
    ///
    /// Returns `None` for unknown opcodes; callers MUST log and skip the
    /// frame rather than mis-parsing it as another type.
    pub fn from_u32(value: u32) -> Option<Self> {
        match value {
            0 => Some(PacketType::Handshake),
            1 => Some(PacketType::Frame),
            2 => Some(PacketType::Close),
            3 => Some(PacketType::Ping),
            4 => Some(PacketType::Pong),
            _ => None,
        }
    }
}

#[derive(serde::Deserialize, serde::Serialize)]
pub(crate) struct Handshake {
    pub(crate) v: u32,
    pub(crate) client_id: String,
}

pub fn encode(r_type: PacketType, data: &str) -> Vec<u8> {
    let mut buffer: Vec<u8> = Vec::with_capacity(8 + data.len());

    // Write the packet type
    buffer.extend_from_slice(&(r_type as u32).to_le_bytes());

    // Write the data size
    buffer.extend_from_slice(&(data.len() as u32).to_le_bytes());

    // Write the data
    buffer.extend_from_slice(data.as_bytes());

    buffer
}

/// Appends an already-encoded frame (`opcode`, `body`) to `out` without
/// re-serializing `body`. Used for the pre-encoded READY reply and the
/// message echo path, where the payload bytes are already on hand.
pub fn encode_into(out: &mut Vec<u8>, opcode: u32, body: &[u8]) {
    out.extend_from_slice(&opcode.to_le_bytes());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(body);
}

#[allow(clippy::result_large_err)]
pub(crate) fn send_empty(
    event_sender: &mut mpsc::Sender<ActivityCmd>,
    pid: u64,
) -> Result<(), mpsc::SendError<ActivityCmd>> {
    info!("[IPC] Sending empty activity");

    let activity = ActivityCmd {
        args: Some(ActivityCmdArgs {
            activity: None,
            code: None,
            pid: Some(pid),
        }),
        ..ActivityCmd::empty()
    };
    event_sender.send(activity)
}

fn send_empty_log<T: IpcFacilitator>(ipc: &mut T, pid: u64) {
    if let Err(err) = send_empty(ipc.event_sender(), pid) {
        error!("[IPC] Error sending empty activity: {}", err);
    }
}

/// Reads one frame header (opcode + length). Returns `None` when the peer
/// went away; the caller MUST `send_empty` and break.
fn read_header(reader: &mut BufReader<&mut Stream>) -> Option<(u32, u32)> {
    let mut header = [0u8; 8];
    match reader.read_exact(&mut header) {
        Ok(()) => (),
        Err(err) => {
            debug!(
                "[IPC] Error reading frame header, socket likely closed: {}",
                err
            );
            return None;
        }
    }
    let opcode = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
    let len = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
    Some((opcode, len))
}

pub(crate) fn handle_stream<T: IpcFacilitator>(ipc: &mut T, stream: &mut Stream) {
    // Write helper: pushes bytes through the buffered reader's inner stream.
    // (`BufReader` buffers only reads, so writing via `get_mut` never loses
    // bytes; this also keeps ONE mutable borrow of `stream` for the loop.)
    fn write_frame(reader: &mut BufReader<&mut Stream>, bytes: &[u8], what: &str) {
        if let Err(err) = reader.get_mut().write_all(bytes) {
            error!("[IPC] Error sending {}: {}", what, err);
        }
    }

    // One buffered reader for the whole connection (hoisted out of the loop:
    // the old code rebuilt a BufReader per frame, discarding readahead).
    //
    // NOTE on stalls: `interprocess::local_socket::Stream` exposes no read
    // timeout (only `set_nonblocking`), so a peer that connects and then
    // sends nothing blocks this thread in `read_exact` by design. That is the
    // whole timeout story: each connection gets its own handler thread, and
    // when the peer closes the socket the blocked read returns `Err`, the
    // loop breaks, and the thread exits. A stalled peer therefore costs one
    // parked thread — never the listener — and leaves no residue behind.
    // Oversize frames are still rejected up front via the `MAX_FRAME` length
    // check below, and frame bodies are parsed from bytes (`from_slice`), so
    // no unbounded allocation or UTF-8 pre-validation cost exists either.
    let mut reader = BufReader::new(&mut *stream);
    // Body buffer reused across frames; capacity only grows to the largest
    // frame seen on this connection.
    let mut body: Vec<u8> = Vec::new();
    loop {
        let current_pid = ipc.pid();

        let (opcode, len) = match read_header(&mut reader) {
            Some(header) => header,
            None => {
                send_empty_log(ipc, current_pid);
                break;
            }
        };

        if len > MAX_FRAME {
            warn!(
                "[IPC] Frame length {} exceeds cap {}, closing connection",
                len, MAX_FRAME
            );
            send_empty_log(ipc, current_pid);
            break;
        }

        let Some(r_type) = PacketType::from_u32(opcode) else {
            warn!("[IPC] Unknown opcode {}, skipping frame body", opcode);
            // Drain the body (bounded by the MAX_FRAME check above) so the next
            // read starts on a frame boundary, then wait for the next frame.
            body.resize(len as usize, 0);
            if reader.read_exact(&mut body).is_err() {
                send_empty_log(ipc, current_pid);
                break;
            }
            continue;
        };

        body.clear();
        body.resize(len as usize, 0);
        if let Err(err) = reader.read_exact(&mut body) {
            debug!("[IPC] Error reading frame body: {}", err);
            send_empty_log(ipc, current_pid);
            break;
        }

        debug!("[IPC] Recieved frame, opcode {}", opcode);

        match r_type {
            PacketType::Handshake => {
                debug!("[IPC] Recieved handshake");
                let Ok(data) = serde_json::from_slice::<Handshake>(&body) else {
                    warn!("[IPC] Error parsing handshake");
                    continue;
                };

                if data.v != 1 {
                    warn!("[IPC] Invalid version: {}", data.v);
                    continue;
                }

                ipc.set_handshake(true);
                ipc.set_client_id(data.client_id);

                // Send CONNECTION_RESPONSE (pre-encoded once at startup)
                write_frame(&mut reader, &READY_FRAME, "connection response");
            }
            PacketType::Frame => {
                if !ipc.handshake() {
                    warn!("[IPC] Did not handshake yet, ignoring frame");
                    continue;
                }

                let mut activity_cmd = match serde_json::from_slice::<ActivityCmd>(&body) {
                    Ok(cmd) => cmd,
                    Err(err) => {
                        error!("[IPC] Error parsing activity command: {}", err);
                        continue;
                    }
                };

                let args = match &activity_cmd.args {
                    Some(args) => args,
                    None => {
                        warn!("[IPC] Invalid activity command, skipping");

                        // Send empty activity
                        send_empty_log(ipc, current_pid);
                        continue;
                    }
                };

                let app_id = ipc.client_id().to_string();
                activity_cmd.application_id = Some(app_id);

                ipc.set_pid(args.pid.unwrap_or_default());
                ipc.set_nonce(activity_cmd.nonce_str().into_owned());

                if let Err(err) = ipc.event_sender().send(activity_cmd) {
                    error!("[IPC] Error sending activity command: {}", err);
                }

                // "IPC will echo back every command you send as a response.
                //  Use this as a lock-step feature to avoid flooding messages.
                //  Can be used to validate messages such as the Presence or Subscribes."
                // XXX: arRPC does some editing of the message, setting data.name: "", data.type: 0, evt: null etc... why?
                let mut resp = Vec::with_capacity(8 + body.len());
                encode_into(&mut resp, PacketType::Frame as u32, &body);

                write_frame(&mut reader, &resp, "connection response");
            }
            PacketType::Close => {
                info!("[IPC] Recieved close");

                // Send message with an empty activity
                let activity_cmd = ActivityCmd {
                    application_id: Some(ipc.client_id().to_string()),
                    cmd: "SET_ACTIVITY".to_string(),
                    data: None,
                    evt: None,
                    args: Some(ActivityCmdArgs {
                        pid: Some(ipc.pid()),
                        activity: None,
                        code: None,
                    }),
                    nonce: Value::String(ipc.nonce().to_string()),
                };

                if let Err(err) = ipc.event_sender().send(activity_cmd) {
                    error!("[IPC] Error sending activity command: {}", err);
                }

                // Reset per-connection state and exit; the listener keeps accepting
                // new connections on the same socket (no listener churn).
                ipc.set_handshake(false);
                ipc.set_client_id(String::new());
                ipc.set_pid(0);

                break;
            }
            PacketType::Ping => {
                debug!("[IPC] Recieved ping");

                // Send a pong
                let mut resp = Vec::with_capacity(8 + body.len());
                encode_into(&mut resp, PacketType::Pong as u32, &body);

                write_frame(&mut reader, &resp, "pong");
            }
            PacketType::Pong => {
                debug!("[IPC] Recieved pong");
            }
        }
    }
}
