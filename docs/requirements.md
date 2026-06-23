# DockerDAS Initial Requirements Draft

Status: Draft  
Date: 2026-06-23  
Scope: MVP and near-term architecture alignment

## 1. Product Position

DockerDAS SHALL be a portable, SSD-ingest-first, mixed-disk DAS object
appliance.

DockerDAS SHALL be useful to non-Mnemosyne users. Mnemosyne/Synoptikon support
SHALL be implemented as an adapter over the public core model.

DockerDAS SHALL NOT require equal-size disks.

DockerDAS SHALL NOT require classic RAID.

DockerDAS SHALL NOT claim to be a backup solution.

## 2. Core Use Cases

DockerDAS SHALL support these primary use cases:

- reuse older HDDs in USB-C DAS enclosures;
- ingest writes onto a mandatory SSD;
- settle verified object copies onto heterogeneous HDD members;
- expose object storage through an S3-compatible interface;
- provide CLI and Web UI health visibility;
- support portable movement of the DAS between hosts;
- support incremental addition and retirement of disks;
- support per-store redundancy and retention policy;
- export storage configuration for Mnemosyne/Mneion integration.

## 3. Non-Goals for MVP

The MVP SHALL NOT include:

- DockerDAS-native parity or erasure coding;
- read/write SMB or NFS as a primary write path;
- fine-grained HDD physical zone placement;
- a custom S3 implementation written by DockerDAS;
- production backup claims;
- deep Synoptikon platform service integration.

## 4. Platform Requirements

DockerDAS SHALL provide full MVP support on Linux first.

DockerDAS SHALL provide macOS beta support for:

- pool detection;
- metadata inspection;
- health summary;
- settled object read/export;
- read-only SMB/S3 export where feasible.

DockerDAS SHALL use Docker/Compose as the default deployment path for
supporting services.

DockerDAS MAY support native/systemd service management where appropriate.

## 5. Storage Hardware Model

DockerDAS SHALL model these disk roles:

- mandatory SSD ingest disk;
- HDD capacity member;
- replacement disk;
- retired disk.

DockerDAS SHALL support mixed HDD sizes and mixed performance classes.

DockerDAS SHALL identify disks using composite identity:

- DockerDAS disk UUID;
- observed hardware serials where available;
- enclosure hints;
- bay hints where available;
- size;
- partition and filesystem fingerprints.

DockerDAS SHALL model enclosures using best-effort USB topology inference plus
user confirmation and naming.

## 6. Portability Requirements

DockerDAS SHALL NOT rely on hidden host-local state as the sole authority for a
pool.

DockerDAS SHALL store live metadata on the mandatory SSD.

DockerDAS SHALL replicate recovery metadata snapshots onto HDD members.

DockerDAS SHALL support dirty attach behavior after unclean unplug.

On dirty attach, DockerDAS SHALL offer:

- read-only import;
- repair;
- forced read-write import.

Read-only import SHALL be the default recommendation.

DockerDAS SHALL recommend safe eject but tolerate unplug and recover dirty
state.

## 7. Metadata Requirements

DockerDAS SHALL use SQLite for live SSD metadata in the MVP.

DockerDAS SHALL replicate both:

- SQLite backup snapshots;
- canonical manifest and append-only placement log exports.

Metadata SHALL be versioned.

Metadata SHALL be checksummed.

Metadata SHALL support recovery of committed HDD objects if the SSD fails.

The MVP MAY lose pending SSD-only ingest objects if the SSD fails before
settlement.

## 8. Store Model

DockerDAS SHALL organize data into stores.

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

DockerDAS SHALL provide adaptive defaults by store class.

DockerDAS SHALL allow per-store overrides.

Initial core store classes SHALL be:

- `reproducible_cache`;
- `generated_data`;
- `critical_metadata`;
- `export_bundle`;
- `ingest_staging`.

Mnemosyne-specific store aliases SHALL live in the Mnemosyne adapter.

## 9. Redundancy and Placement Requirements

DockerDAS SHALL support global pool redundancy defaults with per-store
overrides.

DockerDAS SHALL support copy-based redundancy in the MVP.

DockerDAS SHALL allow store policy to control enclosure-aware placement.

DockerDAS SHALL support placement weighting by:

- available capacity;
- disk health score;
- benchmarked performance;
- temperature and wear indicators;
- prior write load;
- enclosure diversity where required by store policy.

DockerDAS SHOULD avoid placing two copies of the same protected object on the
same disk.

DockerDAS SHOULD prefer distinct enclosures when required or preferred by store
policy and available capacity allows it.

## 10. Ingest Requirements

DockerDAS SHALL require a mandatory SSD ingest device.

Normal S3/API writes SHALL use SSD-first ingest.

Write acknowledgement policy SHALL be configurable per store:

- acknowledge after SSD ingest;
- acknowledge after HDD placement satisfies store policy.

DockerDAS SHALL support CLI-managed direct-to-HDD imports as an initial bypass
for large reproducible downloads.

Direct-to-HDD bypass SHALL NOT be the default S3/API write path.

Risky bypass behavior SHALL require both policy allowance and action-time
confirmation.

## 11. Object Lifecycle Requirements

DockerDAS SHALL track object state through at least:

- received on SSD;
- hash verified;
- placement planned;
- copying to HDD;
- HDD copy verified;
- protected;
- SSD eviction eligible.

DockerDAS SHALL support per-store object mutability policy.

DockerDAS SHALL support per-store deletion behavior:

- immediate delete for disposable/cache stores where configured;
- tombstone and garbage collect later for protected stores.

## 12. Integrity Requirements

DockerDAS SHALL compute hashes on ingest.

DockerDAS SHALL verify each HDD copy before marking it valid.

DockerDAS SHALL support scheduled scrubs of stored copies.

Read-time verification MAY be added later for critical stores.

## 13. Health Requirements

DockerDAS SHALL model disk health states:

- healthy;
- watch;
- suspect;
- draining;
- retired;
- failed.

DockerDAS SHALL ingest health signals from:

- SMART metrics where available;
- IO errors;
- failed checksum verification;
- USB reset and disconnect events;
- temperature history;
- benchmark drift;
- user trust overrides.

When a disk becomes suspect, DockerDAS SHALL stop placing new protected data on
that disk.

When a disk becomes suspect, DockerDAS SHALL automatically evacuate protected
stores.

For `reproducible_cache`, DockerDAS SHALL evacuate opportunistically if spare
capacity exists after higher-value data is safe.

## 14. Disk Add, Retire, and Replace Requirements

When a new disk is detected, DockerDAS SHALL propose it as a candidate and
require explicit user confirmation before formatting or adding it.

DockerDAS SHALL provide explicit disk retirement workflow.

Protected stores SHALL be drained before a disk is safe to remove.

Reproducible cache objects MAY be marked redownload-required during retirement.

Forced retire SHALL require both policy allowance and explicit action-time
confirmation.

## 15. Capacity Requirements

DockerDAS SHALL support per-store capacity exhaustion behavior.

DockerDAS SHALL implement SSD ingest backpressure and priority queue behavior.

DockerDAS SHALL pause or reject lower-priority writes before allowing SSD
pressure to jeopardize critical work.

Per-store SSD budgets MAY be added later.

## 16. Performance Requirements

DockerDAS SHALL benchmark disks and pools.

DockerDAS SHALL use benchmark results in placement scoring.

DockerDAS SHALL not implement fine-grained HDD physical region placement in the
MVP.

DockerDAS MAY support coarse disk zones later, such as:

- fast;
- bulk;
- archive.

## 17. Object Service Requirements

DockerDAS SHALL orchestrate an existing object service before implementing any
native S3 service.

DockerDAS SHALL include a milestone to benchmark Garage and RustFS.

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

DockerDAS SHALL provide CLI and S3-compatible access from the outset.

DockerDAS SHALL provide a Web UI that initially supports:

- dashboard;
- disk health;
- queue state;
- pool capacity;
- warnings;
- safe operations;
- logs.

Full setup wizard MAY come later.

SMB/NFS SHALL initially be read-only exports of settled/protected data.

## 19. Security Requirements

DockerDAS SHALL support a single local admin credential for Web UI/API control.

DockerDAS SHALL generate or manage separate per-store service credentials.

DockerDAS SHALL protect metadata secrets in the MVP.

Store-level data encryption MAY come later.

## 20. Notifications and Observability

DockerDAS SHALL provide `dockerdas health`.

Default health output SHALL provide a simple consumer summary.

`dockerdas health --verbose` and `dockerdas health --json` SHALL provide
detailed technical and machine-readable output.

DockerDAS SHALL support configurable notification sinks.

Initial notification support MAY be local logs only.

Future notification sinks MAY include:

- desktop notification;
- email;
- webhook;
- Prometheus-style metrics.

## 21. Mnemosyne Adapter Requirements

The public core SHALL not depend on Mnemosyne.

The repository SHALL include or allow a separate adapter crate/module for
Mnemosyne integration.

Initial Mnemosyne support SHALL generate Mneion-compatible storage definition
and binding snippets.

Later Mnemosyne support MAY provide:

- `dockerdas mnemosyne export`;
- `dockerdas mnemosyne register`;
- `dockerdas mnemosyne verify`.

The adapter SHALL respect the Mnemosyne storage boundary where Limen mediates
artefact ingress and egress and public storage contracts remain object-style.

## 22. License Requirement

DockerDAS SHALL be licensed under the Mozilla Public License 2.0 unless this
decision is later superseded.

