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
- three distinct provisioner, writer, and reader credential references and
  sealed identities; and
- a fresh explicit bucket name.

The historical r237 bootstrap store
`r237_s4_bootstrap_custody` and bucket
`dos-r237-s4-bootstrap-custody` are permanently denied.  A bootstrap store,
existing bucket, existing ledger, ordinary registry record, or pre-existing
object is never adopted, upgraded in place, repaired, or replaced.

Normal `StoreServiceDefinition`, mutable registry, CLI fallback, compose
layout, and folder-profile binding have no custody-profile field at all; their
strict decoders reject a raw custody field rather than ignoring it. The only
admission shape is `CustodyStoreDefinitionV1`, handled by the preverified
daemon custody-admission route. It writes only the custody ledger and never a
normal registry, normal credential registry, capacity state, profile binding,
or folder backend. The dedicated plan instead names three distinct identities:

1. an attended provisioner used only to create a fresh bucket and issue
   grants;
2. a runtime writer with a write-only Garage grant; and
3. an independent runtime reader with a read-only Garage grant.

The concrete `GarageCustodyProvisioner` first requires Garage to report the
bucket missing, executes the dedicated key/import/create/grant plan without
accepting any idempotent conflict, and rereads Garage's key-grant table. It
fails closed unless that table contains exactly the sealed writer with `W` and
the sealed reader with `R`, with no owner grant or extra key. Its proof binds
the target, definition, three roles, request digest, absence/creation evidence,
nonce, and timestamp. The provisioner credential is not persisted.
`GarageCustodyS3Writer` uses conditional content-addressed S3 PUT and records
the exact local policy identity, non-shortening/delete prohibitions, hold
authority, and sealed retention timestamp as checked metadata; the distinct
`GarageCustodyS3Reader` performs HEAD and GET readback. Garage 2.3 has no
native S3 Object Lock, retention, or legal-hold API. Consequently those
metadata fields are *evidence checked against the sealed DAS ledger*, not a
claim of provider enforcement: a raw S3 object lacking them is a detected
trusted-administrator limitation and a custody failure, not a mutable
fallback or a COMPLIANCE/WORM claim. Neither
exposes a normal profile, copy, multipart, delete, restore, reconcile,
lifecycle, migration, or administrative route.

## Isolated custody plane and sealed admission catalog

Custody is not a protected flag on the normal Garage service. The daemon
requires a separately supplied custody Garage configuration before it will
admit a ledger, provision a fresh bucket, or retain an object. Its Compose
file, project directory and name, service name, configuration path, metadata
path, data path, and S3 endpoint must all differ from the normal Garage
plane. The normal service lifecycle and normal provisioner only use the
normal configuration; they have no custody-plane configuration or lifecycle
operation. Starting, stopping, or otherwise handing off the isolated custody
service remains an attended host-integration responsibility and is not a
normal DAS API operation in this source release.

Admission writes a daemon-owned canonical JSONL catalog outside the mutable
registry. Before it issues the first Garage command it atomically claims both
the `StoreId` and bucket name. The same daemon retains the resulting
fresh-bucket proof and claim in a one-way pending-admission slot; only that
exact proof can then create the ledger and append the catalog entry. A failed,
interrupted, restarted, or detached admission leaves a terminal claim: it is
neither released, adopted, deleted, nor retried. A completed entry binds the
definition digest, a daemon-derived opaque ledger path, the observed
sealed-ledger digest, and creation time. Normal store layout and normal
registry read/upsert/delete paths consult this catalog, so an ordinary store
cannot claim either the custody `StoreId` or the custody bucket under an alias.
Malformed catalog records, duplicate records, or dangling claims deny those
normal paths rather than being interpreted as absence.

An enabled custody plane must provide exactly one explicit canonical catalog
binding during daemon composition. That binding is injected into normal
registry reads/writes, normal layout, provisioning, and reconciliation guards;
the normal fallback is never inherited by an active custody plane. The path
resolver resolves an existing parent before a new claim, rejects caller
symlinks and normalisation ambiguity, and compares canonical identities. The
Garage authority comparator likewise canonicalises scheme, host/IP loopback
aliases, and port, so `localhost`, `127.0.0.1`, and `[::1]` cannot present one
plane as two endpoints.

This is the enforcement point for ordinary storage: normal definitions cannot
express the custody plane, ordinary mutable state cannot bind its endpoint or
bucket, and the central definition/registry guard denies catalogue aliases.
It is intentionally not a brittle assertion that every historical request
handler has independently rediscovered custody semantics. Any future route
that introduces a new normal store-definition or registry mutation boundary
must use the same central guard and is a review-required change.

The only daemon data mutation is `CustodyRetainRequest`. It contains two
opaque, distinct handoff references but no raw credential, catalog path, or
ledger path. The daemon consumes the writer and reader references once through
an attended host credential-authority boundary, bound to the catalogued store
and definition digest, before issuing the S3 commands. Its sole production
resolver reads an opaque file name from systemd's private
`CREDENTIALS_DIRECTORY`; it has no filesystem-path, registry, Keychain,
network, environment-secret, or API fallback. It atomically creates a
hash-only, empty one-use marker before reading the handoff. Thus a race,
restart, malformed handoff, or binding mismatch is terminal without
persisting raw credential material. The in-memory resolver remains solely for
regression tests.

The source includes, but packages do not install, a custody Garage Compose
template, custody service template, and systemd credential drop-in template.
They use the fixed distinct `dasobjectstore-custody` project,
`garage-custody` service, custody-only configuration/metadata/data paths, and
loopback `127.0.0.1:3901`. The packaged daemon configuration keeps custody
`enabled: false`; if an attended manifest enables it, daemon startup requires
the systemd credential directory and fails closed when it is absent. The
normal service lifecycle owns neither the template nor the custody service.
Rendering, installing, enabling, credential loading, or starting these assets
is a later attended formal transaction, not a package side effect.

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

The supported formal delivery boundary is the strict v2 off-NUC attestation
journal. It receives the exact raw JCS bytes of a signed pre-read request and
of the later signed attestation; whitespace, field substitution, unknown
fields, duplicate/noncanonical wire representations, an arbitrary digest
hook, and non-RFC4648 Base64 are rejected. One pinned Ed25519 public authority
is represented with its identifier, public key, and key digest. DAS never
generates, imports, stores, or discovers the private signing key.

Before any remote read, the off-NUC SQLite journal durably records a signed,
target-bound pre-read request and nonce. The request includes every expected
measurement: machine identity; endpoint authority, TLS and routing; reader;
store/bucket namespace; policy, ledger and ledger head; inventory and lockset;
verifier executable and provenance; receipt; release train/stage/purpose;
nonce, sequence/predecessor, issue time and expiry. Each request admits one
terminal attempt only. A failed, timed-out, incomplete, malformed, or invalidly
signed response consumes that request before a later response can replace it.
Only a signed `passed` observation with the exact repeated request, direct
readback, marker, receipt, and raw-evidence digests advances its monotonic
checkpoint.

The formal consumer works against that same durable journal, not against an
arbitrary attestation DTO. In one transaction it re-verifies both strict raw
JCS signatures and freshness, the complete exact request/measurement contract,
the passing first attempt, and atomically retains the unique request and
attestation identifiers, marker, raw-evidence, receipt, policy, ledger, and
raw-record digests. Reuse and crash-partial state are terminal. The legacy v1
observation DTO is not formal-gate authority.

The older source observation shape is retained only for compatibility tests;
it is not a formal approval mechanism. An accepted v2 attestation must include:

- a pinned authority signature, verifier ID, unique nonce, strictly increasing
  sequence, prior-attestation hash, issue time and expiry;
- the receipt and ledger head;
- the DAS executable digest, Garage image digest, Garage configuration digest,
  and S3 endpoint observed by the verifier; and
- a full inventory digest and dedicated custody-marker digest.

It also includes a required attestation identifier, release train, stage,
purpose, and raw-evidence digest. The off-NUC state records each first attempt
as accepted, failed, timed out, or incomplete evidence; accepted state advance,
nonce consumption, and its accepted first-attempt record must occur in one
atomic transaction. A retry cannot erase the first observation. The formal
gate consumes the identifier, one-use marker digest, and raw-evidence digest
together through an atomic external marker. Reuse, mismatch, and a
crash-partial marker are terminal failures.

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
mutation bypasses. They cover immutable store and bucket claims, incomplete
admission denial, malformed/duplicate catalogue fail-closed behaviour,
claim-before-provision race denial, strict secret-free daemon transport,
one-use systemd credential handoffs with hash-only markers, absence/default
denial, and rejection of a custody configuration that shares any normal Garage
coordinate. The daemon
integration contract covers admission, catalog/ledger binding, isolated S3
endpoint selection, conditional PUT, policy-metadata/readback divergence,
writer HEAD, independent reader GET, and one-use handoff consumption. They
also cover raw-JCS Ed25519 authority binding, nonce issuance, one-terminal
attempt consumption, timeout/replacement denial, monotonic sequence,
previous-hash continuity, expiry, full-measurement substitution, and atomic
formal consumption.

The deployment/integration suite still required before a candidate can be
considered must exercise a real isolated Garage service, process/credential
separation, crash/restart/partial-write and concurrent-writer races, raw S3
substitution/tampering, stale/replay/rollback attacks, backup restore,
authority sharing, and a negative administrative test.  The negative test is
important: it demonstrates why the release must retain the honest local
trusted-administrator label.
