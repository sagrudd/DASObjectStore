#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
package_name="dasobjectstore"
version="$(cargo metadata --no-deps --format-version 1 --manifest-path "$repo_root/Cargo.toml" \
  | sed -n 's/.*"name":"dasobjectstore-cli","version":"\([^"]*\)".*/\1/p')"
version="${version:-0.3.7}"
release="${release:-1}"

if ! command -v rpmbuild >/dev/null 2>&1; then
  cat >&2 <<ERROR
rpmbuild is required to build the DASObjectStore RPM.
On AlmaLinux/RHEL: sudo dnf install rpm-build
On Ubuntu: sudo apt-get install rpm
ERROR
  exit 1
fi

packaging_debian="$repo_root/packaging/debian"
packaging_linux="$repo_root/packaging/linux"
bash "$packaging_debian/validate-package-assets.sh"

cargo build --release -p dasobjectstore-cli --manifest-path "$repo_root/Cargo.toml"
cargo build --release -p dasobjectstore-daemon --manifest-path "$repo_root/Cargo.toml"

rpm_root="$repo_root/target/rpm/rpmbuild"
staging_root="$repo_root/target/rpm/staging"
payload_name="${package_name}-${version}"
payload_root="$staging_root/$payload_name"
spec_path="$rpm_root/SPECS/${package_name}.spec"
source_path="$rpm_root/SOURCES/${payload_name}.tar.gz"

rm -rf "$payload_root"
install -d \
  "$payload_root/etc/dasobjectstore" \
  "$payload_root/usr/bin" \
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
install -m 0644 "$repo_root/README.md" "$payload_root/usr/share/doc/$package_name/README.md"
install -m 0644 "$repo_root/LICENSE" "$payload_root/usr/share/licenses/$package_name/LICENSE"
install -m 0644 "$packaging_linux/etc/dasobjectstore/daemon.json" \
  "$payload_root/etc/dasobjectstore/daemon.json"
install -m 0644 "$packaging_linux/systemd/dasobjectstored.service" \
  "$payload_root/usr/lib/systemd/system/dasobjectstored.service"
install -m 0644 "$packaging_linux/sysusers.d/dasobjectstore.conf" \
  "$payload_root/usr/lib/sysusers.d/dasobjectstore.conf"
install -m 0644 "$packaging_linux/tmpfiles.d/dasobjectstore.conf" \
  "$payload_root/usr/lib/tmpfiles.d/dasobjectstore.conf"

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

Requires:       acl
Requires:       ca-certificates
Requires:       systemd
Requires(post): coreutils
Requires(post): findutils
Requires(post): shadow-utils
Requires(post): systemd

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
install -d -o root -g "\$service_group" -m 0750 /etc/dasobjectstore
find /etc/dasobjectstore -maxdepth 1 -type f -name '*.json' -exec chgrp "\$service_group" {} + -exec chmod 0640 {} +

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
fi

%files
%config(noreplace) /etc/dasobjectstore/daemon.json
/usr/bin/dasobjectstore
/usr/bin/dasobjectstore-server
/usr/bin/dasobjectstored
/usr/lib/systemd/system/dasobjectstored.service
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
