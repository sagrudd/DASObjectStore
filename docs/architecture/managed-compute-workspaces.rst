Managed compute workspaces
==========================

Status
------

This document fixes the production architecture for first-class mutable compute
workspaces. The implementation is intentionally delivered in dependency order.
Schema version 1 and live-metadata schema 0.9 establish the domain and durable
boundaries. The metadata repositories now reserve aggregate capacity
atomically, expose path-redacted inspection, and durably coordinate workspace
operations, but the host provisioning provider is not yet enabled and the
system does not advertise a usable workspace.

Authority boundary
------------------

An immutable ObjectStore contains checksum-bound, governed objects and durable
placements. A compute workspace is a leased, quota-bound mutable filesystem.
Workspace files never become catalogue objects merely because they exist.
Promotion is the only transition from mutable workspace content into the normal
immutable ingest, acknowledgement, catalogue, and destage pipeline.

Ordinary clients receive logical workspace, operation, object, and export
identities. They never receive disk roots, placement paths, private branch
paths, provider command lines, or host credentials.

Lifecycle
---------

The workspace state machine is explicit::

  Requested -> CapacityReserved -> Provisioning -> Ready
  Ready -> Attached -> Active -> Ready
  Ready|Attached|Active -> PromotionPending -> Ready
  Ready|Attached -> Closing -> Closed -> CleanupPending -> Cleaned
  CapacityReserved|Provisioning|Ready|Attached|Active|PromotionPending
      -> Expired -> CleanupPending

A non-terminal workspace may enter ``Failed`` with a typed reason. Invalid
transitions fail before mutation. State mutations use a monotonically
increasing generation so concurrent operations can compare and swap.

Capacity authority
------------------

Workspace capacity is an aggregate physical reservation, not a logical
ObjectStore quota. Creation measures candidate disks outside a write
transaction, then uses ``BEGIN IMMEDIATE`` to:

* replay an identical request identity or reject a conflicting digest;
* recheck pool and disk eligibility;
* subtract minimum-free policy and all active physical claims;
* select healthy, writable, non-draining disks deterministically;
* insert the workspace and every per-disk allocation atomically.

The deterministic initial policy consumes the largest eligible fractional free
capacity first with a stable disk-ID tie-break. It can satisfy a request larger
than any single disk. Filesystem provisioning happens after commit and must not
hold a SQLite transaction.

Live-metadata schema 0.8 provides ``disk_capacity_claims`` as the shared
authority for workspace reservations and short-lived ingest, destage, repair,
and evacuation writes. Outstanding capacity is ``reserved - consumed`` so
physical free-space measurements do not double-count bytes already written.
Claim acquisition uses an immediate transaction, disk-state revalidation, and
request identity conflict protection. Immutable ingest releases only after its
object metadata commit; destage releases atomically with verified placement
promotion; interrupted or ambiguous writes retain their claims for safe retry.

Provider and privilege boundaries
---------------------------------

Aggregation and export are separate provider contracts:

``WorkspaceAggregationProvider``
  Validate, provision, inspect, recover, and unmount an aggregate namespace.

``WorkspaceExportProvider``
  Publish, inspect, reconcile, and revoke a client-scoped export.

The first Linux aggregation provider is mergerfs. Its reviewed profiles use an
explicit create policy, minimum-free floor, ``inodecalc=path-hash``,
``never-forget-nodes=true``, permissions enforcement, and version-gated
``moveonenospc`` behavior. Cache options are selected from the detected kernel
and mergerfs versions: modern kernels may use ``cache.files=off``; older
mmap-capable combinations require a reviewed ``auto-full`` profile. Removed
legacy options such as ``use_ino`` are never emitted. Production quota
enforcement requires project quotas on every selected filesystem; mergerfs
accounting alone is not a hard quota.

The main daemon remains unprivileged. The root-owned, socket-activated
``dasobjectstore-workspace-host`` broker accepts only
versioned typed operations for managed branches, project quotas, aggregate
mounts, and DASObjectStore-owned NFS fragments. Its first delivered protocol
supports branch provision, recovery inspection, and rollback. Disk roots come
only from root-owned configuration; callers cannot submit commands or paths.
Project identities are allocated transactionally and per-branch hard quotas sum
to the workspace logical quota. Exact marker and quota replay is idempotent.
Rollback removes only empty branches carrying the exact ownership marker.
This preserves the daemon's ``NoNewPrivileges`` and ``ProtectSystem``
protections. Protocol v2 adds aggregate mount, inspection, and unmount without
adding a general command or path surface. The broker derives every branch and
mount path from root-owned configuration, assigns a workspace-specific FUSE
identity, and verifies the live mergerfs process command line against the exact
branch set and reviewed option profile. A workspace cannot become ``ready``
until that evidence and every branch quota are simultaneously valid.

NFSv4 attachment
----------------

Only the configured aggregate
``/srv/dasobjectstore/workspaces/<workspace-id>`` is exported. Client addresses
are parsed from an administrator-owned, root-owned broker registry; wildcard
and public exports are invalid. Protocol v3 accepts only the opaque workspace
identity, registered client identity, and read-only/read-write mode. It does
not accept a path, network address, or arbitrary export option.
``root_squash`` is mandatory in the delivered provider; there is no
``no_root_squash`` request surface.

Each workspace/client attachment owns one exports.d fragment so independently
registered compute hosts can be revoked without rewriting another attachment.
Publication is atomic and idempotent, reload failure restores the prior
fragment, and unrelated administrator exports are never rewritten. The
attachment response contains
only the server, export path, NFS version, access mode, recommended mount
options, and workspace identity. DASObjectStore never remotely mounts the
compute host.

Attachment intent is durable in live SQLite. On daemon restart, requested,
attached, and detach-requested rows are independently reconciled through
broker inspection. The broker resolves the current registered client address
and the daemon records that non-secret evidence. A missing aggregate, changed
fragment, symlink, client-registry conflict, or reload failure is retained as
``needs_review`` rather than rewritten or treated as transient success.

NFS over FUSE is treated as a constrained provider combination. Inputs are
materialized before attachment, daemon-side namespace mutation is forbidden
while attached, and promotion requires quiesced stable output. Missing or
changed branches revoke the export. Container UID/GID behavior under
``root_squash`` must pass the privileged acceptance test.

Durable operations and recovery
-------------------------------

The existing summary-only administrator job registry is not authoritative for
workspace work because it marks active jobs failed at daemon restart. The
live-metadata 0.9 operation repository is authoritative for workspace
provisioning, materialization, promotion, and cleanup. Operations carry a
request identity and digest, bounded attempt count, renewable lease, lease
epoch, monotonically increasing generation, stage, byte/unit progress,
cancellation request, typed failure, path-free result, and completion time.

Workers claim operations under ``BEGIN IMMEDIATE``. Every renewal, checkpoint,
cancellation, and completion is fenced by generation and, for worker actions,
lease owner. Checkpoints are append-only, size-bounded, path- and
secret-rejecting JSON records committed atomically with monotonic summary
progress. Exact checkpoint and terminal-result retries are idempotent.

On restart, an unexpired lease remains authoritative. An expired lease is
returned to the queue only when the current stage explicitly declares
idempotent replay, or an append-only checkpoint proves resumability. Cancelled,
attempt-exhausted, malformed, or externally ambiguous work enters
``needs_review`` without silently replaying a host mutation. Recovery examines
metadata only; hashing and provider inspection remain outside SQLite write
transactions.

Materialization resolves a verified placement internally, copies into a
daemon-owned partial, checkpoints progress, verifies size and SHA-256, fsyncs,
and atomically publishes the destination. Promotion securely opens a bounded
workspace-relative file, hashes it, validates lineage, and hands one
deterministic job to the existing SSD-first immutable ingest pipeline. Bundle
promotion is complete only when all required members have authoritative
acceptance evidence.

Restart reconciliation never selects replacement disks silently and never
releases a reservation while a branch may contain files. It validates branch
ownership markers, provider-plan digests, mounts, exports, and operation leases.
Ambiguity fails closed and is reported by a dry-run repair command.

Closure and cleanup
-------------------

Closure refuses active work and missing required promotions, records final
accounting, and revokes exports before cleanup eligibility. Cleanup removes only
marker-owned branches for the exact workspace, proves that no immutable
placement or other workspace is in scope, and releases reservations only after
branch removal is proven. Automatic expiry remains report-only until policy
explicitly enables application.

Delivery and acceptance
-----------------------

The ordered implementation sequence is:

#. domain, schema, aggregate reservation, read-only inspection, shared physical
   claims, and the durable operation/recovery repository (delivered);
#. branch provisioning, rollback, and provider-state restart reconciliation;
#. privileged broker, mergerfs provider, hard quotas, and readiness;
#. NFSv4 attach/detach and client isolation;
#. durable verified materialization;
#. bounded checkpoint inventory and capacity/health reporting;
#. verified single and bundle promotion with lineage;
#. closure, expiry, cleanup, audit, and repair;
#. CLI/API/authentication contracts and opt-in Linux loopback/NFS acceptance;
#. synthetic end-to-end acceptance, then the governed AlleleAnchor HG002 case.

The privileged Linux test uses only synthetic loopback filesystems and is
explicitly gated. Held-out HG005 data is outside acceptance scope.
