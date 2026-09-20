#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

# Fail before fixture setup when this checkout's published release coordinates
# drift. The remote package builders execute the same guard before any
# closure or compilation work.
bash "$repo_root/packaging/validate-release-version.sh" "$repo_root" >/dev/null

# The normal appliance DEB/RPM builders copy this descriptor unchanged, so
# require its release coordinate to stay aligned with the published product
# manifest and make that payload relationship explicit in this cheap guard.
python3 - "$repo_root" <<'PY'
import json
import sys
from pathlib import Path

root = Path(sys.argv[1])
manifest = json.loads((root / "product-manifest.json").read_text(encoding="utf-8"))
descriptor = json.loads(
    (root / "packaging/linux/opt/dasobjectstore/plugin-process-descriptor.json").read_text(
        encoding="utf-8"
    )
)
if descriptor.get("productId") != manifest.get("product", {}).get("id"):
    raise SystemExit("release version regression: descriptor product id drift")
if descriptor.get("version") != manifest.get("product", {}).get("version"):
    raise SystemExit("release version regression: descriptor version drift")
PY
for builder in packaging/debian/build-deb.sh packaging/rpm/build-rpm.sh; do
  grep -Fq 'plugin-process-descriptor.json' "$repo_root/$builder"
done

fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT
mkdir -p "$fixture/crates/example"
git -C "$fixture" init -q
git -C "$fixture" config user.name "DAS release guard"
git -C "$fixture" config user.email "release-guard@invalid"
printf '[workspace.package]\nversion = "1.2.3"\n' >"$fixture/Cargo.toml"
printf '# Changelog\n\n## 1.2.3 - 2026-08-11\n' >"$fixture/CHANGELOG.md"
printf '{"product":{"id":"dasobjectstore","version":"1.2.3"}}\n' >"$fixture/product-manifest.json"
printf 'first\n' >"$fixture/crates/example/source.rs"
git -C "$fixture" add .
git -C "$fixture" commit -qm initial

printf 'changed\n' >>"$fixture/crates/example/source.rs"
git -C "$fixture" add .
git -C "$fixture" commit -qm collision
if bash "$repo_root/packaging/validate-release-version.sh" "$fixture" >/dev/null 2>&1; then
  printf 'release version regression: same-version package change was accepted\n' >&2
  exit 1
fi

sed -i.bak 's/1.2.3/1.2.4/g' "$fixture/Cargo.toml" "$fixture/CHANGELOG.md" "$fixture/product-manifest.json"
rm -f "$fixture/Cargo.toml.bak" "$fixture/CHANGELOG.md.bak" "$fixture/product-manifest.json.bak"
bash "$repo_root/packaging/validate-release-version.sh" "$fixture" >/dev/null

sed -i.bak 's/1.2.4/1.2.5/g' "$fixture/product-manifest.json"
rm -f "$fixture/product-manifest.json.bak"
if bash "$repo_root/packaging/validate-release-version.sh" "$fixture" >/dev/null 2>&1; then
  printf 'release version regression: product manifest mismatch was accepted\n' >&2
  exit 1
fi
printf 'release version uniqueness regression passed\n'
