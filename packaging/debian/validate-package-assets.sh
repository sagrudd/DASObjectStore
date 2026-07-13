#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
service="$repo_root/packaging/linux/systemd/dasobjectstored.service"
web_service="$repo_root/packaging/linux/systemd/dasobjectstore-server.service"
source_access_service="$repo_root/packaging/linux/systemd/dasobjectstore-source-access.service"
source_access_path="$repo_root/packaging/linux/systemd/dasobjectstore-source-access.path"
source_access_helper="$repo_root/packaging/linux/usr/libexec/dasobjectstore/prepare-external-mount-traversal"
mount_policy_helper="$repo_root/packaging/linux/usr/libexec/dasobjectstore/configure-external-mount-policy"
sysusers="$repo_root/packaging/linux/sysusers.d/dasobjectstore.conf"
tmpfiles="$repo_root/packaging/linux/tmpfiles.d/dasobjectstore.conf"
daemon_config="$repo_root/packaging/linux/etc/dasobjectstore/daemon.json"
web_config="$repo_root/packaging/linux/opt/dasobjectstore/config.json"
pam_service="$repo_root/packaging/linux/pam.d/dasobjectstore"
reporting_wrapper="$repo_root/packaging/reporting/gnostikon-workflow-control"
postinst="$repo_root/packaging/debian/postinst"
build_deb="$repo_root/packaging/debian/build-deb.sh"
build_rpm="$repo_root/packaging/rpm/build-rpm.sh"
build_remote_deb="$repo_root/packaging/debian/build-remote-deb.sh"
build_remote_rpm="$repo_root/packaging/rpm/build-remote-rpm.sh"
prepare_web_dist="$repo_root/packaging/web/prepare-web-dist.sh"
package_auth_guard="$repo_root/packaging/validate-package-auth-content.sh"

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
  if ! grep -Fq -- "$expected" "$path"; then
    printf 'package asset %s must contain: %s\n' "$path" "$expected" >&2
    exit 1
  fi
}

require_absent() {
  local path="$1"
  local forbidden="$2"
  if grep -Fq -- "$forbidden" "$path"; then
    printf 'package build asset %s must not contain: %s\n' "$path" "$forbidden" >&2
    exit 1
  fi
}

require_file "$service"
require_file "$web_service"
require_file "$source_access_service"
require_file "$source_access_path"
require_file "$source_access_helper"
require_file "$mount_policy_helper"
require_file "$sysusers"
require_file "$tmpfiles"
require_file "$daemon_config"
require_file "$web_config"
require_file "$pam_service"
require_file "$reporting_wrapper"
require_file "$postinst"
require_file "$build_deb"
require_file "$build_rpm"
require_file "$build_remote_deb"
require_file "$build_remote_rpm"
require_file "$prepare_web_dist"
require_file "$package_auth_guard"

require_text "$service" "User=dasobjectstore"
require_text "$service" "Group=dasobjectstore"
require_text "$service" "RuntimeDirectory=dasobjectstore"
require_text "$service" "ProtectHome=read-only"
require_text "$service" "ReadWritePaths=/run/dasobjectstore /var/lib/dasobjectstore /var/log/dasobjectstore /srv/dasobjectstore"

require_text "$web_service" "User=dasobjectstore"
require_text "$web_service" "Group=dasobjectstore"
require_text "$web_service" "NoNewPrivileges=false"
require_text "$web_service" "ExecStart=/usr/bin/dasobjectstore-server --config /opt/dasobjectstore/config.json --generate-missing-tls"
require_text "$web_service" "ReadWritePaths=/run/dasobjectstore /var/lib/dasobjectstore /var/log/dasobjectstore /opt/dasobjectstore"
require_text "$source_access_service" "ExecStart=/usr/libexec/dasobjectstore/prepare-external-mount-traversal"
require_text "$source_access_path" "PathChanged=/run/media"
require_text "$source_access_path" "PathChanged=/media"
require_text "$source_access_helper" 'setfacl -m "u:${service_user}:--x" "$path"'
require_text "$mount_policy_helper" 'UDISKS_MOUNT_OPTIONS_EXFAT_DEFAULTS'
require_text "$mount_policy_helper" 'UDISKS_MOUNT_OPTIONS_NTFS_DEFAULTS'

require_text "$sysusers" "u dasobjectstore"
require_text "$sysusers" "g dasobjectstore"
require_text "$sysusers" "g dasobjectstore-admin"

require_text "$tmpfiles" "z /srv/dasobjectstore 0750 dasobjectstore dasobjectstore -"
require_text "$tmpfiles" "d /opt/dasobjectstore 0750 dasobjectstore dasobjectstore -"
require_text "$tmpfiles" "d /var/lib/dasobjectstore/object-service 0700 dasobjectstore dasobjectstore -"
require_text "$tmpfiles" "d /var/lib/dasobjectstore/report-rebuild 0750 dasobjectstore dasobjectstore -"
require_text "$tmpfiles" "d /var/lib/dasobjectstore/telemetry 0750 dasobjectstore dasobjectstore -"
require_text "$web_config" "\"bind_address\": \"0.0.0.0\""
require_text "$web_config" "\"https_port\": 8448"
require_text "$daemon_config" "\"socket_path\": \"/run/dasobjectstore/dasobjectstored.sock\""
require_text "$daemon_config" "\"telemetry\": {"
require_text "$daemon_config" "\"cadence_seconds\": 30"
require_text "$pam_service" "auth required pam_unix.so"
require_text "$pam_service" "account required pam_unix.so"

require_text "$postinst" "service_user=\"dasobjectstore\""
require_text "$postinst" "managed_root=\"/srv/dasobjectstore\""
require_text "$postinst" "product_root=\"/opt/dasobjectstore\""
require_text "$postinst" 'ensure_owned_dir /var/lib/dasobjectstore/report-rebuild 0750'
require_text "$postinst" 'ensure_owned_dir /var/lib/dasobjectstore/object-service 0700'
require_text "$postinst" 'ensure_owned_dir /var/lib/dasobjectstore/telemetry 0750'
require_text "$postinst" 'ensure_container_runtime_access'
require_text "$postinst" 'admin_group="dasobjectstore-admin"'
require_text "$postinst" 'ensure_web_admin_peer_membership'
require_text "$postinst" 'dasobjectstore-source-access.path'
require_text "$postinst" 'start dasobjectstore-source-access.service'
require_text "$postinst" 'configure-external-mount-policy'
require_text "$postinst" 'usermod -aG "$admin_group" "$service_user"'
require_text "$postinst" 'usermod -aG docker "$service_user"'
require_text "$postinst" 'restart dasobjectstore-server.service'
require_text "$postinst" "find /etc/dasobjectstore -maxdepth 1 -type f -name '*.json'"
require_text "$postinst" "-exec chgrp \"\$service_group\" {} +"
require_text "$postinst" "-exec chmod 0640 {} +"
require_text "$postinst" "chown root:\"\$service_group\" /usr/libexec/dasobjectstore/dasobjectstore-local-auth-helper"
require_text "$postinst" "chmod 4750 /usr/libexec/dasobjectstore/dasobjectstore-local-auth-helper"
require_text "$postinst" 'reject_user_owned_managed_root "$managed_root"'
require_text "$postinst" 'repair_managed_tree "$managed_root/ssd"'
require_text "$postinst" 'repair_managed_tree "$root"'
require_text "$postinst" "systemctl enable --now dasobjectstored.service dasobjectstore-server.service"
require_text "$postinst" "systemctl restart dasobjectstored.service dasobjectstore-server.service"
require_text "$postinst" 'Managed DAS roots must be owned by $service_user:$service_group'

require_text "$build_deb" "cargo build --release -p dasobjectstore-daemon"
require_text "$build_deb" "cargo build --release -p dasobjectstore-remote"
require_text "$build_deb" "dpkg-deb is required to build the DASObjectStore Debian package."
require_text "$build_deb" 'target/release/dasobjectstored'
require_text "$build_deb" 'target/release/dasobjectstore-remote'
require_text "$build_deb" 'target/release/dasobjectstore-local-auth-helper'
require_text "$build_deb" 'packaging/web/prepare-web-dist.sh'
require_text "$build_deb" 'lib/systemd/system/dasobjectstored.service'
require_text "$build_deb" 'lib/systemd/system/dasobjectstore-server.service'
require_text "$build_deb" 'lib/systemd/system/dasobjectstore-source-access.service'
require_text "$build_deb" 'lib/systemd/system/dasobjectstore-source-access.path'
require_text "$build_deb" 'opt/dasobjectstore/config.json'
require_text "$build_deb" 'opt/dasobjectstore/web'
require_text "$build_deb" 'etc/pam.d/dasobjectstore'
require_text "$build_deb" 'usr/lib/sysusers.d/dasobjectstore.conf'
require_text "$build_deb" 'usr/lib/tmpfiles.d/dasobjectstore.conf'
require_text "$build_deb" 'usr/libexec/dasobjectstore/gnostikon-workflow-control'
require_text "$build_deb" 'usr/libexec/dasobjectstore/prepare-external-mount-traversal'
require_text "$build_deb" 'usr/libexec/dasobjectstore/configure-external-mount-policy'
require_text "$build_deb" 'DEBIAN/postinst'
require_text "$build_deb" 'Depends: ca-certificates, acl, libpam0g, udisks2, docker.io, docker-buildx | docker-buildx-plugin'
require_text "$build_rpm" 'Requires:       udisks2'
require_text "$build_rpm" '/usr/libexec/dasobjectstore/configure-external-mount-policy'
require_text "$build_rpm" '/usr/lib/systemd/system/dasobjectstore-source-access.path'
require_text "$build_deb" 'X-DASObjectStore-Build-Depends: rustc, cargo, trunk, wasm32-unknown-unknown, clang, libclang-dev, libpam0g-dev, dpkg, docker-buildx'
require_text "$build_deb" 'X-Prosopikon-Native-Dependency-Markers: $prosopikon_pam_marker'
require_text "$build_deb" 'sudo apt-get install clang libclang-dev libpam0g-dev'
require_text "$repo_root/Makefile" 'gnostikon'
require_text "$repo_root/Makefile" 'report-provider'
require_text "$repo_root/Makefile" 'grammateus_report_provider'
require_text "$reporting_wrapper" 'render-report-pdf'
require_text "$reporting_wrapper" 'prewarm-report-provider'
require_text "$reporting_wrapper" 'verify_container_runtime_access'
require_text "$reporting_wrapper" 'grammateus_report_provider install'
require_text "$reporting_wrapper" 'sudo usermod -aG docker dasobjectstore'
require_text "$reporting_wrapper" 'grammateus_markdown_pdf'
require_text "$reporting_wrapper" 'docker_args=(run --rm'

require_text "$build_rpm" "rpmbuild"
require_text "$build_rpm" "cargo build --release -p dasobjectstore-daemon"
require_text "$build_rpm" "cargo build --release -p dasobjectstore-remote"
require_text "$build_rpm" 'target/release/dasobjectstored'
require_text "$build_rpm" 'target/release/dasobjectstore-remote'
require_text "$build_rpm" 'target/release/dasobjectstore-local-auth-helper'
require_text "$build_rpm" 'packaging/web/prepare-web-dist.sh'
require_text "$build_rpm" 'usr/lib/systemd/system/dasobjectstored.service'
require_text "$build_rpm" 'etc/pam.d/dasobjectstore'
require_text "$build_rpm" 'usr/lib/sysusers.d/dasobjectstore.conf'
require_text "$build_rpm" 'usr/lib/tmpfiles.d/dasobjectstore.conf'
require_text "$build_rpm" 'systemd-sysusers /usr/lib/sysusers.d/dasobjectstore.conf'
require_text "$build_rpm" 'systemd-tmpfiles --create /usr/lib/tmpfiles.d/dasobjectstore.conf'
require_text "$build_rpm" 'install -d -o "\$service_user" -g "\$service_group" -m 0750 /var/lib/dasobjectstore/report-rebuild'
require_text "$build_rpm" 'install -d -o "\$service_user" -g "\$service_group" -m 0700 /var/lib/dasobjectstore/object-service'
require_text "$build_rpm" 'install -d -o "\$service_user" -g "\$service_group" -m 0750 /var/lib/dasobjectstore/telemetry'
require_text "$build_rpm" '/usr/libexec/dasobjectstore/dasobjectstore-local-auth-helper'
require_text "$build_rpm" '/usr/libexec/dasobjectstore/gnostikon-workflow-control'
require_text "$build_rpm" 'prewarm-report-provider'
require_text "$build_rpm" 'grammateus_report_provider install --image grammateus/report:0.8.1'
require_text "$build_rpm" 'BuildRequires:  clang'
require_text "$build_rpm" 'BuildRequires:  libclang-devel'
require_text "$build_rpm" 'BuildRequires:  pam-devel'
require_text "$build_rpm" 'Provides:       prosopikon-native(pam)'
require_text "$build_rpm" 'Requires:       pam'
require_text "$build_rpm" 'usermod -aG docker "\$service_user"'
require_text "$build_rpm" 'admin_group="dasobjectstore-admin"'
require_text "$build_rpm" 'usermod -aG "\$admin_group" "\$service_user"'
require_text "$build_rpm" 'Requires:       /usr/bin/docker'
require_text "$build_rpm" 'Requires:       docker-buildx-plugin'
require_text "$build_rpm" 'Requires:         awscli'
require_text "$build_rpm" 'sudo dnf install clang libclang-devel pam-devel'

require_text "$build_remote_deb" "cargo build --release -p dasobjectstore-remote"
require_text "$build_remote_deb" "dpkg-deb is required to build the DASObjectStore remote Debian package."
require_text "$build_remote_deb" 'target/release/dasobjectstore-remote'
require_text "$build_remote_deb" 'docs/user/remote-client.rst'
require_text "$build_remote_deb" 'Package: $package_name'
require_text "$build_remote_deb" 'Suggests: awscli'

require_text "$build_remote_rpm" "rpmbuild is required to build the DASObjectStore remote RPM."
require_text "$build_remote_rpm" "cargo build --release -p dasobjectstore-remote"
require_text "$build_remote_rpm" 'target/release/dasobjectstore-remote'
require_text "$build_remote_rpm" 'docs/user/remote-client.rst'
require_text "$build_remote_rpm" '/usr/bin/dasobjectstore-remote'
require_text "$build_remote_rpm" 'Recommends:      awscli'
require_absent "$build_deb" 'development-self-signing'
require_absent "$build_rpm" 'development-self-signing'
require_absent "$build_remote_deb" 'development-self-signing'
require_absent "$build_remote_rpm" 'development-self-signing'

require_text "$prepare_web_dist" "trunk build --release"
require_text "$prepare_web_dist" "wasm32-unknown-unknown"
require_text "$prepare_web_dist" "validate_prosopikon_checkout"
require_text "$prepare_web_dist" "prosopikon-core must expose the auth and pam features"
require_text "$prepare_web_dist" "*.wasm"
require_text "$prepare_web_dist" "--allow-fallback"
require_text "$prepare_web_dist" "target/web-fallback/dist"
require_text "$build_deb" "validate-package-auth-content.sh"
require_text "$build_rpm" "validate-package-auth-content.sh"
require_text "$build_remote_deb" "validate-package-auth-content.sh"
require_text "$build_remote_rpm" "validate-package-auth-content.sh"
