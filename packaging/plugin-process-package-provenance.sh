#!/usr/bin/env bash
# Immutable evidence for the source-only plugin-process DEB fixture.
set -euo pipefail

das_plugin_process_sha256() {
  shasum -a 256 "$1" | awk '{print $1}'
}

das_plugin_process_tree_sha256() {
  local tree=$1
  (cd "$tree" && find . -type f -exec shasum -a 256 {} \; | LC_ALL=C sort | shasum -a 256 | awk '{print $1}')
}

das_plugin_process_write_provenance() {
  local package_path=$1 repo_root=$2 version=$3 architecture=$4 web_dist=$5 server=$6
  local source_revision source_epoch descriptor
  source_revision="${DASOBJECTSTORE_SOURCE_REVISION:-$(git -C "$repo_root" rev-parse HEAD)}"
  source_epoch="${SOURCE_DATE_EPOCH:-$(git -C "$repo_root" log -1 --format=%ct)}"
  descriptor="$repo_root/packaging/linux/opt/dasobjectstore/plugin-process-descriptor.json"
  [[ "$source_revision" =~ ^[0-9a-f]{40}$ ]] || { echo "plugin package requires an exact source revision" >&2; return 1; }
  [[ "$source_epoch" =~ ^[0-9]+$ ]] || { echo "plugin package requires SOURCE_DATE_EPOCH" >&2; return 1; }
  printf '{"schema":"mnemosyne.dasobjectstore.plugin-process-package-provenance.v1","package_name":"dasobjectstore-plugin-process","package_version":"%s","architecture":"%s","source_revision":"%s","source_date_epoch":%s,"cargo_toml_sha256":"%s","cargo_lock_sha256":"%s","descriptor_source_sha256":"%s","web_assets_sha256":"%s","server_sha256":"%s","package_sha256":"%s","command":"packaging/debian/build-plugin-process-deb.sh"}\n' \
    "$version" "$architecture" "$source_revision" "$source_epoch" \
    "$(das_plugin_process_sha256 "$repo_root/Cargo.toml")" \
    "$(das_plugin_process_sha256 "$repo_root/Cargo.lock")" \
    "$(das_plugin_process_sha256 "$descriptor")" \
    "$(das_plugin_process_tree_sha256 "$web_dist")" \
    "$(das_plugin_process_sha256 "$server")" \
    "$(das_plugin_process_sha256 "$package_path")" > "$package_path.provenance.json"
}
