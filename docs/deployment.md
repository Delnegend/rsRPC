# Deployment & Operations

## Install

### Homebrew (Recommended)

```bash
brew install Delnegend/tap/rsrpc
```

For Windows games under Proton or Wine, install the bridge:

```bash
brew install Delnegend/tap/rsrpc-bridge
```

### Prebuilt Binary

Download the latest Linux release from [GitHub Releases](https://github.com/Delnegend/rsRPC/releases):

```bash
curl -fSLO https://github.com/Delnegend/rsRPC/releases/latest/download/rsrpc-x86_64-unknown-linux-gnu.zip
unzip rsrpc-*.zip
install -m 755 rsrpc-x86_64-unknown-linux-gnu ~/.local/bin/rsrpc
```

## Run as a Service

### Homebrew Services

```bash
brew services start rsrpc
tail -f /tmp/rsrpc.log
```

### Systemd User Service

Create `~/.config/systemd/user/rsrpc.service`:

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

Enable and start the service:

```bash
systemctl --user daemon-reload
systemctl --user enable --now rsrpc
```

## Flatpak Permissions

When running Discord or Vesktop through Flatpak, grant access to the host IPC socket:

```bash
flatpak override --user --filesystem=xdg-run/discord-ipc-0
```

## Upgrade

```bash
brew upgrade rsrpc
```
