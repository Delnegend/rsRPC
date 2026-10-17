---
type: guide
title: Quickstart
description: Get up and running with rsRPC on Linux in under 60 seconds.
tags: [quickstart, guide, install, run]
verified:
  - by: openwiki/0.7.2
    at: 2026-10-17T15:02:42.014Z
sources:
  - id: openwiki-source-58783f5f62c54fb0c3ee36cf
    resource: repo://crates/cli/src/main.rs
generated: { by: "openwiki", at: "2026-10-17T15:02:42.014Z" }
---

# Quickstart

Get running with `rsrpc` on Linux in less than 60 seconds.

```mermaid
sequenceDiagram
    actor Player as Gamer
    participant Game as Running Game
    participant RSRPC as rsRPC Daemon
    participant Discord as Discord Client

    Player->>RSRPC: Launch rsrpc-cli
    RSRPC->>RSRPC: Bind discord-ipc-0 & Hub :1337
    Player->>Game: Launch game
    Game->>RSRPC: Connect to discord-ipc-0 (or scanned)
    RSRPC->>RSRPC: Middleware cleanup (threads & watermarks)
    RSRPC->>Discord: Forward activity payload
    Discord-->>Player: Rich Presence displays in profile
```
*End-to-end presence flow from game launch through rsRPC to the Discord client.*

## 1. Quick Launch (Pre-compiled Binary)

Download and extract the latest release binary:

```bash
# Download and unpack
curl -fSLO https://github.com/Delnegend/rsRPC/releases/latest/download/rsrpc-x86_64-unknown-linux-gnu.zip
unzip rsrpc-*.zip

# Start the daemon
./rsrpc-cli
```

Open your Discord client (Vesktop, Discord Web, or Discord Desktop) and launch a game. Your Rich Presence updates automatically.

Press `Ctrl+C` to stop the daemon cleanly.

## 2. Compile From Source

To build and run directly from this repository:

```bash
# Build the optimized release binary
cargo build -p rsrpc-cli --release

# Run
./target/release/rsrpc-cli
```

## 3. Windows Games on Steam (Proton)

If you play Windows games through Steam Play / Proton, enable the built-in bridge:

```bash
# 1. Build or copy bridge artifacts
just bridge
mkdir -p ~/.local/share/rpc-bridge
cp dist/rsrpc-bridge.exe dist/bridge.sh ~/.local/share/rpc-bridge/
chmod +x ~/.local/share/rpc-bridge/bridge.sh

# 2. Add to game launch options in Steam
~/.local/share/rpc-bridge/bridge.sh %command%
```

## 4. Documentation Map

| Area | Documentation Page | Purpose |
|---|---|---|
| **System Architecture** | [`/openwiki/architecture/overview.md`](architecture/overview.md) | Component design, event loops, connector hub, and invariants |
| **Activity Sanitization** | [`/openwiki/architecture/middleware.md`](architecture/middleware.md) | Middleware pipeline, thread-name filter, and mod watermark cleaner |
| **Wine / Proton Bridge** | [`/openwiki/integrations/proton-bridge.md`](integrations/proton-bridge.md) | Bridging Windows game named pipes to Linux sockets under Proton |
| **Configuration Options** | [`/openwiki/operations/configuration.md`](operations/configuration.md) | CLI flags, environment variables, and `RPCConfig` reference |
| **Production Deployment** | [`/openwiki/operations/deployment.md`](operations/deployment.md) | Systemd user service, logging, and operational health checks |
