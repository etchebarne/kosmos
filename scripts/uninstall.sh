#!/usr/bin/env sh
# Remove a user-level Kosmos install (the one created by install.sh).
# Does not touch system installs (e.g. AUR / pacman) — use `pacman -R kosmos-bin` for those.
set -eu

APP_ID="net.etchebarne.Kosmos"
INSTALL_ROOT="$HOME/.local"
APP_DIR="$INSTALL_ROOT/Kosmos.app"
BIN_LINK="$INSTALL_ROOT/bin/kosmos"
DESKTOP_FILE="$INSTALL_ROOT/share/applications/$APP_ID.desktop"

removed=0

if [ -d "$APP_DIR" ]; then
    rm -rf "$APP_DIR"
    echo "Removed $APP_DIR"
    removed=1
fi

if [ -L "$BIN_LINK" ] || [ -f "$BIN_LINK" ]; then
    rm -f "$BIN_LINK"
    echo "Removed $BIN_LINK"
    removed=1
fi

if [ -f "$DESKTOP_FILE" ]; then
    rm -f "$DESKTOP_FILE"
    echo "Removed $DESKTOP_FILE"
    removed=1
fi

if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$INSTALL_ROOT/share/applications" >/dev/null 2>&1 || true
fi

if [ "$removed" -eq 0 ]; then
    echo "No user-level Kosmos install found under $INSTALL_ROOT"
else
    echo "Uninstalled."
fi
