#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DESKTOP_DIR="$ROOT_DIR/desktop"

if ! command -v rpmbuild >/dev/null 2>&1; then
    if command -v pacman >/dev/null 2>&1; then
        install_hint="sudo pacman -S rpm-tools"
    elif command -v apt-get >/dev/null 2>&1; then
        install_hint="sudo apt-get install rpm"
    elif command -v dnf >/dev/null 2>&1; then
        install_hint="sudo dnf install rpm-build"
    elif command -v zypper >/dev/null 2>&1; then
        install_hint="sudo zypper install rpm-build"
    else
        install_hint="Install the package that provides rpmbuild for your distribution."
    fi

    echo "Missing required command: rpmbuild" >&2
    echo "$install_hint" >&2
    exit 1
fi

cargo build --release --package kosmos-server

if [[ ! -d "$DESKTOP_DIR/node_modules" ]] || [[ ! -x "$DESKTOP_DIR/node_modules/.bin/electron-builder" ]]; then
    bun install --cwd "$DESKTOP_DIR"
fi

bun run --cwd "$DESKTOP_DIR" build
bun run --cwd "$DESKTOP_DIR" package:linux
