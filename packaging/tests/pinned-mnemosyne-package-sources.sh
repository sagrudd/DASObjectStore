#!/usr/bin/env bash
set -euo pipefail

source_helper="${1:?expected pinned Mnemosyne source helper}"
workspace="$(mktemp -d "${TMPDIR:-/tmp}/das-pinned-sources.XXXXXX")"

cleanup() {
  find "$workspace" -depth -delete
}
trap cleanup EXIT

make_checkout() {
  local checkout="$1" manifest="$2"
  mkdir -p "$checkout"
  git init -q "$checkout"
  printf '%s\n' "$manifest" >"$checkout/Cargo.toml"
  git -C "$checkout" add Cargo.toml
  git -C "$checkout" -c user.name=package-test -c user.email=package-test@example.invalid \
    commit -qm fixture
  git -C "$checkout" rev-parse HEAD
}

pistis_revision="$(make_checkout "$workspace/pistis" 'name = "pistis-fixture"')"
prosopikon_thesaurophylax_revision="$(make_checkout "$workspace/prosopikon-thesaurophylax" 'name = "thesaurophylax-prosopikon-fixture"')"
prosopikon_revision="$(make_checkout "$workspace/prosopikon" "pistis-canonical = { git = \"https://github.com/sagrudd/pistis.git\", rev = \"$pistis_revision\" }")"
mkdir -p "$workspace/prosopikon/crates/prosopikon-core"
printf '%s\n' "thesaurophylax-api = { version = \"=0.94.6\", git = \"https://github.com/sagrudd/thesaurophylax.git\", rev = \"$prosopikon_thesaurophylax_revision\", optional = true }" >"$workspace/prosopikon/crates/prosopikon-core/Cargo.toml"
git -C "$workspace/prosopikon" add crates/prosopikon-core/Cargo.toml
git -C "$workspace/prosopikon" -c user.name=package-test -c user.email=package-test@example.invalid commit -qm fixture-thesaurophylax
prosopikon_revision="$(git -C "$workspace/prosopikon" rev-parse HEAD)"
thesaurophylax_revision="$(make_checkout "$workspace/thesaurophylax" 'name = "thesaurophylax-fixture"')"
proxenos_revision="$(make_checkout "$workspace/proxenos" "thesaurophylax-api = { version = \"=0.72.3\", git = \"https://github.com/sagrudd/thesaurophylax.git\", rev = \"$thesaurophylax_revision\" }")"

mkdir -p "$workspace/dasobjectstore"
cat >"$workspace/dasobjectstore/Cargo.toml" <<EOF
prosopikon-core = { version = "=0.47.0", git = "https://github.com/sagrudd/prosopikon.git", rev = "$prosopikon_revision" }
proxenos = { version = "=0.57.0", git = "https://github.com/sagrudd/proxenos.git", rev = "$proxenos_revision" }
EOF
cat >"$workspace/dasobjectstore/Cargo.lock" <<EOF
version = 4

[[package]]
name = "proxenos"
version = "0.57.0"
source = "git+https://github.com/sagrudd/proxenos.git?rev=$proxenos_revision#$proxenos_revision"

[[package]]
name = "thesaurophylax-api"
version = "0.72.3"
source = "git+https://github.com/sagrudd/thesaurophylax.git?rev=$thesaurophylax_revision#$thesaurophylax_revision"

[[package]]
name = "thesaurophylax-core"
version = "0.72.3"
source = "git+https://github.com/sagrudd/thesaurophylax.git?rev=$thesaurophylax_revision#$thesaurophylax_revision"

[[package]]
name = "thesaurophylax-policy"
version = "0.72.3"
source = "git+https://github.com/sagrudd/thesaurophylax.git?rev=$thesaurophylax_revision#$thesaurophylax_revision"

[[package]]
name = "thesaurophylax-store"
version = "0.72.3"
source = "git+https://github.com/sagrudd/thesaurophylax.git?rev=$thesaurophylax_revision#$thesaurophylax_revision"

[[package]]
name = "thesaurophylax-api"
version = "0.93.1"
source = "git+https://github.com/sagrudd/thesaurophylax.git?rev=$prosopikon_thesaurophylax_revision#$prosopikon_thesaurophylax_revision"

[[package]]
name = "thesaurophylax-core"
version = "0.93.1"
source = "git+https://github.com/sagrudd/thesaurophylax.git?rev=$prosopikon_thesaurophylax_revision#$prosopikon_thesaurophylax_revision"

[[package]]
name = "thesaurophylax-policy"
version = "0.93.1"
source = "git+https://github.com/sagrudd/thesaurophylax.git?rev=$prosopikon_thesaurophylax_revision#$prosopikon_thesaurophylax_revision"
EOF

source "$source_helper"
das_package_configure_pinned_mnemosyne_sources "$workspace/dasobjectstore"

[[ "$DAS_PACKAGE_PROSOPIKON_REVISION" == "$prosopikon_revision" ]]
[[ "$DAS_PACKAGE_PISTIS_REVISION" == "$pistis_revision" ]]
[[ "$DAS_PACKAGE_PROXENOS_REVISION" == "$proxenos_revision" ]]
[[ "$DAS_PACKAGE_THESAUROPHYLAX_REVISION" == "$thesaurophylax_revision" ]]

# When Proxenos and Prosopikon use the same immutable supplier revision,
# Cargo deduplicates their shared API/core/policy packages. The supplier store
# package is checked when reachable, but need not appear when unused.
shared_revision="$(make_checkout "$workspace/thesaurophylax-shared" 'name = "thesaurophylax-shared-fixture"')"
rm -rf "$workspace/thesaurophylax"
mv "$workspace/thesaurophylax-shared" "$workspace/thesaurophylax"
sed -i.bak -e "s/$thesaurophylax_revision/$shared_revision/g" -e 's/=0.72.3/=0.94.6/' "$workspace/proxenos/Cargo.toml"
rm -f "$workspace/proxenos/Cargo.toml.bak"
git -C "$workspace/proxenos" add Cargo.toml
git -C "$workspace/proxenos" -c user.name=package-test -c user.email=package-test@example.invalid commit -qm fixture-shared-thesaurophylax
proxenos_revision="$(git -C "$workspace/proxenos" rev-parse HEAD)"
sed -i.bak "s/$prosopikon_thesaurophylax_revision/$shared_revision/g" "$workspace/prosopikon/crates/prosopikon-core/Cargo.toml"
rm -f "$workspace/prosopikon/crates/prosopikon-core/Cargo.toml.bak"
git -C "$workspace/prosopikon" add crates/prosopikon-core/Cargo.toml
git -C "$workspace/prosopikon" -c user.name=package-test -c user.email=package-test@example.invalid commit -qm fixture-shared-thesaurophylax
prosopikon_revision="$(git -C "$workspace/prosopikon" rev-parse HEAD)"
thesaurophylax_revision="$shared_revision"
cat >"$workspace/dasobjectstore/Cargo.toml" <<EOF
prosopikon-core = { version = "=0.47.0", git = "https://github.com/sagrudd/prosopikon.git", rev = "$prosopikon_revision" }
proxenos = { version = "=0.57.0", git = "https://github.com/sagrudd/proxenos.git", rev = "$proxenos_revision" }
EOF
cat >"$workspace/dasobjectstore/Cargo.lock" <<EOF
version = 4

[[package]]
name = "proxenos"
version = "0.57.0"
source = "git+https://github.com/sagrudd/proxenos.git?rev=$proxenos_revision#$proxenos_revision"

[[package]]
name = "thesaurophylax-api"
version = "0.94.6"
source = "git+https://github.com/sagrudd/thesaurophylax.git?rev=$shared_revision#$shared_revision"

[[package]]
name = "thesaurophylax-core"
version = "0.94.6"
source = "git+https://github.com/sagrudd/thesaurophylax.git?rev=$shared_revision#$shared_revision"

[[package]]
name = "thesaurophylax-policy"
version = "0.94.6"
source = "git+https://github.com/sagrudd/thesaurophylax.git?rev=$shared_revision#$shared_revision"

[[package]]
name = "thesaurophylax-store"
version = "0.94.6"
source = "git+https://github.com/sagrudd/thesaurophylax.git?rev=$shared_revision#$shared_revision"
EOF
das_package_configure_pinned_mnemosyne_sources "$workspace/dasobjectstore"
[[ "$DAS_PACKAGE_THESAUROPHYLAX_REVISION" == "$shared_revision" ]]

# The actual DASObjectStore source graph locks only the reachable shared
# API/core/policy packages for this dependency set.
shared_source="git+https://github.com/sagrudd/thesaurophylax.git?rev=$shared_revision#$shared_revision"
reachable_lock="$workspace/reachable-thesaurophylax.lock"
cat >"$reachable_lock" <<EOF
[[package]]
name = "thesaurophylax-api"
version = "0.94.6"
source = "$shared_source"

[[package]]
name = "thesaurophylax-core"
version = "0.94.6"
source = "$shared_source"

[[package]]
name = "thesaurophylax-policy"
version = "0.94.6"
source = "$shared_source"
EOF
if ! das_require_locked_thesaurophylax_closure "$reachable_lock" "0.94.6" "$shared_revision" "$shared_revision" >/dev/null; then
  printf 'pinned source validation rejected the reachable shared Thesaurophylax lock closure\n' >&2
  exit 1
fi

valid_lock="$(<"$workspace/dasobjectstore/Cargo.lock")"
cat >>"$workspace/dasobjectstore/Cargo.lock" <<EOF

[[package]]
name = "thesaurophylax-store"
version = "0.99.0"
source = "git+https://github.com/sagrudd/thesaurophylax.git?rev=$prosopikon_thesaurophylax_revision#$prosopikon_thesaurophylax_revision"
EOF
if das_package_configure_pinned_mnemosyne_sources "$workspace/dasobjectstore" 2>/dev/null; then
  printf 'pinned source validation accepted an unexpected Thesaurophylax lock edge\n' >&2
  exit 1
fi
printf '%s\n' "$valid_lock" >"$workspace/dasobjectstore/Cargo.lock"

cat >>"$workspace/dasobjectstore/Cargo.lock" <<EOF

[[package]]
name = "proxenos"
version = "0.99.0"
source = "git+https://github.com/sagrudd/proxenos.git?rev=$pistis_revision#$pistis_revision"
EOF
if das_package_configure_pinned_mnemosyne_sources "$workspace/dasobjectstore" 2>/dev/null; then
  printf 'pinned source validation accepted a duplicate Proxenos lock entry\n' >&2
  exit 1
fi
printf '%s\n' "$valid_lock" >"$workspace/dasobjectstore/Cargo.lock"

artifact="$workspace/dasobjectstore/test.deb"
das_package_write_pinned_dependency_provenance "$artifact"
expected="{\"schema\":\"mnemosyne.dasobjectstore.package-dependencies.v2\",\"prosopikon_revision\":\"$prosopikon_revision\",\"pistis_revision\":\"$pistis_revision\",\"proxenos_revision\":\"$proxenos_revision\",\"thesaurophylax_revision\":\"$thesaurophylax_revision\"}"
[[ "$(<"$artifact.dependencies.json")" == "$expected" ]]

valid_manifest="$(<"$workspace/dasobjectstore/Cargo.toml")"
valid_lock="$(<"$workspace/dasobjectstore/Cargo.lock")"
printf '%s\n' "${valid_manifest/=0.47.0/=0.47.1}" >"$workspace/dasobjectstore/Cargo.toml"
if das_package_configure_pinned_mnemosyne_sources "$workspace/dasobjectstore" 2>/dev/null; then
  printf 'pinned source validation accepted a wrong Prosopikon version\n' >&2
  exit 1
fi
printf '%s\n' "$valid_manifest" >"$workspace/dasobjectstore/Cargo.toml"
printf '%s\n' "${valid_manifest/$prosopikon_revision/$pistis_revision}" >"$workspace/dasobjectstore/Cargo.toml"
if das_package_configure_pinned_mnemosyne_sources "$workspace/dasobjectstore" 2>/dev/null; then
  printf 'pinned source validation accepted a wrong Prosopikon revision\n' >&2
  exit 1
fi
printf '%s\n' "$valid_manifest" >"$workspace/dasobjectstore/Cargo.toml"

stale_lock="$(sed "s/$thesaurophylax_revision/$pistis_revision/g" "$workspace/dasobjectstore/Cargo.lock")"
printf '%s\n' "$stale_lock" >"$workspace/dasobjectstore/Cargo.lock"
if das_package_configure_pinned_mnemosyne_sources "$workspace/dasobjectstore" 2>/dev/null; then
  printf 'pinned source validation accepted a stale Thesaurophylax Cargo.lock closure\n' >&2
  exit 1
fi

printf '%s\n' "$valid_lock" >"$workspace/dasobjectstore/Cargo.lock"

printf '# uncommitted fixture drift\n' >>"$workspace/proxenos/Cargo.toml"
if das_package_configure_pinned_mnemosyne_sources "$workspace/dasobjectstore" 2>/dev/null; then
  printf 'pinned source validation accepted a dirty Proxenos checkout\n' >&2
  exit 1
fi

shared_source="git+https://github.com/sagrudd/thesaurophylax.git?rev=$thesaurophylax_revision#$thesaurophylax_revision"
shared_lock="$workspace/shared-thesaurophylax.lock"
cat >"$shared_lock" <<EOF
[[package]]
name = "thesaurophylax-api"
version = "0.94.6"
source = "$shared_source"

[[package]]
name = "thesaurophylax-core"
version = "0.94.6"
source = "$shared_source"

[[package]]
name = "thesaurophylax-policy"
version = "0.94.6"
source = "$shared_source"

[[package]]
name = "thesaurophylax-store"
version = "0.94.6"
source = "$shared_source"
EOF
if ! das_require_locked_thesaurophylax_closure "$shared_lock" "0.94.6" "$thesaurophylax_revision" "$thesaurophylax_revision" >/dev/null; then
  printf 'pinned source validation rejected the valid shared Thesaurophylax lock closure\n' >&2
  exit 1
fi
cat >>"$shared_lock" <<EOF

[[package]]
name = "thesaurophylax-store"
version = "0.94.6"
source = "$shared_source"
EOF
if das_require_locked_thesaurophylax_closure "$shared_lock" "0.94.6" "$thesaurophylax_revision" "$thesaurophylax_revision" >/dev/null 2>&1; then
  printf 'pinned source validation accepted a duplicate shared Thesaurophylax record\n' >&2
  exit 1
fi

printf 'pinned source validation: supported projection, shared closure, and denials passed\n'
