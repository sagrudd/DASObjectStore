# Custody-only service extraction — source evidence

Issue: #203. Baseline: `9dd2c5f7d97bd777370f486ef1098a79df8e7af2`
(0.182.1). Prospective source: 0.183.0. Programme #265 accepted the connected
extraction at `5c1746891d25ea47397a1ed1236fe6606fc491d7`, merged as
`9a8bb46cf7b165590d01afcaff85640899770c95`. Kanon #339 source coordination
`c892a6d` preceded implementation. No resolved release lockset is produced;
the immutable r239 selected DAS 0.181.0 source
`4aa461164683afc11cb6bc6de95bf0baa15b2293` is not modified or replaced.

## Implementation boundary

`runtime/custody_service.rs` owns the formerly embedded custody orchestration.
`GarageServiceController` delegates its existing admission and retention methods
to the extracted implementation. Explicit borrowed bindings preserve the real
runner and one-use authorities; `CustodyServiceState` retains pending in-process
provisioner results while the existing catalogue owns durable namespace claims.
Dropping this state does not reset claims or authorize adoption of orphaned work.
The state permanently binds its first valid exact catalogue and custody/excluded
plane configurations; reuse with a different composition denies before effects.

The constructor has no catalogue/configuration defaults. It applies the existing
ordinary-versus-custody configuration validator; this is configuration alias
denial, not proof of operating-system or administrator exclusion. No lifecycle,
registry, placement, ingress, provider, credential issuer or execution-admission
constructor is added. Existing Garage adapters, catalogues, ledgers, request
formats, credential handoffs and immutable failure semantics are reused.

## Verification

The existing connected daemon tests cover successful admitted conditional
retention/readback, sealed handoff-reference denial, one-use replay denial,
default configuration denial and catalogue/endpoint isolation. Added direct
controller fixtures exercise admission without a normal daemon controller,
all eight existing alias coordinates, and a failed provisioning attempt whose
durable claim prevents another attempt even with fresh in-process state and
synthetic handoff, plus cross-composition state reuse denial. They use real
catalogue persistence and synthetic runners
and authority adapters, not a running Garage service or installed credentials.

## Local results

macOS, rustc 1.90.0 (`1159e78c4`), Cargo 1.90.0 (`840b83a10`). Existing cache
used with `CARGO_INCREMENTAL=0`; no host or installed-service tests were run.

- `cargo test --locked --offline -p dasobjectstore-daemon --lib custody`:
  30 passed, including four new direct composition regressions.
- `cargo test --locked --offline -p dasobjectstore-daemon --lib`:
  931 passed, no ignored tests.
- `cargo test --locked --offline -p dasobjectstore-daemon --all-targets
  --all-features --quiet`: 1,017 passed across nine groups, no ignored tests.
- `cargo clippy --locked --offline -p dasobjectstore-daemon --all-targets
  --all-features -- -D warnings`: passed.
- `RUSTDOCFLAGS='-D warnings' cargo doc --locked --offline
  -p dasobjectstore-daemon --all-features --no-deps --document-private-items`:
  passed.
- Scoped changed-Rust-file formatting and `git diff --check`: passed.
  Whole-workspace `cargo fmt --all -- --check` remains blocked solely by the
  unchanged object-service `lib.rs` module ordering at line 1. No unrelated
  formatting correction or gate waiver is included.
- Existing `application-authentication-package-guard.sh`,
  `package-provenance-regression.sh` and `formal-remote-release-regression.sh`:
  passed. These are synthetic package guard regressions, not package acceptance.
- Sphinx dummy build: completed with five pre-existing underline warnings in
  ADR-0007 and `user/storage-assurance.rst`; not a zero-warning documentation gate.
- `cargo audit --no-fetch --no-yanked --stale`: exit 0 with three existing
  unmaintained warnings (bincode, proc-macro-error, rustls-pemfile), cached
  advisory revision `faedffd5118c1835e13cca3babb6059afb1eb8d0`. This deliberately
  bounded cached check is not a fresh advisory/yanked-dependency qualification.
- `cargo deny --offline check`: failed advisories/licenses; no policy config
  exists, and default policy rejects the existing licence closure. Bans and
  sources passed. The external dependency closure is unchanged; no suppression,
  policy waiver or dependency update is included.

Missing full-workspace, native or release evidence is not inferred from these
results. This source-only change cannot qualify or install any package, approve
a live companion, or satisfy S0–S8 by itself.
