#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
"$repo_root/packaging/validate-release-version.sh" "$repo_root"
package_name="dasobjectstore-remote"
version="$(cargo metadata --no-deps --format-version 1 --manifest-path "$repo_root/Cargo.toml" \
  | sed -n 's/.*"name":"dasobjectstore-remote","version":"\([^"]*\)".*/\1/p')"
version="${version:-0.4.2}"
release="${release:-1}"
: "${TERRAFORM_SUCCESSOR_LOCKSET:?formal remote RPM package requires TERRAFORM_SUCCESSOR_LOCKSET}"
: "${TERRAFORM_CATALOGUE:?formal remote RPM package requires TERRAFORM_CATALOGUE}"
: "${TERRAFORM_SOURCES_LOCK:?formal remote RPM package requires TERRAFORM_SOURCES_LOCK}"
read -r DAS_PACKAGE_LOCKSET_ID DAS_PACKAGE_LOCKSET_CONTENT_DIGEST DAS_PACKAGE_LOCKSET_REGISTRY_DIGEST DAS_PACKAGE_RELEASE_SOURCE_REVISION < <(
  python3 "$repo_root/packaging/validate-formal-remote-release.py" \
    --repo-root "$repo_root" \
    --lockset "$TERRAFORM_SUCCESSOR_LOCKSET" \
    --catalogue "$TERRAFORM_CATALOGUE" \
    --sources-lock "$TERRAFORM_SOURCES_LOCK" \
    --package-version "$version"
)
export DAS_PACKAGE_LOCKSET_ID DAS_PACKAGE_LOCKSET_CONTENT_DIGEST DAS_PACKAGE_LOCKSET_REGISTRY_DIGEST DAS_PACKAGE_RELEASE_SOURCE_REVISION
source "$repo_root/packaging/package-provenance.sh"; das_package_provenance_init "$repo_root"
source "$repo_root/packaging/pinned-mnemosyne-package-sources.sh"; das_package_configure_pinned_mnemosyne_sources "$repo_root"
source "$repo_root/packaging/cargo-target-dir.sh"

if ! command -v rpmbuild >/dev/null 2>&1; then
  cat >&2 <<ERROR
rpmbuild is required to build the DASObjectStore remote RPM.
On AlmaLinux/RHEL: sudo dnf install rpm-build
On Ubuntu: sudo apt-get install rpm
ERROR
  exit 1
fi

cargo build --release --locked -p dasobjectstore-remote --manifest-path "$repo_root/Cargo.toml"

cargo_target_dir="$(das_cargo_target_dir "$repo_root")"
rpm_root="$cargo_target_dir/rpm/rpmbuild"
staging_root="$cargo_target_dir/rpm/staging"
payload_name="${package_name}-${version}"
payload_root="$staging_root/$payload_name"
spec_path="$rpm_root/SPECS/${package_name}.spec"
source_path="$rpm_root/SOURCES/${payload_name}.tar.gz"

rm -rf "$payload_root"
install -d \
  "$payload_root/etc/dasobjectstore-remote/site-trust.d" \
  "$payload_root/etc/dasobjectstore-remote/site-trust-sources.d" \
  "$payload_root/usr/bin" \
  "$payload_root/usr/share/doc/$package_name" \
  "$payload_root/usr/share/licenses/$package_name"
install -m 0755 "$cargo_target_dir/release/dasobjectstore-remote" \
  "$payload_root/usr/bin/dasobjectstore-remote"
install -m 0644 "$repo_root/README.md" "$payload_root/usr/share/doc/$package_name/README.md"
install -m 0644 "$repo_root/docs/user/remote-client.rst" \
  "$payload_root/usr/share/doc/$package_name/remote-client.rst"
install -m 0644 "$repo_root/docs/user/remote-s3-uploads.rst" \
  "$payload_root/usr/share/doc/$package_name/remote-s3-uploads.rst"
install -m 0644 "$repo_root/docs/contracts/remote-site-trust-provisioning-v1.md" \
  "$payload_root/usr/share/doc/$package_name/remote-site-trust-provisioning-v1.md"
install -m 0644 "$repo_root/docs/schemas/dasobjectstore.remote-site-trust-source.v1.schema.json" \
  "$payload_root/usr/share/doc/$package_name/remote-site-trust-source-v1.schema.json"
install -m 0644 "$repo_root/LICENSE" "$payload_root/usr/share/licenses/$package_name/LICENSE"

bash "$repo_root/packaging/validate-package-auth-content.sh" "$payload_root"
das_package_normalize_tree "$payload_root"

install -d "$rpm_root/BUILD" "$rpm_root/RPMS" "$rpm_root/SOURCES" "$rpm_root/SPECS" "$rpm_root/SRPMS"
tar -C "$staging_root" --sort=name --mtime="@$DAS_PACKAGE_SOURCE_EPOCH" --owner=0 --group=0 --numeric-owner -cf - "$payload_name" | gzip -n >"$source_path"

cat >"$spec_path" <<SPEC
Name:           $package_name
Version:        $version
Release:        $release%{?dist}
Summary:        Remote upload client for DASObjectStore object services
License:        MPL-2.0
URL:            https://github.com/sagrudd/DASObjectStore
Source0:        %{name}-%{version}.tar.gz

Requires:       ca-certificates
Requires:       openssh-clients
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
%dir /etc/dasobjectstore-remote
%dir /etc/dasobjectstore-remote/site-trust.d
%dir /etc/dasobjectstore-remote/site-trust-sources.d
/usr/bin/dasobjectstore-remote
%doc /usr/share/doc/dasobjectstore-remote/README.md
%doc /usr/share/doc/dasobjectstore-remote/remote-client.rst
%doc /usr/share/doc/dasobjectstore-remote/remote-s3-uploads.rst
%doc /usr/share/doc/dasobjectstore-remote/remote-site-trust-provisioning-v1.md
%doc /usr/share/doc/dasobjectstore-remote/remote-site-trust-source-v1.schema.json
%license /usr/share/licenses/dasobjectstore-remote/LICENSE

%changelog
* Wed Jul 08 2026 DASObjectStore contributors <noreply@example.invalid> - $version-$release
- Build remote-only RPM package.
SPEC

SOURCE_DATE_EPOCH="$DAS_PACKAGE_SOURCE_EPOCH" rpmbuild \
  --define "_topdir $rpm_root" \
  -bb "$spec_path"

find "$rpm_root/RPMS" -type f -name "${package_name}-${version}-${release}*.rpm" -print | while read -r package_path; do
  das_package_write_formal_remote_provenance "$package_path" "$(rpm -qp --qf '%{ARCH}' "$package_path")" "$version"
  das_package_write_pinned_dependency_provenance "$package_path"
  printf '%s\n' "$package_path"
done
