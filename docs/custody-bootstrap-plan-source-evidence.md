# Custody bootstrap planner source verification — issue #199

This records source-development checks only, not package or live qualification.
Programme #251 accepts only the non-mutating first slice. Kanon #331 proposes
its source coordinates. No immutable 0.181 package selection is modified.

Local checks on macOS:

- Object-service suite: 97 unit tests and one compose integration test pass.
  Nine new planner tests include recursive required-field deletion, every
  truncated raw manifest prefix, duplicate and inherited unknown fields,
  target/time/role/retention/hold/isolation/independent-evidence matrices.
  The ninth test covers the independently reviewed programme #253 correction:
  terminal receipts are bounded executor-only policies, never guessed hashes.
- Dedicated CLI: three tests pass, including actual descriptor/file-to-plan
  roundtrip, unchanged input bytes/directory inventory, malformed-input
  redaction, no-follow parent/final aliases and oversized/device/directory
  denial. This is a Unix file-adapter test, not Linux runtime qualification.
- `cargo clippy -p dasobjectstore-object-service --all-targets --locked
  --offline -- -D warnings` passes.
- Dedicated CLI Clippy with `--tests --no-deps` and `-D warnings` passes for
  the selected crate. Seventeen preexisting macOS warnings in the Linux-only
  r237 observer dependency remain visible, not suppressed or counted clean.
- Strict object-service rustdoc and scoped rustfmt checks pass.
- `cargo audit --no-fetch --json` reports no known vulnerabilities in the
  locally retained advisory database; unmaintained/yanked warnings remain.
  No online freshness claim is made. The first attempt with unsupported
  `--locked` was rejected by cargo-audit and is not counted as a check pass.
- `cargo deny --offline check` fails: the repository has no deny configuration,
  so default license rejection and existing unmaintained advisories remain.
  No allowlist or advisory exception was added. Cargo.lock changes only the
  thirteen workspace package version entries, not any dependency or pin.
- Strict Sphinx fails on five preexisting title-underline warnings in
  `adr/0007-pistis-only-human-authority.rst` and `user/storage-assurance.rst`;
  the new page has no warning after its title fix. No warning is suppressed.

Full workspace/all-feature tests, native Linux namespace/systemd/Garage tests,
formal package gates and release-stage evidence were not run or claimed by
this bounded source slice. Those missing/failed checks remain explicit review
limitations. The executable exposes no runtime/apply path, daemon socket,
credential lookup, service control or live observation. Its raw input file
reads may update normal filesystem access metadata, as documented.
