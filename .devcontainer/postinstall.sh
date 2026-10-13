#!/usr/bin/env bash
set -e
# Toolchain, mold, and just are pre-installed in the Dockerfile for cache +
# editors that skip postCreateCommand. Nothing to install here — the hook just
# verifies the shared runtime dir is writable, since rsrpc creates the
# discord-ipc-0 socket there (host Discord reads it from the same path).

if [ -w "/run/user/1000" ]; then
    echo "Runtime dir shared and writable: /run/user/1000"
else
    echo "WARNING: /run/user/1000 is not writable — rsrpc cannot create discord-ipc-0."
fi
