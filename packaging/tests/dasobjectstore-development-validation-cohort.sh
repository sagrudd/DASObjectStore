#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
asset_root="$repo_root/packaging/expedition/dasobjectstore-development-validation"
stager="$repo_root/packaging/expedition/stage-development-validation-cohort.sh"
payload_root="$(mktemp -d)"
escape_root="$(mktemp -d)"
trap 'rm -rf "$payload_root" "$escape_root"' EXIT

python3 - "$asset_root" <<'PY'
import json
import sys
from pathlib import Path

root = Path(sys.argv[1])
manifest = json.loads((root / "manifest.json").read_text(encoding="utf-8"))
policy = json.loads((root / "policy.json").read_text(encoding="utf-8"))
catalog = json.loads((root / "task-catalog.json").read_text(encoding="utf-8"))
selector = json.loads((root / "cohort.json").read_text(encoding="utf-8"))

assert manifest["schema_version"] == "1.4.0"
assert manifest["source"] == {
    "repository": "https://github.com/sagrudd/DASObjectStore",
    "revision": "86284df32372d7758f2917928cd6adef2eb93516",
}
assert manifest["source_dependencies"] == [
    {
        "repository": "https://github.com/sagrudd/prosopikon",
        "revision": "f09749273ef382c1b42bf04a77d96189dd7361b3",
        "mount_path": "../prosopikon",
    },
    {
        "repository": "https://github.com/sagrudd/proxenos",
        "revision": "d4c3054fb7d88c9f718d2987ec19bf7bc444d391",
        "mount_path": "../proxenos",
    },
    {
        "repository": "https://github.com/sagrudd/thesaurophylax",
        "revision": "0bfb16857d135d2830de2cf53d245b68ed2d051f",
        "mount_path": "../thesaurophylax",
    },
]
assert manifest["trust"] == "trusted_revision"
assert manifest["targets"] == ["linux_amd64", "linux_arm64"]
assert manifest["requested_capabilities"] == ["read_only_source", "network"]
assert manifest["extends"] == {
    "id": "mnemosyne/dasobjectstore-development-validation",
    "version": "1.0.0",
}
assert policy["id"] == manifest["extends"]["id"]
assert policy["version"] == manifest["extends"]["version"]
assert policy["approved_tasks"] == ["dasobjectstore-development-validation"]
assert policy["allowed_capabilities"] == manifest["requested_capabilities"]
command = catalog["tasks"]["dasobjectstore-development-validation"]["command"]
assert command[:2] == ["bash", "-ceu"]
assert len(command) == 3
command_text = command[2]
for required in (
    "for directory in /workspace /prosopikon /proxenos /thesaurophylax",
    "safe.directory \"$directory\"",
    'url."file:///prosopikon".insteadOf https://github.com/sagrudd/prosopikon.git',
    'url."file:///proxenos".insteadOf https://github.com/sagrudd/proxenos.git',
    'url."file:///thesaurophylax".insteadOf https://github.com/sagrudd/thesaurophylax.git',
    "cargo fetch --locked",
    "CARGO_NET_OFFLINE=true",
    "cargo fmt --all -- --check",
    "tools/check-rust-module-size.sh",
    "cargo clippy --workspace --all-targets -- -D warnings",
    "cargo test --workspace --locked --offline",
):
    assert required in command_text
assert "EXPEDITION_SOURCE_TOKEN" not in command_text
assert "Authorization:" not in command_text
assert set(selector) == {
    "schema_version",
    "manifest_path",
    "policy_path",
    "task_catalog_path",
    "vault_secret_id",
    "jenkins_credential_id",
}
assert selector == {
    "schema_version": "mnemosyne.expedition.github-development-cohort.v1",
    "manifest_path": "/usr/share/mnemosyne-expedition/dasobjectstore-development-validation/manifest.json",
    "policy_path": "/usr/share/mnemosyne-expedition/dasobjectstore-development-validation/policy.json",
    "task_catalog_path": "/usr/share/mnemosyne-expedition/dasobjectstore-development-validation/task-catalog.json",
    "vault_secret_id": "github.dasobjectstore-development-source",
    "jenkins_credential_id": "dasobjectstore-development-source",
}
assert "token" not in (root / "cohort.json").read_text(encoding="utf-8").lower()
PY

"$stager" "$payload_root"
expected_files=$'./usr/share/mnemosyne-expedition/dasobjectstore-development-validation/Containerfile.ci\n./usr/share/mnemosyne-expedition/dasobjectstore-development-validation/README.md\n./usr/share/mnemosyne-expedition/dasobjectstore-development-validation/manifest.json\n./usr/share/mnemosyne-expedition/dasobjectstore-development-validation/policy.json\n./usr/share/mnemosyne-expedition/dasobjectstore-development-validation/task-catalog.json\n./usr/share/mnemosyne-expedition/development-cohorts/dasobjectstore-development-validation.json'
actual_files="$(cd "$payload_root" && find . -type f -print | LC_ALL=C sort)"
if [[ "$actual_files" != "$expected_files" ]]; then
  printf 'development-validation cohort staging produced an unexpected payload:\n%s\n' "$actual_files" >&2
  exit 1
fi

cmp "$asset_root/manifest.json" "$payload_root/usr/share/mnemosyne-expedition/dasobjectstore-development-validation/manifest.json"
cmp "$asset_root/policy.json" "$payload_root/usr/share/mnemosyne-expedition/dasobjectstore-development-validation/policy.json"
cmp "$asset_root/task-catalog.json" "$payload_root/usr/share/mnemosyne-expedition/dasobjectstore-development-validation/task-catalog.json"
cmp "$asset_root/cohort.json" "$payload_root/usr/share/mnemosyne-expedition/development-cohorts/dasobjectstore-development-validation.json"

if "$stager" / >/dev/null 2>&1; then
  printf 'development-validation cohort stager must refuse the live root filesystem\n' >&2
  exit 1
fi

symlink_root="$escape_root/symlink-root"
ln -s "$payload_root" "$symlink_root"
if "$stager" "$symlink_root" >/dev/null 2>&1; then
  printf 'development-validation cohort stager must refuse a symlinked payload root\n' >&2
  exit 1
fi
rm -f "$symlink_root"

unsafe_parent_root="$escape_root/unsafe-parent-root"
unsafe_parent_escape="$escape_root/unsafe-parent-escape"
mkdir "$unsafe_parent_root" "$unsafe_parent_escape"
ln -s "$unsafe_parent_escape" "$unsafe_parent_root/usr"
if "$stager" "$unsafe_parent_root" >/dev/null 2>&1; then
  printf 'development-validation cohort stager must refuse a symlinked payload parent\n' >&2
  exit 1
fi
if find "$unsafe_parent_escape" -mindepth 1 -print -quit | grep -q .; then
  printf 'development-validation cohort stager wrote through a symlinked payload parent\n' >&2
  exit 1
fi
