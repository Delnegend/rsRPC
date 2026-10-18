---
type: operations
title: Deployment & Operations
description: Deployment guide covering installation, systemd user service setup, Flatpak permissions, and operational health checks.
tags: [deployment, systemd, operations, service, flatpak]
sources:
  - id: openwiki-source-58783f5f62c54fb0c3ee36cf
    resource: repo://crates/cli/src/main.rs
  - id: openwiki-source-c59fe4336a371ea1052a01dd
    resource: repo://justfile
  - id: openwiki-source-23775c3de52f3ab95a13cb8b
    resource: repo://README.md
generated: { by: "omp", at: "2026-10-18T06:59:24.615Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-18T07:08:32.938Z
---

# Deployment & Operations

This guide describes deploying, managing, and monitoring `rsrpc` as a long-running daemon under Linux.

## Installation

Homebrew is the primary install path. The tap installs the prebuilt release
binary where one is published (Linux x86_64, macOS arm64) and builds from the
release tag elsewhere, and a scheduled bot bumps it on every release:

```bash
brew install Delnegend/tap/rsrpc
rsrpc --version
```

Playing Proton or Wine games? Add the bridge formula in the same command:

```bash
brew install Delnegend/tap/rsrpc-bridge
```

### Installing from a Release Instead

Every release also publishes pre-compiled binaries, which is the route to take
without Homebrew:

```bash
# Download latest Linux release
curl -fSLO https://github.com/Delnegend/rsRPC/releases/latest/download/rsrpc-x86_64-unknown-linux-gnu.zip
unzip rsrpc-*.zip
install -m 755 rsrpc-x86_64-unknown-linux-gnu ~/.local/bin/rsrpc
```

## Running as a Background Service

The formula declares a service, so Homebrew manages the daemon for you:

```bash
brew services start rsrpc     # start now and at every login
brew services stop rsrpc      # stop
brew services restart rsrpc   # reload after a config change
brew services list            # show status
```

On Linux this writes a systemd user unit (`sh.brew.rsrpc.service`); on macOS it
registers a launchd job. Because the unit is generated from the formula, it
survives `brew upgrade`: the `ExecStart` points at `$(brew --prefix)/opt/rsrpc`,
which the upgrade re-points, rather than at a versioned keg path.

The unit restarts the daemon on failure with a 5-second delay, so a crash is
recovered from automatically. Note that a port-1337 conflict is fatal by design
and exits non-zero — under `Restart=on-failure` that means the unit retries
every 5 seconds until the conflict clears, so check the log if the service will
not stay up.

Logs are appended to files rather than the journal:

```bash
tail -f /tmp/rsrpc.log
tail -f /tmp/rsrpc.error.log
```

### Configuring the Service

Override environment variables without editing the formula by dropping a file at
`$HOMEBREW_USER_CONFIG_HOME/services/rsrpc.env`, which defaults to
`~/.homebrew/services/rsrpc.env`:

```bash
# Example: turn up logging
echo "RUST_LOG=debug" >> ~/.homebrew/services/rsrpc.env
brew services restart rsrpc
```

`RUST_LOG` is the usual knob; the CLI reads it ahead of `RSRPC_LOGS_ENABLED`. If
you export `HOMEBREW_USER_CONFIG_HOME`, the file moves with it.

### Managing It by Hand Instead

If you would rather own the unit yourself — or you installed a release binary
outside Homebrew — write `~/.config/systemd/user/rsrpc.service`:

```ini
[Unit]
Description=rsRPC Discord Rich Presence daemon
After=network-online.target

[Service]
Type=simple
ExecStart=%h/.local/bin/rsrpc
Restart=on-failure
RestartSec=5s
Environment=RUST_LOG=info

[Install]
WantedBy=default.target
```

Use the absolute path of whichever binary you installed; a bare `ExecStart=rsrpc`
only works if that path is on the service manager's `PATH`. Then:

```bash
systemctl --user daemon-reload
systemctl --user enable --now rsrpc
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
| **Daemon Process** | `pgrep -a rsrpc` | Process PID visible | Start service: `systemctl --user start rsrpc` |
| **IPC Socket** | `ls -l "$XDG_RUNTIME_DIR"/discord-ipc-0` | Socket file exists with read/write permissions | Check directory permissions or unbind stale files |
| **Client Connector** | `ss -tlnp \| grep 1337` | Listening on `127.0.0.1:1337` | Verify port 1337 is not taken by another process |
| **Tracing Logs** | `journalctl --user -u rsrpc -n 20` | `Done! Watching for activity...` | Check logs for bind or permission errors |

A service that starts and then immediately exits is usually the client
connector failing to bind port 1337, because that is the one connector whose
bind failure aborts startup: the daemon logs the error and exits non-zero
instead of lingering as a process that serves nothing. Look for a
`Client connector failed` line in the journal.
