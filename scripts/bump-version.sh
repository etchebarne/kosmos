#!/usr/bin/env bash
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

CURRENT=$(awk '/^\[workspace\.package\]/{f=1; next} f && /^version *= *"/{ match($0, /"([^"]+)"/, a); print a[1]; exit }' Cargo.toml)

if [ -z "$CURRENT" ]; then
    echo "Could not detect current version in Cargo.toml" >&2
    exit 1
fi

if [ $# -eq 0 ]; then
    echo "Current version: $CURRENT"
    echo ""
    echo "Usage: $0 <version>"
    echo "       $0 patch|minor|major"
    echo ""
    echo "Examples:"
    echo "  $0 0.2.0"
    echo "  $0 patch   # $CURRENT -> $(echo "$CURRENT" | awk -F. '{print $1"."$2"."$3+1}')"
    echo "  $0 minor   # $CURRENT -> $(echo "$CURRENT" | awk -F. '{print $1"."$2+1".0"}')"
    echo "  $0 major   # $CURRENT -> $(echo "$CURRENT" | awk -F. '{print $1+1".0.0"}')"
    exit 1
fi

NEW_VERSION="$1"

case "$NEW_VERSION" in
    patch) NEW_VERSION=$(echo "$CURRENT" | awk -F. '{print $1"."$2"."$3+1}') ;;
    minor) NEW_VERSION=$(echo "$CURRENT" | awk -F. '{print $1"."$2+1".0"}') ;;
    major) NEW_VERSION=$(echo "$CURRENT" | awk -F. '{print $1+1".0.0"}') ;;
esac

if [ "$CURRENT" = "$NEW_VERSION" ]; then
    echo "Version is already $CURRENT"
    exit 1
fi

echo "Bumping version: $CURRENT -> $NEW_VERSION"
echo ""

for cargo_toml in Cargo.toml crates/*/Cargo.toml; do
    sed -i "s/^version = \"$CURRENT\"/version = \"$NEW_VERSION\"/" "$cargo_toml"
    echo "  updated $cargo_toml"
done

for pkgbuild in aur/*/PKGBUILD; do
    sed -i "s/^pkgver=$CURRENT/pkgver=$NEW_VERSION/" "$pkgbuild"
    echo "  updated $pkgbuild"
done

if command -v cargo >/dev/null 2>&1; then
    cargo update --workspace --offline >/dev/null 2>&1 || \
        cargo update --workspace >/dev/null 2>&1 || true
    echo "  updated Cargo.lock"
fi

echo ""
echo "Done. Review with 'git diff', then commit and tag:"
echo "  git commit -am \"Release v$NEW_VERSION\""
echo "  git tag v$NEW_VERSION"
