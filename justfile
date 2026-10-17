# rsRPC task runner. `just check` is the single CI gate.

# Format, lint, test, docs, and audit — must pass before merge.
check:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace --all-targets
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
    # Gated in CI via security scans; local run runs if cargo-audit is installed.
    if command -v cargo-audit >/dev/null 2>&1; then cargo audit --deny warnings; else echo "cargo-audit not installed locally; skipping (enforced in CI)"; fi

# Set every crate version (release workflow only, not by hand).
bump version:
    #!/usr/bin/env bash
    set -euo pipefail
    for manifest in crates/lib/Cargo.toml crates/cli/Cargo.toml crates/bridge/Cargo.toml; do \
      sed -i -E '0,/^version = ".*"/s//version = "{{version}}"/' "$manifest"; \
    done
    # Rewrites Cargo.lock from the manifests; never hand-edit versions in it.
    cargo metadata --format-version 1 --no-deps >/dev/null

# Build the Windows Proton/Wine bridge executable using container cross-compilation.
bridge:
    podman run --rm -v "$(pwd)":/ws:z -w /ws docker.io/library/rust:1-bookworm bash -c '\
      rustup target add x86_64-pc-windows-gnu >/dev/null 2>&1 && \
      apt-get update -qq && apt-get install -y -qq gcc-mingw-w64-x86-64 >/dev/null 2>&1 && \
      cargo build -p rsrpc-bridge --target x86_64-pc-windows-gnu --release'
    mkdir -p dist
    cp target/x86_64-pc-windows-gnu/release/rsrpc-bridge.exe dist/
    cp crates/bridge/scripts/bridge.sh dist/
