#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
package_name="dasobjectstore"
prosopikon_pam_marker="mnemosyne.prosopikon.native.pam"
version="$(cargo metadata --no-deps --format-version 1 --manifest-path "$repo_root/Cargo.toml" \
  | sed -n 's/.*"name":"dasobjectstore-cli","version":"\([^"]*\)".*/\1/p')"
version="${version:-0.4.2}"

if ! command -v dpkg-deb >/dev/null 2>&1; then
  cat >&2 <<ERROR
dpkg-deb is required to build the DASObjectStore Debian package.
On Ubuntu/Debian: sudo apt-get install dpkg
ERROR
  exit 1
fi

if ! command -v clang >/dev/null 2>&1 || ! ldconfig -p 2>/dev/null | grep -Eq 'libclang(-[0-9]+)?\.so'; then
  cat >&2 <<ERROR
Native DASObjectStore package builds require clang, libclang, and PAM headers.
On Ubuntu/Debian: sudo apt-get install clang libclang-dev libpam0g-dev
ERROR
  exit 1
fi

arch="$(dpkg --print-architecture 2>/dev/null || uname -m)"
build_root="$repo_root/target/deb/${package_name}_${version}_${arch}"
package_path="$repo_root/target/deb/${package_name}_${version}_${arch}.deb"

packaging_debian="$repo_root/packaging/debian"
packaging_linux="$repo_root/packaging/linux"
packaging_product="$packaging_linux/opt/dasobjectstore"
packaging_reporting="$repo_root/packaging/reporting"
web_dist="$(bash "$repo_root/packaging/web/prepare-web-dist.sh")"
bash "$packaging_debian/validate-package-assets.sh"

cargo build --release -p dasobjectstore-cli --manifest-path "$repo_root/Cargo.toml"
# Package builds are deliberately feature-minimal: development self-signing
# is a workspace-only test aid and must never enter a DEB payload.
cargo build --release --no-default-features -p dasobjectstore-daemon --manifest-path "$repo_root/Cargo.toml"
cargo build --release -p dasobjectstore-remote --manifest-path "$repo_root/Cargo.toml"

rm -rf "$build_root"
install -d \
  "$build_root/DEBIAN" \
  "$build_root/etc/dasobjectstore" \
  "$build_root/etc/pam.d" \
  "$build_root/lib/systemd/system" \
  "$build_root/opt/dasobjectstore" \
  "$build_root/opt/dasobjectstore/web" \
  "$build_root/usr/bin" \
  "$build_root/usr/libexec/dasobjectstore" \
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
install -m 0750 "$repo_root/target/release/dasobjectstore-local-auth-helper" \
  "$build_root/usr/libexec/dasobjectstore/dasobjectstore-local-auth-helper"
install -m 0755 "$packaging_reporting/gnostikon-workflow-control" \
  "$build_root/usr/libexec/dasobjectstore/gnostikon-workflow-control"
install -m 0755 "$packaging_linux/usr/libexec/dasobjectstore/prepare-external-mount-traversal" \
  "$build_root/usr/libexec/dasobjectstore/prepare-external-mount-traversal"
install -m 0755 "$packaging_linux/usr/libexec/dasobjectstore/configure-external-mount-policy" \
  "$build_root/usr/libexec/dasobjectstore/configure-external-mount-policy"
install -m 0644 "$repo_root/README.md" "$build_root/usr/share/doc/$package_name/README.md"
install -m 0644 "$packaging_linux/etc/dasobjectstore/daemon.json" \
  "$build_root/etc/dasobjectstore/daemon.json"
install -m 0644 "$packaging_linux/pam.d/dasobjectstore" \
  "$build_root/etc/pam.d/dasobjectstore"
install -m 0644 "$packaging_product/config.json" \
  "$build_root/opt/dasobjectstore/config.json"
install -m 0644 "$packaging_linux/systemd/dasobjectstored.service" \
  "$build_root/lib/systemd/system/dasobjectstored.service"
install -m 0644 "$packaging_linux/systemd/dasobjectstore-server.service" \
  "$build_root/lib/systemd/system/dasobjectstore-server.service"
install -m 0644 "$packaging_linux/systemd/dasobjectstore-source-access.service" \
  "$build_root/lib/systemd/system/dasobjectstore-source-access.service"
install -m 0644 "$packaging_linux/systemd/dasobjectstore-source-access.path" \
  "$build_root/lib/systemd/system/dasobjectstore-source-access.path"
install -m 0644 "$packaging_linux/sysusers.d/dasobjectstore.conf" \
  "$build_root/usr/lib/sysusers.d/dasobjectstore.conf"
install -m 0644 "$packaging_linux/tmpfiles.d/dasobjectstore.conf" \
  "$build_root/usr/lib/tmpfiles.d/dasobjectstore.conf"
cp -a "$web_dist/." "$build_root/opt/dasobjectstore/web/"
install -m 0755 "$packaging_debian/postinst" "$build_root/DEBIAN/postinst"

bash "$repo_root/packaging/validate-package-auth-content.sh" "$build_root"

cat >"$build_root/DEBIAN/control" <<CONTROL
Package: $package_name
Version: $version
Section: utils
Priority: optional
Architecture: $arch
Maintainer: DASObjectStore contributors
Depends: ca-certificates, acl, libpam0g, udisks2, docker.io, docker-buildx | docker-buildx-plugin, awscli
X-DASObjectStore-Build-Depends: rustc, cargo, trunk, wasm32-unknown-unknown, clang, libclang-dev, libpam0g-dev, dpkg, docker-buildx
X-Prosopikon-Native-Dependency-Markers: $prosopikon_pam_marker
Homepage: https://github.com/sagrudd/DASObjectStore
Description: SSD-first DAS-backed object store for bioinformatics
 DASObjectStore provides CLI and service binaries for staging objects on SSD
 and settling verified copies onto DAS or NAS storage endpoints. Long-running
 CLI operations may expose embedded terminal views through command flags.
CONTROL

dpkg-deb --build --root-owner-group "$build_root" "$package_path"
printf '%s\n' "$package_path"
