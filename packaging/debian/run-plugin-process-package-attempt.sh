#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "usage: $0 --sealed-root SEALED_CLOSURE --attempt-root EXTERNAL_EMPTY_DIRECTORY --diagnostic-root EXTERNAL_EMPTY_DIRECTORY" >&2
  exit 2
}

sealed_root=''
attempt_root=''
diagnostic_root=''
while [[ $# -gt 0 ]]; do
  case "$1" in
    --sealed-root) sealed_root=${2-}; shift 2 ;;
    --attempt-root) attempt_root=${2-}; shift 2 ;;
    --diagnostic-root) diagnostic_root=${2-}; shift 2 ;;
    *) usage ;;
  esac
done
[[ -n "$sealed_root" && -n "$attempt_root" && -n "$diagnostic_root" ]] || usage

# Keep copied-source and generated-artifact modes independent of the caller.
umask 022

diagnostic_log=''
diagnostic_status=''

record_diagnostic() {
  [[ -n "$diagnostic_log" ]] || return 0
  printf '%s\n' "$1" >> "$diagnostic_log" 2>/dev/null || true
}

die() {
  record_diagnostic "preflight_failure=$*"
  echo "F05 package attempt: $*" >&2
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

canonical_directory() {
  local path=$1 label=$2
  [[ "$path" = /* && -d "$path" && ! -L "$path" ]] || die "requires an absolute, non-symlink $label"
  (cd "$path" && pwd -P)
}

reject_symlink_ancestry "$sealed_root" 'sealed closure root'
reject_symlink_ancestry "$attempt_root" 'external attempt root'
reject_symlink_ancestry "$diagnostic_root" 'external diagnostic root'
sealed_root=$(canonical_directory "$sealed_root" 'sealed closure root')
attempt_root=$(canonical_directory "$attempt_root" 'external attempt root')
diagnostic_root=$(canonical_directory "$diagnostic_root" 'external diagnostic root')
[[ "$attempt_root" != "$sealed_root" && "$attempt_root" != "$sealed_root"/* && "$sealed_root" != "$attempt_root"/* ]] || die 'sealed closure and external attempt root must not overlap'
[[ "$diagnostic_root" != "$sealed_root" && "$diagnostic_root" != "$sealed_root"/* && "$sealed_root" != "$diagnostic_root"/* ]] || die 'sealed closure and external diagnostic root must not overlap'
[[ "$diagnostic_root" != "$attempt_root" && "$diagnostic_root" != "$attempt_root"/* && "$attempt_root" != "$diagnostic_root"/* ]] || die 'external diagnostic root and external attempt root must not overlap'
diagnostic_log="$diagnostic_root/preflight.log"
diagnostic_status="$diagnostic_root/terminal-status"
if [[ "${DASOBJECTSTORE_F05_BWRAP_NETWORK_NAMESPACE:-}" != 1 ]]; then
  [[ -z "$(find "$diagnostic_root" -mindepth 1 -maxdepth 1 -print -quit)" ]] || die 'external diagnostic root must be empty'
else
  [[ -f "$diagnostic_log" && ! -L "$diagnostic_log" ]] || die 'requires an outer preflight diagnostic log'
  [[ "$(find "$diagnostic_root" -mindepth 1 -maxdepth 1 -type f -printf '%f\n')" == 'preflight.log' ]] || die 'external diagnostic root contains unexpected entries'
fi
[[ -z "$(find "$attempt_root" -mindepth 1 -maxdepth 1 -print -quit)" ]] || die 'external attempt root must be empty'
record_diagnostic 'attempt_root_empty=PASS'
[[ -f "$sealed_root/f05-inputs.sha256" && -d "$sealed_root/source" ]] || die 'sealed closure requires f05-inputs.sha256 and source'
if find "$sealed_root" -type l -print -quit | grep -q .; then
  die 'sealed closure must not contain symlinks'
fi
if [[ "${DASOBJECTSTORE_F05_BWRAP_NETWORK_NAMESPACE:-}" != 1 ]]; then
  [[ -x /usr/bin/bwrap ]] || die 'requires /usr/bin/bwrap network isolation'
  record_diagnostic 'bwrap_launch=PASS'
  exec /usr/bin/bwrap --unshare-net --ro-bind / / --ro-bind "$sealed_root" /opt --bind "$attempt_root" /var/tmp --bind "$diagnostic_root" /var/cache --proc /proc --dev /dev \
    --setenv DASOBJECTSTORE_F05_BWRAP_NETWORK_NAMESPACE 1 -- /usr/bin/bash /opt/source/packaging/debian/run-plugin-process-package-attempt.sh --sealed-root /opt --attempt-root /var/tmp --diagnostic-root /var/cache
fi
[[ "$(/usr/sbin/ip -o link show | awk -F': ' '{print $2}' | sed 's/@.*//')" == lo ]] || die 'requires loopback-only network interfaces'
[[ -z "$(/usr/sbin/ip -4 route show)" ]] || die 'requires empty IPv4 routes'

status_file="$diagnostic_status"
sealed_status=0
finish() {
  local status=$?
  set +e
  (cd "$sealed_root" && shasum -a 256 -c f05-inputs.sha256) >/dev/null 2>&1 || sealed_status=1
  if [[ "$sealed_status" -ne 0 && "$status" -eq 0 ]]; then
    status=1
  fi
  printf 'exit_code=%s\n' "$status" > "$status_file"
  record_diagnostic "terminal_exit_code=$status"
  exit "$status"
}
trap finish EXIT HUP INT TERM

(cd "$sealed_root" && shasum -a 256 -c f05-inputs.sha256) >/dev/null 2>&1 || die 'sealed closure has a missing or altered input'

copied_closure="$attempt_root/closure"
cp -a "$sealed_root" "$copied_closure"
[[ -d "$copied_closure/source" && ! -L "$copied_closure" ]] || die 'failed to make a physical copied closure'
if find "$copied_closure" -type l -print -quit | grep -q .; then
  die 'copied closure must not contain symlinks'
fi
chmod -R u+w "$copied_closure"

copied_source="$copied_closure/source"
copied_manifest="$copied_source/Cargo.toml"
copied_lock="$copied_source/Cargo.lock"
copied_config="$copied_source/.cargo/f05-vendor-config.toml"
for copied_input in "$copied_manifest" "$copied_lock" "$copied_config"; do
  [[ -f "$copied_input" && ! -L "$copied_input" ]] || die "copied closure requires a physical non-symlink $(basename "$copied_input")"
done
sed -E "s|^directory = \".*\"$|directory = \"$copied_source/vendor\"|" "$copied_config" > "$copied_config.next"
mv "$copied_config.next" "$copied_config"
(
  cd "$copied_closure"
  find . -type f ! -name f05-inputs.sha256 -print | LC_ALL=C sort | while IFS= read -r input; do
    printf '%s  %s\n' "$(shasum -a 256 "$input" | awk '{print $1}')" "${input#./}"
  done > f05-inputs.sha256
)
server="$attempt_root/target/release/dasobjectstore-server"
web_dist="$copied_source/crates/dasobjectstore-gui-web/dist"
output_dir="$attempt_root/output"
install -d -m 0755 "$attempt_root/home" "$attempt_root/cargo-home/server" "$attempt_root/target" "$attempt_root/tmp" "$output_dir"

export DASOBJECTSTORE_F05_STAGED_CLOSURE_ROOT="$copied_closure"
export DASOBJECTSTORE_F05_ATTEMPT_ROOT="$attempt_root"
export HOME="$attempt_root/home"
export CARGO_HOME="$attempt_root/cargo-home/server"
export CARGO_NET_OFFLINE=true
export CARGO_TARGET_DIR="$attempt_root/target"
export TMPDIR="$attempt_root/tmp"
export RUSTC="$copied_closure/toolchain/bin/rustc"
export PATH="$copied_closure/network-denied-bin:$copied_closure/toolchain/bin:/usr/bin:/bin"

cp "$copied_source/.cargo/f05-vendor-config.toml" "$CARGO_HOME/config.toml"
(
  cd "$copied_source"
  "$copied_closure/toolchain/bin/cargo" build --manifest-path "$copied_manifest" --offline --config "$copied_config" --locked --release -p dasobjectstore-cli --bin dasobjectstore-server
)
"$copied_source/packaging/web/prepare-web-dist.sh"
"$copied_source/packaging/debian/build-plugin-process-deb.sh" --server "$server" --web-dist "$web_dist" --output-dir "$output_dir"
