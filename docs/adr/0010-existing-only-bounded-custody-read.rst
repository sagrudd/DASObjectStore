Existing-only verification and bounded custody reads
===================================================

Status: ACCEPTED bounded source design. Lead-agent independent review of exact
proposal commit ``567ca93ee5c41d689224b66df0e4be159a10ecd7`` and raw ADR hash
``1fe78fbcd7e6d68787196daf90d376059e02768cb462d310315a32aab3f54e37``
is recorded in review comment 5584210018. This is delegated source-work
acceptance, not a personal owner signature or host execution/custody eligibility.
The prerequisite local AWS FIFO compatibility test passed; its narrow evidence
is retained separately. Kanon #339 ``f95174a`` coordinates source 0.185.0.

Date: 2026-09-08. Decision owner: project owner with delegated source review.
Issue: https://github.com/sagrudd/DASObjectStore/issues/207.
Baseline: ``c577dec41b92830a92977d101805a92817fe5777`` (DAS #206).
Implementation is additive source minor 0.185.0. Existing rusqlite 0.37 gains
the Kanon-coordinated ``hooks`` feature (``0cea8fb``) for cooperative SQLite
deadline interruption; there is no dependency version change.

Context and scope
-----------------

The existing ``verify_custody_readback_receipt`` uses ``open_ledger`` with
``SQLITE_OPEN_READ_WRITE`` after reading configuration on another connection.
``GarageCustodyS3Reader::read_exact`` runs AWS GET into a scratch file then
uses unbounded ``fs::read``. ``SystemServiceCommandRunner::output`` buffers
unbounded diagnostics. Its cancellable variant does not drain pipes until
process exit and is not a sufficient bounded-output implementation.

Reuse the existing receipt, sealed-configuration, event-chain and object
verification algorithms, but compose them into a coherent existing-only
verification operation and a genuinely bounded acquisition path. Do not
expose a new endpoint, select authentication, retain/reopen credentials,
alter the sealed profile, add a journal or claim reader continuation.

The programme custody bootstrap design and local trusted-administrator class
remain controlling. Read availability, independently admitted companion,
actual verifier/signing composition and isolation are not supplied here.

Proposed library contract
-------------------------

Add an operation alongside existing custody verification, with concrete public
types and no serialized protocol::

    CustodyReadLimits { maximum_bytes: u64, timeout: Duration }
    VerifiedCustodyRead {
        receipt: CustodyIntegrityReceiptV1,
        configuration_sha256: String,
        ledger_head_sha256: String,
        bytes: Vec<u8>,
    }
    verify_custody_readback_existing(
        ledger: &Path,
        expected: &CustodyIntegrityReceiptV1,
        reader: &mut impl BoundedCustodyObjectReader,
        limits: CustodyReadLimits,
    ) -> Result<VerifiedCustodyRead, CustodyReadError>

The new reader capability receives only the content-addressed key and exact
length selected from the verified ledger receipt, plus the remaining deadline.
Its concrete Garage implementation uses the already bound endpoint, bucket,
identity and already supplied in-memory reader credential. It never resolves
a handoff or changes its lifetime. No caller policy, URL, command, credential
or retention override appears in the verification request.

``maximum_bytes`` is an explicit service-composition resource budget, not a
new custody-content policy: nonzero, representable by usize/isize, and at least
the verified expected length. The allocation uses fallible reservation and
checked arithmetic. The whole-call timeout is nonzero and at most 300 seconds
(a proposed source safety ceiling, not an existing approved execution window).
Actual deployment may choose a smaller bound. Existing planner's 1 MiB JSON
limit is not an object-byte limit. No payload is allocated or backend invoked
when bounds or ledger preconditions fail.

Existing-only database boundary
-------------------------------

Require an absolute canonical service-selected path and a trusted,
non-symlink parent chain. Reject symlink/nonregular/missing database,
pre-existing ``-wal``, ``-shm`` or ``-journal`` files, and unsafe writable
parent permissions before SQLite access. The new path explicitly supports
only quiesced rollback-journal custody ledgers; do not impose this policy on
historical ordinary openers. No create, migration, checkpoint, recovery,
journal-mode setting, backup or repair is permitted.

Use SQLite READ_ONLY, connection-local ``query_only`` and ``trusted_schema``
restrictions, no immutable-URI shortcut that ignores concurrent changes, and
one read transaction for configuration, exact persisted receipt, current
version, complete event-chain validation and ledger head. Factor helpers to
accept this connection instead of independently reopening files. Validate the
expected existing schema and reject unknown triggers/views or altered critical
DDL before reading trusted evidence; preserve the existing immutable triggers.
Freeze the expected schema from the actual historical creation routine in
implementation tests, not a newly invented on-disk schema.

Hold the read transaction while acquiring and verifying the bytes. This binds
the result to one ledger snapshot and excludes conflicting commits in the
supported rollback-journal mode. A busy acquisition, sidecar appearance,
replacement, detectable database/path identity change or deadline expiration
denies the result. Recheck protected path identity and sidecar absence before
returning. Do not silently reopen/retry on conflict. All receipt/configuration/
policy comparisons and the final byte digest use the existing algorithms.

Path checks are not proof of a descriptor-relative SQLite open against a
malicious trusted owner swapping away and back. The supported threat boundary
requires trusted ownership/quiescence. Tests must prove detection of retained
replacement and conflict; documentation must not claim arbitrary-swap immunity.
Returned head is the verified snapshot head, not independent off-NUC currentness
or actual observation-time authority. No failed operation returns partial
bytes or a ``VerifiedCustodyRead`` value.

Bounded concrete acquisition
----------------------------

Do not claim that polling the length of a regular scratch file bounds writes:
a backend can exceed the limit between polls. Do not reuse the old generic
command runner's unbounded output capture. Add a narrow Unix concrete bounded
GET runner, with explicit owned descriptors and fixed argument construction.
Legacy runner/reader APIs remain unchanged.

Proposed mechanism: a create-new, mode-0700 private scratch directory and
mode-0600 FIFO used solely as the AWS GET output path, rather than a growing
regular file. The supervisor exclusively owns the nonblocking read descriptor,
starts the direct AWS executable without a shell, and concurrently drains
body/stdout/stderr. The body retains no more than the verified expected length;
one additional byte is detected as an overrun and discarded before denial.
Bound command diagnostics to 64 KiB each; an excess denies, never logs raw
diagnostics. Use a monotonic whole-operation deadline, not only a socket timeout.

The direct process starts in its own process group. It remains unreaped while
status is observed through ``waitid(WNOWAIT)``, so its PID/group identity cannot
be reused before group termination and the final wait. This owns that process
group, not descendants deliberately escaping it through a new session;
trusted executable provenance and external containment remain required.
On timeout, overrun,
malformed output, read failure or cancellation, terminate the owned group,
reap the direct child, close descriptors and discard retained bytes. Require
successful child termination, exact body length and successful byte digest
verification before success. No retry or range fallback. The FIFO permits no
unbounded disk payload. Do not interpret a temporary FIFO EOF before writer
open as a completed response; completion requires the process outcome.

Only the exact bound credential environment is forwarded; suppress inherited
AWS profile/config/metadata fallback, pager, CLI auto-prompt and automatic
request retries. Select the executable from trusted service configuration, not
the untrusted request. Its provenance remains a deployment input, not proven
by a path string. Existing S3 GET/receipt semantics are retained; this is not
a new wire signing protocol or direct Garage access for an outside verifier.

All supported outcomes remove only this operation's owned FIFO/scratch
directory, never a ledger/object/catalogue/claim. Scratch cleanup failure is
reported as failure and cannot authorize broader cleanup. SIGKILL/power loss
may leave private scratch: do not adopt or silently reuse it. A hung kernel or
host failure cannot be represented as a hard real-time guarantee; external
containment remains separate. Non-Unix has an explicit unsupported result
before effects, not a weaker fallback.

Compatibility and alternatives
------------------------------

Add new types/methods; retain historical single-object and batch behavior and
all persisted formats. The bounded reader is a concrete companion to the
existing Garage adapter, not an always-unavailable production stub. Reuse
existing workspace dependencies where feasible; any required Unix feature
addition must be coordinated before implementation. No new binary/unit/stable
identifier, transport route or deployment path is selected by this ADR.

Rejected: unbounded GET then length check; regular-file polling as a hard disk
cap; buffering ``Command::output``; a reader callback asserted as independent
verification; silently reopening consumed credentials; modifying ordinary
WAL readers globally. This slice returns locally verified bytes, not a signed
off-NUC attestation or persistent reader service.

Validation and fault matrix
---------------------------

Use real temporary SQLite ledgers and a real local child/FIFO fixture. All
process tests use synthetic bytes and no credentials/network/service activation.

* Positive genuine retained receipt and exact bytes; one snapshot's full chain,
  policy, configuration and head; byte-for-byte unchanged database and directory
  inventory before/after verification. Legacy tests remain passing.
* Missing database/parent, symlink/database/parent alias, unsafe permissions,
  each sidecar, malformed schema/unknown trigger, corrupt configuration/event/
  receipt, substituted receipt/version/key/reader identity: no GET, no database
  creation/recovery/writes, no partial verified value.
* Competing writer transaction, busy lock, persistent inode/parent replacement
  and sidecar creation before/during acquisition: deny without retry or repair.
  Deterministic synchronization must prove the contested boundary was reached.
* Empty/short/exact/one-extra/large-overrun body, wrong digest, zero/overflow/
  undersized budgets, invalid timeout: prove byte and allocation bounds and
  exact error phase. Large overrun must not create a growing regular file.
* Child never opens FIFO, opens but stalls, exits before bytes, exits nonzero
  after exact bytes, floods stdout/stderr, leaves descendant descriptors open,
  or ignores termination: deadline path kills owned processes and never passes
  partial evidence. Verify process/descriptor cleanup, not merely return code.
* FIFO/path substitution and scratch cleanup failure remain contained; no
  unrelated path is removed. Parent environment/profile poisoning does not
  influence command credentials/endpoint. No diagnostics expose fixture secrets.
* Relevant full tests, strict Clippy/rustdoc/fmt, compatibility/lock checks;
  native Unix evidence clearly separated from synthetic process tests. No
  real Garage/systemd/package acceptance is inferred.

Independent review must accept the above concrete limit/process/filesystem
choices before implementation. Endpoint authentication/framing, protected
reader reopening, lifecycle and off-NUC signer/measurement composition remain
separate dependent contracts, not implicit additions to this proposal.
