#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
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
Native DASObjectStore package builds require clang, libclang, and PAM headers.
On AlmaLinux/RHEL: sudo dnf install clang libclang-devel pam-devel
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
cargo build --release -p dasobjectstore-daemon --manifest-path "$repo_root/Cargo.toml"
cargo build --release -p dasobjectstore-remote --manifest-path "$repo_root/Cargo.toml"

rpm_root="$repo_root/target/rpm/rpmbuild"
staging_root="$repo_root/target/rpm/staging"
payload_name="${package_name}-${version}"
payload_root="$staging_root/$payload_name"
spec_path="$rpm_root/SPECS/${package_name}.spec"
source_path="$rpm_root/SOURCES/${payload_name}.tar.gz"

rm -rf "$payload_root"
install -d \
  "$payload_root/etc/dasobjectstore" \
  "$payload_root/etc/pam.d" \
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
install -m 0755 "$repo_root/target/release/dasobjectstored" \
  "$payload_root/usr/bin/dasobjectstored"
install -m 0755 "$repo_root/target/release/dasobjectstore-remote" \
  "$payload_root/usr/bin/dasobjectstore-remote"
install -m 0750 "$repo_root/target/release/dasobjectstore-local-auth-helper" \
  "$payload_root/usr/libexec/dasobjectstore/dasobjectstore-local-auth-helper"
install -m 0755 "$packaging_reporting/gnostikon-workflow-control" \
  "$payload_root/usr/libexec/dasobjectstore/gnostikon-workflow-control"
install -m 0644 "$repo_root/README.md" "$payload_root/usr/share/doc/$package_name/README.md"
install -m 0644 "$repo_root/LICENSE" "$payload_root/usr/share/licenses/$package_name/LICENSE"
install -m 0644 "$packaging_linux/etc/dasobjectstore/daemon.json" \
  "$payload_root/etc/dasobjectstore/daemon.json"
install -m 0644 "$packaging_linux/pam.d/dasobjectstore" \
  "$payload_root/etc/pam.d/dasobjectstore"
install -m 0644 "$packaging_product/config.json" \
  "$payload_root/opt/dasobjectstore/config.json"
install -m 0644 "$packaging_linux/systemd/dasobjectstored.service" \
  "$payload_root/usr/lib/systemd/system/dasobjectstored.service"
install -m 0644 "$packaging_linux/systemd/dasobjectstore-server.service" \
  "$payload_root/usr/lib/systemd/system/dasobjectstore-server.service"
install -m 0644 "$packaging_linux/sysusers.d/dasobjectstore.conf" \
  "$payload_root/usr/lib/sysusers.d/dasobjectstore.conf"
install -m 0644 "$packaging_linux/tmpfiles.d/dasobjectstore.conf" \
  "$payload_root/usr/lib/tmpfiles.d/dasobjectstore.conf"
cp -a "$web_dist/." "$payload_root/opt/dasobjectstore/web/"

install -d "$rpm_root/BUILD" "$rpm_root/RPMS" "$rpm_root/SOURCES" "$rpm_root/SPECS" "$rpm_root/SRPMS"
tar -C "$staging_root" -czf "$source_path" "$payload_name"

cat >"$spec_path" <<SPEC
Name:           $package_name
Version:        $version
Release:        $release%{?dist}
Summary:        SSD-first DAS-backed object store for bioinformatics
License:        MPL-2.0
URL:            https://github.com/sagrudd/DASObjectStore
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  cargo
BuildRequires:  clang
BuildRequires:  libclang-devel
BuildRequires:  pam-devel
BuildRequires:  rust
# WebAssembly packaging also requires Trunk and the wasm32-unknown-unknown Rust
# target; those are usually installed through rustup/cargo rather than RPM.
Requires:       acl
Requires:       ca-certificates
Requires:       /usr/bin/docker
Requires:       pam
Requires:       systemd
Requires(post): coreutils
Requires(post): findutils
Requires(post): shadow-utils
Requires(post): systemd
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
managed_root="/srv/dasobjectstore"

if command -v systemd-sysusers >/dev/null 2>&1; then
  systemd-sysusers /usr/lib/sysusers.d/dasobjectstore.conf || true
fi
if ! getent group "\$service_group" >/dev/null; then
  groupadd --system "\$service_group"
fi
if ! id -u "\$service_user" >/dev/null 2>&1; then
  useradd --system --gid "\$service_group" --home-dir /var/lib/dasobjectstore --no-create-home --shell /sbin/nologin "\$service_user"
fi

install -d -o "\$service_user" -g "\$service_group" -m 0750 /run/dasobjectstore
install -d -o "\$service_user" -g "\$service_group" -m 0750 /var/lib/dasobjectstore
install -d -o "\$service_user" -g "\$service_group" -m 0750 /var/log/dasobjectstore
install -d -o "\$service_user" -g "\$service_group" -m 0750 /opt/dasobjectstore
install -d -o "\$service_user" -g "\$service_group" -m 0750 /opt/dasobjectstore/tls
install -d -o root -g "\$service_group" -m 0750 /etc/dasobjectstore
find /etc/dasobjectstore -maxdepth 1 -type f -name '*.json' -exec chgrp "\$service_group" {} + -exec chmod 0640 {} +
if [ -f /opt/dasobjectstore/config.json ]; then
  chown root:"\$service_group" /opt/dasobjectstore/config.json
  chmod 0640 /opt/dasobjectstore/config.json
fi
if [ -f /usr/libexec/dasobjectstore/dasobjectstore-local-auth-helper ]; then
  chown root:"\$service_group" /usr/libexec/dasobjectstore/dasobjectstore-local-auth-helper
  chmod 4750 /usr/libexec/dasobjectstore/dasobjectstore-local-auth-helper
fi

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

install -d -o "\$service_user" -g "\$service_group" -m 0750 "\$managed_root"
for root in "\$managed_root/ssd" "\$managed_root"/hdd/*; do
  [ -d "\$root" ] || continue
  chown "\$service_user:\$service_group" "\$root"
  chmod 0750 "\$root"
  find "\$root" -path "\$root/lost+found" -prune -o -mindepth 1 -exec chown "\$service_user:\$service_group" {} +
  find "\$root" -path "\$root/lost+found" -prune -o -type d -exec chmod 0750 {} +
  find "\$root" -path "\$root/lost+found" -prune -o -type f -exec chmod 0640 {} +
done

if command -v systemd-tmpfiles >/dev/null 2>&1; then
  systemd-tmpfiles --create /usr/lib/tmpfiles.d/dasobjectstore.conf || true
fi
if command -v systemctl >/dev/null 2>&1; then
  systemctl daemon-reload || true
  systemctl enable --now dasobjectstored.service dasobjectstore-server.service || true
  systemctl restart dasobjectstored.service dasobjectstore-server.service || true
fi

%files
%config(noreplace) /etc/dasobjectstore/daemon.json
%config(noreplace) /etc/pam.d/dasobjectstore
%config(noreplace) /opt/dasobjectstore/config.json
/opt/dasobjectstore/web
/usr/bin/dasobjectstore
/usr/bin/dasobjectstore-server
/usr/bin/dasobjectstored
/usr/bin/dasobjectstore-remote
/usr/libexec/dasobjectstore/dasobjectstore-local-auth-helper
/usr/libexec/dasobjectstore/gnostikon-workflow-control
/usr/lib/systemd/system/dasobjectstored.service
/usr/lib/systemd/system/dasobjectstore-server.service
/usr/lib/sysusers.d/dasobjectstore.conf
/usr/lib/tmpfiles.d/dasobjectstore.conf
%doc /usr/share/doc/dasobjectstore/README.md
%license /usr/share/licenses/dasobjectstore/LICENSE

%changelog
* Tue Jul 07 2026 DASObjectStore contributors <noreply@example.invalid> - $version-$release
- Build native RPM package from shared Linux service assets.
SPEC

rpmbuild \
  --define "_topdir $rpm_root" \
  -bb "$spec_path"

find "$rpm_root/RPMS" -type f -name "${package_name}-${version}-${release}*.rpm" -print
