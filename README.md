<div align="center">

# rsRPC

**Shows what game you're playing on Discord, for Linux setups where the official client can't.**

[![CI](https://img.shields.io/github/actions/workflow/status/Delnegend/rsRPC/ci.yml?branch=main&style=flat-square)](https://github.com/Delnegend/rsRPC/actions)
[![Release](https://img.shields.io/github/v/release/Delnegend/rsRPC?style=flat-square)](https://github.com/Delnegend/rsRPC/releases)
[![License](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](LICENSE)

</div>

---

## Quick Start

```bash
# 1. Install
brew install Delnegend/tap/rsrpc

# 2. Run it; it fetches Discord's game list automatically
rsrpc
```

Playing Windows games through Steam Play or Proton? Install the bridge too, then point the game's Steam launch options at it:

```bash
brew install Delnegend/tap/rsrpc-bridge
# Steam launch options: $(brew --prefix)/opt/rsrpc-bridge/libexec/bridge.sh %command%
```

## Highlights

- **One command to install**: `brew install Delnegend/tap/rsrpc`; the tap follows releases, so `brew upgrade` keeps you current.
- **Zero setup for most games**: downloads Discord's detectable-games list on start, no config file needed.
- **Catches games three ways**: running-process scan, local socket, and browser websocket, so odd launchers still show up.
- **Windows games too**: the `rsrpc-bridge` formula covers Proton and Wine games that never touch the host IPC socket.
- **Your list, your rules**: point it at your own `detectable.json`, or add and scan games while it runs.

## Documentation

- **[Architecture](docs/architecture.md)**: Core architecture, process scanner automaton, and client connector invariants.
- **[Development](docs/development.md)**: Building from source, running test suites, and cross-compiling the Proton bridge.
- **[Deployment](docs/deployment.md)**: Homebrew tap installation, systemd user service setup, and Flatpak socket permissions.

## License

[MIT](LICENSE)
