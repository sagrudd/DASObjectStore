#!/usr/bin/env bash
# Stages reviewed custody templates as package documentation only. This helper
# never renders a template, writes a credential, or targets a live filesystem.
set -euo pipefail

readonly CUSTODY_REVIEW_DOC_RELATIVE_DIR="usr/share/doc/dasobjectstore/custody-review"

das_stage_custody_review_assets() {
  local payload_root="${1:?package payload root is required}"
  local repo_root
  local destination
  local asset
  local -a assets=(
    "README.rst"
    "custody-garage.compose.yml.template"
    "dasobjectstore-custody-garage.service.template"
    "dasobjectstored-custody-credentials.conf.template"
  )

  if [[ "$payload_root" == "/" ]]; then
    printf 'refusing to stage custody review assets into the live root filesystem\n' >&2
    return 1
  fi
  if [[ ! -d "$payload_root" ]]; then
    printf 'package payload root does not exist: %s\n' "$payload_root" >&2
    return 1
  fi
  if [[ -L "$payload_root" ]]; then
    printf 'refusing to stage custody review assets through a symlinked payload root\n' >&2
    return 1
  fi

  repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
  destination="$payload_root/$CUSTODY_REVIEW_DOC_RELATIVE_DIR"
  for path in \
    "$payload_root/usr" \
    "$payload_root/usr/share" \
    "$payload_root/usr/share/doc" \
    "$payload_root/usr/share/doc/dasobjectstore" \
    "$destination"; do
    if [[ -L "$path" ]]; then
      printf 'refusing to stage custody review assets through a symlinked payload path: %s\n' "$path" >&2
      return 1
    fi
  done
  install -d -m 0755 "$destination"
  for asset in "${assets[@]}"; do
    if [[ -L "$destination/$asset" ]]; then
      printf 'refusing to stage custody review assets through a symlinked destination asset: %s\n' "$destination/$asset" >&2
      return 1
    fi
  done
  install -m 0644 \
    "$repo_root/packaging/linux/systemd/dasobjectstore-custody-garage.service.template" \
    "$destination/dasobjectstore-custody-garage.service.template"
  install -m 0644 \
    "$repo_root/packaging/linux/templates/custody-garage.compose.yml.template" \
    "$destination/custody-garage.compose.yml.template"
  install -m 0644 \
    "$repo_root/packaging/linux/systemd/dasobjectstored-custody-credentials.conf.template" \
    "$destination/dasobjectstored-custody-credentials.conf.template"
  install -m 0644 \
    "$repo_root/docs/user/local-custody-review-assets.rst" \
    "$destination/README.rst"
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  if [[ "$#" -ne 1 ]]; then
    printf 'usage: %s <package-payload-root>\n' "$0" >&2
    exit 64
  fi
  das_stage_custody_review_assets "$1"
fi
