#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
package_name="dasobjectstore"
version="$(cargo metadata --no-deps --format-version 1 --manifest-path "$repo_root/Cargo.toml" \
  | sed -n 's/.*"name":"dasobjectstore-cli","version":"\([^"]*\)".*/\1/p')"
version="${version:-0.0.0}"
arch="$(dpkg --print-architecture 2>/dev/null || uname -m)"
build_root="$repo_root/target/deb/${package_name}_${version}_${arch}"
package_path="$repo_root/target/deb/${package_name}_${version}_${arch}.deb"

cargo build --release -p dasobjectstore-cli --manifest-path "$repo_root/Cargo.toml"

rm -rf "$build_root"
install -d "$build_root/DEBIAN" "$build_root/usr/bin" "$build_root/usr/share/doc/$package_name"
install -m 0755 "$repo_root/target/release/dasobjectstore" "$build_root/usr/bin/dasobjectstore"
install -m 0755 "$repo_root/target/release/dasobjectstore-server" \
  "$build_root/usr/bin/dasobjectstore-server"
install -m 0644 "$repo_root/README.md" "$build_root/usr/share/doc/$package_name/README.md"

cat >"$build_root/DEBIAN/control" <<CONTROL
Package: $package_name
Version: $version
Section: utils
Priority: optional
Architecture: $arch
Maintainer: DASObjectStore contributors
Depends: ca-certificates, acl
Homepage: https://github.com/sagrudd/DASObjectStore
Description: SSD-first DAS-backed object store for bioinformatics
 DASObjectStore provides CLI and service binaries for staging objects on
 SSD and settling verified copies onto DAS or NAS storage endpoints.
CONTROL

dpkg-deb --build --root-owner-group "$build_root" "$package_path"
printf '%s\n' "$package_path"
