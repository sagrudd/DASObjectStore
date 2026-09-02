#!/usr/bin/env bash
set -euo pipefail

repo_root="${1:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
current_version="$(awk '/^version = "/ { value=$0; sub(/^version = "/, "", value); sub(/"$/, "", value); print value; exit }' "$repo_root/Cargo.toml")"

if [[ -z "$current_version" ]]; then
  printf 'release version guard: workspace version is missing\n' >&2
  exit 1
fi

changelog_version="$(awk '/^## [^ ]+ - / { print $2; exit }' "$repo_root/CHANGELOG.md")"
if [[ "$changelog_version" != "$current_version" ]]; then
  printf 'release version guard: Cargo.toml %s does not match CHANGELOG.md %s\n' \
    "$current_version" "${changelog_version:-missing}" >&2
  exit 1
fi

product_manifest_version="$(python3 - "$repo_root/product-manifest.json" <<'PY'
import json
import sys

try:
    manifest = json.load(open(sys.argv[1], encoding="utf-8"))
    product = manifest["product"]
    if product["id"] != "dasobjectstore":
        raise ValueError("product id is not dasobjectstore")
    version = product["version"]
    if not isinstance(version, str) or not version:
        raise ValueError("product version is missing")
except (OSError, ValueError, KeyError, TypeError, json.JSONDecodeError) as error:
    print(f"release version guard: invalid product-manifest.json: {error}", file=sys.stderr)
    raise SystemExit(1)

print(version)
PY
)"
if [[ "$product_manifest_version" != "$current_version" ]]; then
  printf 'release version guard: Cargo.toml %s does not match product-manifest.json %s\n' \
    "$current_version" "${product_manifest_version:-missing}" >&2
  exit 1
fi

if git -C "$repo_root" rev-parse --verify HEAD^ >/dev/null 2>&1; then
  parent_manifest="$(git -C "$repo_root" show HEAD^:Cargo.toml 2>/dev/null)"
  parent_version="$(awk '/^version = "/ { value=$0; sub(/^version = "/, "", value); sub(/"$/, "", value); print value; exit }' <<<"$parent_manifest")"
  if [[ "$parent_version" == "$current_version" ]] \
    && ! git -C "$repo_root" diff --quiet HEAD^ HEAD -- Cargo.toml Cargo.lock CHANGELOG.md crates packaging; then
    printf 'release version guard: package-relevant revision reuses parent version %s\n' \
      "$current_version" >&2
    exit 1
  fi
fi

printf 'release version guard passed: %s\n' "$current_version"
