#!/usr/bin/env bash

das_package_provenance_init() {
  local repo_root="$1"
  DAS_PACKAGE_SOURCE_REVISION="${DASOBJECTSTORE_SOURCE_REVISION:-}"
  [ -n "$DAS_PACKAGE_SOURCE_REVISION" ] || DAS_PACKAGE_SOURCE_REVISION="$(git -C "$repo_root" rev-parse HEAD 2>/dev/null || true)"
  case "$DAS_PACKAGE_SOURCE_REVISION" in *[!0-9a-f]*|'') echo "exact source revision required" >&2; return 1;; esac
  DAS_PACKAGE_SOURCE_EPOCH="${SOURCE_DATE_EPOCH:-}"
  [ -n "$DAS_PACKAGE_SOURCE_EPOCH" ] || DAS_PACKAGE_SOURCE_EPOCH="$(git -C "$repo_root" log -1 --format=%ct 2>/dev/null || true)"
  case "$DAS_PACKAGE_SOURCE_EPOCH" in *[!0-9]*|'') echo "non-negative SOURCE_DATE_EPOCH required" >&2; return 1;; esac
  export DAS_PACKAGE_SOURCE_REVISION DAS_PACKAGE_SOURCE_EPOCH
}
das_package_normalize_tree() { find "$1" -exec touch -h -d "@$DAS_PACKAGE_SOURCE_EPOCH" {} +; }
das_package_write_provenance() {
  local package_path="$1" profile="$2" arch="$3"
  printf '{"schema":"mnemosyne.dasobjectstore.package-provenance.v1","source_revision":"%s","source_date_epoch":%s,"profile":"%s","architecture":"%s","package_sha256":"%s"}\n' "$DAS_PACKAGE_SOURCE_REVISION" "$DAS_PACKAGE_SOURCE_EPOCH" "$profile" "$arch" "$(sha256sum "$package_path" | awk '{print $1}')" >"$package_path.provenance.json"
}
