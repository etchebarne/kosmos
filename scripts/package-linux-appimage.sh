#!/usr/bin/env bash
# Build an AppImage from the Linux Kosmos.app bundle.
# Input: target/release/kosmos-linux-<arch>.tar.gz from bundle-linux.sh
# Output: target/release/Kosmos-<version>-<arch>.AppImage
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
RELEASE_DIR="$TARGET_DIR/release"
APPIMAGETOOL="${APPIMAGETOOL:-appimagetool}"

require_command() {
    local command_name="$1"
    if ! command -v "$command_name" >/dev/null 2>&1; then
        echo "Missing required command: $command_name" >&2
        exit 1
    fi
}

workspace_version() {
    awk '
        /^\[workspace\.package\]/ { in_workspace_package = 1; next }
        /^\[/ { in_workspace_package = 0 }
        in_workspace_package && /^version *= *"/ {
            gsub(/^version *= *"/, "")
            gsub(/".*/, "")
            print
            exit
        }
    ' "$ROOT/Cargo.toml"
}

set_appimage_arch() {
    local system_arch="$1"
    case "$system_arch" in
        x86_64|amd64)
            BUNDLE_ARCH="x86_64"
            APPIMAGE_ARCH="x86_64"
            ;;
        *)
            echo "Unsupported AppImage architecture: $system_arch" >&2
            exit 1
            ;;
    esac
}

extract_bundle() {
    local archive="$1"
    local destination="$2"

    if [ ! -f "$archive" ]; then
        echo "Missing bundle: $archive" >&2
        echo "Run ./scripts/bundle-linux.sh first." >&2
        exit 1
    fi

    tar -xzf "$archive" -C "$destination"
}

write_app_run() {
    local app_dir="$1"

    cat > "$app_dir/AppRun" <<'SH'
#!/usr/bin/env sh
set -eu

APPDIR="$(dirname "$(readlink -f "$0")")"
export KOSMOS_APP_ID="${KOSMOS_APP_ID:-net.etchebarne.Kosmos.AppImage}"
export LD_LIBRARY_PATH="$APPDIR/usr/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

escape_desktop_exec_path() {
    printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g; s/`/\\`/g; s/\$/\\$/g'
}

install_desktop_metadata() {
    [ -n "${APPIMAGE:-}" ] || return 0
    [ -f "$APPIMAGE" ] || return 0
    [ -n "${HOME:-}" ] || return 0

    data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
    desktop_dir="$data_home/applications"
    icon_dir="$data_home/icons/hicolor/512x512/apps"
    desktop_file="$desktop_dir/$KOSMOS_APP_ID.desktop"
    icon_name="$KOSMOS_APP_ID"
    escaped_appimage="$(escape_desktop_exec_path "$APPIMAGE")"

    mkdir -p "$desktop_dir" "$icon_dir"
    cp "$APPDIR/usr/share/icons/hicolor/512x512/apps/kosmos.png" "$icon_dir/$icon_name.png"
    cat > "$desktop_file" <<EOF
[Desktop Entry]
Version=1.0
Type=Application
Name=Kosmos
GenericName=Code Editor
Comment=A highly customizable and versatile tab-based code editor.
Exec="$escaped_appimage" %U
Icon=$icon_name
Terminal=false
Categories=Development;TextEditor;IDE;
Keywords=kosmos;editor;code;
StartupNotify=true
StartupWMClass=$KOSMOS_APP_ID
EOF

    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database "$desktop_dir" >/dev/null 2>&1 || true
    fi
}

install_desktop_metadata || true
exec "$APPDIR/usr/bin/kosmos" "$@"
SH
    chmod +x "$app_dir/AppRun"
}

prepare_app_dir() {
    local bundle_dir="$1"
    local app_dir="$2"

    mkdir -p "$app_dir/usr/share/licenses/kosmos"
    cp -a "$bundle_dir/bin" "$app_dir/usr/"
    cp -a "$bundle_dir/lib" "$app_dir/usr/"
    cp -a "$bundle_dir/share" "$app_dir/usr/"
    cp -a "$bundle_dir/LICENSE" "$app_dir/usr/share/licenses/kosmos/LICENSE"

    export APP_NAME="Kosmos"
    appimage_app_id="net.etchebarne.Kosmos.AppImage"

    export APP_CLI="AppRun"
    export APP_ICON="kosmos"
    export APP_ARGS="%U"
    export APP_ID="$appimage_app_id"
    export DO_STARTUP_NOTIFY="true"
    envsubst < "$ROOT/packaging/linux/Kosmos.desktop.in" \
        > "$app_dir/${appimage_app_id}.desktop"

    cp "$bundle_dir/share/icons/hicolor/512x512/apps/kosmos.png" "$app_dir/kosmos.png"
    cp "$app_dir/kosmos.png" "$app_dir/.DirIcon"
    write_app_run "$app_dir"
}

require_command envsubst
require_command "$APPIMAGETOOL"

KOSMOS_VERSION="$(workspace_version)"
if [ -z "$KOSMOS_VERSION" ]; then
    echo "Could not detect version in Cargo.toml" >&2
    exit 1
fi

set_appimage_arch "$(uname -m)"

ARCHIVE="$RELEASE_DIR/kosmos-linux-$BUNDLE_ARCH.tar.gz"
OUTPUT="$RELEASE_DIR/Kosmos-${KOSMOS_VERSION}-${APPIMAGE_ARCH}.AppImage"

TEMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TEMP_DIR"' EXIT

extract_bundle "$ARCHIVE" "$TEMP_DIR"
BUNDLE_DIR="$TEMP_DIR/Kosmos.app"
APP_DIR="$TEMP_DIR/Kosmos.AppDir"

prepare_app_dir "$BUNDLE_DIR" "$APP_DIR"

rm -f "$OUTPUT"
ARCH="$APPIMAGE_ARCH" APPIMAGE_EXTRACT_AND_RUN=1 \
    "$APPIMAGETOOL" "$APP_DIR" "$OUTPUT"

echo "AppImage: $OUTPUT"
