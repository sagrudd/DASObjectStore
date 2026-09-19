#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "usage: $0 --sealed-root SEALED_CLOSURE --attempt-root EXTERNAL_EMPTY_DIRECTORY --diagnostic-root EXTERNAL_EMPTY_DIRECTORY [--stage-cache-root LEASED_STAGE_CACHE_DIRECTORY] [--preflight-only]" >&2
  echo "       $0 --sealed-root WRITABLE_CLOSURE_STAGE --stage-closure-config" >&2
  echo "       $0 --sealed-root WRITABLE_CLOSURE_STAGE --stage-closure-toolchain-inputs ABSOLUTE_IMMUTABLE_INPUT_ROOT" >&2
  echo "       $0 --sealed-root WRITABLE_CLOSURE_STAGE --stage-closure-git-inputs ABSOLUTE_IMMUTABLE_INPUT_ROOT" >&2
  echo "       $0 --sealed-root WRITABLE_CLOSURE_STAGE --stage-closure-provenance-inputs ABSOLUTE_IMMUTABLE_INPUT_ROOT" >&2
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
stage_closure_git_inputs=0
git_input_root=''
stage_closure_provenance_inputs=0
provenance_input_root=''
while [[ $# -gt 0 ]]; do
  case "$1" in
    --sealed-root) sealed_root=${2-}; shift 2 ;;
    --attempt-root) attempt_root=${2-}; shift 2 ;;
    --diagnostic-root) diagnostic_root=${2-}; shift 2 ;;
    --stage-cache-root) stage_cache_root=${2-}; shift 2 ;;
    --preflight-only) preflight_only=1; shift ;;
    --stage-closure-config) stage_closure_config=1; shift ;;
    --stage-closure-toolchain-inputs) stage_closure_toolchain_inputs=1; toolchain_input_root=${2-}; shift 2 ;;
    --stage-closure-git-inputs) stage_closure_git_inputs=1; git_input_root=${2-}; shift 2 ;;
    --stage-closure-provenance-inputs) stage_closure_provenance_inputs=1; provenance_input_root=${2-}; shift 2 ;;
    *) usage ;;
  esac
done
(( stage_closure_config + stage_closure_toolchain_inputs + stage_closure_git_inputs + stage_closure_provenance_inputs <= 1 )) || usage
if [[ "$stage_closure_config" -eq 1 ]]; then
  [[ -n "$sealed_root" && -z "$attempt_root" && -z "$diagnostic_root" && -z "$stage_cache_root" && "$preflight_only" -eq 0 ]] || usage
elif [[ "$stage_closure_toolchain_inputs" -eq 1 ]]; then
  [[ -n "$sealed_root" && -n "$toolchain_input_root" && -z "$attempt_root" && -z "$diagnostic_root" && -z "$stage_cache_root" && "$preflight_only" -eq 0 ]] || usage
elif [[ "$stage_closure_git_inputs" -eq 1 ]]; then
  [[ -n "$sealed_root" && -n "$git_input_root" && -z "$attempt_root" && -z "$diagnostic_root" && -z "$stage_cache_root" && "$preflight_only" -eq 0 ]] || usage
elif [[ "$stage_closure_provenance_inputs" -eq 1 ]]; then
  [[ -n "$sealed_root" && -n "$provenance_input_root" && -z "$attempt_root" && -z "$diagnostic_root" && -z "$stage_cache_root" && "$preflight_only" -eq 0 ]] || usage
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
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

sha256_stream() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum | awk '{print $1}'
  else
    shasum -a 256 | awk '{print $1}'
  fi
}

verify_sha256_manifest() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum -c "$1"
  else
    shasum -a 256 -c "$1"
  fi
}

sha256_tree() {
  local root=$1
  (
    cd "$root"
    find . -type f -print0 | LC_ALL=C sort -z | xargs -0 -r /usr/bin/bash -c '
      for input; do
        if command -v sha256sum >/dev/null 2>&1; then sha256sum "$input"; else shasum -a 256 "$input"; fi
      done
    ' bash | sha256_stream
  )
}

write_batched_manifest() {
  local root=$1 manifest="$1/f05-inputs.sha256" temporary="$1/f05-inputs.sha256.next"
  (
    cd "$root"
    find . -type f ! -name f05-inputs.sha256 ! -name f05-inputs.sha256.next -print0 | LC_ALL=C sort -z |
      xargs -0 -r -n 128 -P "$hash_jobs" /usr/bin/bash -c '
        for input; do
          if command -v sha256sum >/dev/null 2>&1; then digest=$(sha256sum "$input" | cut -d " " -f1); else digest=$(shasum -a 256 "$input" | cut -d " " -f1); fi
          printf "%s  %s\\n" "$digest" "${input#./}"
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
  (cd "$sealed_root" && verify_sha256_manifest f05-inputs.sha256) >/dev/null 2>&1 || die 'closure stage manifest does not bind generated inputs'
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

tool_inventory_root_value() {
  local file=$1 key=$2 value
  value=$(sed -n "s/^${key}=\(.*\)$/\1/p" "$file")
  [[ "$(printf '%s\n' "$value" | sed '/^$/d' | wc -l)" -eq 1 ]] || die "toolchain input inventory requires exactly one ${key}"
  printf '%s\n' "$value"
}

workspace_package_version() {
  local manifest=$1 value
  value=$(awk '
    $0 == "[workspace.package]" { active=1; next }
    /^\[/ { active=0 }
    active && /^version = "[0-9]+\.[0-9]+\.[0-9]+"$/ { print substr($0, 12, length($0) - 12) }
  ' "$manifest")
  [[ "$(printf '%s\n' "$value" | sed '/^$/d' | wc -l)" -eq 1 && "$value" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die 'toolchain input stage requires one strict candidate workspace version'
  printf '%s\n' "$value"
}

probe_declared_staged_tool() {
  local section=$1 tool=$2 expected_version expected_sha isolated_tool actual_output actual_status

  expected_version=$(tool_inventory_value "$input_inventory" "$section" version)
  expected_sha=$(tool_inventory_value "$input_inventory" "$section" sha256)
  [[ "$(sha256_file "$tool")" = "$expected_sha" ]] || die "toolchain input stage rejects a substituted $section input"
  [[ -x /usr/bin/bwrap ]] || die 'toolchain input stage requires Bubblewrap for isolated executable admission'
  isolated_tool="/opt/${tool#"$toolchain_input_root"/}"
  if actual_output=$(/usr/bin/bwrap --unshare-net --ro-bind / / --ro-bind "$toolchain_input_root" /opt --proc /proc --dev /dev -- "$isolated_tool" --version 2>&1); then
    actual_status=0
  else
    actual_status=$?
  fi
  printf 'toolchain_probe tool=%s path=%s expected_version=%q expected_sha256=%s actual_exit=%s actual_output=%q\n' \
    "$section" "$tool" "$expected_version" "$expected_sha" "$actual_status" "$actual_output"
  [[ "$actual_status" -eq 0 && "$actual_output" = "$expected_version" ]] || die "toolchain input stage rejects an unlaunchable or version-mismatched $section input"
}

produce_closure_toolchain_inputs() {
  local input_receipt input_manifest input_inventory admission_receipt candidate source_tree witness staged_runner
  local candidate_revision source_tree_revision witness_revision candidate_image candidate_image_sha candidate_version
  local input_revision input_image input_image_sha input_inventory_sha input_manifest_sha receipt temporary
  local inventory_target inventory_version inventory_binding_revision inventory_binding_version prior_inventory_sha
  local admitted_revision admitted_version admitted_inventory_sha admitted_image admitted_image_sha admitted_prior_inventory_sha
  local required inventory_sha

  input_receipt="$toolchain_input_root/tool-inputs.toml"
  input_manifest="$toolchain_input_root/tool-inputs.sha256"
  input_inventory="$toolchain_input_root/tool-input-inventory.txt"
  admission_receipt="$sealed_root/inputs/toolchain-input-admission-receipt.toml"
  candidate="$sealed_root/inputs/component-candidate-input.toml"
  source_tree="$sealed_root/inputs/source-tree"
  witness="$sealed_root/inputs/compiled-dependency-witness.json"
  staged_runner="$sealed_root/source/packaging/debian/run-plugin-process-package-attempt.sh"

  [[ "$toolchain_input_root" = /* && -d "$toolchain_input_root" && ! -L "$toolchain_input_root" ]] || die 'toolchain input stage requires an absolute, physical input root'
  [[ -z "$(find "$toolchain_input_root" -type l -print -quit)" ]] || die 'toolchain input stage rejects symlinked inputs'
  [[ -z "$(find "$toolchain_input_root" -perm /0222 -print -quit)" ]] || die 'toolchain input stage requires an immutable input root'
  for required in "$input_receipt" "$input_manifest" "$input_inventory" "$admission_receipt" "$candidate" "$source_tree" "$witness" "$staged_runner"; do
    [[ -f "$required" && ! -L "$required" ]] || die 'toolchain input stage requires physical receipt, manifest, and identity witnesses'
  done
  [[ "$(sha256_file "$staged_runner")" = "$(sha256_file "$0")" ]] || die 'toolchain input stage must execute the runner bytes that it binds'
  inventory_sha=$(sha256_file "$input_inventory")
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
  (cd "$toolchain_input_root" && verify_sha256_manifest tool-inputs.sha256) >/dev/null 2>&1 || die 'toolchain input stage rejects altered input bytes'
  [[ "$(sha256_file "$toolchain_input_root/toolchain/bin/cargo")" = "$(tool_inventory_value "$input_inventory" cargo sha256)" ]] || die 'toolchain input stage rejects a substituted cargo input'
  [[ "$(sha256_file "$toolchain_input_root/toolchain/bin/rustc")" = "$(tool_inventory_value "$input_inventory" rustc sha256)" ]] || die 'toolchain input stage rejects a substituted rustc input'
  [[ "$(sha256_file "$toolchain_input_root/toolchain/bin/trunk")" = "$(tool_inventory_value "$input_inventory" trunk sha256)" ]] || die 'toolchain input stage rejects a substituted trunk input'
  [[ "$(sha256_file "$toolchain_input_root/toolchain/trunk-tools/wasm-bindgen-0.2.128/wasm-bindgen")" = "$(tool_inventory_value "$input_inventory" wasm_bindgen sha256)" ]] || die 'toolchain input stage rejects a substituted wasm-bindgen input'
  [[ "$(sha256_file "$toolchain_input_root/toolchain/trunk-tools/wasm-opt-version_123/wasm-opt")" = "$(tool_inventory_value "$input_inventory" wasm_opt sha256)" ]] || die 'toolchain input stage rejects a substituted wasm-opt input'
  [[ "$(sha256_tree "$toolchain_input_root/toolchain/lib/rustlib/wasm32-unknown-unknown")" = "$(tool_inventory_value "$input_inventory" wasm_sysroot tree_sha256)" ]] || die 'toolchain input stage rejects a substituted wasm sysroot tree'
  [[ "$(sha256_tree "$toolchain_input_root/toolchain")" = "$(tool_inventory_value "$input_inventory" prior_staged_toolchain tree_sha256)" ]] || die 'toolchain input stage rejects a substituted toolchain tree'
  [[ "$(sha256_tree "$toolchain_input_root/network-denied-bin")" = "$(tool_inventory_value "$input_inventory" network_denial tree_sha256)" && "$(sha256_file "$toolchain_input_root/network-denied-bin/git")" = "$(tool_inventory_value "$input_inventory" network_denial sha256)" ]] || die 'toolchain input stage rejects a substituted network-denial input'
  [[ "$(sha256_tree "$toolchain_input_root/staging-home")" = "$(tool_inventory_value "$input_inventory" staging_home tree_sha256)" ]] || die 'toolchain input stage rejects a substituted staging-home tree'

  # Hashes prove byte identity; these isolated probes additionally prove the
  # exact staged executables can start on the intended Linux worker.  Do this
  # before copying any tool into a closure or allowing Cargo/package work.
  probe_declared_staged_tool cargo "$toolchain_input_root/toolchain/bin/cargo"
  probe_declared_staged_tool rustc "$toolchain_input_root/toolchain/bin/rustc"
  probe_declared_staged_tool trunk "$toolchain_input_root/toolchain/bin/trunk"
  probe_declared_staged_tool wasm_bindgen "$toolchain_input_root/toolchain/trunk-tools/wasm-bindgen-0.2.128/wasm-bindgen"
  probe_declared_staged_tool wasm_opt "$toolchain_input_root/toolchain/trunk-tools/wasm-opt-version_123/wasm-opt"

  candidate_revision=$(sed -n 's/^source_revision = "\([0-9a-f]\{40\}\)"$/\1/p' "$candidate")
  source_tree_revision=$(sed -n 's/^revision=\([0-9a-f]\{40\}\)$/\1/p' "$source_tree")
  witness_revision=$(sed -n 's/.*"source_revision": "\([0-9a-f]\{40\}\)".*/\1/p' "$witness")
  candidate_image=$(tool_input_toml_value "$candidate" toolchain_image)
  candidate_image_sha=$(tool_input_toml_value "$candidate" toolchain_image_sha256)
  candidate_version=$(workspace_package_version "$sealed_root/source/Cargo.toml")
  inventory_target=$(tool_inventory_root_value "$input_inventory" source_contract_target)
  inventory_version=$(tool_inventory_root_value "$input_inventory" source_contract_version)
  inventory_binding_revision=$(tool_inventory_value "$input_inventory" candidate_binding source_revision)
  inventory_binding_version=$(tool_inventory_value "$input_inventory" candidate_binding workspace_version)
  prior_inventory_sha=$(tool_inventory_value "$input_inventory" reusable_tool_provenance inventory_sha256)
  admitted_revision=$(tool_input_toml_value "$admission_receipt" source_revision)
  admitted_version=$(tool_input_toml_value "$admission_receipt" workspace_version)
  admitted_inventory_sha=$(tool_input_toml_value "$admission_receipt" inventory_sha256)
  admitted_image=$(tool_input_toml_value "$admission_receipt" toolchain_image)
  admitted_image_sha=$(tool_input_toml_value "$admission_receipt" toolchain_image_sha256)
  admitted_prior_inventory_sha=$(tool_input_toml_value "$admission_receipt" reusable_tool_provenance_inventory_sha256)
  input_revision=$(tool_input_toml_value "$input_receipt" source_revision)
  input_image=$(tool_input_toml_value "$input_receipt" toolchain_image)
  input_image_sha=$(tool_input_toml_value "$input_receipt" toolchain_image_sha256)
  input_inventory_sha=$(tool_input_toml_value "$input_receipt" inventory_sha256)
  [[ "$candidate_revision" =~ ^[0-9a-f]{40}$ && "$candidate_revision" = "$source_tree_revision" && "$candidate_revision" = "$witness_revision" && "$candidate_revision" = "$input_revision" ]] || die 'toolchain input stage requires matching candidate image and revision witnesses'
  [[ "$inventory_target" = "$candidate_revision" && "$inventory_binding_revision" = "$candidate_revision" && "$inventory_version" = "$candidate_version" && "$inventory_binding_version" = "$candidate_version" ]] || die 'toolchain input stage rejects inventory not bound to this candidate source and version'
  [[ "$admitted_revision" = "$candidate_revision" && "$admitted_version" = "$candidate_version" && "$admitted_inventory_sha" = "$inventory_sha" && "$admitted_image" = "$candidate_image" && "$admitted_image_sha" = "$candidate_image_sha" && "$admitted_prior_inventory_sha" = "$prior_inventory_sha" ]] || die 'toolchain input stage rejects an admission receipt not bound to this candidate and inventory'
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
  printf 'source_revision = "%s"\nworkspace_version = "%s"\ntoolchain_image = "%s"\ntoolchain_image_sha256 = "%s"\ninventory_sha256 = "%s"\ntool_input_manifest_sha256 = "%s"\nreviewed_inventory_document_sha256 = "%s"\nreusable_tool_provenance_inventory_sha256 = "%s"\n' \
    "$input_revision" "$candidate_version" "$input_image" "$input_image_sha" "$input_inventory_sha" "$input_manifest_sha" "$inventory_sha" "$prior_inventory_sha" > "$temporary"
  mv "$temporary" "$receipt"
  write_batched_manifest "$sealed_root"
  (cd "$sealed_root" && verify_sha256_manifest f05-inputs.sha256) >/dev/null 2>&1 || die 'toolchain input stage manifest does not bind copied inputs'
  printf 'closure_stage_toolchain_inputs=PASS inventory_sha256=%s tool_input_manifest_sha256=%s\n' "$input_inventory_sha" "$input_manifest_sha"
}

produce_closure_git_inputs() {
  local manifest inventory admission cargo_home receipt source lock input_manifest_sha inventory_sha admitted_inventory_sha checkout db name url revision checkout_rel db_rel
  manifest="$git_input_root/git-inputs.sha256"
  inventory="$git_input_root/git-inputs.toml"
  admission="$sealed_root/inputs/git-input-admission-receipt.toml"
  cargo_home="$git_input_root/cargo-home"
  source="$sealed_root/source"
  lock="$source/Cargo.lock"
  [[ -z "$(find "$git_input_root" -type l -print -quit)" && -z "$(find "$git_input_root" -perm /0222 -print -quit)" ]] || die 'git input stage requires immutable physical non-symlink inputs'
  [[ -f "$manifest" && -f "$inventory" && -f "$admission" && ! -L "$admission" && -d "$cargo_home" && -f "$cargo_home/config.toml" ]] || die 'git input stage requires an offline Cargo cache, reviewed inventory, admission receipt, and manifest'
  (cd "$git_input_root" && verify_sha256_manifest git-inputs.sha256) >/dev/null 2>&1 || die 'git input stage rejects altered input bytes'
  inventory_sha=$(sha256_file "$inventory")
  admitted_inventory_sha=$(tool_input_toml_value "$admission" git_input_inventory_sha256)
  [[ "$admitted_inventory_sha" = "$inventory_sha" && "$inventory_sha" =~ ^[0-9a-f]{64}$ ]] || die 'git input stage rejects an inventory not bound by the admission receipt'
  grep -Fx '[net]' "$cargo_home/config.toml" >/dev/null && grep -Fx 'offline = true' "$cargo_home/config.toml" >/dev/null || die 'git input stage requires an offline Cargo cache config'
  [[ ! -e "$sealed_root/cargo-home" && ! -e "$sealed_root/inputs/git-input-receipt.toml" ]] || die 'git input stage refuses to overwrite closure Git inputs'
  while IFS='|' read -r name url revision checkout_rel db_rel; do
    [[ -n "$name" ]] || continue
    grep -Fx "${name}_url = \"$url\"" "$inventory" >/dev/null && grep -Fx "${name}_revision = \"$revision\"" "$inventory" >/dev/null || die "git input stage rejects a mismatched $name inventory binding"
    grep -Fq "git+$url?rev=$revision#$revision" "$lock" || die "git input stage requires the locked $name source"
    checkout="$cargo_home/$checkout_rel"; db="$cargo_home/$db_rel"
    [[ -d "$checkout" && -d "$db" && ! -L "$checkout" && ! -L "$db" ]] || die "git input stage requires physical $name checkout and cache"
    [[ "$(git -c safe.directory="$checkout" -C "$checkout" rev-parse HEAD)" = "$revision" ]] || die "git input stage rejects a mismatched $name revision"
    grep -Fx "${name}_checkout_tree_sha256 = \"$(sha256_tree "$checkout")\"" "$inventory" >/dev/null || die "git input stage rejects a substituted $name checkout"
    grep -Fx "${name}_db_tree_sha256 = \"$(sha256_tree "$db")\"" "$inventory" >/dev/null || die "git input stage rejects a substituted $name cache"
  done <<'SOURCES'
pistis|https://github.com/sagrudd/pistis.git|14e481497d3838d3310df3b0a21232f5d01d6f9f|git/checkouts/pistis-13d5c72a63ff6278/14e4814|git/db/pistis-13d5c72a63ff6278
prosopikon|https://github.com/sagrudd/prosopikon.git|f09749273ef382c1b42bf04a77d96189dd7361b3|git/checkouts/prosopikon-739f7520363f0e4d/f097492|git/db/prosopikon-739f7520363f0e4d
proxenos|https://github.com/sagrudd/proxenos.git|d4c3054fb7d88c9f718d2987ec19bf7bc444d391|git/checkouts/proxenos-10a0a1d74c5551fd/d4c3054|git/db/proxenos-10a0a1d74c5551fd
thesaurophylax|https://github.com/sagrudd/thesaurophylax.git|0bfb16857d135d2830de2cf53d245b68ed2d051f|git/checkouts/thesaurophylax-08d7bd2129966817/0bfb168|git/db/thesaurophylax-08d7bd2129966817
SOURCES
  cp -a "$cargo_home" "$sealed_root/cargo-home"
  [[ -z "$(find "$sealed_root/cargo-home" -type l -print -quit)" ]] || die 'git input stage copied a symlinked Cargo cache'
  input_manifest_sha=$(sha256_file "$manifest")
  receipt="$sealed_root/inputs/git-input-receipt.toml"
  printf 'git_input_manifest_sha256 = "%s"\ngit_input_inventory_sha256 = "%s"\n' "$input_manifest_sha" "$inventory_sha" > "$receipt"
  write_batched_manifest "$sealed_root"
  (cd "$sealed_root" && verify_sha256_manifest f05-inputs.sha256) >/dev/null 2>&1 || die 'git input stage manifest does not bind copied Cargo cache'
  printf 'closure_stage_git_inputs=PASS git_input_manifest_sha256=%s\n' "$input_manifest_sha"
}

produce_closure_provenance_inputs() {
  local input registry identity archive recipe validator validator_receipt source revision expected_revision version expected_version tree archive_sha source_content_sha lock_sha witness candidate tool_receipt image image_sha report
  input=$provenance_input_root
  registry="$input/registry.toml"
  identity="$input/source-identity.toml"
  archive="$input/source-archive.tar"
  recipe="$input/package-recipe.json"
  validator="$input/kanon-component-candidate-input"
  validator_receipt="$input/kanon-component-candidate-input.receipt"
  source="$sealed_root/source"
  witness="$sealed_root/inputs/compiled-dependency-witness.json"
  candidate="$sealed_root/inputs/component-candidate-input.toml"
  tool_receipt="$sealed_root/inputs/toolchain-input-receipt.toml"

  [[ -f "$sealed_root/f05-inputs.sha256" && ! -L "$sealed_root/f05-inputs.sha256" ]] || die 'provenance input stage requires a pre-producer sealed manifest'
  (cd "$sealed_root" && verify_sha256_manifest f05-inputs.sha256) >/dev/null 2>&1 || die 'provenance input stage rejects an altered pre-producer sealed manifest'
  for bound in \
    source/Cargo.toml \
    source/Cargo.lock \
    inputs/toolchain-input-receipt.toml; do
    grep -F "  $bound" "$sealed_root/f05-inputs.sha256" >/dev/null || die "provenance input stage requires pre-producer manifest binding for $bound"
  done
  for generated in "$witness" "$candidate" "$sealed_root/inputs/component-candidate-input.validation.json"; do
    [[ ! -e "$generated" ]] || die 'provenance input stage refuses pre-existing generated provenance outputs'
  done
  [[ "$input" = /* && -d "$input" && ! -L "$input" ]] || die 'provenance input stage requires an absolute physical input root'
  [[ -z "$(find "$input" -type l -print -quit)" && -z "$(find "$input" -perm /0222 -print -quit)" ]] || die 'provenance input stage requires immutable non-symlink inputs'
  for required in "$registry" "$identity" "$archive" "$recipe" "$validator" "$validator_receipt"; do
    [[ -f "$required" && ! -L "$required" ]] || die 'provenance input stage requires physical registry, identity, recipe, validator, and tool receipt inputs'
  done
  [[ -f "$tool_receipt" && ! -L "$tool_receipt" ]] || die 'provenance input stage requires a sealed, externally admitted tool receipt'
  [[ -x "$validator" && "$(sha256_file "$validator")" = "$(tool_input_toml_value "$validator_receipt" sha256)" && "$(tool_input_toml_value "$validator_receipt" revision)" = '4a7b1a16c9864c3eb0b66b60b4bffbe752052cc7' ]] || die 'provenance input stage rejects an unpinned Kanon validator'
  revision=$(tool_input_toml_value "$identity" source_revision)
  # The expected tuple is already bound by the independently admitted tool
  # receipt.  The provenance input may describe an archive, but cannot select
  # the candidate it is allowed to emit.
  expected_revision=$(tool_input_toml_value "$tool_receipt" source_revision)
  expected_version=$(tool_input_toml_value "$tool_receipt" workspace_version)
  version=$(tool_input_toml_value "$identity" workspace_version)
  tree=$(tool_input_toml_value "$identity" git_tree)
  archive_sha=$(tool_input_toml_value "$identity" source_archive_sha256)
  source_content_sha=$(tool_input_toml_value "$identity" source_content_sha256)
  [[ "$revision" =~ ^[0-9a-f]{40}$ && "$revision" = "$expected_revision" && "$version" = "$expected_version" && "$version" = "$(workspace_package_version "$source/Cargo.toml")" && "$tree" =~ ^[0-9a-f]{40}$ && "$archive_sha" = "sha256:$(sha256_file "$archive")" && "$source_content_sha" = "sha256:$(sha256_tree "$source")" ]] || die 'provenance input stage rejects a dirty, wrong, or expected-candidate-mismatched source archive'
  lock_sha="sha256:$(sha256_file "$source/Cargo.lock")"
  image=$(tool_input_toml_value "$tool_receipt" toolchain_image)
  image_sha=$(tool_input_toml_value "$tool_receipt" toolchain_image_sha256)
  [[ "$image" =~ @sha256:[0-9a-f]{64}$ && "$image_sha" =~ ^sha256:[0-9a-f]{64}$ ]] || die 'provenance input stage requires an immutable toolchain image receipt'
  mkdir -p "$sealed_root/inputs"
  printf 'repository=sagrudd/DASObjectStore\nrevision=%s\ngit_tree=%s\nsource_archive_sha256=%s\n' "$revision" "$tree" "${archive_sha#sha256:}" > "$sealed_root/inputs/source-tree"
  printf '{\n  "schema_version": "mnemosyne.f05.compiled-dependency-witness.v1",\n  "source_revision": "%s",\n  "cargo_lock_sha256": "%s",\n  "registry_lock_closure_sha256": "sha256:%s"\n}\n' "$revision" "$lock_sha" "$(sha256_tree "$sealed_root/cargo-home")" > "$witness"
  printf 'schema_version = "mnemosyne.kanon.component-candidate-input.v1"\ncomponent_binary = "dasobjectstore"\nsource_tree_sha256 = "sha256:%s"\n[candidate_build]\nschema_version = "mnemosyne.kanon.candidate-build-admission.v1"\nadmission_id = "das-component-package-f05-%s"\nexecution = "disposable_ci_bootstrap"\nproduct_id = "dasobjectstore"\nrepository = "sagrudd/DASObjectStore"\nsource_revision = "%s"\nregistry_snapshot_sha256 = "sha256:%s"\ncargo_lock_sha256 = "%s"\ncompiled_dependency_witness_sha256 = "sha256:%s"\ntoolchain_image = "%s"\ntoolchain_image_sha256 = "%s"\ntarget_os = "linux"\ntarget_architecture = "amd64"\nfeatures = []\nrecipe_sha256 = "sha256:%s"\njenkins_task_id = "candidate-build-admission"\n' \
    "$(sha256_file "$sealed_root/inputs/source-tree")" "$revision" "$revision" "$(sha256_file "$registry")" "$lock_sha" "$(sha256_file "$witness")" "$image" "$image_sha" "$(sha256_file "$recipe")" > "$candidate"
  report=$("$validator" component-candidate-input validate --input "$candidate" --registry "$registry" --source-tree "$sealed_root/inputs/source-tree" --cargo-lock "$source/Cargo.lock" --compiled-dependency-witness "$witness" --recipe "$recipe") || die 'provenance input stage Kanon validator execution failed'
  grep -F '"valid":true' <<<"$report" >/dev/null || die 'provenance input stage Kanon validator rejected emitted inputs'
  printf '%s\n' "$report" > "$sealed_root/inputs/component-candidate-input.validation.json"
  write_batched_manifest "$sealed_root"
  (cd "$sealed_root" && verify_sha256_manifest f05-inputs.sha256) >/dev/null 2>&1 || die 'provenance input stage manifest does not bind emitted inputs'
  printf 'closure_stage_provenance_inputs=PASS source_revision=%s validator_sha256=%s\n' "$revision" "$(sha256_file "$validator")"
}

reject_symlink_ancestry "$sealed_root" 'sealed closure root'
if [[ "$stage_closure_config" -eq 0 && "$stage_closure_toolchain_inputs" -eq 0 && "$stage_closure_git_inputs" -eq 0 && "$stage_closure_provenance_inputs" -eq 0 ]]; then
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
if [[ "$stage_closure_git_inputs" -eq 1 ]]; then
  reject_symlink_ancestry "$git_input_root" 'git input root'
  git_input_root=$(canonical_directory "$git_input_root" 'git input root')
  produce_closure_git_inputs
  exit 0
fi
if [[ "$stage_closure_provenance_inputs" -eq 1 ]]; then
  reject_symlink_ancestry "$provenance_input_root" 'provenance input root'
  provenance_input_root=$(canonical_directory "$provenance_input_root" 'provenance input root')
  produce_closure_provenance_inputs
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
  (cd "$sealed_root" && verify_sha256_manifest f05-inputs.sha256) >/dev/null 2>&1 || sealed_status=1
  if [[ "$sealed_status" -ne 0 && "$status" -eq 0 ]]; then
    status=1
  fi
  printf 'exit_code=%s\n' "$status" > "$status_file"
  record_diagnostic "terminal_exit_code=$status"
  exit "$status"
}
trap finish EXIT HUP INT TERM

(cd "$sealed_root" && verify_sha256_manifest f05-inputs.sha256) >/dev/null 2>&1 || die 'sealed closure has a missing or altered input'

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
  copied_config_sha256=$(sed -E 's|^directory = ".*"$|directory = "/mnt/current/source/vendor"|' "$sealed_root/source/.cargo/f05-vendor-config.toml" | sha256_stream)
  stage_cache_key=$(printf '%s\n%s\n%s\n' "$sealed_manifest_sha256" "$runner_sha256" "$copied_config_sha256" | sha256_stream)
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
    (cd "$cache_stage" && verify_sha256_manifest f05-inputs.sha256) >/dev/null 2>&1 || die 'leased stage cache has a missing or altered input'
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
copied_preflight_config="$copied_closure/preflight-cargo-config.toml"
copied_cargo_home="$copied_closure/cargo-home"
for copied_input in "$copied_manifest" "$copied_lock" "$copied_config"; do
  [[ -f "$copied_input" && ! -L "$copied_input" ]] || die "copied closure requires a physical non-symlink $(basename "$copied_input")"
done
[[ -d "$copied_cargo_home" && ! -L "$copied_cargo_home" && -f "$copied_cargo_home/config.toml" && ! -L "$copied_cargo_home/config.toml" ]] || die 'copied closure requires a physical staged offline Cargo cache'
grep -Fx '[net]' "$copied_cargo_home/config.toml" >/dev/null && grep -Fx 'offline = true' "$copied_cargo_home/config.toml" >/dev/null || die 'copied closure requires an offline staged Cargo cache config'
# The cache config is deliberately bound to /mnt/current so the immutable
# cache has a stable receipt.  Derive a new config only in this writable
# attempt copy before the preparer sees it; never rewrite the cached bytes.
sed -E "s|^directory = \".*\"$|directory = \"$copied_source/vendor\"|" "$copied_config" > "$copied_config.next"
mv "$copied_config.next" "$copied_config"
cat > "$copied_preflight_config" <<EOF
[net]
offline = true

[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "$copied_source/vendor"
EOF
[[ -f "$copied_preflight_config" && ! -L "$copied_preflight_config" ]] || die 'preflight requires a physical copied Cargo source configuration'
grep -Fx "directory = \"$copied_source/vendor\"" "$copied_preflight_config" >/dev/null || die 'preflight requires a copied vendor configuration'
if [[ ! -x "$copied_closure/network-denied-bin/shasum" ]]; then
  cat > "$copied_closure/network-denied-bin/shasum" <<'SHASUM'
#!/bin/sh
set -eu
if [ "${1-}" = -a ] && [ "${2-}" = 256 ]; then shift 2; fi
exec /usr/bin/sha256sum "$@"
SHASUM
  chmod 0755 "$copied_closure/network-denied-bin/shasum"
fi
[[ -f "$copied_closure/network-denied-bin/shasum" && ! -L "$copied_closure/network-denied-bin/shasum" ]] || die 'copied closure requires a physical SHA-256 compatibility command'
write_batched_manifest "$copied_closure"

server="$attempt_root/target/release/dasobjectstore-server"
web_dist="$copied_source/crates/dasobjectstore-gui-web/dist"
output_dir="$attempt_root/output"
install -d -m 0755 "$attempt_root/home" "$attempt_root/tmp"
export DASOBJECTSTORE_F05_STAGED_CLOSURE_ROOT="$copied_closure"
export DASOBJECTSTORE_F05_ATTEMPT_ROOT="$attempt_root"
export HOME="$attempt_root/home"
export CARGO_HOME="$copied_cargo_home"
export CARGO_NET_OFFLINE=true
export CARGO_TARGET_DIR="$attempt_root/target"
export TMPDIR="$attempt_root/tmp"
export RUSTC="$copied_closure/toolchain/bin/rustc"
export PATH="$copied_closure/network-denied-bin:$copied_closure/toolchain/bin:/usr/bin:/bin"

if [[ "$preflight_only" -eq 1 ]]; then
  # This is the identical copied-vendor binding required by
  # prepare-web-dist.sh before it can invoke Trunk.  Keep it on the
  # preflight side of every build, web-preparation, and package command.
  grep -Fx "directory = \"$copied_source/vendor\"" "$copied_config" >/dev/null || die 'preflight requires a vendor config bound to the copied staged source'
  (cd "$copied_closure" && verify_sha256_manifest f05-inputs.sha256) >/dev/null 2>&1 || die 'preflight copied closure has a missing or altered input'
  [[ -z "$(find "$stage_cache_root" -perm /0222 -print -quit)" ]] || die 'preflight leased stage cache must remain immutable'
  (cd "$copied_source" && "$copied_closure/toolchain/bin/cargo" tree --manifest-path "$copied_manifest" --offline --config "$copied_preflight_config" --locked --target x86_64-unknown-linux-gnu -p dasobjectstore-cli --edges normal,build)
  record_diagnostic "preflight_only=PASS offline_locked_resolution=PASS copied_vendor=$copied_source/vendor copied_preflight_config=$copied_preflight_config"
  exit 0
fi
install -d -m 0755 "$attempt_root/target" "$output_dir"
(
  cd "$copied_source"
  "$copied_closure/toolchain/bin/cargo" build --manifest-path "$copied_manifest" --offline --config "$copied_config" --locked --release -p dasobjectstore-cli --bin dasobjectstore-server
)
"$copied_source/packaging/web/prepare-web-dist.sh"
"$copied_source/packaging/debian/build-plugin-process-deb.sh" --server "$server" --web-dist "$web_dist" --output-dir "$output_dir"
