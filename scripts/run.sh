#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SERVER_BIN="$ROOT_DIR/target/debug/kosmos-server"
UI_BUILD_DIR="$ROOT_DIR/ui/build"
UI_BIN="$UI_BUILD_DIR/kosmos-ui"
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
cmake -S "$ROOT_DIR/ui" -B "$UI_BUILD_DIR"
cmake --build "$UI_BUILD_DIR"

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

"$UI_BIN" "$@"
