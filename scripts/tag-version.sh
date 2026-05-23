#!/usr/bin/env bash
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

REMOTE=${1:-origin}
CURRENT=$(awk '/^\[workspace\.package\]/{f=1; next} f && /^version *= *"/{ match($0, /"([^"]+)"/, a); print a[1]; exit }' Cargo.toml)

if [ -z "$CURRENT" ]; then
    echo "Could not detect current version in Cargo.toml" >&2
    exit 1
fi

TAG="v$CURRENT"
COMMIT=$(git rev-parse --short HEAD)
SUBJECT=$(git log -1 --pretty=%s)

LOCAL_TAG="no"
if git rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then
    LOCAL_TAG="yes"
fi

REMOTE_TAG="no"
if git ls-remote --exit-code --tags "$REMOTE" "refs/tags/$TAG" >/dev/null 2>&1; then
    REMOTE_TAG="yes"
fi

echo "Tag release version"
echo ""
echo "  Version:        $CURRENT"
echo "  Tag:            $TAG"
echo "  Commit:         $COMMIT $SUBJECT"
echo "  Remote:         $REMOTE"
echo "  Local tag:      $LOCAL_TAG"
echo "  Remote tag:     $REMOTE_TAG"
echo ""

if [ "$LOCAL_TAG" = "yes" ] || [ "$REMOTE_TAG" = "yes" ]; then
    echo "Refusing to continue because $TAG already exists."
    exit 1
fi

read -r -p "Create local tag $TAG on HEAD? [y/N] " CREATE_TAG
case "$CREATE_TAG" in
    y|Y|yes|YES) ;;
    *)
        echo "Cancelled."
        exit 1
        ;;
esac

git tag "$TAG"
echo "Created local tag $TAG."
echo ""
echo "About to push: $TAG -> $REMOTE"
read -r -p "Type PUSH to push $TAG to $REMOTE: " CONFIRM_PUSH

if [ "$CONFIRM_PUSH" != "PUSH" ]; then
    echo "Push cancelled. Local tag $TAG was created but not pushed."
    exit 1
fi

git push "$REMOTE" "$TAG"
echo "Pushed $TAG to $REMOTE."
