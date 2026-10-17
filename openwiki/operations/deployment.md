---
type: operations
title: Deployment & Operations
description: Deployment guide covering installation, systemd user service setup, Flatpak permissions, and operational health checks.
tags: [deployment, systemd, operations, service, flatpak]
verified:
  - by: openwiki/0.7.2
    at: 2026-10-17T15:02:42.014Z
sources:
  - id: openwiki-source-58783f5f62c54fb0c3ee36cf
    resource: repo://crates/cli/src/main.rs
  - id: openwiki-source-c59fe4336a371ea1052a01dd
    resource: repo://justfile
generated: { by: "openwiki", at: "2026-10-17T15:02:42.014Z" }
---

# Deployment & Operations

This guide describes deploying, managing, and monitoring `rsrpc` as a long-running daemon under Linux.

## Installation from Release

Every release publishes pre-compiled binaries:

```bash
# Download latest Linux release
curl -fSLO https://github.com/Delnegend/rsRPC/releases/latest/download/rsrpc-x86_64-unknown-linux-gnu.zip
unzip rsrpc-*.zip
install -m 755 rsrpc-cli ~/.local/bin/rsrpc-cli
```

## Systemd User Service (Linux)

Running `rsrpc` as a systemd user service ensures automatic startup upon login and automatic restarts on failure.

Create `~/.config/systemd/user/rsrpc.service`:

```ini
[Unit]
Description=rsRPC Discord Rich Presence daemon
After=network-online.target

[Service]
Type=simple
ExecStart=%h/.local/bin/rsrpc-cli
Restart=on-failure
RestartSec=5s
Environment=RUST_LOG=info

[Install]
WantedBy=default.target
```

Enable and start the service:

```bash
systemctl --user daemon-reload
systemctl --user enable --now rsrpc
```

Check service status and logs:

```bash
systemctl --user status rsrpc
journalctl --user -u rsrpc -f
```

## Flatpak Permissions

When running Discord, Vesktop, or Steam through Flatpak, the sandboxed apps must be granted access to the host's IPC socket:

```bash
# Allow Flatpak applications access to the Discord IPC socket
flatpak override --user --filesystem=xdg-run/discord-ipc-0
```

## Operational Health Checks

Verify that the daemon is functioning properly:

| Check | Command | Expected Healthy Output | Action if Failing |
|---|---|---|---|
| **Daemon Process** | `pgrep -a rsrpc-cli` | Process PID visible | Start service: `systemctl --user start rsrpc` |
| **IPC Socket** | `ls -l "$XDG_RUNTIME_DIR"/discord-ipc-0` | Socket file exists with read/write permissions | Check directory permissions or unbind stale files |
| **Client Connector** | `ss -tlnp \| grep 1337` | Listening on `127.0.0.1:1337` | Verify port 1337 is not taken by another process |
| **Tracing Logs** | `journalctl --user -u rsrpc -n 20` | `Done! Watching for activity...` | Check logs for bind or permission errors |
