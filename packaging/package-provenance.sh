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
das_package_normalize_tree() {
  local stamp
  if date -u -d "@$DAS_PACKAGE_SOURCE_EPOCH" +%Y%m%d%H%M.%S >/dev/null 2>&1; then
    stamp="$(date -u -d "@$DAS_PACKAGE_SOURCE_EPOCH" +%Y%m%d%H%M.%S)"
  else
    stamp="$(date -u -r "$DAS_PACKAGE_SOURCE_EPOCH" +%Y%m%d%H%M.%S)"
  fi
  find "$1" -exec touch -h -t "$stamp" {} +
}
das_package_write_provenance() {
  local package_path="$1" profile="$2" arch="$3"
  printf '{"schema":"mnemosyne.dasobjectstore.package-provenance.v1","source_revision":"%s","source_date_epoch":%s,"profile":"%s","architecture":"%s","package_sha256":"%s"}\n' "$DAS_PACKAGE_SOURCE_REVISION" "$DAS_PACKAGE_SOURCE_EPOCH" "$profile" "$arch" "$(sha256sum "$package_path" | awk '{print $1}')" >"$package_path.provenance.json"
}
das_package_write_formal_remote_provenance() {
  local package_path="$1" arch="$2" package_version="$3"
  : "${DAS_PACKAGE_LOCKSET_ID:?formal remote package provenance requires a lockset id}"
  : "${DAS_PACKAGE_LOCKSET_CONTENT_DIGEST:?formal remote package provenance requires a lockset content digest}"
  : "${DAS_PACKAGE_LOCKSET_REGISTRY_DIGEST:?formal remote package provenance requires a lockset registry digest}"
  : "${DAS_PACKAGE_RELEASE_SOURCE_REVISION:?formal remote package provenance requires a release source revision}"
  if [[ "$DAS_PACKAGE_SOURCE_REVISION" != "$DAS_PACKAGE_RELEASE_SOURCE_REVISION" ]]; then
    echo "formal remote package provenance source revision does not match release input" >&2
    return 1
  fi
  printf '{"schema":"mnemosyne.dasobjectstore.package-provenance.v2","component_id":"dasobjectstore-remote","package_name":"dasobjectstore-remote","package_version":"%s","source_revision":"%s","source_date_epoch":%s,"profile":"remote","architecture":"%s","lockset_id":"%s","lockset_content_digest":"%s","lockset_registry_digest":"%s","package_sha256":"%s"}\n' \
    "$package_version" "$DAS_PACKAGE_SOURCE_REVISION" "$DAS_PACKAGE_SOURCE_EPOCH" "$arch" \
    "$DAS_PACKAGE_LOCKSET_ID" "$DAS_PACKAGE_LOCKSET_CONTENT_DIGEST" "$DAS_PACKAGE_LOCKSET_REGISTRY_DIGEST" \
    "$(sha256sum "$package_path" | awk '{print $1}')" >"$package_path.provenance.json"
}
