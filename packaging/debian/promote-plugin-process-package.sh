#!/usr/bin/env bash
# Promote an already-built plugin-process package and its provenance sidecar.
# This helper intentionally has no build, installer, service, or network path.
set -euo pipefail

usage() {
  echo "usage: $0 --source-deb FILE --provenance FILE --expected-sha256 SHA256 --destination-dir DIRECTORY --source-revision REVISION --package-version VERSION --architecture ARCHITECTURE" >&2
  exit 2
}

source_deb=''
source_provenance=''
expected_sha256=''
destination_dir=''
source_revision=''
package_version=''
architecture=''
while [[ $# -gt 0 ]]; do
  case "$1" in
    --source-deb) source_deb=${2-}; shift 2 ;;
    --provenance) source_provenance=${2-}; shift 2 ;;
    --expected-sha256) expected_sha256=${2-}; shift 2 ;;
    --destination-dir) destination_dir=${2-}; shift 2 ;;
    --source-revision) source_revision=${2-}; shift 2 ;;
    --package-version) package_version=${2-}; shift 2 ;;
    --architecture) architecture=${2-}; shift 2 ;;
    *) usage ;;
  esac
done
[[ -n "$source_deb" && -n "$source_provenance" && -n "$expected_sha256" && -n "$destination_dir" && -n "$source_revision" && -n "$package_version" && -n "$architecture" ]] || usage

die() {
  echo "plugin-process promotion: $*" >&2
  exit 1
}

reject_symlink_ancestry() {
  local path=$1 label=$2 component current=''
  [[ "$path" = /* ]] || die "requires an absolute $label"
  IFS=/ read -r -a components <<< "$path"
  for component in "${components[@]}"; do
    [[ -z "$component" ]] && continue
    [[ "$component" != . && "$component" != .. ]] || die "$label must not contain . or .. path components"
    current="$current/$component"
    [[ ! -L "$current" ]] || die "$label must not pass through a symlink: $current"
  done
}

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    die 'requires a SHA-256 command (sha256sum or shasum)'
  fi
}

[[ "$expected_sha256" =~ ^[0-9a-f]{64}$ ]] || die 'requires a lowercase SHA-256 digest'
[[ "$source_revision" =~ ^[0-9a-f]{40}$ ]] || die 'requires an exact source revision'
[[ "$package_version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] || die 'requires a strict package SemVer'
[[ "$architecture" == amd64 ]] || die 'requires the plugin-process amd64 architecture'

reject_symlink_ancestry "$source_deb" 'source DEB'
reject_symlink_ancestry "$source_provenance" 'source provenance'
reject_symlink_ancestry "$destination_dir" 'destination directory'
[[ -f "$source_deb" && ! -L "$source_deb" ]] || die 'requires a physical source DEB'
[[ -f "$source_provenance" && ! -L "$source_provenance" ]] || die 'requires a physical source provenance sidecar'
[[ -d "$destination_dir" && ! -L "$destination_dir" ]] || die 'requires a physical caller-owned destination directory'
destination_dir="$(cd "$destination_dir" && pwd -P)"

source_digest="$(sha256 "$source_deb")"
[[ "$source_digest" == "$expected_sha256" ]] || die 'source DEB digest does not match the expected digest'
jq -e \
  --arg revision "$source_revision" \
  --arg version "$package_version" \
  --arg architecture "$architecture" \
  --arg digest "$expected_sha256" \
  '.schema == "mnemosyne.dasobjectstore.plugin-process-package-provenance.v1" and
   .package_name == "dasobjectstore-plugin-process" and
   .source_revision == $revision and
   .package_version == $version and
   .architecture == $architecture and
   .package_sha256 == $digest' \
  "$source_provenance" >/dev/null || die 'provenance does not bind the expected plugin-process identity and package bytes'

source_name="$(basename "$source_deb")"
provenance_name="$(basename "$source_provenance")"
[[ "$source_name" == *.deb ]] || die 'source package must be a DEB file'
promotion_id="dasobjectstore-plugin-process-${package_version}-${architecture}-${expected_sha256}"
stage_dir="$(mktemp -d "$destination_dir/.${promotion_id}.staging.XXXXXX")"
final_dir="$destination_dir/$promotion_id"
committed=0
cleanup() {
  local status=$?
  if [[ "$committed" -ne 1 ]]; then
    [[ -n "${stage_dir:-}" && -d "$stage_dir" ]] && rm -rf "$stage_dir"
    [[ -n "${final_dir:-}" && -d "$final_dir" ]] && rm -rf "$final_dir"
  fi
  exit "$status"
}
trap cleanup EXIT HUP INT TERM
[[ ! -e "$final_dir" ]] || die 'destination already contains this promoted package identity'

cp -p "$source_deb" "$stage_dir/$source_name"
cp -p "$source_provenance" "$stage_dir/$provenance_name"
[[ "$(sha256 "$source_deb")" == "$expected_sha256" ]] || die 'source DEB changed during promotion'
[[ "$(sha256 "$stage_dir/$source_name")" == "$expected_sha256" ]] || die 'staged DEB digest does not match the expected digest'
jq -e \
  --arg revision "$source_revision" \
  --arg version "$package_version" \
  --arg architecture "$architecture" \
  --arg digest "$expected_sha256" \
  '.schema == "mnemosyne.dasobjectstore.plugin-process-package-provenance.v1" and
   .package_name == "dasobjectstore-plugin-process" and
   .source_revision == $revision and
   .package_version == $version and
   .architecture == $architecture and
   .package_sha256 == $digest' \
  "$stage_dir/$provenance_name" >/dev/null || die 'staged provenance does not bind the expected package identity'

source_provenance_digest="$(sha256 "$source_provenance")"
jq -n \
  --arg source_deb_sha256 "$source_digest" \
  --arg source_provenance_sha256 "$source_provenance_digest" \
  --arg destination_deb_sha256 "$expected_sha256" \
  --arg source_revision "$source_revision" \
  --arg package_version "$package_version" \
  --arg architecture "$architecture" \
  '{schema:"mnemosyne.dasobjectstore.plugin-process-promotion-receipt.v1",package_name:"dasobjectstore-plugin-process",source_deb_sha256:$source_deb_sha256,source_provenance_sha256:$source_provenance_sha256,destination_deb_sha256:$destination_deb_sha256,source_revision:$source_revision,package_version:$package_version,architecture:$architecture}' \
  > "$stage_dir/promotion-receipt.json"

# Both paths are immediate children of the caller-owned destination directory,
# so this directory rename is an atomic same-filesystem promotion.
mv "$stage_dir" "$final_dir"
stage_dir=''
[[ "$(sha256 "$final_dir/$source_name")" == "$expected_sha256" ]] || die 'destination DEB digest changed after promotion'
jq -e \
  --arg digest "$expected_sha256" \
  --arg provenance_digest "$source_provenance_digest" \
  '.schema == "mnemosyne.dasobjectstore.plugin-process-promotion-receipt.v1" and
   .package_name == "dasobjectstore-plugin-process" and
   .source_deb_sha256 == $digest and
   .destination_deb_sha256 == $digest and
   .source_provenance_sha256 == $provenance_digest' \
  "$final_dir/promotion-receipt.json" >/dev/null || die 'promotion receipt does not bind the promoted destination bytes'
committed=1
