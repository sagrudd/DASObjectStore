#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
package_name="dasobjectstore"
version="$(cargo metadata --no-deps --format-version 1 --manifest-path "$repo_root/Cargo.toml" \
  | sed -n 's/.*"name":"dasobjectstore-cli","version":"\([^"]*\)".*/\1/p')"
version="${version:-0.4.1}"

if ! command -v dpkg-deb >/dev/null 2>&1; then
  cat >&2 <<ERROR
dpkg-deb is required to build the DASObjectStore Debian package.
On Ubuntu/Debian: sudo apt-get install dpkg
ERROR
  exit 1
fi

arch="$(dpkg --print-architecture 2>/dev/null || uname -m)"
build_root="$repo_root/target/deb/${package_name}_${version}_${arch}"
package_path="$repo_root/target/deb/${package_name}_${version}_${arch}.deb"

packaging_debian="$repo_root/packaging/debian"
packaging_linux="$repo_root/packaging/linux"
bash "$packaging_debian/validate-package-assets.sh"

cargo build --release -p dasobjectstore-cli --manifest-path "$repo_root/Cargo.toml"
cargo build --release -p dasobjectstore-daemon --manifest-path "$repo_root/Cargo.toml"
cargo build --release -p dasobjectstore-remote --manifest-path "$repo_root/Cargo.toml"

rm -rf "$build_root"
install -d \
  "$build_root/DEBIAN" \
  "$build_root/etc/dasobjectstore" \
  "$build_root/lib/systemd/system" \
  "$build_root/usr/bin" \
  "$build_root/usr/lib/sysusers.d" \
  "$build_root/usr/lib/tmpfiles.d" \
  "$build_root/usr/share/doc/$package_name"
install -m 0755 "$repo_root/target/release/dasobjectstore" "$build_root/usr/bin/dasobjectstore"
install -m 0755 "$repo_root/target/release/dasobjectstore-server" \
  "$build_root/usr/bin/dasobjectstore-server"
install -m 0755 "$repo_root/target/release/dasobjectstored" \
  "$build_root/usr/bin/dasobjectstored"
install -m 0755 "$repo_root/target/release/dasobjectstore-remote" \
  "$build_root/usr/bin/dasobjectstore-remote"
install -m 0644 "$repo_root/README.md" "$build_root/usr/share/doc/$package_name/README.md"
install -m 0644 "$packaging_linux/etc/dasobjectstore/daemon.json" \
  "$build_root/etc/dasobjectstore/daemon.json"
install -m 0644 "$packaging_linux/systemd/dasobjectstored.service" \
  "$build_root/lib/systemd/system/dasobjectstored.service"
install -m 0644 "$packaging_linux/sysusers.d/dasobjectstore.conf" \
  "$build_root/usr/lib/sysusers.d/dasobjectstore.conf"
install -m 0644 "$packaging_linux/tmpfiles.d/dasobjectstore.conf" \
  "$build_root/usr/lib/tmpfiles.d/dasobjectstore.conf"
install -m 0755 "$packaging_debian/postinst" "$build_root/DEBIAN/postinst"

cat >"$build_root/DEBIAN/control" <<CONTROL
Package: $package_name
Version: $version
Section: utils
Priority: optional
Architecture: $arch
Maintainer: DASObjectStore contributors
Depends: ca-certificates, acl
Suggests: awscli
Homepage: https://github.com/sagrudd/DASObjectStore
Description: SSD-first DAS-backed object store for bioinformatics
 DASObjectStore provides CLI and service binaries for staging objects on SSD
 and settling verified copies onto DAS or NAS storage endpoints. Long-running
 CLI operations may expose embedded terminal views through command flags.
CONTROL

dpkg-deb --build --root-owner-group "$build_root" "$package_path"
printf '%s\n' "$package_path"
