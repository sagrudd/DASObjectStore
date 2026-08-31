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
prosopikon_revision="$(make_checkout "$workspace/prosopikon" "pistis-canonical = { git = \"https://github.com/sagrudd/pistis.git\", rev = \"$pistis_revision\" }")"
thesaurophylax_revision="$(make_checkout "$workspace/thesaurophylax" 'name = "thesaurophylax-fixture"')"
proxenos_revision="$(make_checkout "$workspace/proxenos" "thesaurophylax-api = { git = \"https://github.com/sagrudd/thesaurophylax.git\", rev = \"$thesaurophylax_revision\" }")"

mkdir -p "$workspace/dasobjectstore"
cat >"$workspace/dasobjectstore/Cargo.toml" <<EOF
prosopikon-core = { git = "https://github.com/sagrudd/prosopikon.git", rev = "$prosopikon_revision" }
proxenos = { git = "https://github.com/sagrudd/proxenos.git", rev = "$proxenos_revision" }
EOF

source "$source_helper"
das_package_configure_pinned_mnemosyne_sources "$workspace/dasobjectstore"

[[ "$DAS_PACKAGE_PROSOPIKON_REVISION" == "$prosopikon_revision" ]]
[[ "$DAS_PACKAGE_PISTIS_REVISION" == "$pistis_revision" ]]
[[ "$DAS_PACKAGE_PROXENOS_REVISION" == "$proxenos_revision" ]]
[[ "$DAS_PACKAGE_THESAUROPHYLAX_REVISION" == "$thesaurophylax_revision" ]]

artifact="$workspace/dasobjectstore/test.deb"
das_package_write_pinned_dependency_provenance "$artifact"
expected="{\"schema\":\"mnemosyne.dasobjectstore.package-dependencies.v2\",\"prosopikon_revision\":\"$prosopikon_revision\",\"pistis_revision\":\"$pistis_revision\",\"proxenos_revision\":\"$proxenos_revision\",\"thesaurophylax_revision\":\"$thesaurophylax_revision\"}"
[[ "$(<"$artifact.dependencies.json")" == "$expected" ]]

printf '# uncommitted fixture drift\n' >>"$workspace/proxenos/Cargo.toml"
if das_package_configure_pinned_mnemosyne_sources "$workspace/dasobjectstore" 2>/dev/null; then
  printf 'pinned source validation accepted a dirty Proxenos checkout\n' >&2
  exit 1
fi
