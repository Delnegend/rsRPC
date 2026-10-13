# rsRPC task runner. `just check` is the single CI gate.

# Format, lint, test, and audit — must pass before merge.
check:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace --all-targets
    cargo audit --deny warnings || true

# Set every crate version (release workflow only, not by hand).
bump version:
    #!/usr/bin/env bash
    set -euo pipefail
    for manifest in lib/Cargo.toml cli/Cargo.toml; do \
      sed -i -E '0,/^version = ".*"/s//version = "{{version}}"/' "$manifest"; \
    done
    sed -i -E '/^name = "rsrpc(-cli)?"/ { n; s/version = ".*"/version = "{{version}}"/; }' Cargo.lock
    cargo metadata --format-version 1 --no-deps >/dev/null
