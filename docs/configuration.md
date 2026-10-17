# Configuration Reference

## CLI flags (`rsrpc-cli`)

| Flag | Default | Description |
|---|---|---|
| `-d, --detectable-file <file>` | Discord's list | Path to a custom `detectable.json` game list |
| `-n, --no-process-scan` | `false` | Skip the running-process scan; socket + websocket detection only |
| `-h, --help` | — | Print help |
| `-V, --version` | — | Print version |

With no flags the CLI downloads the game list from
`https://discord.com/api/v9/applications/detectable` (32 MiB cap) on every start.

With `--no-process-scan` it starts with an empty list (`[]`): only games that
connect over the socket or websocket are reported.

## Environment variables

| Variable | Default | Description |
|---|---|---|
| `RSRPC_LOGS_ENABLED` | `0` (`1` when run as the CLI binary) | Set to `1` to enable log lines |
| `XDG_RUNTIME_DIR` | — | First choice for the IPC socket directory on Linux |
| `TMPDIR` / `TMP` / `TEMP` | — | Fallback socket directories, in order |

## `RPCConfig` (library)

All fields default to on; the websocket port defaults to `1337`:

| Field | Type | Default | Description |
|---|---|---|---|
| `enable_process_scanner` | `bool` | `true` | Scan running processes against the detectable list |
| `enable_ipc_connector` | `bool` | `true` | Listen on the `discord-ipc-0` socket |
| `enable_websocket_connector` | `bool` | `true` | Listen for websocket clients on `port` |
| `enable_secondary_events` | `bool` | `true` | Also start the websocket connector for secondary events |
| `port` | `u16` | `1337` | TCP port for the client/websocket connector |

## Library API

```rust
use rsrpc::{RPCConfig, RPCServer};

fn main() {
  let mut server = RPCServer::from_file("./detectable.json", RPCConfig::default());
  // or: RPCServer::from_json_str(detectable_json, RPCConfig::default())
  server.start();
}
```

After `start()`:

| Method | Description |
|---|---|
| `append_detectables(vec)` | Add games to the detectable list while running |
| `remove_detectable_by_name(name)` | Remove a game by name while running |
| `scan_for_processes()` | Trigger a process scan immediately |

Before `start()`:

| Method | Description |
|---|---|
| `on_process_scan_complete(cb)` | Callback after each scan; `state.obs_open` is true when OBS/Streamlabs is running |

Worked examples live in [`examples/`](../examples/): `custom.rs` (add/remove
games at runtime) and `streamer_mode_detect.rs` (OBS detection callback).
