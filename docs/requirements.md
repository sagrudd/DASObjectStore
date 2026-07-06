# DASObjectStore Initial Requirements Draft

Status: Draft  
Date: 2026-06-23  
Scope: MVP and near-term architecture alignment

## 1. Product Position

DASObjectStore SHALL be a portable, SSD-ingest-first, mixed-disk DAS object
appliance.

DASObjectStore SHALL be implemented primarily in Rust.

DASObjectStore SHALL be useful to non-Mnemosyne users.

Bringing DASObjectStore under the Synoptikon umbrella as a formal Mnemosyne
product/plugin SHALL be the priority integration path. The standalone monolith
SHALL reuse the same domain model and SHALL NOT fork into a separate
non-Mnemosyne architecture.

DASObjectStore SHALL NOT require equal-size disks.

DASObjectStore SHALL NOT require classic RAID.

DASObjectStore SHALL NOT claim to be a backup solution.

## 2. Core Use Cases

DASObjectStore SHALL support these primary use cases:

- reuse older HDDs in USB-C DAS enclosures;
- ingest writes onto a mandatory SSD;
- settle verified object copies onto heterogeneous HDD members;
- expose object storage through an S3-compatible interface;
- provide CLI and Web UI health visibility;
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

## 5. Storage Hardware Model

DASObjectStore SHALL model these disk roles:

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

DASObjectStore SHALL store live metadata on the mandatory SSD.

DASObjectStore SHALL replicate recovery metadata snapshots onto HDD members.

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

DASObjectStore SHALL require a mandatory SSD ingest device.

Normal S3/API writes SHALL use SSD-first ingest.

Write acknowledgement policy SHALL be configurable per store:

- acknowledge after SSD ingest;
- acknowledge after HDD placement satisfies store policy.

The store policy model represents these modes as `AfterSsdIngest` and
`AfterHddPlacement`.

DASObjectStore SHALL support CLI-managed direct-to-HDD imports as an initial bypass
for massive public or otherwise reproducible datasets.

Direct-to-HDD import SHALL initially be limited to `reproducible_cache` stores
whose policy explicitly uses `DirectToHdd` ingest.

Direct-to-HDD import SHALL require an expected SHA-256 content hash before
writing to HDD.

Direct-to-HDD import SHOULD record source metadata such as URL, accession, or
provenance URI when available so lost cache objects can be redownloaded or
rehydrated.

Direct-to-HDD bypass SHALL NOT be the default S3/API write path.

Direct-to-HDD command documentation SHALL state adjacent to the command that the
path bypasses SSD capture and is only appropriate for reproducible objects with
known source metadata and expected digests.

Risky bypass behavior SHALL require both policy allowance and action-time
confirmation.

Direct-to-HDD import SHALL write to the selected HDD destination, verify the
result against the expected hash, and only then report the import as successful.

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

Per-store SSD budgets MAY be added later.

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
