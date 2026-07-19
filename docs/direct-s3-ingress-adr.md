# ADR: DASObjectStore-owned direct S3 ingress

Status: Accepted, opt-in rollout

Date: 2026-07-19
Scope: standalone/appliance S3 ingress; existing Garage-backed objects remain compatible

## Context

The legacy remote-upload path accepts a complete object into Garage and later
reconciles that payload into the ObjectStore's managed SSD.  That is correct as
a compatibility and recovery path, but it writes a complete payload twice
before HDD settlement and makes `AfterSsdIngest` depend on a second transfer.

Garage does not expose a supported pre-persistence stream hook that can make
its payload write and DASObjectStore's catalogue, capacity, placement, and
destage transaction one authority.  Modifying Garage internals would couple
the storage safety boundary to an upstream implementation detail.

## Decision

DASObjectStore owns an S3 protocol gateway in front of Garage.  The gateway is
an explicit deployment mode, not an automatic upgrade:

- `garage_legacy` (default) leaves the established Garage endpoint and
  reconciliation workflow unchanged;
- `direct_gateway` binds the public S3 listener and moves Garage to a private
  loopback upstream port for legacy data and recovery operations.

The public endpoint remains AWS Signature Version 4 compatible.  Authentication
resolves the access key in the daemon-managed Garage credential registry and
binds it to exactly one `(ObjectStore, bucket)` pair.  A request cannot select a
store, profile, enclosure, managed root, or placement target.  Filesystem and
placement choices remain daemon-owned.

For a direct PUT, the gateway passes a bounded body stream over the provider
Unix-socket protocol.  The daemon resolves the current storage profile and
capacity authority, writes to a store-private temporary namespace on the
managed SSD, and calculates the size and SHA-256 digest while consuming the
stream.  Network transfer does not hold a SQLite write transaction.  Only the
short publication step serializes metadata.

Publication has these durable ordering rules:

1. validate content length, computed checksum, cancellation, credential scope,
   key safety, and held capacity;
2. synchronize the complete staged file and atomically publish it inside the
   same managed filesystem;
3. commit catalogue visibility, verified SSD placement, `remote_s3` origin,
   capacity settlement, acknowledgement state, and the durable HDD destage
   identity through the daemon's recoverable publication boundary;
4. return success for `AfterSsdIngest`; or continue waiting for the required
   verified HDD placement for `AfterHddPlacement`.

GET, HEAD, and list are catalogue-authoritative.  They do not use a Garage
provider listing to infer managed-object truth.  Existing Garage payloads are
still recoverable through `store repair --reconcile-s3`.

## Namespace and identity

An appliance or dedicated-drive profile receives a digest-derived,
store-private backend namespace beneath its authoritative managed root.  The
digest is an implementation namespace, not a user-facing identifier.  The
persisted profile binding is not rewritten as an artificial folder binding.

The direct-ingress journal identity includes store, credential scope, bucket,
key, version, a content-derived operation ID, and expected size.  The computed
checksum is persisted when the journal enters its verified state.  An exact
retry may replay durable publication without transferring the payload again. A
retry with different content is a conflict unless an explicit future
replacement/versioning policy permits it; accepted content is never silently
overwritten.

## Recovery and garbage collection

The daemon journal distinguishes receiving, verified, published, accepted, and
aborted states.  Startup recovery may finish or replay a verified publication;
it must not expose a merely partial file.  Garbage collection may reclaim
aborted or expired non-resumable temporary data only after journal and
catalogue checks prove that it is not an accepted placement.  Incomplete,
explicitly resumable multipart state is retained until its bounded expiry.

Reconciliation treats an already catalogued direct object as authoritative and
must not create a second managed copy.  Duplicate Garage payload removal is a
separate, dry-run-first operation performed only after catalogue and placement
durability are proven.  Enabling the gateway never deletes legacy Garage data.

## Compatibility and rollout

Existing store-scoped credentials, bucket names, AWS CLI path-style workflows,
and legacy reconciliation remain supported.  A rollout changes listener
ownership, not the public endpoint URL:

```text
before: client -> :3900 Garage -> later reconciliation -> managed SSD
after:  client -> :3900 DASObjectStore gateway -> managed SSD -> HDD destage
                            \\-> :3901 Garage (private legacy/recovery service)
```

Rollback returns listener ownership to Garage and sets `garage_legacy`.  Direct
objects remain ordinary catalogue-visible managed objects during rollback.
Garage reconciliation remains for objects that were actually accepted by the
legacy provider; it is not a substitute for direct-ingress publication.

## Rejected alternatives

- **HTTP success followed by asynchronous reconciliation:** still writes the
  complete payload twice and cannot satisfy `AfterSsdIngest` semantics.
- **Garage event after persistence:** observes the object too late to avoid the
  second complete payload.
- **Garage fork or storage-engine extension:** introduces an unnecessary
  upstream coupling and splits transactional authority.
- **Trusting a store or path supplied by the client:** violates tenant and
  managed-root isolation.

## Consequences

The direct path removes one complete pre-destage payload write and one complete
read.  DASObjectStore now owns more of the S3 protocol surface and must maintain
strict SigV4, URI canonicalization, multipart, backpressure, idempotency, and
S3-compatible error tests.  `garage_legacy` remains the safe default until the
appliance acceptance matrix in the operator runbook passes.

The detailed security and recovery review is in
[Direct S3 ingress threat and failure analysis](direct-s3-ingress-threat-model.md).
The appliance migration, rollback, and acceptance procedure is in the user
guide under `direct-s3-ingress`.
