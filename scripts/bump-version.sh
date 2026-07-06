#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

DESKTOP_PACKAGE="desktop/package.json"
CORE_MANIFEST="core/Cargo.toml"
SERVER_MANIFEST="server/Cargo.toml"
AUR_PKGBUILD="aur/kosmos-bin/PKGBUILD"

current_version() {
    grep -m1 -E '"version": "[0-9]+\.[0-9]+\.[0-9]+"' "$DESKTOP_PACKAGE" \
        | sed -E 's/.*"version": "([^"]+)".*/\1/'
}

is_semver() {
    [[ "$1" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]
}

next_version() {
    local current="$1"
    local bump="$2"
    local major minor patch

    IFS=. read -r major minor patch <<< "$current"

    case "$bump" in
        patch)
            printf "%s.%s.%s\n" "$major" "$minor" "$((patch + 1))"
            ;;
        minor)
            printf "%s.%s.0\n" "$major" "$((minor + 1))"
            ;;
        major)
            printf "%s.0.0\n" "$((major + 1))"
            ;;
        *)
            printf "%s\n" "$bump"
            ;;
    esac
}

usage() {
    local current="$1"
    local major minor patch

    IFS=. read -r major minor patch <<< "$current"

    cat <<USAGE
Current version: $current

Usage: $0 <version>
       $0 patch|minor|major

Examples:
  $0 $major.$minor.$((patch + 1))
  $0 patch   # $current -> $major.$minor.$((patch + 1))
  $0 minor   # $current -> $major.$((minor + 1)).0
  $0 major   # $current -> $((major + 1)).0.0
USAGE
}

require_current_version() {
    local current="$1"

    require_contains "$DESKTOP_PACKAGE" "\"version\": \"$current\""
    require_contains "$CORE_MANIFEST" "version = \"$current\""
    require_contains "$SERVER_MANIFEST" "version = \"$current\""
    require_contains "$AUR_PKGBUILD" "pkgver=$current"
}

require_contains() {
    local file="$1"
    local pattern="$2"

    if ! grep -q "$pattern" "$file"; then
        echo "$file is not on version $CURRENT" >&2
        exit 1
    fi
}

bump_version() {
    local current="$1"
    local next="$2"

    perl -0pi -e "s/\"version\": \"$current\"/\"version\": \"$next\"/g" "$DESKTOP_PACKAGE"
    perl -0pi -e "s/version = \"$current\"/version = \"$next\"/g" "$CORE_MANIFEST" "$SERVER_MANIFEST"
    perl -0pi -e "s/^pkgver=$current$/pkgver=$next/m; s/^pkgrel=.*/pkgrel=1/m" "$AUR_PKGBUILD"
}

CURRENT="$(current_version)"

if ! is_semver "$CURRENT"; then
    echo "Could not read a valid current version from $DESKTOP_PACKAGE" >&2
    exit 1
fi

if [[ $# -eq 0 ]]; then
    usage "$CURRENT"
    exit 1
fi

NEXT="$(next_version "$CURRENT" "$1")"

if ! is_semver "$NEXT"; then
    echo "Version must be patch, minor, major, or x.y.z" >&2
    exit 1
fi

if [[ "$CURRENT" == "$NEXT" ]]; then
    echo "Version is already $CURRENT" >&2
    exit 1
fi

require_current_version "$CURRENT"

echo "Bumping version: $CURRENT -> $NEXT"

bump_version "$CURRENT" "$NEXT"
cargo check --workspace >/dev/null

echo "Updated:"
echo "  $DESKTOP_PACKAGE"
echo "  $CORE_MANIFEST"
echo "  $SERVER_MANIFEST"
echo "  $AUR_PKGBUILD"
echo "  Cargo.lock"
