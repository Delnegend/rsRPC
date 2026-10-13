FROM debian:13-slim

ARG MOLD_VERSION=2.42.0

ENV LANG=C.UTF-8
ENV LC_ALL=C.UTF-8
ENV SHELL=/bin/bash

RUN --mount=type=cache,target=/var/cache/apt \
    apt-get update && apt-get install -y --no-install-recommends \
    build-essential ca-certificates curl git openssh-client \
    clang sudo pkg-config zip unzip && \
    rm -rf /var/lib/apt/lists/*

RUN groupadd -g 1000 dev && \
    useradd -m -u 1000 -g dev -s /bin/bash dev && \
    echo "dev ALL=(ALL) NOPASSWD: ALL" > /etc/sudoers.d/dev && \
    chmod 0440 /etc/sudoers.d/dev && \
    mkdir -p /usr/local/cargo /run/user/1000 && \
    chown -R dev:dev /usr/local/cargo /run/user/1000 && \
    chmod 0700 /run/user/1000 && \
    printf 'export PATH=/usr/local/cargo/bin:${PATH}\nexport XDG_RUNTIME_DIR=/run/user/1000\n' > /etc/profile.d/dev-path.sh

SHELL ["/bin/bash", "-c"]
USER dev

ENV CARGO_HOME=/usr/local/cargo
ENV PATH="/usr/local/cargo/bin:${PATH}"
ENV XDG_RUNTIME_DIR=/run/user/1000

# Rust stable toolchain (minimal) + analyzer/fmt/clippy, pre-installed so the
# container works even if postCreateCommand is skipped
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --profile minimal --no-modify-path && \
    /usr/local/cargo/bin/rustup component add rust-analyzer rustfmt clippy

# mold — pinned to keep linking fast/reproducible for native (gnu) builds.
# Note: project .cargo/config.toml merges with this CARGO_HOME-level config.
RUN curl -fsSL -o /tmp/mold.tar.gz "https://github.com/rui314/mold/releases/download/v${MOLD_VERSION}/mold-${MOLD_VERSION}-x86_64-linux.tar.gz" && \
    echo "f5ed2f6e31d1ada4f07fe766fe0de7a73104d1c5cdc59086fcecc16a43720b6d  /tmp/mold.tar.gz" | sha256sum -c - && \
    tar -xf /tmp/mold.tar.gz -C /usr/local/cargo "mold-${MOLD_VERSION}-x86_64-linux" && \
    rm /tmp/mold.tar.gz && \
    printf '[target.x86_64-unknown-linux-gnu]\nlinker = "clang"\nrustflags = ["-C", "link-arg=-fuse-ld=/usr/local/cargo/mold-%s-x86_64-linux/bin/mold"]\n' "$MOLD_VERSION" > /usr/local/cargo/config.toml

# cargo-binstall + just (task runner for `just check` / `just bump`)
RUN curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash && \
    cargo binstall -y just
