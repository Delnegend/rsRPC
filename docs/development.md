# Development Guide

## Prerequisites

- A container runtime (Docker or Podman) — open the repo in the dev container
  and everything below is pre-installed.
- Without containers: Rust stable ≥1.100 (the 14-day dependency quarantine in
  `.cargo/config.toml` is ignored on older toolchains) plus `just`.

## Build & test

```bash
# Compile the CLI (release flags: LTO, stripped, panic=abort)
cargo build -p rsrpc-cli --release

# Run the full gate: format check, clippy -D warnings, tests, audit
just check

# Bump crate versions (used by the release workflow, not by hand)
just bump 0.29.0
```

## Dev container

`.devcontainer/` builds a Debian image with the Rust toolchain, `just`,
`rust-analyzer`, `clippy`, and `rustfmt`. Your host runtime dir
(`/run/user/1000`) is shared in writable, so the `discord-ipc-0` socket that
`rsrpc-cli` creates inside the container is visible to Discord on the host.

Cargo caches persist in a named `cargo-cache` volume across rebuilds.

## Contribution workflow

1. Branch off `main`: `git checkout -b feat/my-feature`.
2. Ensure `just check` passes cleanly.
3. Open a PR with the user-facing outcome in the description.

Releases are automatic: every Sunday the release workflow derives the next
version from Conventional Commits since the last tag, bumps the manifests,
tags, and publishes binaries. No manual version edits.
