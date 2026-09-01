#!/usr/bin/env bash
# shellcheck shell=bash

# The workspace patches private Mnemosyne dependencies to sibling checkouts.
# Release package builds must prove the checked-out revisions rather than
# silently accepting whatever a developer happens to have locally.
das_package_configure_pinned_mnemosyne_sources() {
  local repo_root="$1"
  local workspace_root prosopikon_root pistis_root proxenos_root thesaurophylax_root
  local prosopikon_revision proxenos_version proxenos_revision
  local thesaurophylax_version thesaurophylax_revision pistis_revisions
  local proxenos_contract thesaurophylax_contract

  if ! command -v git >/dev/null 2>&1; then
    printf 'DASObjectStore package build requires git to verify pinned Mnemosyne sources\n' >&2
    return 1
  fi

  workspace_root="$(cd "$repo_root/.." && pwd)"
  prosopikon_root="$workspace_root/prosopikon"
  pistis_root="$workspace_root/pistis"
  proxenos_root="$workspace_root/proxenos"
  thesaurophylax_root="$workspace_root/thesaurophylax"
  prosopikon_revision="$(sed -n 's/^prosopikon-core = { git = "https:\/\/github.com\/sagrudd\/prosopikon.git", rev = "\([0-9a-f]\{40\}\)" }$/\1/p' "$repo_root/Cargo.toml")"
  proxenos_contract="$(sed -n 's/^proxenos = { version = "=\([0-9][0-9.]*\)", git = "https:\/\/github.com\/sagrudd\/proxenos.git", rev = "\([0-9a-f]\{40\}\)" }$/\1 \2/p' "$repo_root/Cargo.toml")"

  if [[ ! "$prosopikon_revision" =~ ^[0-9a-f]{40}$ ]]; then
    printf 'DASObjectStore package build requires one exact Prosopikon revision in Cargo.toml\n' >&2
    return 1
  fi
  if [[ "$(printf '%s\n' "$proxenos_contract" | sed '/^$/d' | wc -l | tr -d ' ')" != "1" ]]; then
    printf 'DASObjectStore package build requires one exact Proxenos version and revision in Cargo.toml\n' >&2
    return 1
  fi
  read -r proxenos_version proxenos_revision <<<"$proxenos_contract"
  if [[ ! "$proxenos_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ || ! "$proxenos_revision" =~ ^[0-9a-f]{40}$ ]]; then
    printf 'DASObjectStore package build requires one valid exact Proxenos version and revision in Cargo.toml\n' >&2
    return 1
  fi

  thesaurophylax_contract="$(sed -n 's/^thesaurophylax-api = { version = "=\([0-9][0-9.]*\)", git = "https:\/\/github.com\/sagrudd\/thesaurophylax.git", rev = "\([0-9a-f]\{40\}\)" }$/\1 \2/p' "$proxenos_root/Cargo.toml" 2>/dev/null)"
  if [[ "$(printf '%s\n' "$thesaurophylax_contract" | sed '/^$/d' | wc -l | tr -d ' ')" != "1" ]]; then
    printf 'DASObjectStore package build requires one exact Thesaurophylax version and revision in sibling Proxenos\n' >&2
    return 1
  fi
  read -r thesaurophylax_version thesaurophylax_revision <<<"$thesaurophylax_contract"
  if [[ ! "$thesaurophylax_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ || ! "$thesaurophylax_revision" =~ ^[0-9a-f]{40}$ ]]; then
    printf 'DASObjectStore package build requires one valid exact Thesaurophylax version and revision in sibling Proxenos\n' >&2
    return 1
  fi

  pistis_revisions="$(awk -F 'rev = "' '/^pistis-(canonical|cose|crypto|protocol) = \{ git = "https:\/\/github.com\/sagrudd\/pistis.git", rev = "/ { split($2, value, "\""); print value[1] }' "$prosopikon_root/Cargo.toml" 2>/dev/null | sort -u)"
  if [[ "$(printf '%s\n' "$pistis_revisions" | sed '/^$/d' | wc -l | tr -d ' ')" != "1" || ! "$pistis_revisions" =~ ^[0-9a-f]{40}$ ]]; then
    printf 'DASObjectStore package build requires one exact Pistis revision in sibling Prosopikon\n' >&2
    return 1
  fi

  das_require_clean_pinned_checkout "$prosopikon_root" "$prosopikon_revision" "Prosopikon" || return 1
  das_require_clean_pinned_checkout "$pistis_root" "$pistis_revisions" "Pistis" || return 1
  das_require_clean_pinned_checkout "$proxenos_root" "$proxenos_revision" "Proxenos" || return 1
  das_require_clean_pinned_checkout "$thesaurophylax_root" "$thesaurophylax_revision" "Thesaurophylax" || return 1
  das_require_locked_git_package "$repo_root/Cargo.lock" "proxenos" "$proxenos_version" "$proxenos_revision" "Proxenos" || return 1
  das_require_locked_git_package "$repo_root/Cargo.lock" "thesaurophylax-api" "$thesaurophylax_version" "$thesaurophylax_revision" "Thesaurophylax API" || return 1
  das_require_locked_git_package "$repo_root/Cargo.lock" "thesaurophylax-core" "$thesaurophylax_version" "$thesaurophylax_revision" "Thesaurophylax core" || return 1
  das_require_locked_git_package "$repo_root/Cargo.lock" "thesaurophylax-policy" "$thesaurophylax_version" "$thesaurophylax_revision" "Thesaurophylax policy" || return 1
  das_require_locked_git_package "$repo_root/Cargo.lock" "thesaurophylax-store" "$thesaurophylax_version" "$thesaurophylax_revision" "Thesaurophylax store" || return 1
  export DAS_PACKAGE_PROSOPIKON_REVISION="$prosopikon_revision"
  export DAS_PACKAGE_PISTIS_REVISION="$pistis_revisions"
  export DAS_PACKAGE_PROXENOS_REVISION="$proxenos_revision"
  export DAS_PACKAGE_THESAUROPHYLAX_REVISION="$thesaurophylax_revision"
}

das_require_locked_git_package() {
  local lockfile="$1" package_name="$2" expected_version="$3" expected_revision="$4" label="$5"
  local expected_source actual_records

  if [[ ! -f "$lockfile" ]]; then
    printf 'DASObjectStore package build requires Cargo.lock to verify %s closure\n' "$label" >&2
    return 1
  fi
  case "$package_name" in
    proxenos)
      expected_source="git+https://github.com/sagrudd/proxenos.git?rev=${expected_revision}#${expected_revision}"
      ;;
    thesaurophylax-*)
      expected_source="git+https://github.com/sagrudd/thesaurophylax.git?rev=${expected_revision}#${expected_revision}"
      ;;
    *)
      printf 'DASObjectStore package build cannot verify unknown custody package %s\n' "$package_name" >&2
      return 1
      ;;
  esac

  actual_records="$(awk -v package_name="$package_name" '
    function emit() {
      if (name == package_name) {
        print version "|" source
      }
    }
    /^\[\[package\]\]$/ {
      emit()
      name = ""
      version = ""
      source = ""
      next
    }
    /^name = "/ {
      name = $0
      sub(/^name = "/, "", name)
      sub(/"$/, "", name)
      next
    }
    /^version = "/ {
      version = $0
      sub(/^version = "/, "", version)
      sub(/"$/, "", version)
      next
    }
    /^source = "/ {
      source = $0
      sub(/^source = "/, "", source)
      sub(/"$/, "", source)
    }
    END { emit() }
  ' "$lockfile")"
  if [[ "$(printf '%s\n' "$actual_records" | sed '/^$/d' | wc -l | tr -d ' ')" != "1" || "$actual_records" != "${expected_version}|${expected_source}" ]]; then
    printf 'DASObjectStore package build requires locked %s %s at %s; found %s\n' \
      "$label" "$expected_version" "$expected_revision" "${actual_records:-unresolved}" >&2
    return 1
  fi
}

das_require_clean_pinned_checkout() {
  local checkout="$1" expected_revision="$2" label="$3" actual_revision

  if [[ ! -f "$checkout/Cargo.toml" ]]; then
    printf 'DASObjectStore package build requires sibling %s checkout: %s\n' "$label" "$checkout" >&2
    return 1
  fi
  # Isolated container builders commonly mount source owned by the invoking
  # host user. Scope Git's safe-directory exception to this verified path;
  # do not weaken the builder's global Git configuration.
  actual_revision="$(git -c safe.directory="$checkout" -C "$checkout" rev-parse HEAD 2>/dev/null || true)"
  if [[ "$actual_revision" != "$expected_revision" ]]; then
    printf 'DASObjectStore package build requires %s at %s, found %s\n' "$label" "$expected_revision" "${actual_revision:-unresolved}" >&2
    return 1
  fi
  if [[ -n "$(git -c safe.directory="$checkout" -C "$checkout" status --porcelain 2>/dev/null)" ]]; then
    printf 'DASObjectStore package build requires a clean %s checkout: %s\n' "$label" "$checkout" >&2
    return 1
  fi
}

das_package_write_pinned_dependency_provenance() {
  local package_path="$1"
  printf '{"schema":"mnemosyne.dasobjectstore.package-dependencies.v2","prosopikon_revision":"%s","pistis_revision":"%s","proxenos_revision":"%s","thesaurophylax_revision":"%s"}\n' \
    "$DAS_PACKAGE_PROSOPIKON_REVISION" "$DAS_PACKAGE_PISTIS_REVISION" \
    "$DAS_PACKAGE_PROXENOS_REVISION" "$DAS_PACKAGE_THESAUROPHYLAX_REVISION" >"$package_path.dependencies.json"
}
