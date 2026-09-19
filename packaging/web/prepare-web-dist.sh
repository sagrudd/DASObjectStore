#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
web_root="$repo_root/crates/dasobjectstore-gui-web"
prosopikon_core_root="$repo_root/../prosopikon/crates/prosopikon-core"
prosopikon_yew_root="$repo_root/../prosopikon/crates/prosopikon-yew"
readonly F05_PROSOPIKON_REVISION=f09749273ef382c1b42bf04a77d96189dd7361b3
staged_closure_root=${DASOBJECTSTORE_F05_STAGED_CLOSURE_ROOT:-}
staged_cargo=''
staged_rustc=''
staged_trunk=''
staged_trunk_tools=''
staged_wasm_target=''
dist="${DASOBJECTSTORE_PREBUILT_WEB_DIST:-$web_root/dist}"
attempt_root=${DASOBJECTSTORE_F05_ATTEMPT_ROOT:-}
allow_fallback=0

if [[ "${1:-}" == "--allow-fallback" ]]; then
  allow_fallback=1
fi

staged_closure_enabled() {
  [[ -n "$staged_closure_root" ]]
}

staged_closure_error() {
  printf 'F05 staged web closure %s\n' "$*" >&2
  exit 1
}

staged_closure_manifest_requires() {
  local required=$1
  grep -Fq "  $required" "$staged_closure_root/f05-inputs.sha256" || staged_closure_error "requires f05-inputs.sha256 to bind $required"
}

validate_f05_staged_closure() {
  staged_closure_enabled || return 0
  [[ "$staged_closure_root" = /* ]] || staged_closure_error 'requires an absolute DASOBJECTSTORE_F05_STAGED_CLOSURE_ROOT'
  [[ -d "$staged_closure_root" && ! -L "$staged_closure_root" ]] || staged_closure_error 'requires a real staged closure root'
  staged_closure_root="$(cd "$staged_closure_root" && pwd -P)"
  repo_root="$(cd "$repo_root" && pwd -P)"
  [[ "$repo_root" = "$staged_closure_root/source" ]] || staged_closure_error 'requires stage/source to be the repository root'
  [[ "$attempt_root" = /* && -d "$attempt_root" && ! -L "$attempt_root" ]] || staged_closure_error 'requires an absolute, non-symlink F05 attempt root'
  attempt_root="$(cd "$attempt_root" && pwd -P)"
  [[ "$staged_closure_root" = "$attempt_root"/* && "$repo_root" = "$attempt_root"/* && "$web_root" = "$attempt_root"/* && "$dist" = "$attempt_root"/* ]] || staged_closure_error 'requires copied closure source and web output within the F05 attempt root'
  [[ -f "$staged_closure_root/f05-inputs.sha256" ]] || staged_closure_error 'requires f05-inputs.sha256'
  [[ -d "$repo_root/vendor" && -f "$repo_root/.cargo/f05-vendor-config.toml" ]] || staged_closure_error 'requires a staged vendor tree and vendor config'
  [[ -d "$staged_closure_root/staging-home" && -d "$staged_closure_root/network-denied-bin" ]] || staged_closure_error 'requires isolated staged HOME and network denial inputs'
  staged_cargo="$staged_closure_root/toolchain/bin/cargo"
  staged_rustc="$staged_closure_root/toolchain/bin/rustc"
  staged_trunk="$staged_closure_root/toolchain/bin/trunk"
  staged_trunk_tools="$staged_closure_root/toolchain/trunk-tools"
  staged_wasm_target="$staged_closure_root/toolchain/lib/rustlib/wasm32-unknown-unknown"
  for staged_tool in "$staged_cargo" "$staged_rustc" "$staged_trunk"; do
    [[ "$staged_tool" = "$staged_closure_root"/* && -x "$staged_tool" && ! -L "$staged_tool" ]] || staged_closure_error 'requires absolute, non-symlink staged cargo, rustc, and trunk tools'
  done
  for staged_tool in "$staged_trunk_tools/wasm-bindgen-0.2.128/wasm-bindgen" "$staged_trunk_tools/wasm-opt-version_123/wasm-opt"; do
    [[ "$staged_tool" = "$staged_closure_root"/* && -x "$staged_tool" && ! -L "$staged_tool" ]] || staged_closure_error 'requires hash-bound staged wasm-bindgen 0.2.128 and wasm-opt version_123 tools'
  done
  [[ -d "$staged_wasm_target" && ! -L "$staged_wasm_target" ]] || staged_closure_error 'requires a staged wasm32-unknown-unknown target'
  [[ ! -e "$repo_root/../prosopikon" ]] || staged_closure_error 'rejects an ambient Prosopikon sibling'
  (cd "$staged_closure_root" && shasum -a 256 -c f05-inputs.sha256) >/dev/null 2>&1 || staged_closure_error 'has a missing or altered staged input'
  for staged_input in source/.cargo/f05-vendor-config.toml inputs/component-candidate-input.toml inputs/source-tree inputs/compiled-dependency-witness.json inputs/package-recipe.json toolchain/bin/cargo toolchain/bin/rustc toolchain/bin/trunk; do
    staged_closure_manifest_requires "$staged_input"
  done
  for staged_input in toolchain/trunk-tools/wasm-bindgen-0.2.128/wasm-bindgen toolchain/trunk-tools/wasm-opt-version_123/wasm-opt; do
    staged_closure_manifest_requires "$staged_input"
  done
  find "$staged_wasm_target" -type f -print -quit | grep -q . || staged_closure_error 'requires a non-empty staged wasm32-unknown-unknown target'
  find "$staged_wasm_target" -type f | while IFS= read -r wasm_path; do
    wasm_input=${wasm_path#"$staged_wasm_target/"}
    staged_closure_manifest_requires "toolchain/lib/rustlib/wasm32-unknown-unknown/$wasm_input"
  done
  find "$repo_root/vendor" -type f -print -quit | grep -q . || staged_closure_error 'requires a non-empty staged vendor tree'
  find "$repo_root/vendor" -type f | while IFS= read -r vendor_path; do
    vendor_input=${vendor_path#"$repo_root/vendor/"}
    staged_closure_manifest_requires "source/vendor/$vendor_input"
  done
  grep -Fx "prosopikon-core = { git = \"https://github.com/sagrudd/prosopikon.git\", rev = \"$F05_PROSOPIKON_REVISION\" }" "$repo_root/Cargo.toml" >/dev/null || staged_closure_error 'requires the exact Prosopikon manifest revision'
  grep -Fx "prosopikon-yew = { git = \"https://github.com/sagrudd/prosopikon.git\", rev = \"$F05_PROSOPIKON_REVISION\" }" "$repo_root/Cargo.toml" >/dev/null || staged_closure_error 'requires the exact Prosopikon Yew manifest revision'
  grep -F "git+https://github.com/sagrudd/prosopikon.git?rev=$F05_PROSOPIKON_REVISION#$F05_PROSOPIKON_REVISION" "$repo_root/Cargo.lock" >/dev/null || staged_closure_error 'requires the exact Prosopikon lock revision'
  grep -F "directory = \"$repo_root/vendor\"" "$repo_root/.cargo/f05-vendor-config.toml" >/dev/null || staged_closure_error 'requires a vendor config bound to the staged source'
}

validate_prosopikon_checkout() {
  if [[ ! -f "$prosopikon_core_root/Cargo.toml" || ! -f "$prosopikon_yew_root/Cargo.toml" ]]; then
    cat >&2 <<ERROR
Prosopikon is required to package the DASObjectStore web interface.
Expected sibling checkout: $repo_root/../prosopikon

Run: make pull
or clone/update sagrudd/prosopikon beside DASObjectStore before running make web, make deb, or make rpm.
ERROR
    return 1
  fi

  if ! grep -Eq '^[[:space:]]*auth[[:space:]]*=' "$prosopikon_core_root/Cargo.toml"; then
    cat >&2 <<ERROR
The Prosopikon checkout at $repo_root/../prosopikon is too old for DASObjectStore.
prosopikon-core must expose the auth feature used by the Web/API package.

Run: make pull
or update sagrudd/prosopikon beside DASObjectStore before running make web, make deb, or make rpm.
ERROR
    return 1
  fi

  cargo metadata --manifest-path "$repo_root/Cargo.toml" --format-version 1 --no-deps >/dev/null
}

build_web_dist() {
  if staged_closure_enabled; then
    validate_f05_staged_closure
  else
    validate_prosopikon_checkout || return 1
  fi

  if ! staged_closure_enabled && ! command -v trunk >/dev/null 2>&1; then
    cat >&2 <<'ERROR'
trunk is required to package the DASObjectStore web interface.
Install it with: cargo install trunk
ERROR
    return 1
  fi

  if ! staged_closure_enabled && ! rustup target list --installed 2>/dev/null | grep -qx 'wasm32-unknown-unknown'; then
    cat >&2 <<'ERROR'
The wasm32-unknown-unknown Rust target is required to package the DASObjectStore web interface.
Install it with: rustup target add wasm32-unknown-unknown
ERROR
    return 1
  fi

  rm -rf "$dist"
  (
    cd "$web_root"
    if staged_closure_enabled; then
      local isolated_cargo_home
      isolated_cargo_home="$attempt_root/cargo-home/web"
      rm -rf "$isolated_cargo_home"
      install -d "$isolated_cargo_home" "$attempt_root/home" "$attempt_root/target"
      cp "$repo_root/.cargo/f05-vendor-config.toml" "$isolated_cargo_home/config.toml"
      env -i HOME="$attempt_root/home" CARGO_HOME="$isolated_cargo_home" CARGO_NET_OFFLINE=true CARGO_TARGET_DIR="$attempt_root/target" PATH="$staged_closure_root/network-denied-bin:$staged_closure_root/toolchain/bin:/usr/bin:/bin" RUSTC="$staged_rustc" TRUNK_TOOLS_DIR="$staged_trunk_tools" "$staged_trunk" build --release >&2
    else
      env -u NO_COLOR trunk build --release >&2
    fi
  )
}

use_prebuilt_web_dist() {
  if [[ -z "${DASOBJECTSTORE_PREBUILT_WEB_DIST:-}" ]]; then
    return 1
  fi
  if [[ "$dist" != /* ]]; then
    printf 'DASOBJECTSTORE_PREBUILT_WEB_DIST must be an absolute path: %s\n' "$dist" >&2
    return 2
  fi
  validate_prosopikon_checkout
  validate_web_dist
}

validate_web_dist() {
  if [[ ! -f "$dist/index.html" ]]; then
    printf 'DASObjectStore web build did not produce %s\n' "$dist/index.html" >&2
    return 1
  fi
  if ! find "$dist" -maxdepth 1 -type f -name '*.wasm' | grep -q .; then
    printf 'DASObjectStore web build did not produce a WebAssembly bundle in %s\n' "$dist" >&2
    return 1
  fi
  if ! find "$dist" -maxdepth 1 -type f -name '*.js' | grep -q .; then
    printf 'DASObjectStore web build did not produce a JavaScript bundle in %s\n' "$dist" >&2
    return 1
  fi
  if grep -q 'Install the Trunk WebAssembly toolchain before packaging' "$dist/index.html"; then
    printf 'DASObjectStore web dist contains the developer fallback page, not the operator interface\n' >&2
    return 1
  fi
}

if staged_closure_enabled && [[ -n "${DASOBJECTSTORE_PREBUILT_WEB_DIST:-}" ]]; then
  staged_closure_error 'does not accept a prebuilt web distribution'
fi

if use_prebuilt_web_dist; then
  printf '%s\n' "$dist"
  exit 0
fi

if [[ -n "${DASOBJECTSTORE_PREBUILT_WEB_DIST:-}" ]]; then
  exit 1
fi

if staged_closure_enabled && [[ "$allow_fallback" = 1 ]]; then
  staged_closure_error 'does not permit the developer fallback'
fi

if build_web_dist && validate_web_dist; then
  printf '%s\n' "$dist"
  exit 0
fi

if [[ "$allow_fallback" != "1" ]]; then
  exit 1
fi

fallback="$repo_root/target/web-fallback/dist"
rm -rf "$fallback"
install -d "$fallback"
cat >"$fallback/index.html" <<'HTML'
<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>DASObjectStore</title>
    <style>
      body {
        margin: 0;
        font-family: system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
        background: #f7f8fa;
        color: #1f2933;
      }
      main {
        max-width: 760px;
        margin: 12vh auto;
        padding: 0 24px;
      }
      h1 {
        font-size: 32px;
        margin: 0 0 16px;
      }
      p {
        font-size: 16px;
        line-height: 1.5;
      }
      code {
        background: #e8edf3;
        border-radius: 4px;
        padding: 2px 5px;
      }
    </style>
  </head>
  <body>
    <main>
      <h1>DASObjectStore</h1>
      <p>The standalone web service is running. Install the Trunk WebAssembly
      toolchain before packaging to include the full operator interface.</p>
      <p>Health endpoint: <code>/products/dasobjectstore/api/v1/health</code></p>
    </main>
  </body>
</html>
HTML
printf '%s\n' "$fallback"
