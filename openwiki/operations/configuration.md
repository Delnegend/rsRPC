---
type: reference
title: Configuration Reference
description: Complete configuration reference for rsrpc command-line flags, environment variables, and RPCConfig options.
tags: [configuration, cli, environment-variables, rpcconfig, reference]
sources:
  - id: openwiki-source-58783f5f62c54fb0c3ee36cf
    resource: repo://crates/rsrpc/src/main.rs
  - id: openwiki-source-d81d37522b3385f0452a316e
    resource: repo://crates/core/src/lib.rs
  - id: openwiki-source-bae660b604dcb34e4ca17d93
    resource: repo://crates/core/src/server/ipc_unix.rs
  - id: openwiki-source-0da527745131e25b52757419
    resource: repo://crates/core/src/server/websocket.rs
generated: { by: "omp", at: "2026-10-18T06:59:24.615Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-18T06:59:24.615Z
---

# Configuration Reference

This reference covers runtime configuration options across the CLI binary (`rsrpc`), environment variables, and the `rsrpc-core` library interface.

## Command-Line Arguments (`rsrpc`)

Options accepted when invoking the daemon. The installed command, the unpacked
archive directory and the binary's own `--version` all read `rsrpc`:

| Option | Type | Default | Description |
|---|---|---|---|
| `-d, --detectable-file <PATH>` | Path | Remote Discord API | Path to a local JSON file formatted as Discord's `detectable.json` list. Overrides automatic download. |
| `-n, --no-process-scan` | Flag | `false` | Disables the background OS process polling scanner. Only socket and WebSocket game connections are processed. |

Example:

```bash
# Launch with custom detectable games list
rsrpc --detectable-file ./custom-detectable.json

# Launch in socket/websocket-only mode without scanning OS processes
rsrpc --no-process-scan
```

## Environment Variables

| Variable | Default | Purpose |
|---|---|---|
| `RUST_LOG` | `info` | Standard `tracing` filter directive (e.g. `debug`, `trace`, `rsrpc=debug`). |
| `RSRPC_LOGS_ENABLED` | `1` (CLI default) | Fallback toggle if `RUST_LOG` is unset. `0` disables log lines, `1` enables `info` level. |
| `RSRPC_WS_PORTS` | `6463-6472` | Overrides the websocket facade's candidate listen ports: a single port (`6463`) or an inclusive range (`6463-6472`). Malformed values, ports below `1024`, and reversed ranges fall back to the default. |
| `XDG_RUNTIME_DIR` | — | Primary directory for binding and discovering `discord-ipc-0` on Linux. |
| `TMPDIR` / `TMP` / `TEMP` | `/tmp` | Ordered fallback directories when `XDG_RUNTIME_DIR` is not set. |

## Library Configuration (`RPCConfig`)

When using `rsrpc-core` as an embedded crate, configuration is passed via `RPCConfig`:

```rust
use rsrpc_core::{RPCConfig, RPCServer, RpcError};

fn run() -> Result<(), RpcError> {
    let config = RPCConfig {
        enable_process_scanner: true,
        enable_ipc_connector: true,
        enable_websocket_connector: true,
        enable_secondary_events: true,
        enable_gsi: false,
        gsi_port: 3000,
        port: 1337,
    };

    let mut server = RPCServer::from_json_str("[]", config)?;
    server.on_process_scan_complete(|_state| {})?;
    server.start()?;

    Ok(())
}
```

### Start-up Contract

`start` is fallible and its ordering matters:

- Mutators that need a running server (such as `on_process_scan_complete`) fail with `RpcError::NotStarted` before `start` and with `RpcError::AlreadyStarted` after it.
- Calling `start` a second time returns `RpcError::AlreadyStarted`; connectors are never built twice.
- If the client connector cannot bind its port — typically because another instance already owns it — `start` returns `RpcError::Io` with an underlying `io::Error` of kind `AddrInUse`. Nothing is initialized, so the server remains unstarted and mutators still report `NotStarted`; a failed start never leaves a half-started daemon.
- Connector degradations are not errors: when no websocket port can be bound, `start` logs a warning and continues with a disabled facade, and the IPC connector comes up disabled when no socket can be bound.

### Fields

- **`enable_process_scanner` (bool, default: `true`):** Spawns the background Aho-Corasick process scanner thread.
- **`enable_ipc_connector` (bool, default: `true`):** Binds the local `discord-ipc-0` domain socket / named pipe.
- **`enable_websocket_connector` (bool, default: `true`):** Starts the local WebSocket listener on candidate ports `6463`–`6472` inclusive, overridable via `RSRPC_WS_PORTS`.
- **`enable_secondary_events` (bool, default: `true`):** Handles secondary protocol events such as `INVITE_BROWSER`.
- **`enable_gsi` (bool, default: `false`):** Starts an HTTP listener accepting Valve Game State Integration (GSI) telemetry payloads.
- **`gsi_port` (u16, default: `3000`):** TCP port for the GSI HTTP server.
- **`port` (u16, default: `1337`):** TCP port where Discord client applications connect to receive forwarded activities.
