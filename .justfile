# rsRPC task runner. `just check` is the single CI gate.

# Format, lint, test, docs, and audit — must pass before merge.
check:
    cargo fmt --all -- --check
    # A version bump that skipped the lock would be silently rewritten by the
    # next cargo command; fail here instead, before clippy gets to rewrite it.
    cargo metadata --locked --format-version 1 >/dev/null
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace --all-targets
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
    # Gated in CI via security scans; local run runs if cargo-audit is installed.
    if command -v cargo-audit >/dev/null 2>&1; then cargo audit --deny warnings; else echo "cargo-audit not installed locally; skipping (enforced in CI)"; fi


# Build the Windows Proton/Wine bridge executable using container cross-compilation.
bridge:
    podman run --rm -v "$(pwd)":/ws:z -w /ws docker.io/library/rust:1-bookworm bash -c '\
      rustup target add i686-pc-windows-gnu >/dev/null 2>&1 && \
      apt-get update -qq && apt-get install -y -qq gcc-mingw-w64-i686 >/dev/null 2>&1 && \
      cargo build -p rsrpc-bridge --target i686-pc-windows-gnu --release'
    mkdir -p dist
    cp target/i686-pc-windows-gnu/release/rsrpc-bridge.exe dist/
    cp crates/bridge/scripts/bridge.sh dist/

# Language-agnostic version contract for Delnegend/actions/resolve-release.
version action ver="":
    #!/usr/bin/env bash
    set -euo pipefail
    case "{{action}}" in
      get)
        python3 -c "import tomllib;d=tomllib.load(open('Cargo.toml','rb'));print(d.get('workspace',{}).get('package',{}).get('version') or d.get('package',{}).get('version',''))"
        ;;
      set|bump)
        [ -n "{{ver}}" ] || { echo "usage: just version set X.Y.Z" >&2; exit 1; }
        sed -i -E '0,/^version = ".*"/s//version = "{{ver}}"/' Cargo.toml
        cargo update -w
        ;;
      *) echo "usage: just version get|set X.Y.Z" >&2; exit 1 ;;
    esac
