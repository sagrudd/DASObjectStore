#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
web_root="$repo_root/crates/dasobjectstore-gui-web"
dist="$web_root/dist"

if [[ -f "$dist/index.html" ]]; then
  printf '%s\n' "$dist"
  exit 0
fi

if command -v trunk >/dev/null 2>&1; then
  (
    cd "$web_root"
    trunk build --release
  )
  printf '%s\n' "$dist"
  exit 0
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
