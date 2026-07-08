#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
web_root="$repo_root/crates/dasobjectstore-gui-web"
dist="$web_root/dist"
allow_fallback=0

if [[ "${1:-}" == "--allow-fallback" ]]; then
  allow_fallback=1
fi

build_web_dist() {
  if ! command -v trunk >/dev/null 2>&1; then
    cat >&2 <<'ERROR'
trunk is required to package the DASObjectStore web interface.
Install it with: cargo install trunk
ERROR
    return 1
  fi

  if ! rustup target list --installed 2>/dev/null | grep -qx 'wasm32-unknown-unknown'; then
    cat >&2 <<'ERROR'
The wasm32-unknown-unknown Rust target is required to package the DASObjectStore web interface.
Install it with: rustup target add wasm32-unknown-unknown
ERROR
    return 1
  fi

  rm -rf "$dist"
  (
    cd "$web_root"
    env -u NO_COLOR trunk build --release
  )
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
