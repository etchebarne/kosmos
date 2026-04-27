#!/usr/bin/env sh
# Install Kosmos for the current user under ~/.local.
# Downloads the latest release tarball from GitHub unless KOSMOS_BUNDLE_PATH
# is set to an existing local tarball.
set -eu

REPO="${KOSMOS_REPO:-etchebarne/kosmos}"
APP_ID="net.etchebarne.Kosmos"
INSTALL_ROOT="$HOME/.local"
APP_DIR="$INSTALL_ROOT/Kosmos.app"
BIN_LINK="$INSTALL_ROOT/bin/kosmos"
DESKTOP_FILE="$INSTALL_ROOT/share/applications/$APP_ID.desktop"

arch="$(uname -m)"
case "$arch" in
    x86_64|amd64) arch="x86_64" ;;
    *) echo "Unsupported architecture: $arch" >&2; exit 1 ;;
esac

if [ -n "${KOSMOS_BUNDLE_PATH:-}" ]; then
    [ -f "$KOSMOS_BUNDLE_PATH" ] || { echo "KOSMOS_BUNDLE_PATH not found: $KOSMOS_BUNDLE_PATH" >&2; exit 1; }
    tarball="$KOSMOS_BUNDLE_PATH"
else
    if command -v curl >/dev/null 2>&1; then
        fetch() { curl -fL "$@"; }
    elif command -v wget >/dev/null 2>&1; then
        fetch() { wget -O- "$@"; }
    else
        echo "Need curl or wget on PATH" >&2
        exit 1
    fi
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT
    tarball="$tmp/kosmos-linux-$arch.tar.gz"
    url="https://github.com/$REPO/releases/latest/download/kosmos-linux-$arch.tar.gz"
    echo "Downloading $url"
    fetch "$url" > "$tarball"
fi

mkdir -p "$INSTALL_ROOT/bin" "$INSTALL_ROOT/share/applications"
rm -rf "$APP_DIR"
tar -xzf "$tarball" -C "$INSTALL_ROOT"

ln -sf "$APP_DIR/bin/kosmos" "$BIN_LINK"

src_desktop="$APP_DIR/share/applications/$APP_ID.desktop"
icon_path="$APP_DIR/share/icons/hicolor/512x512/apps/kosmos.png"
sed -e "s|^Icon=kosmos\$|Icon=$icon_path|" \
    -e "s|^Exec=kosmos |Exec=$APP_DIR/bin/kosmos |" \
    -e "s|^TryExec=kosmos\$|TryExec=$APP_DIR/bin/kosmos|" \
    "$src_desktop" > "$DESKTOP_FILE"

if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$INSTALL_ROOT/share/applications" >/dev/null 2>&1 || true
fi

echo "Installed to $APP_DIR"
case ":$PATH:" in
    *":$INSTALL_ROOT/bin:"*) echo "Run with: kosmos" ;;
    *) echo "Add $INSTALL_ROOT/bin to PATH, or run: $BIN_LINK" ;;
esac
