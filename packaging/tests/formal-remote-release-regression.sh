#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT
source_root="$fixture/source"
rsync -a --exclude='.git' --exclude='target' "$repo_root/" "$source_root/"
git -C "$source_root" init -q
git -C "$source_root" config user.name "DAS formal package guard"
git -C "$source_root" config user.email "formal-package-guard@invalid"
git -C "$source_root" add .
git -C "$source_root" commit -qm fixture
version="$(cargo metadata --no-deps --format-version 1 --manifest-path "$source_root/Cargo.toml" \
  | sed -n 's/.*"name":"dasobjectstore-remote","version":"\([^"]*\)".*/\1/p')"
head="$(git -C "$source_root" rev-parse HEAD)"
if [[ -z "$version" || ! "$head" =~ ^[0-9a-f]{40}$ ]]; then
  printf 'formal remote release regression: could not derive package version and source revision\n' >&2
  exit 1
fi

write_fixture() {
  python3 - "$fixture" "$version" "$head" <<'PY'
import json
import sys
from pathlib import Path

root = Path(sys.argv[1])
version = sys.argv[2]
revision = sys.argv[3]
content_digest = "sha256:" + "a" * 64
registry_digest = "sha256:" + "b" * 64
(root / "lockset.toml").write_text(
    "\n".join(
        [
            'schema_version = "mnemosyne.kanon.lockset.v1alpha2"',
            'id = "das-test-lockset"',
            f'content_digest = "{content_digest}"',
            f'registry_digest = "{registry_digest}"',
            "",
            "[[components]]",
            'product_id = "dasobjectstore-remote"',
            f'version = "{version}"',
            f'source_revision = "{revision}"',
            "",
            "[components.version_provenance]",
            'package_name = "dasobjectstore-remote"',
            f'source_revision = "{revision}"',
            "",
        ]
    )
)
authority = {
    "lockset_id": "das-test-lockset",
    "content_digest": content_digest,
    "registry_digest": registry_digest,
}
catalogue_authority = {**authority, "kind": "kanon_lockset_projection"}
(root / "catalogue.json").write_text(
    json.dumps(
        {
            "compatibility_authority": catalogue_authority,
            "schema": "mnemosyne.terraform.component-catalogue.v1",
            "components": {
                "dasobjectstore-remote": {
                    "source_lock_key": "dasobjectstore",
                    "package": {"names": {"deb": ["dasobjectstore-remote"], "rpm": ["dasobjectstore-remote"]}},
                }
            },
        }
    )
)
(root / "sources.lock").write_text(
    json.dumps(
        {
            "authority": {
                **authority,
                "schema": "mnemosyne.terraform.sources-lock-authority.v1",
            },
            "components": {"dasobjectstore": {"revision": revision}},
        }
    )
)
PY
}

validate() {
  python3 "$repo_root/packaging/validate-formal-remote-release.py" \
    --repo-root "$source_root" \
    --lockset "$fixture/lockset.toml" \
    --catalogue "$fixture/catalogue.json" \
    --sources-lock "$fixture/sources.lock" \
    --package-version "$version"
}

write_fixture
expected="das-test-lockset sha256:$(printf 'a%.0s' {1..64}) sha256:$(printf 'b%.0s' {1..64}) $head"
actual="$(validate)" || {
  printf 'formal remote release regression: valid Terraform inputs were rejected\n' >&2
  exit 1
}
if [[ "$actual" != "$expected" ]]; then
  printf 'formal remote release regression: accepted authority was not reproduced\n' >&2
  exit 1
fi

printf 'untracked fixture\n' >"$source_root/formal-remote-release-untracked"
if validate >/dev/null 2>&1; then
  printf 'formal remote release regression: dirty source was accepted\n' >&2
  exit 1
fi
rm -f "$source_root/formal-remote-release-untracked"

if python3 "$repo_root/packaging/validate-formal-remote-release.py" \
  --repo-root "$source_root" \
  --lockset "$fixture/missing.toml" \
  --catalogue "$fixture/catalogue.json" \
  --sources-lock "$fixture/sources.lock" \
  --package-version "$version" >/dev/null 2>&1; then
  printf 'formal remote release regression: missing lockset was accepted\n' >&2
  exit 1
fi

if python3 "$repo_root/packaging/validate-formal-remote-release.py" \
  --repo-root "$source_root" \
  --lockset "$fixture/lockset.toml" \
  --catalogue "$fixture/catalogue.json" \
  --sources-lock "$fixture/sources.lock" \
  --package-version 9.9.9 >/dev/null 2>&1; then
  printf 'formal remote release regression: version drift was accepted\n' >&2
  exit 1
fi

python3 - "$fixture/catalogue.json" <<'PY'
import json
import sys

path = sys.argv[1]
value = json.load(open(path, encoding="utf-8"))
value["compatibility_authority"]["content_digest"] = "sha256:" + "c" * 64
with open(path, "w", encoding="utf-8") as handle:
    json.dump(value, handle)
PY
if validate >/dev/null 2>&1; then
  printf 'formal remote release regression: catalogue authority drift was accepted\n' >&2
  exit 1
fi

write_fixture
python3 - "$fixture/sources.lock" <<'PY'
import json
import sys

path = sys.argv[1]
value = json.load(open(path, encoding="utf-8"))
value["components"]["dasobjectstore"]["revision"] = "f" * 40
with open(path, "w", encoding="utf-8") as handle:
    json.dump(value, handle)
PY
if validate >/dev/null 2>&1; then
  printf 'formal remote release regression: source-lock revision drift was accepted\n' >&2
  exit 1
fi

write_fixture
source "$source_root/packaging/package-provenance.sh"
export DAS_PACKAGE_SOURCE_REVISION="$head"
export DAS_PACKAGE_SOURCE_EPOCH=1
export DAS_PACKAGE_RELEASE_SOURCE_REVISION="$head"
export DAS_PACKAGE_LOCKSET_ID=das-test-lockset
export DAS_PACKAGE_LOCKSET_CONTENT_DIGEST="sha256:$(printf 'a%.0s' {1..64})"
export DAS_PACKAGE_LOCKSET_REGISTRY_DIGEST="sha256:$(printf 'b%.0s' {1..64})"
printf 'formal remote fixture\n' >"$fixture/package"
das_package_write_formal_remote_provenance "$fixture/package" all "$version"
python3 - "$fixture/package.provenance.json" "$version" "$head" <<'PY'
import json
import sys

value = json.load(open(sys.argv[1], encoding="utf-8"))
assert value["schema"] == "mnemosyne.dasobjectstore.package-provenance.v2"
assert value["component_id"] == "dasobjectstore-remote"
assert value["package_name"] == "dasobjectstore-remote"
assert value["package_version"] == sys.argv[2]
assert value["source_revision"] == sys.argv[3]
assert value["lockset_id"] == "das-test-lockset"
PY

for builder in packaging/debian/build-remote-deb.sh packaging/rpm/build-remote-rpm.sh; do
  grep -Fq 'packaging/validate-release-version.sh' "$source_root/$builder"
  grep -Fq 'validate-formal-remote-release.py' "$source_root/$builder"
  grep -Fq 'das_package_write_formal_remote_provenance' "$source_root/$builder"
  awk '/validate-formal-remote-release.py/ { guard = NR } /cargo build --release --locked -p dasobjectstore-remote/ { build = NR } END { exit !(guard && build && guard < build) }' "$source_root/$builder"
  if env -u TERRAFORM_SUCCESSOR_LOCKSET -u TERRAFORM_CATALOGUE -u TERRAFORM_SOURCES_LOCK \
    bash "$source_root/$builder" >/dev/null 2>&1; then
    printf 'formal remote release regression: %s accepted missing Terraform inputs\n' "$builder" >&2
    exit 1
  fi
done

printf 'formal remote release regression passed\n'
