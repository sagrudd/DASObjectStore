#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "usage: $0 --sealed-root SEALED_CLOSURE --attempt-root EXTERNAL_EMPTY_DIRECTORY --diagnostic-root EXTERNAL_EMPTY_DIRECTORY [--stage-cache-root LEASED_STAGE_CACHE_DIRECTORY] [--preflight-only]" >&2
  echo "       $0 --sealed-root WRITABLE_CLOSURE_STAGE --stage-closure-config" >&2
  echo "       $0 --sealed-root WRITABLE_CLOSURE_STAGE --stage-closure-toolchain-inputs ABSOLUTE_IMMUTABLE_INPUT_ROOT" >&2
  exit 2
}

sealed_root=''
attempt_root=''
diagnostic_root=''
stage_cache_root=''
preflight_only=0
stage_closure_config=0
stage_closure_toolchain_inputs=0
toolchain_input_root=''
while [[ $# -gt 0 ]]; do
  case "$1" in
    --sealed-root) sealed_root=${2-}; shift 2 ;;
    --attempt-root) attempt_root=${2-}; shift 2 ;;
    --diagnostic-root) diagnostic_root=${2-}; shift 2 ;;
    --stage-cache-root) stage_cache_root=${2-}; shift 2 ;;
    --preflight-only) preflight_only=1; shift ;;
    --stage-closure-config) stage_closure_config=1; shift ;;
    --stage-closure-toolchain-inputs) stage_closure_toolchain_inputs=1; toolchain_input_root=${2-}; shift 2 ;;
    *) usage ;;
  esac
done
(( stage_closure_config + stage_closure_toolchain_inputs <= 1 )) || usage
if [[ "$stage_closure_config" -eq 1 ]]; then
  [[ -n "$sealed_root" && -z "$attempt_root" && -z "$diagnostic_root" && -z "$stage_cache_root" && "$preflight_only" -eq 0 ]] || usage
elif [[ "$stage_closure_toolchain_inputs" -eq 1 ]]; then
  [[ -n "$sealed_root" && -n "$toolchain_input_root" && -z "$attempt_root" && -z "$diagnostic_root" && -z "$stage_cache_root" && "$preflight_only" -eq 0 ]] || usage
else
  [[ -n "$sealed_root" && -n "$attempt_root" && -n "$diagnostic_root" ]] || usage
fi
[[ "$preflight_only" -eq 0 || -n "$stage_cache_root" ]] || {
  echo 'F05 package attempt: preflight-only requires a leased stage cache root' >&2
  exit 2
}

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

hash_jobs=${DASOBJECTSTORE_F05_HASH_JOBS:-4}
[[ "$hash_jobs" =~ ^[1-9][0-9]*$ && "$hash_jobs" -le 16 ]] || die 'requires DASOBJECTSTORE_F05_HASH_JOBS between 1 and 16'

sha256_file() {
  shasum -a 256 "$1" | awk '{print $1}'
}

sha256_tree() {
  local root=$1
  (
    cd "$root"
    find . -type f -print0 | LC_ALL=C sort -z | xargs -0 -r shasum -a 256 | shasum -a 256 | awk '{print $1}'
  )
}

write_batched_manifest() {
  local root=$1 manifest="$1/f05-inputs.sha256" temporary="$1/f05-inputs.sha256.next"
  (
    cd "$root"
    find . -type f ! -name f05-inputs.sha256 ! -name f05-inputs.sha256.next -print0 | LC_ALL=C sort -z |
      xargs -0 -r -n 128 -P "$hash_jobs" /usr/bin/bash -c '
        for input; do
          printf "%s  %s\\n" "$(shasum -a 256 "$input" | cut -d " " -f1)" "${input#./}"
        done
      ' bash | LC_ALL=C sort -k2 > "$temporary"
  )
  mv "$temporary" "$manifest"
}

produce_closure_vendor_config() {
  local source cargo_dir vendor config temporary staged_runner candidate source_tree witness
  local candidate_revision source_tree_revision witness_revision generator_sha

  source="$sealed_root/source"
  cargo_dir="$source/.cargo"
  vendor="$source/vendor"
  config="$cargo_dir/f05-vendor-config.toml"
  staged_runner="$source/packaging/debian/run-plugin-process-package-attempt.sh"
  candidate="$sealed_root/inputs/component-candidate-input.toml"
  source_tree="$sealed_root/inputs/source-tree"
  witness="$sealed_root/inputs/compiled-dependency-witness.json"

  [[ -d "$source" && ! -L "$source" ]] || die 'closure stage requires a physical source root'
  source=$(canonical_directory "$source" 'closure-stage source root')
  [[ "$source" = "$sealed_root/source" ]] || die 'closure-stage source root must remain under the closure root'
  [[ -d "$cargo_dir" && ! -L "$cargo_dir" ]] || die 'closure stage requires a physical source .cargo directory'
  cargo_dir=$(canonical_directory "$cargo_dir" 'closure-stage source .cargo directory')
  [[ "$cargo_dir" = "$source/.cargo" ]] || die 'closure-stage source .cargo directory must remain under the source root'
  [[ -d "$vendor" && ! -L "$vendor" ]] || die 'closure stage requires a physical staged vendor tree'
  vendor=$(canonical_directory "$vendor" 'closure-stage staged vendor tree')
  [[ "$vendor" = "$source/vendor" ]] || die 'closure-stage staged vendor tree must remain under the source root'
  [[ -n "$(find "$vendor" -mindepth 1 -print -quit)" ]] || die 'closure stage requires a non-empty staged vendor tree'
  [[ -f "$staged_runner" && ! -L "$staged_runner" ]] || die 'closure stage requires a physical staged runner'
  [[ -f "$candidate" && ! -L "$candidate" && -f "$source_tree" && ! -L "$source_tree" && -f "$witness" && ! -L "$witness" ]] || die 'closure stage requires physical candidate, source-tree, and dependency-witness inputs'
  candidate_revision=$(sed -n 's/^source_revision = "\([0-9a-f]\{40\}\)"$/\1/p' "$candidate")
  source_tree_revision=$(sed -n 's/^revision=\([0-9a-f]\{40\}\)$/\1/p' "$source_tree")
  witness_revision=$(sed -n 's/.*"source_revision": "\([0-9a-f]\{40\}\)".*/\1/p' "$witness")
  [[ -n "$candidate_revision" && "$candidate_revision" = "$source_tree_revision" && "$candidate_revision" = "$witness_revision" ]] || die 'closure stage requires matching candidate, source-tree, and dependency-witness revisions'
  [[ ! -L "$config" && ! -e "$config.next" ]] || die 'closure stage vendor config path must be absent or a physical file without a pending replacement'
  generator_sha=$(sha256_file "$staged_runner")
  [[ "$generator_sha" = "$(sha256_file "$0")" ]] || die 'closure stage must execute the same runner bytes that it binds into the staged manifest'
  temporary="$config.next"
  printf '[source.vendored-sources]\ndirectory = "%s"\n' "$vendor" > "$temporary"
  mv "$temporary" "$config"
  [[ -f "$config" && ! -L "$config" ]] || die 'closure stage failed to create a physical vendor config'
  grep -Fx "directory = \"$vendor\"" "$config" >/dev/null || die 'closure stage generated vendor config does not bind the physical staged vendor tree'
  write_batched_manifest "$sealed_root"
  (cd "$sealed_root" && shasum -a 256 -c f05-inputs.sha256) >/dev/null 2>&1 || die 'closure stage manifest does not bind generated inputs'
  printf 'closure_stage_vendor_config=PASS generator_runner_sha256=%s config_sha256=%s vendor=%s\n' "$generator_sha" "$(sha256_file "$config")" "$vendor"
}

tool_input_toml_value() {
  local file=$1 key=$2 value
  value=$(sed -n "s/^${key} = \"\([^\"]*\)\"$/\1/p" "$file")
  [[ "$(printf '%s\n' "$value" | sed '/^$/d' | wc -l)" -eq 1 ]] || die "toolchain input receipt requires exactly one ${key}"
  printf '%s\n' "$value"
}

tool_inventory_value() {
  local file=$1 section=$2 key=$3 value
  value=$(awk -v section="$section" -v key="$key" '
    $0 == "[" section "]" { active=1; next }
    /^\[/ { active=0 }
    active && index($0, key "=") == 1 { print substr($0, length(key) + 2) }
  ' "$file")
  [[ "$(printf '%s\n' "$value" | sed '/^$/d' | wc -l)" -eq 1 ]] || die "toolchain input inventory requires exactly one ${section}.${key}"
  printf '%s\n' "$value"
}

produce_closure_toolchain_inputs() {
  local input_receipt input_manifest input_inventory candidate source_tree witness staged_runner
  local candidate_revision source_tree_revision witness_revision candidate_image candidate_image_sha
  local input_revision input_image input_image_sha input_inventory_sha input_manifest_sha receipt temporary
  local required inventory_sha

  input_receipt="$toolchain_input_root/tool-inputs.toml"
  input_manifest="$toolchain_input_root/tool-inputs.sha256"
  input_inventory="$toolchain_input_root/tool-input-inventory.txt"
  candidate="$sealed_root/inputs/component-candidate-input.toml"
  source_tree="$sealed_root/inputs/source-tree"
  witness="$sealed_root/inputs/compiled-dependency-witness.json"
  staged_runner="$sealed_root/source/packaging/debian/run-plugin-process-package-attempt.sh"

  [[ "$toolchain_input_root" = /* && -d "$toolchain_input_root" && ! -L "$toolchain_input_root" ]] || die 'toolchain input stage requires an absolute, physical input root'
  [[ -z "$(find "$toolchain_input_root" -type l -print -quit)" ]] || die 'toolchain input stage rejects symlinked inputs'
  [[ -z "$(find "$toolchain_input_root" -perm /0222 -print -quit)" ]] || die 'toolchain input stage requires an immutable input root'
  for required in "$input_receipt" "$input_manifest" "$input_inventory" "$candidate" "$source_tree" "$witness" "$staged_runner"; do
    [[ -f "$required" && ! -L "$required" ]] || die 'toolchain input stage requires physical receipt, manifest, and identity witnesses'
  done
  [[ "$(sha256_file "$staged_runner")" = "$(sha256_file "$0")" ]] || die 'toolchain input stage must execute the runner bytes that it binds'
  inventory_sha=$(sha256_file "$input_inventory")
  [[ "$inventory_sha" = 'de2fa3ef73e6df2253295488c24833aa8fb4ceb1d9313a27b51dd3fdf309fe4f' ]] || die 'toolchain input stage requires the reviewed physical inventory document'
  for required in \
    "$toolchain_input_root/toolchain/bin/cargo" \
    "$toolchain_input_root/toolchain/bin/rustc" \
    "$toolchain_input_root/toolchain/bin/trunk" \
    "$toolchain_input_root/toolchain/trunk-tools/wasm-bindgen-0.2.128/wasm-bindgen" \
    "$toolchain_input_root/toolchain/trunk-tools/wasm-opt-version_123/wasm-opt" \
    "$toolchain_input_root/network-denied-bin/git"; do
    [[ -f "$required" && -x "$required" && ! -L "$required" ]] || die 'toolchain input stage requires complete executable tool inputs'
  done
  [[ -d "$toolchain_input_root/toolchain/lib/rustlib/wasm32-unknown-unknown" ]] || die 'toolchain input stage requires the wasm32 target'
  [[ -n "$(find "$toolchain_input_root/toolchain/lib/rustlib/wasm32-unknown-unknown" -type f -print -quit)" ]] || die 'toolchain input stage requires a non-empty wasm32 target'
  [[ -d "$toolchain_input_root/network-denied-bin" && -d "$toolchain_input_root/staging-home" ]] || die 'toolchain input stage requires network denial and staging home inputs'
  (cd "$toolchain_input_root" && shasum -a 256 -c tool-inputs.sha256) >/dev/null 2>&1 || die 'toolchain input stage rejects altered input bytes'
  [[ "$(sha256_file "$toolchain_input_root/toolchain/bin/cargo")" = "$(tool_inventory_value "$input_inventory" cargo sha256)" ]] || die 'toolchain input stage rejects a substituted cargo input'
  [[ "$(sha256_file "$toolchain_input_root/toolchain/bin/rustc")" = "$(tool_inventory_value "$input_inventory" rustc sha256)" ]] || die 'toolchain input stage rejects a substituted rustc input'
  [[ "$(sha256_file "$toolchain_input_root/toolchain/bin/trunk")" = "$(tool_inventory_value "$input_inventory" trunk sha256)" ]] || die 'toolchain input stage rejects a substituted trunk input'
  [[ "$(sha256_file "$toolchain_input_root/toolchain/trunk-tools/wasm-bindgen-0.2.128/wasm-bindgen")" = "$(tool_inventory_value "$input_inventory" wasm_bindgen sha256)" ]] || die 'toolchain input stage rejects a substituted wasm-bindgen input'
  [[ "$(sha256_file "$toolchain_input_root/toolchain/trunk-tools/wasm-opt-version_123/wasm-opt")" = "$(tool_inventory_value "$input_inventory" wasm_opt sha256)" ]] || die 'toolchain input stage rejects a substituted wasm-opt input'
  [[ "$(sha256_tree "$toolchain_input_root/toolchain/lib/rustlib/wasm32-unknown-unknown")" = "$(tool_inventory_value "$input_inventory" wasm_sysroot tree_sha256)" ]] || die 'toolchain input stage rejects a substituted wasm sysroot tree'
  [[ "$(sha256_tree "$toolchain_input_root/toolchain")" = "$(tool_inventory_value "$input_inventory" prior_staged_toolchain tree_sha256)" ]] || die 'toolchain input stage rejects a substituted toolchain tree'
  [[ "$(sha256_tree "$toolchain_input_root/network-denied-bin")" = "$(tool_inventory_value "$input_inventory" network_denial tree_sha256)" && "$(sha256_file "$toolchain_input_root/network-denied-bin/git")" = "$(tool_inventory_value "$input_inventory" network_denial sha256)" ]] || die 'toolchain input stage rejects a substituted network-denial input'
  [[ "$(sha256_tree "$toolchain_input_root/staging-home")" = "$(tool_inventory_value "$input_inventory" staging_home tree_sha256)" ]] || die 'toolchain input stage rejects a substituted staging-home tree'

  candidate_revision=$(sed -n 's/^source_revision = "\([0-9a-f]\{40\}\)"$/\1/p' "$candidate")
  source_tree_revision=$(sed -n 's/^revision=\([0-9a-f]\{40\}\)$/\1/p' "$source_tree")
  witness_revision=$(sed -n 's/.*"source_revision": "\([0-9a-f]\{40\}\)".*/\1/p' "$witness")
  candidate_image=$(tool_input_toml_value "$candidate" toolchain_image)
  candidate_image_sha=$(tool_input_toml_value "$candidate" toolchain_image_sha256)
  input_revision=$(tool_input_toml_value "$input_receipt" source_revision)
  input_image=$(tool_input_toml_value "$input_receipt" toolchain_image)
  input_image_sha=$(tool_input_toml_value "$input_receipt" toolchain_image_sha256)
  input_inventory_sha=$(tool_input_toml_value "$input_receipt" inventory_sha256)
  [[ "$candidate_revision" =~ ^[0-9a-f]{40}$ && "$candidate_revision" = "$source_tree_revision" && "$candidate_revision" = "$witness_revision" && "$candidate_revision" = "$input_revision" ]] || die 'toolchain input stage requires matching candidate image and revision witnesses'
  [[ "$candidate_image" = "$input_image" && "$candidate_image_sha" = "$input_image_sha" && "$candidate_image" =~ @sha256:[0-9a-f]{64}$ && "$candidate_image_sha" =~ ^sha256:[0-9a-f]{64}$ && "sha256:${candidate_image##*@sha256:}" = "$candidate_image_sha" ]] || die 'toolchain input stage rejects a candidate image mismatch'
  [[ "$input_inventory_sha" = "$inventory_sha" ]] || die 'toolchain input stage rejects an inventory receipt not bound to the reviewed document'
  [[ ! -e "$sealed_root/toolchain" && ! -e "$sealed_root/network-denied-bin" && ! -e "$sealed_root/staging-home" && ! -e "$sealed_root/inputs/toolchain-input-receipt.toml" ]] || die 'toolchain input stage refuses to overwrite closure inputs'

  cp -a "$toolchain_input_root/toolchain" "$sealed_root/toolchain"
  cp -a "$toolchain_input_root/network-denied-bin" "$sealed_root/network-denied-bin"
  cp -a "$toolchain_input_root/staging-home" "$sealed_root/staging-home"
  [[ -z "$(find "$sealed_root/toolchain" "$sealed_root/network-denied-bin" "$sealed_root/staging-home" -type l -print -quit)" ]] || die 'toolchain input stage copied a symlinked input'
  input_manifest_sha=$(sha256_file "$input_manifest")
  receipt="$sealed_root/inputs/toolchain-input-receipt.toml"
  temporary="$receipt.next"
  printf 'source_revision = "%s"\ntoolchain_image = "%s"\ntoolchain_image_sha256 = "%s"\ninventory_sha256 = "%s"\ntool_input_manifest_sha256 = "%s"\nreviewed_inventory_document_sha256 = "%s"\n' \
    "$input_revision" "$input_image" "$input_image_sha" "$input_inventory_sha" "$input_manifest_sha" "$inventory_sha" > "$temporary"
  mv "$temporary" "$receipt"
  write_batched_manifest "$sealed_root"
  (cd "$sealed_root" && shasum -a 256 -c f05-inputs.sha256) >/dev/null 2>&1 || die 'toolchain input stage manifest does not bind copied inputs'
  printf 'closure_stage_toolchain_inputs=PASS inventory_sha256=%s tool_input_manifest_sha256=%s\n' "$input_inventory_sha" "$input_manifest_sha"
}

reject_symlink_ancestry "$sealed_root" 'sealed closure root'
if [[ "$stage_closure_config" -eq 0 && "$stage_closure_toolchain_inputs" -eq 0 ]]; then
  reject_symlink_ancestry "$attempt_root" 'external attempt root'
  reject_symlink_ancestry "$diagnostic_root" 'external diagnostic root'
fi
if [[ -n "$stage_cache_root" ]]; then
  reject_symlink_ancestry "$stage_cache_root" 'leased stage cache root'
fi
sealed_root=$(canonical_directory "$sealed_root" 'sealed closure root')
if [[ "$stage_closure_config" -eq 1 ]]; then
  produce_closure_vendor_config
  exit 0
fi
if [[ "$stage_closure_toolchain_inputs" -eq 1 ]]; then
  reject_symlink_ancestry "$toolchain_input_root" 'toolchain input root'
  toolchain_input_root=$(canonical_directory "$toolchain_input_root" 'toolchain input root')
  produce_closure_toolchain_inputs
  exit 0
fi
attempt_root=$(canonical_directory "$attempt_root" 'external attempt root')
diagnostic_root=$(canonical_directory "$diagnostic_root" 'external diagnostic root')
if [[ -n "$stage_cache_root" ]]; then
  stage_cache_root=$(canonical_directory "$stage_cache_root" 'leased stage cache root')
fi
[[ "$attempt_root" != "$sealed_root" && "$attempt_root" != "$sealed_root"/* && "$sealed_root" != "$attempt_root"/* ]] || die 'sealed closure and external attempt root must not overlap'
[[ "$diagnostic_root" != "$sealed_root" && "$diagnostic_root" != "$sealed_root"/* && "$sealed_root" != "$diagnostic_root"/* ]] || die 'sealed closure and external diagnostic root must not overlap'
[[ "$diagnostic_root" != "$attempt_root" && "$diagnostic_root" != "$attempt_root"/* && "$attempt_root" != "$diagnostic_root"/* ]] || die 'external diagnostic root and external attempt root must not overlap'
if [[ -n "$stage_cache_root" ]]; then
  [[ "$stage_cache_root" != "$sealed_root" && "$stage_cache_root" != "$sealed_root"/* && "$sealed_root" != "$stage_cache_root"/* ]] || die 'leased stage cache root and sealed closure must not overlap'
  [[ "$stage_cache_root" != "$attempt_root" && "$stage_cache_root" != "$attempt_root"/* && "$attempt_root" != "$stage_cache_root"/* ]] || die 'leased stage cache root and external attempt root must not overlap'
  [[ "$stage_cache_root" != "$diagnostic_root" && "$stage_cache_root" != "$diagnostic_root"/* && "$diagnostic_root" != "$stage_cache_root"/* ]] || die 'leased stage cache root and external diagnostic root must not overlap'
fi
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
  bwrap_args=(--unshare-net)
  bwrap_args+=(--ro-bind / / --ro-bind "$sealed_root" /opt --bind "$attempt_root" /var/tmp --bind "$diagnostic_root" /var/cache --proc /proc --dev /dev)
  inner_args=(--sealed-root /opt --attempt-root /var/tmp --diagnostic-root /var/cache)
  if [[ -n "$stage_cache_root" ]]; then
    bwrap_args+=(--bind "$stage_cache_root" /mnt)
    inner_args+=(--stage-cache-root /mnt)
  fi
  if [[ "$preflight_only" -eq 1 ]]; then
    inner_args+=(--preflight-only)
  fi
  exec /usr/bin/bwrap "${bwrap_args[@]}" --setenv DASOBJECTSTORE_F05_BWRAP_NETWORK_NAMESPACE 1 -- /usr/bin/bash /opt/source/packaging/debian/run-plugin-process-package-attempt.sh "${inner_args[@]}"
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

copy_stage_to_attempt() {
  local stage=$1
  copied_closure="$attempt_root/closure"
  cp -a --reflink=auto "$stage" "$copied_closure"
  [[ -d "$copied_closure/source" && ! -L "$copied_closure" ]] || die 'failed to make a physical copied closure'
  if find "$copied_closure" -type l -print -quit | grep -q .; then
    die 'copied closure must not contain symlinks'
  fi
  chmod -R u+w "$copied_closure"
}

copied_closure=''
if [[ -n "$stage_cache_root" ]]; then
  sealed_manifest_sha256=$(sha256_file "$sealed_root/f05-inputs.sha256")
  runner_sha256=$(sha256_file "$0")
  cache_config="$stage_cache_root/current/source/.cargo/f05-vendor-config.toml"
  copied_config_sha256=$(sed -E 's|^directory = ".*"$|directory = "/mnt/current/source/vendor"|' "$sealed_root/source/.cargo/f05-vendor-config.toml" | shasum -a 256 | awk '{print $1}')
  stage_cache_key=$(printf '%s\n%s\n%s\n' "$sealed_manifest_sha256" "$runner_sha256" "$copied_config_sha256" | shasum -a 256 | awk '{print $1}')
  cache_stage="$stage_cache_root/current"
  cache_receipt="$stage_cache_root/stage-reuse-receipt"
  expected_receipt=$(printf 'lease_key=%s\nsealed_manifest_sha256=%s\nrunner_sha256=%s\ncopied_config_sha256=%s\n' "$stage_cache_key" "$sealed_manifest_sha256" "$runner_sha256" "$copied_config_sha256")

  if [[ -e "$cache_receipt" || -e "$cache_stage" ]]; then
    [[ -f "$cache_receipt" && -d "$cache_stage" && ! -L "$cache_receipt" && ! -L "$cache_stage" ]] || die 'leased stage cache requires both a physical receipt and current stage'
    [[ ! -w "$stage_cache_root" ]] || die 'leased stage cache root must be immutable before reuse'
    [[ -z "$(find "$stage_cache_root" -perm /0222 -print -quit)" ]] || die 'leased stage cache must be immutable before reuse'
    [[ -z "$(find "$stage_cache_root" -type l -print -quit)" ]] || die 'leased stage cache must not contain symlinks'
    cmp -s <(printf '%s' "$expected_receipt") "$cache_receipt" || die 'leased stage cache receipt does not bind this manifest, runner, and copied config'
    [[ -f "$cache_config" && ! -L "$cache_config" ]] || die 'leased stage cache requires a physical copied vendor config'
    [[ "$(sha256_file "$cache_config")" = "$copied_config_sha256" ]] || die 'leased stage cache copied vendor config does not match its receipt'
    (cd "$cache_stage" && shasum -a 256 -c f05-inputs.sha256) >/dev/null 2>&1 || die 'leased stage cache has a missing or altered input'
    record_diagnostic "stage_cache_reuse=PASS key=$stage_cache_key"
    copy_stage_to_attempt "$cache_stage"
  else
    [[ -z "$(find "$stage_cache_root" -mindepth 1 -maxdepth 1 -print -quit)" ]] || die 'leased stage cache without a receipt must be empty'
    cp -a --reflink=auto "$sealed_root" "$cache_stage"
    [[ -d "$cache_stage/source" && ! -L "$cache_stage" ]] || die 'failed to make a physical immutable stage cache'
    [[ -z "$(find "$cache_stage" -type l -print -quit)" ]] || die 'leased stage cache must not contain symlinks'
    chmod -R u+w "$cache_stage"
    sed -E 's|^directory = ".*"$|directory = "/mnt/current/source/vendor"|' "$cache_config" > "$cache_config.next"
    mv "$cache_config.next" "$cache_config"
    write_batched_manifest "$cache_stage"
    printf '%s' "$expected_receipt" > "$cache_receipt"
    chmod -R a-w "$cache_stage"
    chmod a-w "$cache_receipt" "$stage_cache_root"
    [[ ! -w "$stage_cache_root" ]] || die 'leased stage cache root must become immutable'
    record_diagnostic "stage_cache_cold=PASS key=$stage_cache_key"
    copy_stage_to_attempt "$cache_stage"
  fi
else
  copied_closure="$attempt_root/closure"
  cp -a --reflink=auto "$sealed_root" "$copied_closure"
  [[ -d "$copied_closure/source" && ! -L "$copied_closure" ]] || die 'failed to make a physical copied closure'
  if find "$copied_closure" -type l -print -quit | grep -q .; then
    die 'copied closure must not contain symlinks'
  fi
  chmod -R u+w "$copied_closure"
fi

[[ -n "$copied_closure" ]] || die 'failed to select a copied closure'

copied_source="$copied_closure/source"
copied_manifest="$copied_source/Cargo.toml"
copied_lock="$copied_source/Cargo.lock"
copied_config="$copied_source/.cargo/f05-vendor-config.toml"
for copied_input in "$copied_manifest" "$copied_lock" "$copied_config"; do
  [[ -f "$copied_input" && ! -L "$copied_input" ]] || die "copied closure requires a physical non-symlink $(basename "$copied_input")"
done
# The cache config is deliberately bound to /mnt/current so the immutable
# cache has a stable receipt.  Derive a new config only in this writable
# attempt copy before the preparer sees it; never rewrite the cached bytes.
sed -E "s|^directory = \".*\"$|directory = \"$copied_source/vendor\"|" "$copied_config" > "$copied_config.next"
mv "$copied_config.next" "$copied_config"
write_batched_manifest "$copied_closure"

if [[ "$preflight_only" -eq 1 ]]; then
  # This is the identical copied-vendor binding required by
  # prepare-web-dist.sh before it can invoke Trunk.  Keep it on the
  # preflight side of every build, web-preparation, and package command.
  grep -Fx "directory = \"$copied_source/vendor\"" "$copied_config" >/dev/null || die 'preflight requires a vendor config bound to the copied staged source'
  (cd "$copied_closure" && shasum -a 256 -c f05-inputs.sha256) >/dev/null 2>&1 || die 'preflight copied closure has a missing or altered input'
  [[ -z "$(find "$stage_cache_root" -perm /0222 -print -quit)" ]] || die 'preflight leased stage cache must remain immutable'
  record_diagnostic "preflight_only=PASS copied_vendor=$copied_source/vendor"
  exit 0
fi

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
