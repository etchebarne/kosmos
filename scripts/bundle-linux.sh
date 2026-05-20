#!/usr/bin/env bash
# Build a relocatable Kosmos.app bundle and tar it for distribution.
# Output: target/release/kosmos-linux-<arch>.tar.gz
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
ARCH="$(uname -m)"

export RUSTFLAGS="${RUSTFLAGS:-} -C link-args=-Wl,--disable-new-dtags,-rpath,\$ORIGIN/../lib"

cargo build --release --manifest-path "$ROOT/Cargo.toml" -p kosmos

if command -v llvm-objcopy >/dev/null 2>&1; then
    llvm-objcopy --strip-debug "$TARGET_DIR/release/kosmos"
elif command -v objcopy >/dev/null 2>&1; then
    objcopy --strip-debug "$TARGET_DIR/release/kosmos"
fi

TEMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TEMP_DIR"' EXIT
APP_DIR="$TEMP_DIR/Kosmos.app"

mkdir -p "$APP_DIR/bin" "$APP_DIR/lib" \
    "$APP_DIR/share/applications" \
    "$APP_DIR/share/icons/hicolor/scalable/apps"
for size in 16 32 48 64 128 256 512; do
    mkdir -p "$APP_DIR/share/icons/hicolor/${size}x${size}/apps"
done

cp "$TARGET_DIR/release/kosmos" "$APP_DIR/bin/kosmos"

# Bundle non-system .so dependencies. Skip core glibc / GPU / compositor libs
# that must come from the host (driver-specific or ABI-tied to the kernel).
SKIP_LIBS='(^|/)lib(c|stdc\+\+|gcc_s|m|pthread|dl|rt|resolv|nsl|util)\.so|(^|/)ld-linux.*\.so|(^|/)lib(GL|GLX|EGL|vulkan)\.so|(^|/)libwayland-.*\.so|(^|/)libxkbcommon.*\.so|(^|/)libX.*\.so|(^|/)libxcb.*\.so|(^|/)lib(drm|gbm|asound|bsd|md)\.so'
while read -r lib; do
    [ -f "$lib" ] && cp -L "$lib" "$APP_DIR/lib/"
done < <(ldd "$APP_DIR/bin/kosmos" | awk '{print $3}' | grep -v '^$' | grep -v -E "$SKIP_LIBS" || true)

for forbidden_lib in libc.so* libm.so* libpthread.so* libdl.so* librt.so* libresolv.so* libnsl.so* libutil.so* ld-linux*.so*; do
    if compgen -G "$APP_DIR/lib/$forbidden_lib" >/dev/null; then
        echo "Refusing to bundle host ABI library: $forbidden_lib" >&2
        exit 1
    fi
done

# Icons
for size in 16 32 48 64 128 256 512; do
    cp "$ROOT/assets/icon/icon-${size}.png" \
        "$APP_DIR/share/icons/hicolor/${size}x${size}/apps/kosmos.png"
done
cp "$ROOT/assets/icon/icon.svg" \
    "$APP_DIR/share/icons/hicolor/scalable/apps/kosmos.svg"

# Desktop file: render with simple Icon=/Exec= names; the user installer
# rewrites these to absolute paths, the AUR package leaves them as hicolor
# theme lookups.
export APP_NAME="Kosmos"
export APP_CLI="kosmos"
export APP_ICON="kosmos"
export APP_ARGS="%U"
export APP_ID="net.etchebarne.Kosmos"
export DO_STARTUP_NOTIFY="true"
envsubst < "$ROOT/packaging/linux/Kosmos.desktop.in" \
    > "$APP_DIR/share/applications/${APP_ID}.desktop"

cp "$ROOT/LICENSE" "$APP_DIR/LICENSE"

ARCHIVE="$TARGET_DIR/release/kosmos-linux-${ARCH}.tar.gz"
rm -f "$ARCHIVE"
tar -czf "$ARCHIVE" -C "$TEMP_DIR" "Kosmos.app"

echo "Bundle: $ARCHIVE"
