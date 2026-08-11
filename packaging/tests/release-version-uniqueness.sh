#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT
mkdir -p "$fixture/crates/example"
git -C "$fixture" init -q
git -C "$fixture" config user.name "DAS release guard"
git -C "$fixture" config user.email "release-guard@invalid"
printf '[workspace.package]\nversion = "1.2.3"\n' >"$fixture/Cargo.toml"
printf '# Changelog\n\n## 1.2.3 - 2026-08-11\n' >"$fixture/CHANGELOG.md"
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

sed -i.bak 's/1.2.3/1.2.4/g' "$fixture/Cargo.toml" "$fixture/CHANGELOG.md"
rm -f "$fixture/Cargo.toml.bak" "$fixture/CHANGELOG.md.bak"
bash "$repo_root/packaging/validate-release-version.sh" "$fixture" >/dev/null
printf 'release version uniqueness regression passed\n'
