# Finite custody batch: source evidence

Issue [#205](https://github.com/sagrudd/DASObjectStore/issues/205), accepted
[ADR 0009](adr/0009-finite-inventory-custody-session.rst), source coordination
Kanon #339 `930e58f`; proposed source version 0.184.0.

`CustodyServiceController::retain_custody_inventory` connects the existing
catalogue and ledger to the actual Garage conditional writer and independent
reader adapters. It validates the entire explicitly selected inventory before
consuming either sealed handoff. Both credentials are scoped to one synchronous
call and dropped afterward. There is no externally reusable session object.

The library inventory is service-composition data, not owner admission. No
transport accepts expected inventory or policy authority. Existing single-object
API and persisted formats are unchanged. Planner hashes retain `sha256:`;
library inventory hashes are bare hex, matching custody receipts. Aggregate
size overflow now denies in shared validation. Input timestamp validation
retains existing semantics; it is not independent wallclock freshness.

Tests use actual temporary catalogue/ledger and systemd-handoff consumption
files with synthetic credential strings and a synthetic command runner.
They do not run a systemd unit or connect to Garage. This is neither native
runtime/package qualification nor proof of ordinary-plane isolation.

Validation on macOS, locked/offline, 2026-09-08:

- Daemon and object-service all-target/all-feature tests: 1,127 passed, no
  failures or ignored tests. This includes seven connected batch tests and
  five independently authored pure inventory/planner tests. Optional
  environment-dependent existing integration checks are not native runtime
  qualification merely because their test result is successful.
- Strict Clippy for those crates/all targets/all features: passed (8.55s).
- Rustdoc with warnings denied, all features/private items: passed (18.78s).
- Formatting and diff checks: passed.
- Module-size guard: failed with existing baseline violations independently
  reproduced on clean extraction `26e375ca`. No new batch module exceeds its
  limit; no exception or unrelated refactor was added.
- Cached `cargo audit --no-fetch`: 1,240 advisory records, 464 dependencies;
  existing warnings for unmaintained bincode, proc-macro-error, rustls-pemfile,
  and yanked chacha20 0.10.1 remain. This is not a fresh advisory lookup.
  No third-party dependency changed; lock changes are only workspace versions.
- No repository cargo-deny policy is present. No package was built/selected,
  and no package provenance, native Garage or systemd acceptance is claimed.

The real-filesystem tests prove consumed-reference reconstruction denial and
four concurrent in-process calls with one successful batch; they do not claim
multi-process native service qualification. The fault matrix injects put,
post-put HEAD and readback failure at every object position, and a genuine
SQLite trigger abort after the second object's readback. The latter is a
database append failure, not a simulated physical disk-full or power-loss test.
No process-crash, fsync-failure or live storage-availability result is claimed.

This capability alone does not make NUC custody eligible. It does not supply
reader continuation, a custody read endpoint, lifecycle containment, off-NUC
verification or an admitted execution companion. Partial failure preserves
already retained objects and consumed handoffs, including an object whose
backend write succeeded but whose ledger append failed; no retry, deletion,
orphan adoption or new credential issuance is performed.
