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
    for manifest in lib/Cargo.toml cli/Cargo.toml; do \
      sed -i -E '0,/^version = ".*"/s//version = "{{version}}"/' "$manifest"; \
    done
    # Rewrites Cargo.lock from the manifests; never hand-edit versions in it.
    cargo metadata --format-version 1 --no-deps >/dev/null
