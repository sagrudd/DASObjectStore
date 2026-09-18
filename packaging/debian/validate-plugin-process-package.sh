#!/usr/bin/env bash
set -euo pipefail

package_path=${1:?usage: $0 PACKAGE.deb}
[[ -f "$package_path" ]] || { echo "plugin package does not exist: $package_path" >&2; exit 1; }
command -v dpkg-deb >/dev/null || { echo "dpkg-deb is required" >&2; exit 1; }
command -v jq >/dev/null || { echo "jq is required" >&2; exit 1; }

[[ "$(dpkg-deb -f "$package_path" Package)" = dasobjectstore-plugin-process ]] || { echo "unexpected plugin package name" >&2; exit 1; }
[[ "$(dpkg-deb -f "$package_path" Architecture)" = amd64 ]] || { echo "plugin package must be amd64" >&2; exit 1; }
version=$(dpkg-deb -f "$package_path" Version)
root=$(mktemp -d "${TMPDIR:-/tmp}/dasobjectstore-plugin-process-package.XXXXXX")
trap 'rm -rf "$root"' EXIT HUP INT TERM
dpkg-deb -x "$package_path" "$root"

server="$root/usr/bin/dasobjectstore-server"
descriptor="$root/opt/dasobjectstore/plugin-process-descriptor.json"
[[ -x "$server" && -f "$descriptor" ]] || { echo "plugin package is missing server or descriptor" >&2; exit 1; }
file -b "$server" | grep -Eq 'ELF 64-bit.*x86-64' || { echo "plugin server must be a Linux amd64 ELF executable" >&2; exit 1; }
jq -e --arg version "$version" '
  .schema == "mnemosyne.plugin-process-descriptor/v1" and
  .productId == "dasobjectstore" and
  .upstreamUnixSocket == "/run/dasobjectstore/plugin-process.sock" and
  .healthPath == "/health" and
  .uiMount == "/products/dasobjectstore/" and
  .apiMount == "/products/dasobjectstore/api/" and
  .audience == "monas:dasobjectstore" and .version == $version
' "$descriptor" >/dev/null || { echo "plugin descriptor does not match the DEB contract" >&2; exit 1; }

[[ -f "$root/opt/dasobjectstore/web/index.html" ]] || { echo "plugin package is missing web index" >&2; exit 1; }
find "$root/opt/dasobjectstore/web" -type f -name '*.wasm' -print -quit | grep -q . || { echo "plugin package is missing WebAssembly assets" >&2; exit 1; }
find "$root/opt/dasobjectstore/web" -type f -name '*.js' -print -quit | grep -q . || { echo "plugin package is missing JavaScript assets" >&2; exit 1; }
for forbidden in etc lib/systemd usr/lib usr/libexec usr/share; do
  [[ ! -e "$root/$forbidden" ]] || { echo "plugin package contains forbidden appliance path: /$forbidden" >&2; exit 1; }
done
if find "$root" -type l -print -quit | grep -q .; then
  echo "plugin package must not contain symlinks" >&2
  exit 1
fi
