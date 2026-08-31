#!/usr/bin/env bash
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$repo_root/packaging/package-provenance.sh"
source "$repo_root/packaging/cargo-target-dir.sh"
for builder in packaging/debian/build-deb.sh packaging/debian/build-remote-deb.sh packaging/rpm/build-rpm.sh packaging/rpm/build-remote-rpm.sh; do grep -q das_package_provenance_init "$repo_root/$builder"; grep -q das_package_normalize_tree "$repo_root/$builder"; done
for builder in packaging/debian/build-deb.sh packaging/debian/build-remote-deb.sh; do grep -q 'SOURCE_DATE_EPOCH=.*dpkg-deb' "$repo_root/$builder"; done
for builder in packaging/rpm/build-rpm.sh packaging/rpm/build-remote-rpm.sh; do grep -q -- '--sort=name --mtime=' "$repo_root/$builder"; done
for builder in packaging/debian/build-remote-deb.sh packaging/rpm/build-remote-rpm.sh; do
  grep -Fq 'packaging/cargo-target-dir.sh' "$repo_root/$builder"
  grep -Fq 'das_cargo_target_dir' "$repo_root/$builder"
  grep -Fq '"$cargo_target_dir/release/dasobjectstore-remote"' "$repo_root/$builder"
done
work="$(mktemp -d)"; trap 'rm -rf "$work"' EXIT
[[ "$(env -u CARGO_TARGET_DIR bash -c 'source "$1"; das_cargo_target_dir "$2"' _ "$repo_root/packaging/cargo-target-dir.sh" "$repo_root")" == "$repo_root/target" ]]
[[ "$(CARGO_TARGET_DIR=isolated-target bash -c 'source "$1"; das_cargo_target_dir "$2"' _ "$repo_root/packaging/cargo-target-dir.sh" "$repo_root")" == "$repo_root/isolated-target" ]]
[[ "$(CARGO_TARGET_DIR="$work/external-target" bash -c 'source "$1"; das_cargo_target_dir "$2"' _ "$repo_root/packaging/cargo-target-dir.sh" "$repo_root")" == "$work/external-target" ]]
export SOURCE_DATE_EPOCH=1 DASOBJECTSTORE_SOURCE_REVISION=0123456789abcdef
das_package_provenance_init "$repo_root"
for pass in one two; do root="$work/$pass"; install -d "$root/DEBIAN" "$root/usr/share/das"; printf 'fixture\n' >"$root/usr/share/das/value"; printf 'Package: das-fixture\nVersion: 1\nArchitecture: all\nDescription: fixture\n' >"$root/DEBIAN/control"; das_package_normalize_tree "$root"; dpkg-deb --build --root-owner-group "$root" "$work/$pass.deb" >/dev/null; das_package_write_provenance "$work/$pass.deb" fixture all; done
cmp "$work/one.deb" "$work/two.deb"; cmp "$work/one.deb.provenance.json" "$work/two.deb.provenance.json"
echo package-provenance-regression=passed
