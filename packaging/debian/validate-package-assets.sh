#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
service="$repo_root/packaging/linux/systemd/dasobjectstored.service"
sysusers="$repo_root/packaging/linux/sysusers.d/dasobjectstore.conf"
tmpfiles="$repo_root/packaging/linux/tmpfiles.d/dasobjectstore.conf"
daemon_config="$repo_root/packaging/linux/etc/dasobjectstore/daemon.json"
postinst="$repo_root/packaging/debian/postinst"
build_deb="$repo_root/packaging/debian/build-deb.sh"

require_file() {
  local path="$1"
  if [[ ! -f "$path" ]]; then
    printf 'missing package asset: %s\n' "$path" >&2
    exit 1
  fi
}

require_text() {
  local path="$1"
  local expected="$2"
  if ! grep -Fq "$expected" "$path"; then
    printf 'package asset %s must contain: %s\n' "$path" "$expected" >&2
    exit 1
  fi
}

require_file "$service"
require_file "$sysusers"
require_file "$tmpfiles"
require_file "$daemon_config"
require_file "$postinst"
require_file "$build_deb"

require_text "$service" "User=dasobjectstore"
require_text "$service" "Group=dasobjectstore"
require_text "$service" "RuntimeDirectory=dasobjectstore"
require_text "$service" "ProtectHome=read-only"
require_text "$service" "ReadWritePaths=/run/dasobjectstore /var/lib/dasobjectstore /var/log/dasobjectstore /srv/dasobjectstore"

require_text "$sysusers" "u dasobjectstore"
require_text "$sysusers" "g dasobjectstore"

require_text "$tmpfiles" "z /srv/dasobjectstore 0750 dasobjectstore dasobjectstore -"
require_text "$daemon_config" "\"socket_path\": \"/run/dasobjectstore/dasobjectstored.sock\""

require_text "$postinst" "service_user=\"dasobjectstore\""
require_text "$postinst" "managed_root=\"/srv/dasobjectstore\""
require_text "$postinst" 'reject_user_owned_managed_root "$managed_root"'
require_text "$postinst" 'Managed DAS roots must be owned by $service_user:$service_group'

require_text "$build_deb" "cargo build --release -p dasobjectstore-daemon"
require_text "$build_deb" 'target/release/dasobjectstored'
require_text "$build_deb" 'lib/systemd/system/dasobjectstored.service'
require_text "$build_deb" 'usr/lib/sysusers.d/dasobjectstore.conf'
require_text "$build_deb" 'usr/lib/tmpfiles.d/dasobjectstore.conf'
require_text "$build_deb" 'DEBIAN/postinst'
