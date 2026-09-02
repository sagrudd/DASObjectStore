#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
"$repo_root/packaging/validate-release-version.sh" "$repo_root"
package_name="dasobjectstore-remote"
version="$(cargo metadata --no-deps --format-version 1 --manifest-path "$repo_root/Cargo.toml" \
  | sed -n 's/.*"name":"dasobjectstore-remote","version":"\([^"]*\)".*/\1/p')"
version="${version:-0.4.2}"
: "${TERRAFORM_SUCCESSOR_LOCKSET:?formal remote Debian package requires TERRAFORM_SUCCESSOR_LOCKSET}"
: "${TERRAFORM_CATALOGUE:?formal remote Debian package requires TERRAFORM_CATALOGUE}"
: "${TERRAFORM_SOURCES_LOCK:?formal remote Debian package requires TERRAFORM_SOURCES_LOCK}"
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

if ! command -v dpkg-deb >/dev/null 2>&1; then
  cat >&2 <<ERROR
dpkg-deb is required to build the DASObjectStore remote Debian package.
On Ubuntu/Debian: sudo apt-get install dpkg
ERROR
  exit 1
fi

arch="$(dpkg --print-architecture 2>/dev/null || uname -m)"
cargo_target_dir="$(das_cargo_target_dir "$repo_root")"
build_root="$cargo_target_dir/deb/${package_name}_${version}_${arch}"
package_path="$cargo_target_dir/deb/${package_name}_${version}_${arch}.deb"

cargo build --release --locked -p dasobjectstore-remote --manifest-path "$repo_root/Cargo.toml"

rm -rf "$build_root"
install -d \
  "$build_root/DEBIAN" \
  "$build_root/etc/dasobjectstore-remote/site-trust.d" \
  "$build_root/etc/dasobjectstore-remote/site-trust-sources.d" \
  "$build_root/usr/bin" \
  "$build_root/usr/share/doc/$package_name"
install -m 0755 "$cargo_target_dir/release/dasobjectstore-remote" \
  "$build_root/usr/bin/dasobjectstore-remote"
install -m 0644 "$repo_root/README.md" "$build_root/usr/share/doc/$package_name/README.md"
install -m 0644 "$repo_root/docs/user/remote-client.rst" \
  "$build_root/usr/share/doc/$package_name/remote-client.rst"
install -m 0644 "$repo_root/docs/user/remote-s3-uploads.rst" \
  "$build_root/usr/share/doc/$package_name/remote-s3-uploads.rst"
install -m 0644 "$repo_root/docs/contracts/remote-site-trust-provisioning-v1.md" \
  "$build_root/usr/share/doc/$package_name/remote-site-trust-provisioning-v1.md"
install -m 0644 "$repo_root/docs/schemas/dasobjectstore.remote-site-trust-source.v1.schema.json" \
  "$build_root/usr/share/doc/$package_name/remote-site-trust-source-v1.schema.json"

bash "$repo_root/packaging/validate-package-auth-content.sh" "$build_root"
das_package_normalize_tree "$build_root"

cat >"$build_root/DEBIAN/control" <<CONTROL
Package: $package_name
Version: $version
Section: utils
Priority: optional
Architecture: $arch
Maintainer: DASObjectStore contributors
Depends: ca-certificates, openssh-client
Suggests: awscli
Homepage: https://github.com/sagrudd/DASObjectStore
Description: remote upload client for DASObjectStore object services
 dasobjectstore-remote is the lightweight client for workstations, sequencers,
 and analysis hosts that upload files or folders to DASObjectStore through an
 S3-compatible endpoint without installing the local appliance daemon.
CONTROL

SOURCE_DATE_EPOCH="$DAS_PACKAGE_SOURCE_EPOCH" dpkg-deb --build --root-owner-group "$build_root" "$package_path"
das_package_write_formal_remote_provenance "$package_path" "$arch" "$version"
das_package_write_pinned_dependency_provenance "$package_path"
printf '%s\n' "$package_path"
