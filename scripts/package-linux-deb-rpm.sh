#!/usr/bin/env bash
# Build Debian and RPM packages from the Linux Kosmos.app bundle.
# Input: target/release/kosmos-linux-<arch>.tar.gz from bundle-linux.sh
# Outputs: target/release/kosmos_<version>_<deb-arch>.deb
#          target/release/kosmos-<version>-1.<rpm-arch>.rpm
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
RELEASE_DIR="$TARGET_DIR/release"

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

set_package_arches() {
    local system_arch="$1"
    case "$system_arch" in
        x86_64|amd64)
            BUNDLE_ARCH="x86_64"
            DEB_ARCH="amd64"
            RPM_ARCH="x86_64"
            ;;
        *)
            echo "Unsupported package architecture: $system_arch" >&2
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

install_package_files() {
    local app_dir="$1"
    local package_root="$2"

    mkdir -p "$package_root/opt" \
        "$package_root/usr/bin" \
        "$package_root/usr/share/applications" \
        "$package_root/usr/share/icons" \
        "$package_root/usr/share/licenses/kosmos"

    cp -a "$app_dir" "$package_root/opt/Kosmos.app"
    ln -s /opt/Kosmos.app/bin/kosmos "$package_root/usr/bin/kosmos"
    cp -a "$app_dir/share/applications/net.etchebarne.Kosmos.desktop" \
        "$package_root/usr/share/applications/net.etchebarne.Kosmos.desktop"
    cp -a "$app_dir/share/icons/hicolor" "$package_root/usr/share/icons/"
    cp -a "$app_dir/LICENSE" "$package_root/usr/share/licenses/kosmos/LICENSE"
}

build_deb_package() {
    local app_dir="$1"
    local temp_dir="$2"
    local output="$3"
    local package_root="$temp_dir/deb-root"

    mkdir -p "$package_root/DEBIAN"
    install_package_files "$app_dir" "$package_root"

    export KOSMOS_VERSION DEB_ARCH
    envsubst < "$ROOT/packaging/linux/deb-control.in" > "$package_root/DEBIAN/control"

    rm -f "$output"
    dpkg-deb --build --root-owner-group "$package_root" "$output"
}

build_rpm_package() {
    local app_dir="$1"
    local temp_dir="$2"
    local output="$3"
    local rpm_topdir="$temp_dir/rpmbuild"
    local spec_file="$rpm_topdir/SPECS/kosmos.spec"

    mkdir -p "$rpm_topdir/BUILD" \
        "$rpm_topdir/BUILDROOT" \
        "$rpm_topdir/RPMS" \
        "$rpm_topdir/SOURCES" \
        "$rpm_topdir/SPECS" \
        "$rpm_topdir/SRPMS"

    export KOSMOS_VERSION RPM_ARCH KOSMOS_APP_DIR="$app_dir"
    envsubst < "$ROOT/packaging/linux/kosmos.spec.in" > "$spec_file"

    rpmbuild -bb "$spec_file" --define "_topdir $rpm_topdir"

    rm -f "$output"
    cp "$rpm_topdir/RPMS/$RPM_ARCH/kosmos-$KOSMOS_VERSION-1.$RPM_ARCH.rpm" "$output"
}

require_command dpkg-deb
require_command envsubst
require_command rpmbuild

KOSMOS_VERSION="$(workspace_version)"
if [ -z "$KOSMOS_VERSION" ]; then
    echo "Could not detect version in Cargo.toml" >&2
    exit 1
fi

set_package_arches "$(uname -m)"

ARCHIVE="$RELEASE_DIR/kosmos-linux-$BUNDLE_ARCH.tar.gz"
DEB_OUTPUT="$RELEASE_DIR/kosmos_${KOSMOS_VERSION}_${DEB_ARCH}.deb"
RPM_OUTPUT="$RELEASE_DIR/kosmos-${KOSMOS_VERSION}-1.${RPM_ARCH}.rpm"

TEMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TEMP_DIR"' EXIT

extract_bundle "$ARCHIVE" "$TEMP_DIR"
APP_DIR="$TEMP_DIR/Kosmos.app"

build_deb_package "$APP_DIR" "$TEMP_DIR" "$DEB_OUTPUT"
build_rpm_package "$APP_DIR" "$TEMP_DIR" "$RPM_OUTPUT"

echo "Debian package: $DEB_OUTPUT"
echo "RPM package: $RPM_OUTPUT"
