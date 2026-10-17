# Deployment & Operations

## Install from a release

Each release ships zipped binaries per platform:

| Asset | Platform |
|---|---|
| `rsrpc-x86_64-unknown-linux-gnu.zip` | Linux x64 |
| `rsrpc-x86_64-pc-windows-msvc.zip` | Windows x64 |
| `rsrpc-aarch64-apple-darwin.zip` | macOS arm64 |

```bash
curl -fSLO https://github.com/Delnegend/rsRPC/releases/latest/download/rsrpc-x86_64-unknown-linux-gnu.zip
unzip rsrpc-*.zip
./rsrpc-cli --detectable-file ./detectable.json  # or no flags for Discord's list
```

## Systemd user service (Linux)

```ini
[Unit]
Description=rsRPC Discord Rich Presence server
After=network-online.target

[Service]
Type=simple
ExecStart=%h/.local/bin/rsrpc-cli
Restart=on-failure
RestartSec=5s

[Install]
WantedBy=default.target
```

Enable with `systemctl --user enable --now rsrpc`.

## Health checks

- **Logs:** run with `RSRPC_LOGS_ENABLED=1` and look for
  `Done! Watching for activity...`.
- **Socket:** `ls "$XDG_RUNTIME_DIR"/discord-ipc-0` exists while running.
- **Status:** launch a detectable game and confirm your Discord status updates.

## Windows games under Proton & Wine

To relay Discord Rich Presence from Windows games running through Steam Proton,
Lutris, or Wine to `rsrpc`, see the **[Proton & Wine Bridge Guide](proton-bridge.md)**.
