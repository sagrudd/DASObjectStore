# DASObjectStore Product Requirements

Status: Draft
Baseline date: 2026-06-23
Campaign re-baseline: 2026-07-12
Scope: common managed object storage for local and appliance deployments

## 1. Product Position

DASObjectStore SHALL provide a common managed object, manifest, ingress,
capacity, lifecycle, and S3 contract across bounded local-folder, dedicated-SSD
drive, and tiered DAS-appliance deployment profiles.

DASObjectStore SHALL be implemented primarily in Rust.

DASObjectStore SHALL be useful to non-Mnemosyne users.

DASObjectStore SHALL be a server/client application. The managed service
boundary is foundational, not optional polish.

DASObjectStore SHALL provide a daemon, `dasobjectstored`, as the managed
storage authority for normal operation.

DASObjectStore SHALL treat the `dasobjectstore` CLI as a client to the daemon
for normal storage-mutating workflows.

Bringing DASObjectStore under the Synoptikon umbrella as a formal Mnemosyne
product/plugin SHALL be the priority integration path. The standalone monolith
SHALL reuse the same domain model and SHALL NOT fork into a separate
non-Mnemosyne architecture.

DASObjectStore SHALL NOT require equal-size disks.

DASObjectStore SHALL NOT require classic RAID.

DASObjectStore SHALL NOT claim to be a backup solution.

## 2. Core Use Cases

DASObjectStore SHALL support these primary use cases:

- expose a size-bounded local folder as a managed hierarchical ObjectStore;
- expose a dedicated SSD filesystem as a monitored single-node ObjectStore;
- reuse older HDDs in USB-C DAS enclosures;
- apply profile-appropriate managed ingress with durable atomic finalization;
- settle verified object copies onto heterogeneous HDD members;
- expose object storage through an S3-compatible interface;
- provide CLI and Web UI health visibility;
- run storage-mutating operations through a managed daemon/client boundary;
- support portable movement of the DAS between hosts;
- support incremental addition and retirement of disks;
- support per-store redundancy and retention policy;
- export storage configuration for Mnemosyne/Mneion integration;
- run as a formal Synoptikon product/plugin and as a standalone HTTPS
  application.

## 3. Non-Goals for MVP

The MVP SHALL NOT include:

- read/write SMB or NFS as a primary write path;
- fine-grained HDD physical zone placement;
- a custom S3 implementation written by DASObjectStore;
- production backup claims;
- DASObjectStore-native parity or erasure coding before object-copy policies
  have shipped.

## 4. Platform Requirements

DASObjectStore SHALL provide full MVP support on Linux first.

DASObjectStore SHALL provide macOS beta support for:

- pool detection;
- metadata inspection;
- health summary;
- settled object read/export;
- read-only SMB/S3 export where feasible.

macOS support boundaries are documented in
[macOS Development and Read/Export Notes](macos-development.md).

DASObjectStore SHALL use Docker/Compose as the default deployment path for
supporting services.

DASObjectStore MAY support native/systemd service management where appropriate.

## 5. Storage Profiles and Hardware Model

DASObjectStore SHALL model deployment profile separately from host authority.
Profiles SHALL initially include:

- ``folder``: one finite-capacity managed directory;
- ``drive``: one dedicated validated SSD filesystem;
- ``appliance``: SSD landing plus managed heterogeneous HDD capacity.

Host authority SHALL support a system daemon and a future per-user daemon with
user-owned XDG state/runtime paths. Products SHALL discover capabilities rather
than infer appliance behavior from profile names.

DASObjectStore appliance mode SHALL model these disk roles:

- mandatory SSD ingest disk;
- HDD capacity member;
- replacement disk;
- retired disk.

DASObjectStore SHALL support mixed HDD sizes and mixed performance classes.

DASObjectStore SHALL identify disks using composite identity:

- DASObjectStore disk UUID;
- observed hardware serials where available;
- enclosure hints;
- bay hints where available;
- size;
- partition and filesystem fingerprints.

DASObjectStore SHALL model enclosures using best-effort USB topology inference plus
user confirmation and naming.

DASObjectStore SHALL treat platform probe data as observed hints. USB bridge and
SMART limitations SHALL be visible to users and are documented in
[Platform Probing Notes](probing.md).

## 6. Portability Requirements

DASObjectStore SHALL NOT rely on hidden host-local state as the sole authority for a
pool.

DASObjectStore SHALL store live metadata on the profile's authoritative local
metadata tier. Appliance mode uses the managed SSD; folder and drive modes use a
protected metadata namespace on the same managed filesystem unless an approved
external metadata policy applies.

DASObjectStore appliance mode SHALL replicate recovery metadata snapshots onto
HDD members. Folder and drive modes SHALL expose their single-failure-domain
status and SHALL support portable manifest export and explicit external
protection policies.

DASObjectStore SHALL support dirty attach behavior after unclean unplug.

On dirty attach, DASObjectStore SHALL offer:

- read-only import;
- repair;
- forced read-write import.

Read-only import SHALL be the default recommendation.

DASObjectStore SHALL recommend safe eject but tolerate unplug and recover dirty
state.

## 7. Metadata Requirements

DASObjectStore SHALL use SQLite for live SSD metadata in the MVP.

DASObjectStore SHALL replicate both:

- SQLite backup snapshots;
- canonical manifest and append-only placement log exports.

Metadata SHALL be versioned.

Metadata SHALL be checksummed.

Metadata SHALL support recovery of committed HDD objects if the SSD fails.

The MVP MAY lose pending SSD-only ingest objects if the SSD fails before
settlement. This loss boundary SHALL be documented in
[Metadata Compatibility and Recovery](metadata-compatibility.md).

Metadata compatibility and recovery guarantees are documented in
[Metadata Compatibility and Recovery](metadata-compatibility.md).

## 8. Store Model

DASObjectStore SHALL organize data into stores.

A store SHALL define:

- class;
- ingest mode;
- copy count;
- placement constraints;
- retention policy;
- deletion behavior;
- repair behavior;
- capacity behavior;
- access credentials;
- export behavior.

DASObjectStore SHALL provide adaptive defaults by store class.

DASObjectStore SHALL allow per-store overrides.

Initial core store classes SHALL be:

- `reproducible_cache`;
- `generated_data`;
- `critical_metadata`;
- `export_bundle`;
- `ingest_staging`.

Mnemosyne-specific store aliases SHALL live in the Mnemosyne adapter.

## 9. Redundancy and Placement Requirements

DASObjectStore SHALL support global pool redundancy defaults with per-store
overrides.

DASObjectStore SHALL support copy-based redundancy in the MVP.

DASObjectStore SHALL allow store policy to control enclosure-aware placement.

DASObjectStore SHALL support placement weighting by:

- available capacity;
- disk health score;
- benchmarked performance;
- temperature and wear indicators;
- prior write load;
- enclosure diversity where required by store policy.

DASObjectStore SHALL score eligible placement candidates deterministically before
copy planning, with stable ordering for equal scores.

DASObjectStore SHALL produce explicit copy plans for one, two, and three requested
copies from scored eligible placement candidates.

DASObjectStore SHALL avoid placing two copies of the same protected object on the
same disk.

DASObjectStore SHOULD prefer distinct enclosures when required or preferred by store
policy and available capacity allows it.

## 10. Ingest Requirements

Appliance profile SHALL require a managed SSD ingest device and SHALL preserve
its existing SSD-first policy except for explicitly policy-approved, verified
local direct-to-HDD imports.

Folder and drive profiles SHALL use private same-filesystem staging so durable
atomic rename is possible; drive profile SHALL use its managed SSD filesystem.

Normal S3/API writes SHALL use daemon-owned managed ingress appropriate to the
profile and SHALL NOT become visible before durable payload and catalogue
finalization.

Write acknowledgement policy SHALL be configurable per store:

- acknowledge after SSD ingest;
- acknowledge after HDD placement satisfies store policy.

Appliance store policy represents these modes as `AfterSsdIngest` and
`AfterHddPlacement`. Folder/drive acknowledgement SHALL use profile-neutral
durable-finalization terminology in future compatibility-sensitive contracts.

`AfterSsdIngest` success SHALL be returned only after one transaction makes
the object catalogue-visible, records a verified and synchronized managed-SSD
placement, and registers a durable restart-safe HDD destage job. The response
SHALL carry per-object, path-free evidence and SHALL be the only authority for
a client to release its local staging copy. HDD settlement SHALL continue
asynchronously with bounded leases, retries, fairness across stores, and
operator-visible failed or review-required states.

The managed SSD copy SHALL remain readable until HDD policy is satisfied.
After verified HDD placements and catalogue promotion commit atomically, an
independent idempotent eviction pass MAY remove the managed SSD copy. SSD
capacity admission SHALL account for the complete incoming file and preserve
the configured critical free-space reserve; durable queued and active destage
bytes SHALL be exposed in diagnostics.

DASObjectStore SHALL support CLI-managed direct-to-HDD imports for massive
server-local datasets where SSD staging is undesirable.

Direct-to-HDD import SHALL use the same endpoint, source-tree, object-type,
copy-count, conflict-policy, progress, TUI, and dry-run semantics as normal
file ingest.

Direct-to-HDD import SHALL be an explicit server-local CLI route and SHALL NOT
silently fall back to SSD staging.

Direct-to-HDD import SHALL calculate content hashes as part of the daemon-owned
copy process without requiring a separate source-side hash argument.

Direct-to-HDD bypass SHALL NOT be the default S3/API write path.

Direct-to-HDD command documentation SHALL state adjacent to the command that
remote S3/API and Web uploads remain SSD-first while server-local imports may
land directly on HDD when policy permits.

Direct-to-HDD import SHALL write to daemon-selected managed HDD destinations,
verify the resulting copies, and only then report the import as successful.

## 11. Object Lifecycle Requirements

DASObjectStore SHALL track object state through at least:

- received on SSD;
- hash verified;
- placement planned;
- copying to HDD;
- HDD copy verified;
- protected;
- SSD eviction eligible.

DASObjectStore SHALL expose object metadata inspection through
`dasobjectstore object inspect <object-id>`.

DASObjectStore SHALL support per-store object mutability policy.

DASObjectStore SHALL support per-store deletion behavior:

- immediate delete for disposable/cache stores where configured;
- tombstone and garbage collect later for protected stores.

## 12. Integrity Requirements

DASObjectStore SHALL compute hashes on ingest.

DASObjectStore SHALL verify each HDD copy before marking it valid.

DASObjectStore SHALL read back each newly written HDD copy and compare its
content hash against the expected ingest hash before reporting it verified.

DASObjectStore SHALL mark an object protected only after the store policy's
required number of distinct verified HDD copies is satisfied.

DASObjectStore SHALL support scheduled scrubs of stored copies.

Read-time verification MAY be added later for critical stores.

## 13. Health Requirements

DASObjectStore SHALL model disk health states:

- healthy;
- watch;
- suspect;
- draining;
- retired;
- failed.

DASObjectStore SHALL ingest health signals from:

- SMART metrics where available;
- IO errors;
- failed checksum verification;
- USB reset and disconnect events;
- temperature history;
- benchmark drift;
- user trust overrides.

When a disk becomes suspect, DASObjectStore SHALL stop placing new protected data on
that disk.

When a disk becomes suspect, DASObjectStore SHALL automatically evacuate protected
stores.

For `reproducible_cache`, DASObjectStore SHALL evacuate opportunistically if spare
capacity exists after higher-value data is safe.

## 14. Disk Add, Retire, and Replace Requirements

When a new disk is detected, DASObjectStore SHALL propose it as a candidate and
require explicit user confirmation before formatting or adding it.

DASObjectStore SHALL provide explicit disk retirement workflow.

Protected stores SHALL be drained before a disk is safe to remove.

Disk retire, drain, replace, and force-retire command documentation SHALL state
adjacent to command examples whether the operation is only planning, whether the
disk is safe to remove, and what data may become unavailable.

Reproducible cache objects MAY be marked redownload-required during retirement.

Only `reproducible_cache` policy may mark an object redownload-required; protected
store classes SHALL preserve recovery or drain semantics instead.

Forced retire SHALL require both policy allowance and explicit action-time
confirmation.

## 15. Capacity Requirements

DASObjectStore SHALL support per-store capacity exhaustion behavior.

DASObjectStore SHALL implement SSD ingest backpressure and priority queue behavior.

DASObjectStore SHALL pause or reject lower-priority writes before allowing SSD
pressure to jeopardize critical work.

DASObjectStore SHALL measure SSD ingest filesystem capacity and evaluate it
against explicit high and critical watermarks before accepting new ingest work.

When SSD pressure is below the high watermark, DASObjectStore SHALL treat HDD
settlement/destage as opportunistic background work so SSD landing remains fast.

When SSD pressure reaches the high watermark, DASObjectStore SHALL prioritize
HDD settlement/destage over lower-priority new ingest work.

When SSD pressure reaches the critical watermark, DASObjectStore SHALL treat
HDD settlement/destage as urgent and reject or pause non-critical ingest.

DASObjectStore SHALL order ingest work by priority, then by age, and SHALL ignore
completed or failed ingest jobs when planning runnable queue work.

DASObjectStore SHALL expose CLI ingest status for SSD capacity and pressure state.

CLI ingest status SHALL report destage urgency so operators can see when SSD
storage is stretched and settlement has been promoted.

DASObjectStore SHALL expose the live ingest queue as JSON for CLI, daemon, and
future Web UI consumers.

DASObjectStore SHALL preserve committed pre-settlement ingest job metadata and
staged SSD payload bytes across process restart.

DASObjectStore SHALL preserve metadata-committed ingest jobs, their linked object
rows, and staged SSD payload references across process restart.

Every ObjectStore SHALL have an explicit capacity policy.

``folder`` ObjectStores SHALL require a finite logical capacity limit. ``drive``
and ``appliance`` profiles MAY offer an ``unlimited`` policy, but it SHALL mean
bounded by backend usable capacity and mandatory free-space reserves.

Capacity admission SHALL evaluate the strictest of logical quota remaining,
transactionally reserved bytes, backend usable space after reserve, and
profile-specific staging/copy amplification. Concurrent and multipart uploads
SHALL reserve capacity before bytes are accepted and SHALL release or expire
reservations after completion, failure, or cancellation.

Logical usage SHALL charge each logical object version its full size even when
physical content is deduplicated. Replicated/staged profiles SHALL separately
report projected and actual physical amplification.

Lowering a quota below current usage SHALL place the store into an explicit
over-quota/no-new-ingress state and SHALL NOT delete data. Reads, verified
deletes, repair, and cleanup SHALL remain available.

SubObject budgets MAY divide a parent quota, but child reservations SHALL update
the child and parent atomically and SHALL never exceed the parent allocation.

## 16. Performance Requirements

DASObjectStore SHALL benchmark disks and pools.

DASObjectStore SHALL use benchmark results in placement scoring.

DASObjectStore SHALL model HDD placement candidates with available capacity,
health state, performance class, write load, and enclosure identity before
weighted scoring is applied.

DASObjectStore SHALL exclude candidates without enough capacity and candidates
that are unsafe for protected copies before scoring.

DASObjectStore SHALL not implement fine-grained HDD physical region placement in the
MVP.

DASObjectStore MAY support coarse disk zones later, such as:

- fast;
- bulk;
- archive.

## 17. Object Service Requirements

DASObjectStore SHALL orchestrate an existing object service before implementing any
native S3 service.

DASObjectStore SHALL include a milestone to benchmark Garage and RustFS.

The object service selection SHALL use a balanced score across:

- throughput;
- concurrency;
- restart safety;
- metadata consistency;
- S3 compatibility;
- operational simplicity;
- resource use;
- portability.

Reliability SHALL be a hard gate.

The selection benchmark SHALL include:

- large object upload/download;
- small object upload/download;
- concurrent clients;
- crash/restart during ingest;
- metadata recovery;
- interrupted writes;
- disk-full behavior;
- disk removal simulation;
- checksum verification;
- SSD ingest and destage compatibility.

Production claims SHALL require long-duration soak testing.

## 18. Interfaces

DASObjectStore SHALL provide CLI and S3-compatible access from the outset.

The CLI SHALL be implemented in Rust using `clap`.

The CLI SHALL NOT directly mutate managed DAS storage in normal operation.
Storage-mutating commands SHALL submit daemon requests or daemon jobs.

The daemon SHALL expose a local client API suitable for CLI, standalone HTTPS,
Web UI, and Synoptikon product integration.

On Linux, the preferred local client transport SHALL be a Unix-domain socket
with peer credential inspection for local actor identity.

Remote/browser control SHALL use the standalone HTTPS or Synoptikon-integrated
HTTPS surfaces rather than raw filesystem permissions.

DASObjectStore SHALL provide a GUI/Web UI that initially supports:

- dashboard;
- disk health;
- queue state;
- pool capacity;
- warnings;
- safe operations;
- logs.

The GUI/Web UI SHALL use `axum` for the Rust HTTP/API layer and `yew` for the
Rust frontend layer.

The GUI/Web UI SHALL be designed for delivery through the sibling Monas and
Synoptikon surfaces in `../monas` and `../mnemosyne`, rather than as an
unrelated standalone UI stack.

Standalone HTTPS operation SHALL default to `https://127.0.0.1:8448`.
Synoptikon-integrated operation SHALL be mounted behind the Synoptikon HTTPS
listener and SHALL use a catalogue-assigned internal product port.

Full setup wizard MAY come later.

SMB/NFS SHALL initially be read-only exports of settled/protected data.

## 19. Security Requirements

DASObjectStore SHALL support a single local admin credential for Web UI/API control.

DASObjectStore SHALL enforce store writer/admin policy at the daemon boundary.

Local Unix group membership MAY authorize store writes, but group membership
SHALL authorize API/job submission rather than granting ordinary users direct
write access to managed DAS roots.

Managed DAS roots SHALL be writable only by the daemon service account or by
package-controlled service identities required for maintenance.

DASObjectStore SHALL generate or manage separate per-store service credentials.

DASObjectStore SHALL protect metadata secrets in the MVP.

Store-level data encryption MAY come later.

## 20. Notifications and Observability

DASObjectStore SHALL provide `dasobjectstore health`.

Default health output SHALL provide a simple consumer summary.

`dasobjectstore health --verbose` and `dasobjectstore health --json` SHALL provide
detailed technical and machine-readable output.

`dasobjectstore health --connections` SHALL report observed DAS host transport
and SHALL warn when USB-attached storage may be using an unverified or slow link
that will reduce object-service performance.

When a better attached DAS connection path is visible in the same probe,
`dasobjectstore health --connections` SHALL recommend that observed path.

When no better attached DAS connection path is visible, `dasobjectstore health
--connections` SHALL say that the probe cannot identify a faster port and SHALL
recommend direct USB-C, USB4, or Thunderbolt attachment without hubs or fallback
cables.

DASObjectStore SHALL support configurable notification sinks.

Initial notification support MAY be local logs only.

Future notification sinks MAY include:

- desktop notification;
- email;
- webhook;
- Prometheus-style metrics.

## 21. Mnemosyne Adapter Requirements

The public core SHALL not depend on Mnemosyne.

The repository SHALL keep Mnemosyne/Synoptikon integration in a separate
`dasobjectstore-mnemosyne` boundary crate/module.

Initial Mnemosyne support SHALL make DASObjectStore a formal Synoptikon
product/plugin with:

- a `mnemosyne.product.manifest.v1` product manifest;
- Synoptikon product catalogue registration;
- Web and API mounts at `/products/dasobjectstore` and
  `/products/dasobjectstore/api`;
- host-mode handling for `standalone` and `synoptikon_integrated`;
- Synoptikon-integrated authentication, entitlement, audit, correlation, project
  context, and storage-authority validation;
- Mneion-compatible storage definition and governance-domain binding export.

`dasobjectstore mnemosyne export`, `register`, and `verify` MAY be CLI surfaces
over the same integration model.

The adapter SHALL respect the Mnemosyne storage boundary where Limen mediates
artefact ingress and egress and public storage contracts remain object-style.

Current Synoptikon and Mneion contracts SHALL be treated as mutable design
inputs when a better integrated storage architecture requires it, provided every
affected product, contract, schema, migration, test, and documentation surface
is updated coherently.

## 22. License Requirement

DASObjectStore SHALL be licensed under the Mozilla Public License 2.0 unless this
decision is later superseded.
