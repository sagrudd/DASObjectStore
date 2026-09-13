#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
asset_root="$repo_root/packaging/expedition/dasobjectstore-development-validation"
payload_root="${1:?usage: stage-development-validation-cohort.sh PAYLOAD_ROOT}"

if [[ "$payload_root" == "/" || ! -d "$payload_root" || -L "$payload_root" ]]; then
  printf 'development-validation cohort staging requires a non-symlink payload root\n' >&2
  exit 1
fi

ensure_directory() {
  local relative="$1"
  local destination="$payload_root/$relative"
  if [[ -e "$destination" || -L "$destination" ]]; then
    if [[ ! -d "$destination" || -L "$destination" ]]; then
      printf 'development-validation cohort staging refuses unsafe directory: %s\n' "$relative" >&2
      exit 1
    fi
    return
  fi
  mkdir "$destination"
  chmod 0755 "$destination"
}

for directory in \
  usr \
  usr/share \
  usr/share/mnemosyne-expedition \
  usr/share/mnemosyne-expedition/dasobjectstore-development-validation \
  usr/share/mnemosyne-expedition/development-cohorts; do
  ensure_directory "$directory"
done

stage_asset() {
  local source_relative="$1"
  local destination_relative="$2"
  local source="$asset_root/$source_relative"
  local destination="$payload_root/$destination_relative"
  if [[ ! -f "$source" || -L "$source" ]]; then
    printf 'development-validation cohort source asset is unsafe: %s\n' "$source_relative" >&2
    exit 1
  fi
  if [[ -L "$destination" ]]; then
    printf 'development-validation cohort staging refuses symlinked destination: %s\n' "$destination_relative" >&2
    exit 1
  fi
  install -m 0644 "$source" "$destination"
}

for asset in manifest.json policy.json task-catalog.json Containerfile.ci README.md; do
  stage_asset "$asset" "usr/share/mnemosyne-expedition/dasobjectstore-development-validation/$asset"
done
stage_asset cohort.json \
  usr/share/mnemosyne-expedition/development-cohorts/dasobjectstore-development-validation.json
