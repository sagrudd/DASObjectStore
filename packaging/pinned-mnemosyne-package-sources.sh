#!/usr/bin/env bash
# shellcheck shell=bash

# Release packages consume immutable Git revisions. Prove that the manifest
# and lockfile agree without substituting mutable sibling worktrees.
das_package_configure_pinned_mnemosyne_sources() {
  local repo_root="$1"
  local prosopikon_revision locked_prosopikon_revisions pistis_revisions

  if ! command -v git >/dev/null 2>&1; then
    printf 'DASObjectStore package build requires git to verify pinned Mnemosyne sources\n' >&2
    return 1
  fi

  prosopikon_revision="$(sed -n 's/^prosopikon-core = { git = "https:\/\/github.com\/sagrudd\/prosopikon.git", rev = "\([0-9a-f]\{40\}\)" }$/\1/p' "$repo_root/Cargo.toml")"

  if [[ ! "$prosopikon_revision" =~ ^[0-9a-f]{40}$ ]]; then
    printf 'DASObjectStore package build requires one exact Prosopikon revision in Cargo.toml\n' >&2
    return 1
  fi

  locked_prosopikon_revisions="$(sed -n 's#^source = "git+https://github.com/sagrudd/prosopikon.git?rev=\([0-9a-f]\{40\}\).*#\1#p' "$repo_root/Cargo.lock" | sort -u)"
  if [[ "$locked_prosopikon_revisions" != "$prosopikon_revision" ]]; then
    printf 'DASObjectStore package build requires Cargo.lock at Prosopikon %s, found %s\n' \
      "$prosopikon_revision" "${locked_prosopikon_revisions:-unresolved}" >&2
    return 1
  fi

  pistis_revisions="$(sed -n 's#^source = "git+https://github.com/sagrudd/pistis.git?rev=\([0-9a-f]\{40\}\).*#\1#p' "$repo_root/Cargo.lock" | sort -u)"
  if [[ "$(printf '%s\n' "$pistis_revisions" | sed '/^$/d' | wc -l | tr -d ' ')" != "1" || ! "$pistis_revisions" =~ ^[0-9a-f]{40}$ ]]; then
    printf 'DASObjectStore package build requires one exact Pistis revision in Cargo.lock\n' >&2
    return 1
  fi

  export DAS_PACKAGE_PROSOPIKON_REVISION="$prosopikon_revision"
  export DAS_PACKAGE_PISTIS_REVISION="$pistis_revisions"
}

das_package_write_pinned_dependency_provenance() {
  local package_path="$1"
  printf '{"schema":"mnemosyne.dasobjectstore.package-dependencies.v1","prosopikon_revision":"%s","pistis_revision":"%s"}\n' \
    "$DAS_PACKAGE_PROSOPIKON_REVISION" "$DAS_PACKAGE_PISTIS_REVISION" >"$package_path.dependencies.json"
}
