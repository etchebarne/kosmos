#!/usr/bin/env bash

set -euo pipefail

repository_root="$(git -C "$(dirname "${BASH_SOURCE[0]}")/.." rev-parse --show-toplevel)"
cd "$repository_root"

failed=0

check_imports() {
  local description="$1"
  local directory="$2"
  local pattern="$3"
  local matches

  matches="$(rg -n --pcre2 "$pattern" "$directory" || true)"
  if [[ -n "$matches" ]]; then
    printf 'Boundary violation (%s):\n%s\n' "$description" "$matches" >&2
    failed=1
  fi
}

check_imports \
  "renderer must not import main or preload" \
  desktop/src/renderer \
  "(?:from\\s*|import\\s*(?:\\(\\s*)?)['\\\"](?:@/(?:main|preload)|(?:\\.\\./)+(?:main|preload))(?:/|['\\\"])"
check_imports \
  "main must not import renderer" \
  desktop/src/main \
  "(?:from\\s*|import\\s*(?:\\(\\s*)?)['\\\"](?:@/renderer|(?:\\.\\./)+renderer)(?:/|['\\\"])"
check_imports \
  "preload must not import renderer" \
  desktop/src/preload \
  "(?:from\\s*|import\\s*(?:\\(\\s*)?)['\\\"](?:@/renderer|(?:\\.\\./)+renderer)(?:/|['\\\"])"

metadata="$(cargo metadata --no-deps --format-version 1)"
server_package="$(printf '%s' "$metadata" | rg -oP '"name":"kosmos-server","version":.*?,"dependencies":\[.*?\],"targets":' || true)"
core_package="$(printf '%s' "$metadata" | rg -oP '"name":"core","version":.*?,"dependencies":\[.*?\],"targets":' || true)"

if ! printf '%s' "$server_package" | rg -q '"name":"core"'; then
  printf 'Boundary violation: kosmos-server must depend on core according to cargo metadata.\n' >&2
  failed=1
fi

if printf '%s' "$core_package" | rg -q '"name":"kosmos-server"'; then
  printf 'Boundary violation: core must not depend on kosmos-server according to cargo metadata.\n' >&2
  failed=1
fi

if ((failed)); then
  exit 1
fi

printf 'Boundary checks passed.\n'
