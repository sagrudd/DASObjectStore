# Local trusted-administrator custody overlay

## Status and scope

This is the source contract for DASObjectStore 0.180.0.  It introduces a
small custody-retention overlay for a **fresh, dedicated Garage bucket** on a
locally administered NUC.  It is not a deployment guide, package, host
transaction, S4–S8 result, or authorisation to contact a NUC, DGX, Docker,
Garage, or any S3 endpoint.

The only supported assurance label is
`local_trusted_administrator_overlay`.  It means that DASObjectStore enforces
append-only retention through its supported APIs, while the release owner
explicitly accepts the NUC administrator boundary.  It must never be
described as independently administered storage, provider-enforced retention,
or a regulatory custody result.

In particular, a person able to administer the NUC can alter the Garage
configuration or data, its Docker service, DAS binary or configuration, local
clock, ledger disk, access keys, or a backup and restore path.  The overlay
does not claim to withstand that person.  Independent review detects ordinary
drift and tampering only to the extent that its monotonic state is kept outside
those same administrative and backup domains.

## Sealed store profile

`CustodyStoreProfileV1` is target-bound and requires all of the following:

- the fixed `local_trusted_administrator_overlay` assurance and retention
  mode;
- an explicit UTC retention-until timestamp and a permanent legal hold;
- different writer and reader credential references, plus a sealed reader
  identity; and
- a fresh explicit bucket name.

The historical r237 bootstrap store
`r237_s4_bootstrap_custody` and bucket
`dos-r237-s4-bootstrap-custody` are permanently denied.  A bootstrap store,
existing bucket, existing ledger, ordinary registry record, or pre-existing
object is never adopted, upgraded in place, repaired, or replaced.

The mutable normal store registry and ordinary layout/provisioning path reject
a custody profile before issuing their usual per-store owner-capable Garage
credential.  The dedicated plan instead names three distinct identities:

1. an attended provisioner used only to create a fresh bucket and issue
   grants;
2. a runtime writer with a write-only Garage grant; and
3. an independent runtime reader with a read-only Garage grant.

The plan neither emits nor persists the provisioner credential, and contains
no owner, list, delete, copy, lifecycle, or runtime administrative grant.
The implementation of the attended plan must additionally prove that no DGX
or user S3 credential grants access to the custody bucket.

## Durable record and allowed operations

The custody ledger is a new private SQLite database, outside the ordinary
mutable store registry and BaseCamp metadata.  Creation is `create_new` only,
requires an already-existing parent directory, syncs the new file, and seals
a canonical-JCS configuration digest.  A replacement database is a terminal
failure, not a migration path.

The ledger records a content-addressed object ID and key
`custody/sha256/<sha256>`, length, type, version, retention-until timestamp,
legal hold, canonical event body, previous event hash, event hash, and a
reader readback receipt.  SQLite triggers prohibit updates and deletes for
configuration, events, object versions, and receipts.  Every accepted record
is one entry in a monotonically sequenced JCS hash chain.

Only two mutating operations exist in the source contract:

- retain a new content-addressed object through a create-if-absent writer
  after the separate reader re-reads the exact bytes and recomputes SHA-256
  and length; and
- append a later retention version after another independent readback.

The source exposes no delete, overwrite, copy, multipart upload, restore,
reconcile, lifecycle, retention shortening, legal-hold clearing, configuration
replacement, ledger reinitialisation, or administrative bypass operation.
Every future adapter for one of those backend paths must consult this same
sealed state and fail closed; it may not add a parallel mutation route.

If a backend write completes before the ledger commits, the result is an
unledgered orphan.  It is intentionally refused rather than silently adopted,
deleted, or repaired.  Exact idempotency is allowed only for an already
ledgered object with an exact immutable receipt.

## Independent readback and off-NUC continuity

`CustodyObjectReader` is deliberately a separate capability from
`CustodyObjectWriter`.  Retention and later extension require the reader
identity sealed in the profile; a writer response alone cannot create a
receipt.  Readback recomputes content hash and size, then binds the result to
the configuration digest and ledger event hash.

The next delivery boundary is an off-NUC attestation authority.  The source
defines, but does not configure or host, its public verification interface and
an external monotonic-state interface.  An accepted attestation must include:

- a pinned authority signature, verifier ID, unique nonce, strictly increasing
  sequence, prior-attestation hash, issue time and expiry;
- the receipt and ledger head;
- the DAS executable digest, Garage image digest, Garage configuration digest,
  and S3 endpoint observed by the verifier; and
- a full inventory digest and dedicated custody-marker digest.

The verifier state must be held outside the NUC, Garage, BaseCamp, and their
ordinary backup/restore paths, and must atomically compare-and-store the prior
checkpoint.  Replayed nonce, expired observation, wrong target or authority,
signature substitution, stale sequence, and previous-hash discontinuity are
all rejected before its state changes.

The marker, builder corpus, delivery corpus, terminal receipts, and verifier
checkpoint require distinct stores and authority paths.  They must not share a
Garage bucket or credential and must not be treated as retained merely because
they appear in the ordinary mutable registry or a backup.

## Formal release consequences

This source release does not reinterpret existing r237 records.  The r237/r7
`dasobjectstore-remote` 0.177.4 selection is immutable and untouched.  Any
future candidate using this overlay needs a new DASObjectStore 0.180.0 Kanon
profile and lock, explicit Terraform projection, package provenance, and a
programme-SOP amendment that records the local trusted-administrator assurance
model and its exclusions.  It needs a separately target-bound S8 approval
before any install or activation.

The SOP must reject a claim of independent or provider-enforced custody for a
local Garage overlay.  If that assurance is not accepted by the responsible
owner, formal progression remains denied regardless of application tests.

## Regression coverage

The source tests cover fresh-only ledger creation, redaction, distinct
provisioner/writer/reader identities, absence of owner grants, canonical
timestamps, legal-hold requirement, no-side-effect inspection, bootstrap
namespace denial, immutable SQLite rows, hash-chain/readback verification,
unledgered-object denial, later-only retention extension, and all named
mutation bypasses.  They also cover off-NUC signature, nonce, monotonic
sequence, previous hash, expiry, and compare-and-store semantics.

The deployment/integration suite still required before a candidate can be
considered must exercise a real isolated Garage service, process/credential
separation, crash/restart/partial-write and concurrent-writer races, raw S3
substitution/tampering, stale/replay/rollback attacks, backup restore,
authority sharing, and a negative administrative test.  The negative test is
important: it demonstrates why the release must retain the honest local
trusted-administrator label.
