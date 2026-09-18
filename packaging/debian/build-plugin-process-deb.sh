#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$repo_root/packaging/plugin-process-package-provenance.sh"

usage() {
  echo "usage: $0 --server LINUX_AMD64_SERVER --web-dist VERIFIED_WEB_DIST --output-dir DIR" >&2
  exit 2
}

server='' web_dist='' output_dir=''
while [[ $# -gt 0 ]]; do
  case "$1" in
    --server) server=${2-}; shift 2 ;;
    --web-dist) web_dist=${2-}; shift 2 ;;
    --output-dir) output_dir=${2-}; shift 2 ;;
    *) usage ;;
  esac
done
[[ -n "$server" && -n "$web_dist" && -n "$output_dir" ]] || usage
[[ -f "$server" && -x "$server" ]] || { echo "plugin package requires an executable server input" >&2; exit 1; }
[[ -d "$web_dist" && -f "$web_dist/index.html" ]] || { echo "plugin package requires a verified web distribution" >&2; exit 1; }
find "$web_dist" -type f -name '*.wasm' -print -quit | grep -q . || { echo "plugin package requires a WebAssembly asset" >&2; exit 1; }
find "$web_dist" -type f -name '*.js' -print -quit | grep -q . || { echo "plugin package requires a JavaScript asset" >&2; exit 1; }
file -b "$server" | grep -Eq 'ELF 64-bit.*x86-64' || { echo "plugin package requires a Linux amd64 server input" >&2; exit 1; }
command -v dpkg-deb >/dev/null || { echo "dpkg-deb is required" >&2; exit 1; }
command -v jq >/dev/null || { echo "jq is required" >&2; exit 1; }

version=$(cargo metadata --locked --no-deps --format-version 1 --manifest-path "$repo_root/Cargo.toml" | jq -r '.packages[] | select(.name == "dasobjectstore-cli") | .version')
[[ "$version" =~ ^0\.186\.0$ ]] || { echo "plugin package requires DASObjectStore 0.186.0" >&2; exit 1; }
source_revision="${DASOBJECTSTORE_SOURCE_REVISION:-$(git -C "$repo_root" rev-parse HEAD)}"
source_epoch="${SOURCE_DATE_EPOCH:-$(git -C "$repo_root" log -1 --format=%ct)}"
[[ "$source_revision" =~ ^[0-9a-f]{40}$ && "$source_epoch" =~ ^[0-9]+$ ]] || { echo "plugin package requires exact source inputs" >&2; exit 1; }

package_name=dasobjectstore-plugin-process
package_path="$output_dir/${package_name}_${version}_amd64.deb"
root="$output_dir/root"
rm -rf "$root"
install -d "$root/DEBIAN" "$root/usr/bin" "$root/opt/dasobjectstore/web"
install -m 0755 "$server" "$root/usr/bin/dasobjectstore-server"
jq --arg version "$version" '.version = $version' "$repo_root/packaging/linux/opt/dasobjectstore/plugin-process-descriptor.json" > "$root/opt/dasobjectstore/plugin-process-descriptor.json"
cp -a "$web_dist/." "$root/opt/dasobjectstore/web/"
cat > "$root/DEBIAN/control" <<CONTROL
Package: $package_name
Version: $version
Section: utils
Priority: optional
Architecture: amd64
Maintainer: DASObjectStore contributors
Description: DASObjectStore plugin-process UI and API fixture
 Plugin-only process fixture; it contains no daemon, service, control socket,
 installation hook, credential, target, or deployment authority.
CONTROL
if date -u -d "@$source_epoch" +%Y%m%d%H%M.%S >/dev/null 2>&1; then
  timestamp=$(date -u -d "@$source_epoch" +%Y%m%d%H%M.%S)
else
  timestamp=$(date -u -r "$source_epoch" +%Y%m%d%H%M.%S)
fi
find "$root" -exec touch -h -t "$timestamp" {} +
SOURCE_DATE_EPOCH="$source_epoch" dpkg-deb --build --root-owner-group "$root" "$package_path" >/dev/null
DASOBJECTSTORE_SOURCE_REVISION="$source_revision" SOURCE_DATE_EPOCH="$source_epoch" das_plugin_process_write_provenance "$package_path" "$repo_root" "$version" amd64 "$web_dist" "$server"
"$repo_root/packaging/debian/validate-plugin-process-package.sh" "$package_path"
printf '%s\n' "$package_path"
