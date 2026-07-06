#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DESKTOP_DIR="$ROOT_DIR/desktop"

cargo build --release --package kosmos-server

if [[ ! -d "$DESKTOP_DIR/node_modules" ]] || [[ ! -x "$DESKTOP_DIR/node_modules/.bin/electron-builder" ]]; then
    bun install --cwd "$DESKTOP_DIR"
fi

bun run --cwd "$DESKTOP_DIR" typecheck
bun run --cwd "$DESKTOP_DIR" build
bun run --cwd "$DESKTOP_DIR" package:linux
