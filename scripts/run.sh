#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SERVER_BIN="$ROOT_DIR/target/debug/kosmos-server"
DESKTOP_BUILD_DIR="$ROOT_DIR/desktop/build"
DESKTOP_BIN="$DESKTOP_BUILD_DIR/kosmos-desktop"
RUNTIME_DIR="${XDG_RUNTIME_DIR:-/tmp}"
export KOSMOS_SOCKET="${KOSMOS_SOCKET:-$RUNTIME_DIR/kosmos/server.sock}"

server_pid=""

cleanup() {
    if [[ -n "$server_pid" ]] && kill -0 "$server_pid" 2>/dev/null; then
        kill "$server_pid"
        wait "$server_pid" 2>/dev/null || true
    fi
}

trap cleanup EXIT INT TERM

cargo build --package kosmos-server
if [[ -f "$DESKTOP_BUILD_DIR/build.ninja" ]]; then
    meson setup --reconfigure "$DESKTOP_BUILD_DIR" "$ROOT_DIR/desktop"
else
    meson setup "$DESKTOP_BUILD_DIR" "$ROOT_DIR/desktop"
fi
meson compile -C "$DESKTOP_BUILD_DIR"

"$SERVER_BIN" &
server_pid="$!"

for _ in {1..50}; do
    if [[ -S "$KOSMOS_SOCKET" ]]; then
        break
    fi

    if ! kill -0 "$server_pid" 2>/dev/null; then
        echo "kosmos server failed to start" >&2
        exit 1
    fi

    sleep 0.1
done

if [[ ! -S "$KOSMOS_SOCKET" ]]; then
    echo "kosmos server socket was not created at $KOSMOS_SOCKET" >&2
    exit 1
fi

"$DESKTOP_BIN" "$@"
