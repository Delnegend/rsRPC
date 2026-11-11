# Development Guide

## Prerequisites

- **Rust:** 1.85+ stable (edition 2024 support)
- **just:** task runner for builds and verification
- **Docker / Podman:** optional, required only for cross-compiling the Windows Proton bridge

## Build & Test

```bash
# Build workspace binaries
cargo build

# Run CI gate checks (format, clippy, tests, docs)
just check

# Run all test suites
cargo test --workspace --all-targets
```

## Cross-Compiling Proton Bridge

The Windows Proton/Wine bridge executable compiles via container cross-compilation:

```bash
just bridge
```

Output binaries land in `dist/` (`rsrpc-bridge.exe` and `bridge.sh`).

## Contribution Workflow

1. Branch off `main`: `git checkout -b feat/my-feature`.
2. Ensure `just check` passes cleanly.
3. Open a PR with clear user outcome in the description.
