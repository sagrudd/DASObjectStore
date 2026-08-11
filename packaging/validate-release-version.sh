#!/usr/bin/env bash
set -euo pipefail

repo_root="${1:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
current_version="$(sed -n 's/^version = "\([^"]*\)"$/\1/p' "$repo_root/Cargo.toml" | head -1)"

if [[ -z "$current_version" ]]; then
  printf 'release version guard: workspace version is missing\n' >&2
  exit 1
fi

changelog_version="$(sed -n 's/^## \([^ ]*\) - .*/\1/p' "$repo_root/CHANGELOG.md" | head -1)"
if [[ "$changelog_version" != "$current_version" ]]; then
  printf 'release version guard: Cargo.toml %s does not match CHANGELOG.md %s\n' \
    "$current_version" "${changelog_version:-missing}" >&2
  exit 1
fi

if git -C "$repo_root" rev-parse --verify HEAD^ >/dev/null 2>&1; then
  parent_version="$(git -C "$repo_root" show HEAD^:Cargo.toml 2>/dev/null \
    | sed -n 's/^version = "\([^"]*\)"$/\1/p' | head -1)"
  if [[ "$parent_version" == "$current_version" ]] \
    && ! git -C "$repo_root" diff --quiet HEAD^ HEAD -- Cargo.toml Cargo.lock CHANGELOG.md crates packaging; then
    printf 'release version guard: package-relevant revision reuses parent version %s\n' \
      "$current_version" >&2
    exit 1
  fi
fi

printf 'release version guard passed: %s\n' "$current_version"
