#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
package_name="dasobjectstore-remote"
version="$(cargo metadata --no-deps --format-version 1 --manifest-path "$repo_root/Cargo.toml" \
  | sed -n 's/.*"name":"dasobjectstore-remote","version":"\([^"]*\)".*/\1/p')"
version="${version:-0.4.2}"
release="${release:-1}"

if ! command -v rpmbuild >/dev/null 2>&1; then
  cat >&2 <<ERROR
rpmbuild is required to build the DASObjectStore remote RPM.
On AlmaLinux/RHEL: sudo dnf install rpm-build
On Ubuntu: sudo apt-get install rpm
ERROR
  exit 1
fi

cargo build --release -p dasobjectstore-remote --manifest-path "$repo_root/Cargo.toml"

rpm_root="$repo_root/target/rpm/rpmbuild"
staging_root="$repo_root/target/rpm/staging"
payload_name="${package_name}-${version}"
payload_root="$staging_root/$payload_name"
spec_path="$rpm_root/SPECS/${package_name}.spec"
source_path="$rpm_root/SOURCES/${payload_name}.tar.gz"

rm -rf "$payload_root"
install -d \
  "$payload_root/usr/bin" \
  "$payload_root/usr/share/doc/$package_name" \
  "$payload_root/usr/share/licenses/$package_name"
install -m 0755 "$repo_root/target/release/dasobjectstore-remote" \
  "$payload_root/usr/bin/dasobjectstore-remote"
install -m 0644 "$repo_root/README.md" "$payload_root/usr/share/doc/$package_name/README.md"
install -m 0644 "$repo_root/docs/user/remote-client.rst" \
  "$payload_root/usr/share/doc/$package_name/remote-client.rst"
install -m 0644 "$repo_root/docs/user/remote-s3-uploads.rst" \
  "$payload_root/usr/share/doc/$package_name/remote-s3-uploads.rst"
install -m 0644 "$repo_root/LICENSE" "$payload_root/usr/share/licenses/$package_name/LICENSE"

bash "$repo_root/packaging/validate-package-auth-content.sh" "$payload_root"

install -d "$rpm_root/BUILD" "$rpm_root/RPMS" "$rpm_root/SOURCES" "$rpm_root/SPECS" "$rpm_root/SRPMS"
tar -C "$staging_root" -czf "$source_path" "$payload_name"

cat >"$spec_path" <<SPEC
Name:           $package_name
Version:        $version
Release:        $release%{?dist}
Summary:        Remote upload client for DASObjectStore object services
License:        MPL-2.0
URL:            https://github.com/sagrudd/DASObjectStore
Source0:        %{name}-%{version}.tar.gz

Requires:       ca-certificates
Recommends:      awscli

%description
dasobjectstore-remote is the lightweight client for workstations, sequencers,
and analysis hosts that upload files or folders to DASObjectStore through an
S3-compatible endpoint without installing the local appliance daemon.

%prep
%setup -q

%build

%install
rm -rf %{buildroot}
cp -a . %{buildroot}/

%files
/usr/bin/dasobjectstore-remote
%doc /usr/share/doc/dasobjectstore-remote/README.md
%doc /usr/share/doc/dasobjectstore-remote/remote-client.rst
%doc /usr/share/doc/dasobjectstore-remote/remote-s3-uploads.rst
%license /usr/share/licenses/dasobjectstore-remote/LICENSE

%changelog
* Wed Jul 08 2026 DASObjectStore contributors <noreply@example.invalid> - $version-$release
- Build remote-only RPM package.
SPEC

rpmbuild \
  --define "_topdir $rpm_root" \
  -bb "$spec_path"

find "$rpm_root/RPMS" -type f -name "${package_name}-${version}-${release}*.rpm" -print
