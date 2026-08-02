#!/usr/bin/env sh
# Verify the source package and its dependency-free external consumer fixture.
# This is package-readiness evidence only; it does not publish, install, issue,
# resolve, transport, or authorise any DASObjectStore reference.
set -eu

crate=dasobjectstore-reference
for fixture in object-ref-v1.json object-ref-v1-max-safe-integer.json evidence-ref-v1.json; do
    cmp "docs/adr/fixtures/$fixture" "crates/dasobjectstore-reference/fixtures/$fixture"
done

cargo package -p "$crate" --allow-dirty
version=$(cargo pkgid -p "$crate" | sed 's/.*#//')
archive="target/package/$crate-$version.crate"
test -f "$archive"

scratch=$(mktemp -d "${TMPDIR:-/tmp}/$crate.XXXXXX")
trap 'rm -rf "$scratch"' EXIT HUP INT TERM
tar -xzf "$archive" -C "$scratch"
package_root="$scratch/$crate-$version"
test -f "$package_root/LICENSE"
test -f "$package_root/README.md"
consumer="$package_root/fixtures/downstream-consumer"
cp "$consumer/Cargo.toml.template" "$consumer/Cargo.toml"
cargo run --manifest-path "$consumer/Cargo.toml" --offline
