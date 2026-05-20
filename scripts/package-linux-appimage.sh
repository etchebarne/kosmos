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
export LD_LIBRARY_PATH="$APPDIR/usr/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
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
    export APP_CLI="AppRun"
    export APP_ICON="kosmos"
    export APP_ARGS="%U"
    export APP_ID="net.etchebarne.Kosmos"
    export DO_STARTUP_NOTIFY="true"
    envsubst < "$ROOT/packaging/linux/Kosmos.desktop.in" \
        > "$app_dir/net.etchebarne.Kosmos.desktop"

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
