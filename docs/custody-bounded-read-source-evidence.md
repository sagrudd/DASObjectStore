# Existing-only bounded custody read: source evidence

Source 0.185.0, accepted [ADR 0010](adr/0010-existing-only-bounded-custody-read.rst),
issue #207. Kanon #339 `f95174a` coordinates this minor; `0cea8fb` coordinates
the existing rusqlite 0.37 `hooks` feature. No dependency version or persisted
record changes. Historical read, retain and finite-batch APIs remain unchanged.

The new `custody::verify_custody_readback_existing` verifies a quiesced,
existing-only SQLite snapshot and invokes `BoundedCustodyObjectReader` with
the exact verified key, length and shared monotonic deadline. A read-only
transaction covers schema, configuration, complete event chain, persisted
receipt/current version and final bytes. The exact schema is obtained using
the historical initializer on a separate in-memory database, never migrated
onto the target. Strict no-sidecar and trusted parent-chain rules apply only
to this new API, not historical ordinary WAL readers.

SQLite lock conflicts deny immediately; its progress hook checks the deadline
every 1,000 VM operations, with a Rust-side check per event. These are bounded
cooperative checks, not hard real-time guarantees over filesystem I/O, allocation
or arbitrary computation inside a foreign adapter. No successful value is
returned after the shared deadline. No independent wallclock/attestation authority
is claimed by an `Instant` budget.

`GarageCustodyS3Reader::into_bounded` supplies a concrete Unix reader using the
already-bound in-memory identity/credential. It never consumes or reopens a
handoff. Exact expected-length memory allocation is fallible and occurs before
process effects. A private FIFO avoids unbounded regular-file body growth;
body and diagnostic drains are bounded and interleaved with deadline checks.
Raw diagnostics are not returned or logged. AWS environment fallback, retries,
pager and auto-prompt are suppressed; no shell is used by production code.

Child status is observed with `waitid(WNOWAIT)` without reaping the group leader.
The owned process group is terminated before the sole final `wait`, preserving
PID ownership through cleanup. This does not contain a malicious executable
that intentionally creates a new session/process group; trusted executable
provenance and external guest/cgroup containment remain separate requirements.
Scratch cleanup only removes the exact identity-checked FIFO and empty owned
directory. Replacement or extra content is preserved and causes denial.

## Actual selected AWS compatibility prerequisite

`tools/custody_aws_fifo_compatibility.py /opt/homebrew/bin/aws` passed locally:
AWS CLI `2.35.19`, Python `3.14.6`, Darwin `25.6.0`, arm64. Exactly one unsigned
GET to an owned disposable loopback HTTP fixture delivered 6,579 synthetic bytes
and preserved FIFO inode/type. No Authorization header, real endpoint or
credential source was used. Resolved launcher:
`/opt/homebrew/Cellar/awscli/2.35.19/libexec/bin/aws`, SHA-256
`ee7c97cdf151b81999a75dba41b1838d53657f72a4cdb9d8b89fa3fc7f43c2cd`.
That hash is the Python launcher, not a complete installed distribution hash.
This proves this selected local client's FIFO behavior, not deployment AWS,
native Garage, systemd, package provenance or authentication qualification.

## Focused evidence

- Seven independently authored lead-agent snapshot tests passed. They cover
  unchanged database/directory, invalid budgets/foreign reader, missing/alias/
  sidecars, unknown schema, wrong same-length bytes and mid-read replacement/
  sidecar appearance.
- Two implementation-author SQL tests passed: real recursive work interrupted
  by the production progress hook and actual exclusive-lock contention denied
  before GET without a busy wait. These are not independent authored tests.
- Six real local process/FIFO tests passed: exact bytes/cleanup, short/extra/
  large body, nonzero child, diagnostic floods, unopened/stalled FIFO, descendant
  held pipes with owned PID observed gone, replacement/cleanup failure,
  unreaped-leader ownership and never-created-FIFO cleanup.
- One joined test passed: actual retained catalogue/ledger, new snapshot
  verifier and concrete FIFO process reader, exact bytes and unchanged ledger.
  Its separate synthetic reader credential is test construction, not production
  reopening authority.

Final locked/offline daemon + object-service all-target/all-feature suite:
1,143 passed, no failures or ignored tests. Strict Clippy passed (40.15s);
warnings-denied private-item rustdoc passed (19.67s); formatting/diff checks
passed. Cached audit uses 1,240 advisories and retains the same four pre-existing
warnings (bincode, proc-macro-error, rustls-pemfile maintenance and chacha20
0.10.1 yanked). This is not a fresh advisory lookup. No repository cargo-deny
policy is present. Existing broad module-size baseline violations remain;
no waiver or unrelated refactor is added and no new module exceeds 1,000 lines.
No package was built/selected and no formal lockset or native acceptance is
inferred from source tests. Tests use only
unique owned local synthetic directories, removed afterward. No production
host, keys, credentials, services or objects were accessed. Endpoint/authentication,
credential continuation, lifecycle and off-NUC signing/measurement composition
remain separate contracts; this source does not make NUC custody eligible.
