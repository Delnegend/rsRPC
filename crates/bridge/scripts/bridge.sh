#!/bin/bash
# rsrpc-bridge Steam Play runner
# Set the game's launch option to: /path/to/bridge.sh %command%

BRIDGE_DIR="$(dirname "${BASH_SOURCE[0]}")"
BRIDGE="$BRIDGE_DIR/rsrpc-bridge.exe"

TEMP_PATH="$XDG_RUNTIME_DIR"
TEMP_PATH=${TEMP_PATH:-"$TMPDIR"}
TEMP_PATH=${TEMP_PATH:-"$TMP"}
TEMP_PATH=${TEMP_PATH:-"$TEMP"}
TEMP_PATH=${TEMP_PATH:-"/tmp"}

VESSEL_PATH="$BRIDGE"
DISCORD_IPC_PATHS=(
    "$TEMP_PATH"
    "$TEMP_PATH/app/com.discordapp.Discord"
    "$TEMP_PATH/.flatpak/dev.vencord.Vesktop/xdg-run"
    "$TEMP_PATH/snap.discord-canary"
    "$TEMP_PATH/snap.discord"
    "/run/user/$UID"
)

for ipc_path in "${DISCORD_IPC_PATHS[@]}"; do
    if [ -d "$ipc_path" ]; then
        for socket in "$ipc_path"/discord-ipc-*; do
            if [ -S "$socket" ]; then
                VESSEL_PATH="$VESSEL_PATH:$socket"
                break 2
            fi
        done
    fi
done

PROTON_REMOTE_DEBUG_CMD="$BRIDGE" PRESSURE_VESSEL_FILESYSTEMS_RW="$VESSEL_PATH:$PRESSURE_VESSEL_FILESYSTEMS_RW" "$@"
