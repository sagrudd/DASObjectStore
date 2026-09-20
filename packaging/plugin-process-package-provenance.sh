#!/usr/bin/env bash
# Immutable evidence for the source-only plugin-process DEB fixture.
set -euo pipefail

das_plugin_process_sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    das_plugin_process_f05_staged_closure_error 'requires a SHA-256 command (sha256sum or shasum)'
  fi
}

das_plugin_process_sha256_stream() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 | awk '{print $1}'
  else
    das_plugin_process_f05_staged_closure_error 'requires a SHA-256 command (sha256sum or shasum)'
  fi
}

das_plugin_process_verify_sha256_manifest() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum -c "$1"
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 -c "$1"
  else
    das_plugin_process_f05_staged_closure_error 'requires a SHA-256 command (sha256sum or shasum)'
  fi
}

das_plugin_process_tree_sha256() {
  local tree=$1
  (cd "$tree" && find . -type f -print0 | LC_ALL=C sort -z | while IFS= read -r -d '' input; do das_plugin_process_sha256 "$input"; done | das_plugin_process_sha256_stream)
}

das_plugin_process_f05_staged_closure_enabled() {
  [[ -n "${DASOBJECTSTORE_F05_STAGED_CLOSURE_ROOT:-}" ]]
}

das_plugin_process_f05_staged_closure_error() {
  echo "plugin package F05 staged closure $*" >&2
  exit 1
}

das_plugin_process_f05_manifest_requires() {
  local staged_root=$1 required=$2
  grep -Fq "  $required" "$staged_root/f05-inputs.sha256" || das_plugin_process_f05_staged_closure_error "requires f05-inputs.sha256 to bind $required"
}

das_plugin_process_strict_semver() {
  [[ "$1" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]
}

das_plugin_process_f05_require_staged_closure() {
  local repo_root=$1 staged_root tool candidate_revision source_tree_revision
  das_plugin_process_f05_staged_closure_enabled || return 0
  staged_root=$DASOBJECTSTORE_F05_STAGED_CLOSURE_ROOT
  [[ "$staged_root" = /* && -d "$staged_root" && ! -L "$staged_root" ]] || das_plugin_process_f05_staged_closure_error 'requires an absolute closure root'
  staged_root="$(cd "$staged_root" && pwd -P)"
  repo_root="$(cd "$repo_root" && pwd -P)"
  [[ "$repo_root" = "$staged_root/source" ]] || das_plugin_process_f05_staged_closure_error 'requires stage/source as the package repository root'
  [[ -f "$staged_root/f05-inputs.sha256" && -f "$staged_root/inputs/component-candidate-input.toml" && -f "$staged_root/inputs/source-tree" && -f "$staged_root/inputs/compiled-dependency-witness.json" && -f "$staged_root/inputs/package-recipe.json" ]] || das_plugin_process_f05_staged_closure_error 'requires admitted candidate and package inputs'
  [[ -d "$repo_root/vendor" && -f "$repo_root/.cargo/f05-vendor-config.toml" ]] || das_plugin_process_f05_staged_closure_error 'requires a staged vendor tree and vendor config'
  [[ -d "$staged_root/staging-home" && -d "$staged_root/network-denied-bin" ]] || das_plugin_process_f05_staged_closure_error 'requires isolated staged home and network denial inputs'
  for tool in "$staged_root/toolchain/bin/cargo" "$staged_root/toolchain/bin/rustc" "$staged_root/toolchain/bin/trunk"; do
    [[ "$tool" = "$staged_root"/* && -x "$tool" && ! -L "$tool" ]] || das_plugin_process_f05_staged_closure_error 'requires absolute, non-symlink staged cargo, rustc, and trunk tools'
  done
  [[ -d "$staged_root/toolchain/lib/rustlib/wasm32-unknown-unknown" && ! -L "$staged_root/toolchain/lib/rustlib/wasm32-unknown-unknown" ]] || das_plugin_process_f05_staged_closure_error 'requires a staged wasm32-unknown-unknown target'
  [[ ! -e "$repo_root/../prosopikon" ]] || das_plugin_process_f05_staged_closure_error 'rejects an ambient Prosopikon sibling'
  (cd "$staged_root" && das_plugin_process_verify_sha256_manifest f05-inputs.sha256) >/dev/null 2>&1 || das_plugin_process_f05_staged_closure_error 'has a missing or altered staged input'
  for required in source/.cargo/f05-vendor-config.toml inputs/component-candidate-input.toml inputs/source-tree inputs/compiled-dependency-witness.json inputs/package-recipe.json toolchain/bin/cargo toolchain/bin/rustc toolchain/bin/trunk; do
    das_plugin_process_f05_manifest_requires "$staged_root" "$required"
  done
  for required in toolchain/trunk-tools/wasm-bindgen-0.2.128/wasm-bindgen toolchain/trunk-tools/wasm-opt-version_123/wasm-opt; do
    das_plugin_process_f05_manifest_requires "$staged_root" "$required"
  done
  for tool in "$staged_root/toolchain/trunk-tools/wasm-bindgen-0.2.128/wasm-bindgen" "$staged_root/toolchain/trunk-tools/wasm-opt-version_123/wasm-opt"; do
    [[ -x "$tool" && ! -L "$tool" ]] || das_plugin_process_f05_staged_closure_error 'requires hash-bound staged wasm-bindgen and wasm-opt tools'
  done
  find "$staged_root/toolchain/lib/rustlib/wasm32-unknown-unknown" -type f -print -quit | grep -q . || das_plugin_process_f05_staged_closure_error 'requires a non-empty staged wasm32-unknown-unknown target'
  find "$staged_root/toolchain/lib/rustlib/wasm32-unknown-unknown" -type f | while IFS= read -r wasm_path; do
    wasm_input=${wasm_path#"$staged_root/toolchain/lib/rustlib/wasm32-unknown-unknown/"}
    das_plugin_process_f05_manifest_requires "$staged_root" "toolchain/lib/rustlib/wasm32-unknown-unknown/$wasm_input"
  done
  find "$repo_root/vendor" -type f -print -quit | grep -q . || das_plugin_process_f05_staged_closure_error 'requires a non-empty staged vendor tree'
  find "$repo_root/vendor" -type f | while IFS= read -r vendor_path; do
    vendor_input=${vendor_path#"$repo_root/vendor/"}
    das_plugin_process_f05_manifest_requires "$staged_root" "source/vendor/$vendor_input"
  done
  grep -Eq '^toolchain_image = ".+@sha256:[0-9a-f]{64}"$' "$staged_root/inputs/component-candidate-input.toml" || das_plugin_process_f05_staged_closure_error 'requires an immutable staged toolchain image'
  grep -Eq '^toolchain_image_sha256 = "sha256:[0-9a-f]{64}"$' "$staged_root/inputs/component-candidate-input.toml" || das_plugin_process_f05_staged_closure_error 'requires an immutable staged toolchain image digest'
  candidate_revision="$(sed -n 's/^source_revision = "\([0-9a-f]*\)"$/\1/p' "$staged_root/inputs/component-candidate-input.toml")"
  source_tree_revision="$(sed -n 's/^revision=\([0-9a-f]*\)$/\1/p' "$staged_root/inputs/source-tree")"
  [[ "$candidate_revision" =~ ^[0-9a-f]{40}$ && "$candidate_revision" == "$source_tree_revision" ]] || das_plugin_process_f05_staged_closure_error 'requires candidate source revision to match the source-tree witness'
  DASOBJECTSTORE_F05_STAGED_CLOSURE_ROOT=$staged_root
  DASOBJECTSTORE_F05_STAGED_CARGO="$staged_root/toolchain/bin/cargo"
  DASOBJECTSTORE_F05_STAGED_RUSTC="$staged_root/toolchain/bin/rustc"
  DASOBJECTSTORE_F05_STAGED_TRUNK="$staged_root/toolchain/bin/trunk"
  DASOBJECTSTORE_F05_STAGED_TRUNK_TOOLS="$staged_root/toolchain/trunk-tools"
  DASOBJECTSTORE_F05_STAGED_WASM_TARGET="$staged_root/toolchain/lib/rustlib/wasm32-unknown-unknown"
  export DASOBJECTSTORE_F05_STAGED_CLOSURE_ROOT DASOBJECTSTORE_F05_STAGED_CARGO DASOBJECTSTORE_F05_STAGED_RUSTC DASOBJECTSTORE_F05_STAGED_TRUNK DASOBJECTSTORE_F05_STAGED_TRUNK_TOOLS DASOBJECTSTORE_F05_STAGED_WASM_TARGET
}

das_plugin_process_f05_staged_provenance() {
  local staged_root=$DASOBJECTSTORE_F05_STAGED_CLOSURE_ROOT
  printf '"f05_staged_inputs_manifest_sha256":"%s","f05_component_candidate_input_sha256":"%s","f05_source_tree_sha256":"%s","f05_dependency_witness_sha256":"%s","f05_package_recipe_sha256":"%s","f05_vendor_tree_sha256":"%s","f05_vendor_config_sha256":"%s","f05_cargo_sha256":"%s","f05_rustc_sha256":"%s","f05_trunk_sha256":"%s","f05_wasm_bindgen_0_2_128_sha256":"%s","f05_wasm_opt_version_123_sha256":"%s","f05_wasm_target_sha256":"%s","f05_toolchain_image":"%s","f05_toolchain_image_sha256":"%s","f05_cargo_version":"%s","f05_rustc_version":"%s","f05_trunk_version":"%s"' \
    "$(das_plugin_process_sha256 "$staged_root/f05-inputs.sha256")" \
    "$(das_plugin_process_sha256 "$staged_root/inputs/component-candidate-input.toml")" \
    "$(das_plugin_process_sha256 "$staged_root/inputs/source-tree")" \
    "$(das_plugin_process_sha256 "$staged_root/inputs/compiled-dependency-witness.json")" \
    "$(das_plugin_process_sha256 "$staged_root/inputs/package-recipe.json")" \
    "$(das_plugin_process_tree_sha256 "$staged_root/source/vendor")" \
    "$(das_plugin_process_sha256 "$staged_root/source/.cargo/f05-vendor-config.toml")" \
    "$(das_plugin_process_sha256 "$DASOBJECTSTORE_F05_STAGED_CARGO")" \
    "$(das_plugin_process_sha256 "$DASOBJECTSTORE_F05_STAGED_RUSTC")" \
    "$(das_plugin_process_sha256 "$DASOBJECTSTORE_F05_STAGED_TRUNK")" \
    "$(das_plugin_process_sha256 "$DASOBJECTSTORE_F05_STAGED_TRUNK_TOOLS/wasm-bindgen-0.2.128/wasm-bindgen")" \
    "$(das_plugin_process_sha256 "$DASOBJECTSTORE_F05_STAGED_TRUNK_TOOLS/wasm-opt-version_123/wasm-opt")" \
    "$(das_plugin_process_tree_sha256 "$DASOBJECTSTORE_F05_STAGED_WASM_TARGET")" \
    "$(sed -n 's/^toolchain_image = "\(.*\)"$/\1/p' "$staged_root/inputs/component-candidate-input.toml")" \
    "$(sed -n 's/^toolchain_image_sha256 = "\(.*\)"$/\1/p' "$staged_root/inputs/component-candidate-input.toml")" \
    "$("$DASOBJECTSTORE_F05_STAGED_CARGO" --version)" \
    "$("$DASOBJECTSTORE_F05_STAGED_RUSTC" --version)" \
    "$("$DASOBJECTSTORE_F05_STAGED_TRUNK" --version)"
}

das_plugin_process_write_provenance() {
  local package_path=$1 repo_root=$2 version=$3 architecture=$4 web_dist=$5 server=$6
  local source_revision source_epoch descriptor f05_fields=''
  source_revision="${DASOBJECTSTORE_SOURCE_REVISION:-$(git -C "$repo_root" rev-parse HEAD)}"
  source_epoch="${SOURCE_DATE_EPOCH:-$(git -C "$repo_root" log -1 --format=%ct)}"
  descriptor="$repo_root/packaging/linux/opt/dasobjectstore/plugin-process-descriptor.json"
  [[ "$source_revision" =~ ^[0-9a-f]{40}$ ]] || { echo "plugin package requires an exact source revision" >&2; return 1; }
  [[ "$source_epoch" =~ ^[0-9]+$ ]] || { echo "plugin package requires SOURCE_DATE_EPOCH" >&2; return 1; }
  if das_plugin_process_f05_staged_closure_enabled; then
    das_plugin_process_f05_require_staged_closure "$repo_root" || return 1
    f05_fields=",$(das_plugin_process_f05_staged_provenance)"
  fi
  printf '{"schema":"mnemosyne.dasobjectstore.plugin-process-package-provenance.v1","package_name":"dasobjectstore-plugin-process","package_version":"%s","architecture":"%s","source_revision":"%s","source_date_epoch":%s,"cargo_toml_sha256":"%s","cargo_lock_sha256":"%s","descriptor_source_sha256":"%s","web_assets_sha256":"%s","server_sha256":"%s","package_sha256":"%s"%s,"command":"packaging/debian/build-plugin-process-deb.sh"}\n' \
    "$version" "$architecture" "$source_revision" "$source_epoch" \
    "$(das_plugin_process_sha256 "$repo_root/Cargo.toml")" \
    "$(das_plugin_process_sha256 "$repo_root/Cargo.lock")" \
    "$(das_plugin_process_sha256 "$descriptor")" \
    "$(das_plugin_process_tree_sha256 "$web_dist")" \
    "$(das_plugin_process_sha256 "$server")" \
    "$(das_plugin_process_sha256 "$package_path")" "$f05_fields" > "$package_path.provenance.json"
}
