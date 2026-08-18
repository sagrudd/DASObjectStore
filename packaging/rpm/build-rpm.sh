#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$repo_root/packaging/package-provenance.sh"; das_package_provenance_init "$repo_root"
source "$repo_root/packaging/pinned-mnemosyne-package-sources.sh"; das_package_configure_pinned_mnemosyne_sources "$repo_root"
package_name="dasobjectstore"
version="$(cargo metadata --no-deps --format-version 1 --manifest-path "$repo_root/Cargo.toml" \
  | sed -n 's/.*"name":"dasobjectstore-cli","version":"\([^"]*\)".*/\1/p')"
version="${version:-0.4.2}"
release="${release:-1}"

if ! command -v rpmbuild >/dev/null 2>&1; then
  cat >&2 <<ERROR
rpmbuild is required to build the DASObjectStore RPM.
On AlmaLinux/RHEL: sudo dnf install rpm-build
On Ubuntu: sudo apt-get install rpm
ERROR
  exit 1
fi

if ! command -v clang >/dev/null 2>&1 || ! ldconfig -p 2>/dev/null | grep -Eq 'libclang(-[0-9]+)?\.so'; then
  cat >&2 <<ERROR
Native DASObjectStore package builds require clang and libclang.
On AlmaLinux/RHEL: sudo dnf install cargo rust clang clang-devel
ERROR
  exit 1
fi

packaging_debian="$repo_root/packaging/debian"
packaging_linux="$repo_root/packaging/linux"
packaging_product="$packaging_linux/opt/dasobjectstore"
packaging_reporting="$repo_root/packaging/reporting"
web_dist="$(bash "$repo_root/packaging/web/prepare-web-dist.sh")"
bash "$packaging_debian/validate-package-assets.sh"

cargo build --release -p dasobjectstore-cli --manifest-path "$repo_root/Cargo.toml"
# Package builds are deliberately feature-minimal: development self-signing
# is a workspace-only test aid and must never enter an RPM payload.
cargo build --release --no-default-features -p dasobjectstore-daemon --manifest-path "$repo_root/Cargo.toml"
cargo build --release -p dasobjectstore-remote --manifest-path "$repo_root/Cargo.toml"
cargo build --release -p dasobjectstore-workspace-host --manifest-path "$repo_root/Cargo.toml"
cargo build --release -p dasobjectstore-mnemosyne --bin dasobjectstore-authority-retirement --manifest-path "$repo_root/Cargo.toml"
cargo build --release -p dasobjectstore-mnemosyne --bin dasobjectstore-authority-retirement-finalize --manifest-path "$repo_root/Cargo.toml"

rpm_root="$repo_root/target/rpm/rpmbuild"
staging_root="$repo_root/target/rpm/staging"
payload_name="${package_name}-${version}"
payload_root="$staging_root/$payload_name"
spec_path="$rpm_root/SPECS/${package_name}.spec"
source_path="$rpm_root/SOURCES/${payload_name}.tar.gz"

rm -rf "$payload_root"
install -d \
  "$payload_root/etc/dasobjectstore" \
  "$payload_root/opt/dasobjectstore" \
  "$payload_root/opt/dasobjectstore/web" \
  "$payload_root/usr/bin" \
  "$payload_root/usr/libexec/dasobjectstore" \
  "$payload_root/usr/lib/systemd/system" \
  "$payload_root/usr/lib/sysusers.d" \
  "$payload_root/usr/lib/tmpfiles.d" \
  "$payload_root/usr/share/doc/$package_name" \
  "$payload_root/usr/share/licenses/$package_name"
install -m 0755 "$repo_root/target/release/dasobjectstore" "$payload_root/usr/bin/dasobjectstore"
install -m 0755 "$repo_root/target/release/dasobjectstore-server" \
  "$payload_root/usr/bin/dasobjectstore-server"
install -m 0755 "$repo_root/target/release/dasobjectstore-s3-gateway" \
  "$payload_root/usr/bin/dasobjectstore-s3-gateway"
install -m 0755 "$repo_root/target/release/dasobjectstored" \
  "$payload_root/usr/bin/dasobjectstored"
install -m 0755 "$repo_root/target/release/dasobjectstore-remote" \
  "$payload_root/usr/bin/dasobjectstore-remote"
install -m 0755 "$repo_root/target/release/dasobjectstore-workspace-host" \
  "$payload_root/usr/libexec/dasobjectstore/dasobjectstore-workspace-host"
install -m 0755 "$repo_root/target/release/dasobjectstore-authority-retirement" \
  "$payload_root/usr/libexec/dasobjectstore/dasobjectstore-authority-retirement"
install -m 0755 "$repo_root/target/release/dasobjectstore-authority-retirement-finalize" \
  "$payload_root/usr/libexec/dasobjectstore/dasobjectstore-authority-retirement-finalize"
install -m 0755 "$packaging_reporting/gnostikon-workflow-control" \
  "$payload_root/usr/libexec/dasobjectstore/gnostikon-workflow-control"
install -m 0755 "$packaging_linux/usr/libexec/dasobjectstore/prepare-external-mount-traversal" \
  "$payload_root/usr/libexec/dasobjectstore/prepare-external-mount-traversal"
install -m 0755 "$packaging_linux/usr/libexec/dasobjectstore/configure-external-mount-policy" \
  "$payload_root/usr/libexec/dasobjectstore/configure-external-mount-policy"
install -m 0755 "$packaging_linux/usr/libexec/dasobjectstore/verify-managed-storage-mounts" \
  "$payload_root/usr/libexec/dasobjectstore/verify-managed-storage-mounts"
install -m 0755 "$packaging_linux/usr/libexec/dasobjectstore/migrate-monas-integrated-config" \
  "$payload_root/usr/libexec/dasobjectstore/migrate-monas-integrated-config"
install -m 0755 "$packaging_linux/usr/libexec/dasobjectstore/manage-monas-access-boundary" \
  "$payload_root/usr/libexec/dasobjectstore/manage-monas-access-boundary"
install -m 0644 "$repo_root/README.md" "$payload_root/usr/share/doc/$package_name/README.md"
install -m 0644 "$repo_root/LICENSE" "$payload_root/usr/share/licenses/$package_name/LICENSE"
install -m 0644 "$packaging_linux/etc/dasobjectstore/daemon.json" \
  "$payload_root/etc/dasobjectstore/daemon.json"
install -m 0640 "$packaging_linux/etc/dasobjectstore/managed-storage.v1.json" \
  "$payload_root/etc/dasobjectstore/managed-storage.v1.json"
install -m 0640 "$packaging_linux/etc/dasobjectstore/s3-gateway.json" \
  "$payload_root/etc/dasobjectstore/s3-gateway.json"
install -m 0640 "$packaging_linux/etc/dasobjectstore/workspace-host.json" \
  "$payload_root/etc/dasobjectstore/workspace-host.json"
install -m 0644 "$packaging_product/config.json" \
  "$payload_root/opt/dasobjectstore/config.json"
install -m 0644 "$packaging_linux/systemd/dasobjectstored.service" \
  "$payload_root/usr/lib/systemd/system/dasobjectstored.service"
install -m 0644 "$packaging_linux/systemd/dasobjectstore-storage-ready.service" \
  "$payload_root/usr/lib/systemd/system/dasobjectstore-storage-ready.service"
install -m 0644 "$packaging_linux/systemd/dasobjectstore-garage.service" \
  "$payload_root/usr/lib/systemd/system/dasobjectstore-garage.service"
install -m 0644 "$packaging_linux/systemd/dasobjectstore-server.service" \
  "$payload_root/usr/lib/systemd/system/dasobjectstore-server.service"
install -m 0644 "$packaging_linux/systemd/dasobjectstore-s3-gateway.service" \
  "$payload_root/usr/lib/systemd/system/dasobjectstore-s3-gateway.service"
install -m 0644 "$packaging_linux/systemd/dasobjectstore-source-access.service" \
  "$payload_root/usr/lib/systemd/system/dasobjectstore-source-access.service"
install -m 0644 "$packaging_linux/systemd/dasobjectstore-source-access.path" \
  "$payload_root/usr/lib/systemd/system/dasobjectstore-source-access.path"
install -m 0644 "$packaging_linux/systemd/dasobjectstore-control.slice" \
  "$payload_root/usr/lib/systemd/system/dasobjectstore-control.slice"
install -m 0644 "$packaging_linux/systemd/dasobjectstore-storage.slice" \
  "$payload_root/usr/lib/systemd/system/dasobjectstore-storage.slice"
install -m 0644 "$packaging_linux/systemd/dasobjectstore-workspace-host.service" \
  "$payload_root/usr/lib/systemd/system/dasobjectstore-workspace-host.service"
install -m 0644 "$packaging_linux/systemd/dasobjectstore-workspace-host.socket" \
  "$payload_root/usr/lib/systemd/system/dasobjectstore-workspace-host.socket"
install -m 0644 "$packaging_linux/systemd/dasobjectstore-authority-retirement.service" \
  "$payload_root/usr/lib/systemd/system/dasobjectstore-authority-retirement.service"
install -m 0644 "$packaging_linux/sysusers.d/dasobjectstore.conf" \
  "$payload_root/usr/lib/sysusers.d/dasobjectstore.conf"
install -m 0644 "$packaging_linux/tmpfiles.d/dasobjectstore.conf" \
  "$payload_root/usr/lib/tmpfiles.d/dasobjectstore.conf"
cp -a "$web_dist/." "$payload_root/opt/dasobjectstore/web/"

bash "$repo_root/packaging/validate-package-auth-content.sh" "$payload_root"
das_package_normalize_tree "$payload_root"

install -d "$rpm_root/BUILD" "$rpm_root/RPMS" "$rpm_root/SOURCES" "$rpm_root/SPECS" "$rpm_root/SRPMS"
tar -C "$staging_root" --sort=name --mtime="@$DAS_PACKAGE_SOURCE_EPOCH" --owner=0 --group=0 --numeric-owner -cf - "$payload_name" | gzip -n >"$source_path"

cat >"$spec_path" <<SPEC
%global debug_package %{nil}

Name:           $package_name
Version:        $version
Release:        $release%{?dist}
Summary:        SSD-first DAS-backed object store for bioinformatics
License:        MPL-2.0
URL:            https://github.com/sagrudd/DASObjectStore
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  cargo
BuildRequires:  clang
BuildRequires:  clang-devel
BuildRequires:  rust
# WebAssembly packaging also requires Trunk and the wasm32-unknown-unknown Rust
# target; those are usually installed through rustup/cargo rather than RPM.
Requires:       acl
Requires:       ca-certificates
Requires:       /usr/bin/docker
Requires:       docker-buildx-plugin
Requires:       mergerfs
Requires:       nfs-utils
Requires:       python3
Requires:       quota
Requires:       systemd
Requires:       udisks2
Requires(post): coreutils
Requires(post): findutils
Requires(post): shadow-utils
Requires(post): systemd
Requires(preun): systemd
Requires(postun): systemd
Recommends:      awscli

%description
DASObjectStore provides CLI and service binaries for staging objects on SSD
and settling verified copies onto DAS or NAS storage endpoints. Long-running
CLI operations may expose embedded terminal views through command flags.

%prep
%setup -q

%build

%install
rm -rf %{buildroot}
cp -a . %{buildroot}/

%post
set -e
service_user="dasobjectstore"
service_group="dasobjectstore"
admin_group="dasobjectstore-admin"
shared_monas_group="mnemosyne-pistis-das"
managed_root="/srv/dasobjectstore"
workspace_aggregate_root="\$managed_root/workspaces"

if command -v systemd-sysusers >/dev/null 2>&1; then
  systemd-sysusers /usr/lib/sysusers.d/dasobjectstore.conf || true
fi
if ! getent group "\$service_group" >/dev/null; then
  groupadd --system "\$service_group"
fi
if ! getent group "\$admin_group" >/dev/null; then
  groupadd --system "\$admin_group"
fi
if ! getent group "\$shared_monas_group" >/dev/null; then
  groupadd --system "\$shared_monas_group"
fi
if ! id -u "\$service_user" >/dev/null 2>&1; then
  useradd --system --gid "\$service_group" --home-dir /var/lib/dasobjectstore --no-create-home --shell /sbin/nologin "\$service_user"
fi
if getent group docker >/dev/null; then
  usermod -aG docker "\$service_user"
else
  cat >&2 <<WARNING
DASObjectStore formal PDF reporting requires access to the Docker API, but the
docker group does not exist. Install or repair Docker, then add \$service_user to
the docker group and restart dasobjectstore-server.service.
WARNING
fi
usermod -aG "\$admin_group" "\$service_user"

install -d -o "\$service_user" -g "\$shared_monas_group" -m 0750 /run/dasobjectstore
install -d -o "\$service_user" -g "\$shared_monas_group" -m 0750 /var/lib/dasobjectstore
install -d -o "\$service_user" -g "\$service_group" -m 0700 /var/lib/dasobjectstore/object-service
install -d -o "\$service_user" -g "\$service_group" -m 0700 /var/lib/dasobjectstore/projection-authority
install -d -o "\$service_user" -g "\$service_group" -m 0750 /var/lib/dasobjectstore/report-rebuild
install -d -o "\$service_user" -g "\$service_group" -m 0750 /var/lib/dasobjectstore/telemetry
install -d -o "\$service_user" -g "\$service_group" -m 0750 /var/log/dasobjectstore
install -d -o "\$service_user" -g "\$service_group" -m 0750 /opt/dasobjectstore
install -d -o "\$service_user" -g "\$service_group" -m 0750 /opt/dasobjectstore/tls
install -d -o root -g "\$service_group" -m 0750 /etc/dasobjectstore
find /etc/dasobjectstore -maxdepth 1 -type f -name '*.json' -exec chgrp "\$service_group" {} + -exec chmod 0640 {} +
store_registry_state=/var/lib/dasobjectstore/stores.json
store_registry_config=/etc/dasobjectstore/stores.json
if [ ! -e "\$store_registry_state" ] && [ -f "\$store_registry_config" ]; then
  install -o "\$service_user" -g "\$shared_monas_group" -m 0640 "\$store_registry_config" "\$store_registry_state"
elif [ -f "\$store_registry_state" ]; then
  chown "\$service_user:\$shared_monas_group" "\$store_registry_state"
  chmod 0640 "\$store_registry_state"
fi
if [ -e /run/dasobjectstore/dasobjectstored.sock ]; then
  /usr/libexec/dasobjectstore/manage-monas-access-boundary publish-socket
fi
if [ -f /opt/dasobjectstore/config.json ]; then
  chown root:"\$service_group" /opt/dasobjectstore/config.json
  chmod 0640 /opt/dasobjectstore/config.json
fi
if [ -f /etc/dasobjectstore/workspace-host.json ]; then
  chown root:root /etc/dasobjectstore/workspace-host.json
  chmod 0640 /etc/dasobjectstore/workspace-host.json
  if ! grep -q '"aggregate_root"' /etc/dasobjectstore/workspace-host.json; then
    temporary="\$(mktemp /etc/dasobjectstore/workspace-host.json.tmp.XXXXXX)"
    sed '/"schema_version"[[:space:]]*:[[:space:]]*1[[:space:]]*,/a\
  "aggregate_root": "/srv/dasobjectstore/workspaces",' /etc/dasobjectstore/workspace-host.json >"\$temporary"
    if grep -q '"aggregate_root"' "\$temporary"; then
      chown root:root "\$temporary"
      chmod 0640 "\$temporary"
      mv -f "\$temporary" /etc/dasobjectstore/workspace-host.json
    else
      rm -f "\$temporary"
      printf >&2 'DASObjectStore retained workspace broker config without aggregate_root; add it explicitly before workspace provisioning.\n'
    fi
  fi
  if ! grep -q '"live_metadata_path"' /etc/dasobjectstore/workspace-host.json; then
    temporary="\$(mktemp /etc/dasobjectstore/workspace-host.json.tmp.XXXXXX)"
    sed '/"schema_version"[[:space:]]*:[[:space:]]*1[[:space:]]*,/a\
  "live_metadata_path": "/srv/dasobjectstore/ssd/.dasobjectstore/live.sqlite",' /etc/dasobjectstore/workspace-host.json >"\$temporary"
    if grep -q '"live_metadata_path"' "\$temporary"; then
      chown root:root "\$temporary"
      chmod 0640 "\$temporary"
      mv -f "\$temporary" /etc/dasobjectstore/workspace-host.json
    else
      rm -f "\$temporary"
      printf >&2 'DASObjectStore retained workspace broker config without live_metadata_path; add it before materialization.\n'
    fi
  fi
  if ! grep -q '"nfs_clients"' /etc/dasobjectstore/workspace-host.json; then
    temporary="\$(mktemp /etc/dasobjectstore/workspace-host.json.tmp.XXXXXX)"
    sed '/"aggregate_root"[[:space:]]*:/a\
  "nfs_clients": {},' /etc/dasobjectstore/workspace-host.json >"\$temporary"
    if grep -q '"nfs_clients"' "\$temporary"; then
      chown root:root "\$temporary"
      chmod 0640 "\$temporary"
      mv -f "\$temporary" /etc/dasobjectstore/workspace-host.json
    else
      rm -f "\$temporary"
      printf >&2 'DASObjectStore retained workspace broker config without nfs_clients; add the root-owned registry explicitly before NFS attachment.\n'
    fi
  fi
fi

repair_managed_tree() {
  root="\$1"
  [ -d "\$root" ] || return 0
  chown "\$service_user:\$service_group" "\$root"
  chmod 0750 "\$root"
  # Package upgrades must not recursively traverse a live data plane. The
  # daemon owns descendant creation; legacy/adopted trees use explicit
  # reconciliation rather than an unsafe package-time ownership rewrite.
  if [ -d "\$root/.dasobjectstore" ]; then
    chown "\$service_user:\$service_group" "\$root/.dasobjectstore"
    chmod 0750 "\$root/.dasobjectstore"
  fi
}

repair_marked_managed_tree() {
  root="\$1"
  [ -d "\$root" ] || return 0
  if [ ! -d "\$root/.dasobjectstore" ]; then
    cat >&2 <<WARNING
DASObjectStore left existing files below \$root untouched because the profile
namespace marker is missing. Explicit profile adoption/reconciliation is
required before package configuration can manage that tree.
WARNING
    return 0
  fi
  repair_managed_tree "\$root"
}

ensure_profile_layout() {
  root="\$1"
  install -d -o "\$service_user" -g "\$service_group" -m 0750 "\$root"
  if [ -e "\$root/ssd" ] && [ ! -d "\$root/ssd" ]; then
    echo "DASObjectStore profile path \$root/ssd is not a directory." >&2
    exit 1
  fi
  if [ -e "\$root/hdd" ] && [ ! -d "\$root/hdd" ]; then
    echo "DASObjectStore profile path \$root/hdd is not a directory." >&2
    exit 1
  fi
  if [ ! -e "\$root/ssd" ]; then
    install -d -o "\$service_user" -g "\$service_group" -m 0750 "\$root/ssd"
  fi
  if [ ! -e "\$root/hdd" ]; then
    install -d -o root -g root -m 0755 "\$root/hdd"
  fi
}

if [ -e "\$managed_root" ]; then
  owner="\$(stat -c '%U' "\$managed_root")"
  group="\$(stat -c '%G' "\$managed_root")"
  if [ "\$owner" != "\$service_user" ] || [ "\$group" != "\$service_group" ]; then
    cat >&2 <<ERROR
DASObjectStore managed root \$managed_root is owned by \$owner:\$group.
Managed DAS roots must be owned by \$service_user:\$service_group so normal users
submit jobs through dasobjectstored instead of writing directly to member disks.
Repair ownership with the formal DASObjectStore disk lockdown command before
continuing package configuration.
ERROR
    exit 1
  fi
fi

ensure_profile_layout "\$managed_root"
install -d -o root -g root -m 0755 "\$workspace_aggregate_root"
install -d -o root -g root -m 0755 /etc/exports.d
repair_marked_managed_tree "\$managed_root/ssd"
for root in "\$managed_root"/hdd/*; do
  repair_marked_managed_tree "\$root"
done

if [ -x /usr/libexec/dasobjectstore/configure-external-mount-policy ]; then
  /usr/libexec/dasobjectstore/configure-external-mount-policy || true
fi

if command -v systemd-tmpfiles >/dev/null 2>&1; then
  systemd-tmpfiles --create /usr/lib/tmpfiles.d/dasobjectstore.conf || true
fi
if command -v systemctl >/dev/null 2>&1; then
  systemctl daemon-reload || true
fi

wrapper="/usr/libexec/dasobjectstore/gnostikon-workflow-control"
if [ -x "\$wrapper" ]; then
  if "\$wrapper" prewarm-report-provider >/dev/null 2>&1; then
    printf 'DASObjectStore formal PDF report provider is installed and prewarmed.\n'
  else
    cat >&2 <<WARNING
DASObjectStore formal PDF reporting could not prewarm the Grammateus provider.
Install or repair Grammateus and initialise the provider with:
  grammateus_report_provider install --image grammateus/report:0.8.1
Then restart dasobjectstore-server.service before rebuilding reports from the
Web interface.
WARNING
  fi
fi

%preun
if [ "\$1" -eq 0 ] && command -v systemctl >/dev/null 2>&1; then
  systemctl disable --now \
    dasobjectstore-source-access.path \
    dasobjectstore-workspace-host.socket \
    dasobjectstore-workspace-host.service \
    dasobjectstore-server.service \
    dasobjectstored.service || true
fi

%postun
if command -v systemctl >/dev/null 2>&1; then
  systemctl daemon-reload || true
  systemctl reset-failed || true
fi
# Persistent configuration, metadata, credentials, telemetry, and managed
# storage roots are deliberately retained. RPM removal never authorizes data
# deletion.

%files
%config(noreplace) /etc/dasobjectstore/daemon.json
%config(noreplace) /etc/dasobjectstore/managed-storage.v1.json
%config(noreplace) /etc/dasobjectstore/s3-gateway.json
%config(noreplace) /etc/dasobjectstore/workspace-host.json
%config(noreplace) /opt/dasobjectstore/config.json
/opt/dasobjectstore/web
/usr/bin/dasobjectstore
/usr/bin/dasobjectstore-server
/usr/bin/dasobjectstore-s3-gateway
/usr/bin/dasobjectstored
/usr/bin/dasobjectstore-remote
/usr/libexec/dasobjectstore/dasobjectstore-workspace-host
/usr/libexec/dasobjectstore/dasobjectstore-authority-retirement
/usr/libexec/dasobjectstore/dasobjectstore-authority-retirement-finalize
/usr/libexec/dasobjectstore/gnostikon-workflow-control
/usr/libexec/dasobjectstore/prepare-external-mount-traversal
/usr/libexec/dasobjectstore/configure-external-mount-policy
/usr/libexec/dasobjectstore/verify-managed-storage-mounts
/usr/libexec/dasobjectstore/migrate-monas-integrated-config
/usr/lib/systemd/system/dasobjectstore-storage-ready.service
/usr/lib/systemd/system/dasobjectstore-garage.service
/usr/lib/systemd/system/dasobjectstored.service
/usr/lib/systemd/system/dasobjectstore-server.service
/usr/lib/systemd/system/dasobjectstore-s3-gateway.service
/usr/lib/systemd/system/dasobjectstore-source-access.service
/usr/lib/systemd/system/dasobjectstore-source-access.path
/usr/lib/systemd/system/dasobjectstore-control.slice
/usr/lib/systemd/system/dasobjectstore-storage.slice
/usr/lib/systemd/system/dasobjectstore-workspace-host.service
/usr/lib/systemd/system/dasobjectstore-workspace-host.socket
/usr/lib/systemd/system/dasobjectstore-authority-retirement.service
/usr/lib/sysusers.d/dasobjectstore.conf
/usr/lib/tmpfiles.d/dasobjectstore.conf
%doc /usr/share/doc/dasobjectstore/README.md
%license /usr/share/licenses/dasobjectstore/LICENSE

%changelog
* Tue Jul 07 2026 DASObjectStore contributors <noreply@example.invalid> - $version-$release
- Build native RPM package from shared Linux service assets.
SPEC

SOURCE_DATE_EPOCH="$DAS_PACKAGE_SOURCE_EPOCH" rpmbuild \
  --define "_topdir $rpm_root" \
  -bb "$spec_path"

find "$rpm_root/RPMS" -type f -name "${package_name}-${version}-${release}*.rpm" -print | while read -r package_path; do
  das_package_write_provenance "$package_path" appliance "$(rpm -qp --qf '%{ARCH}' "$package_path")"
  das_package_write_pinned_dependency_provenance "$package_path"
  printf '%s\n' "$package_path"
done
