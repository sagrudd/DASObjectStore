# DASObjectStore

Portable mixed-disk DAS pooling and service layer for turning old drives into
SMB/S3/NFS storage, with optional object-level duplication and
Synoptikon/Mneion integration.

DASObjectStore is an SSD-ingest-first object appliance for people who have old hard
drives, direct-attached storage enclosures, and a practical need for local
capacity without buying a new stack of large disks.

The core idea is simple:

1. Capture normal writes onto a mandatory SSD.
2. Verify and settle object copies onto heterogeneous HDDs in the DAS.
3. Expose settled data through S3 first, with read-only SMB/NFS exports later.
4. Keep pool identity and recovery metadata on the DAS so it can move between
   hosts.

DASObjectStore is not intended to be a traditional RAID replacement or a guarantee
that local storage is a backup. It is a portable, mixed-disk storage appliance
with explicit health, placement, and redundancy policy.

## Why

Disk storage prices are high, but many home labs and development environments
have older HDDs that are still useful. The hard part is making those disks
usable without pretending that they are all the same size, speed, or health.

DASObjectStore is designed for:

- home lab users who want to reuse mixed old drives;
- developers and data engineers who need a local S3-compatible object store;
- users with portable USB-C DAS enclosures;
- projects with reproducible public datasets that are expensive to redownload;
- Synoptikon/Mnemosyne development environments that need a governed local
  object storage backend.

## Product Direction

DASObjectStore is:

- Rust-based;
- SSD-ingest-first;
- S3/object-first;
- mixed-disk and mixed-size by design;
- portable across hosts through disk-borne metadata;
- health-aware and evacuation-capable;
- copy-policy based rather than classic RAID-first;
- useful without Mnemosyne;
- extensible through an optional Mnemosyne adapter.

The current MVP milestone plan is tracked in [ROADMAP.md](ROADMAP.md).

DASObjectStore is not:

- a backup system by itself;
- normal read/write SMB storage in the MVP;
- a block RAID manager first;
- active compute scratch storage;
- tied to a single host.

## Architecture Sketch

```text
USB-C DAS enclosure(s)
  -> mandatory SSD ingest device
  -> heterogeneous HDD capacity members
  -> DASObjectStore Rust supervisor
  -> SQLite live metadata on SSD
  -> replicated recovery metadata on HDDs
  -> object service selected by benchmark milestone
  -> S3 API and CLI
  -> read-only SMB/NFS exports later
  -> optional Mnemosyne adapter
```

The object service will initially be an existing service orchestrated by
DASObjectStore rather than a custom S3 implementation. Garage and RustFS will be
benchmarked as a milestone before selecting the first-class default.

## Storage Model

DASObjectStore stores data in named stores. A store defines how data is ingested,
placed, protected, retained, and repaired.

Initial core store classes:

- `reproducible_cache`: public or reproducible data with a download cost;
- `generated_data`: derivative outputs and user-generated results;
- `critical_metadata`: manifests, indexes, provenance, credentials references;
- `export_bundle`: packaged data intended for external transfer;
- `ingest_staging`: temporary SSD-backed ingest state.

The pool provides adaptive defaults by store class. Stores can override those
defaults.

Example:

```toml
[pool.default_policy]
placement = "weighted_health_capacity_performance"

[store.public_reference_cache]
class = "reproducible_cache"
copies = 1
ingest_mode = "ssd_first"
on_disk_suspect = "evacuate_if_capacity_available"

[store.synoptikon_derivatives]
class = "generated_data"
copies = 2
ingest_mode = "ssd_first"
prefer_distinct_enclosures = true

[store.system_manifests]
class = "critical_metadata"
copies = 3
ingest_mode = "ssd_first"
prefer_distinct_enclosures = true
retention = "tombstone_then_gc"
```

## Bioinformatics Workflow

The first reference workflow is a local bioinformatics object store that keeps
public, reproducible inputs separate from generated outputs. It uses
`reproducible_cache` for public reference datasets and `generated_data` for
pipeline artefacts that need stronger protection.

See [Bioinformatics Reference Workflow](docs/bioinformatics-reference-workflow.md)
for the draft object boundaries, ingest path, disk-failure behavior, and
Mnemosyne adapter boundary.

## Ingest Model

Normal writes go to the mandatory SSD first. Background workers then copy data
to HDD members according to store policy.

```text
received_on_ssd
  -> hash_verified
  -> placement_planned
  -> copying_to_hdd
  -> hdd_copy_verified
  -> protected
  -> ssd_eviction_eligible
```

When SSD pressure is low, HDD settlement is opportunistic so landing new data on
SSD stays fast. At the high watermark, destage/settlement is prioritized over
lower-priority ingest. At the critical watermark, destage is urgent and normal
ingest should pause or reject non-critical work.

CLI-managed public imports may bypass SSD ingest for reproducible objects with
an expected digest and source URL:

```bash
dasobjectstore ingest direct-import <object-id> \
  --disk-id <disk-id> \
  --source <downloaded-file> \
  --destination <hdd-object-path> \
  --expected-sha256 <sha256> \
  --source-uri <url-or-accession> \
  --policy-file <reproducible-cache-direct-policy.json> \
  --allow-direct-to-hdd-import \
  --confirm "confirm direct-to-hdd import"
```

That bypass is intentionally not the normal S3/API write path. It is limited to
reproducible cache data because it bypasses SSD capture and acknowledgement.

## Portability

DASObjectStore pools must not depend on hidden state from one host.

The live metadata database lives on the mandatory SSD. Recovery snapshots live
on HDD metadata areas. If the SSD fails, the MVP target is recovery of committed
HDD objects. Pending SSD-only ingest data, including writes acknowledged before
HDD settlement, may be lost. A later target is full live metadata reconstruction
from HDD snapshots.

Metadata compatibility and recovery boundaries are documented in
[Metadata Compatibility and Recovery](docs/metadata-compatibility.md).

Each disk has a composite identity:

- DASObjectStore disk UUID;
- observed hardware serials where available;
- enclosure and bay hints;
- size and partition fingerprints.

Enclosures are inferred from USB topology where possible and confirmed by the
user.

## Health and Repair

DASObjectStore assumes old disks will fail.

Disk health states:

```text
healthy -> watch -> suspect -> draining -> retired
healthy -> failed
```

Health inputs include:

- SMART warnings;
- reallocated, pending, and uncorrectable sectors;
- IO errors;
- failed hash verification;
- USB resets and disconnects;
- temperature history;
- latency and throughput drift;
- user trust overrides.

Connection health is exposed through:

```bash
dasobjectstore health --connections
```

USB-attached DAS devices are reported with an explicit performance warning when
the host probe cannot verify the negotiated link speed. Users should prefer a
fast USB-C, USB 3.x, USB4, or Thunderbolt path for object-service workloads.
When the host probe can see a better attached path, the command recommends that
observed device/topology path; otherwise it says that no faster path is visible
and recommends a direct high-speed host port without hubs or fallback cables.

When a disk becomes suspect, DASObjectStore stops placing new protected data on it
and automatically evacuates protected stores. Reproducible cache data is moved
opportunistically if capacity exists.

Explicit disk retirement is a first-class workflow:

```bash
dasobjectstore disk retire <disk-id> \
  --live-sqlite-path <live.sqlite> \
  --recorded-at-utc <timestamp>
dasobjectstore disk drain <disk-id> \
  --live-sqlite-path <live.sqlite>
dasobjectstore disk replace <old-disk-id> \
  --with <new-disk-id> \
  --live-sqlite-path <live.sqlite>
dasobjectstore disk force-retire <disk-id> \
  --live-sqlite-path <live.sqlite> \
  --recorded-at-utc <timestamp> \
  --allow-force-retire \
  --confirm "confirm force retire"
```

`disk retire` and `disk drain` are planning and state-transition workflows for
safe removal. They do not make a disk safe to pull until protected stores have
policy-satisfying verified copies elsewhere. `disk force-retire` bypasses that
safe drain requirement and can leave data unavailable unless it is already
protected or reproducible, so it requires both policy allowance and action-time
confirmation.

Protected stores must satisfy their policy before safe removal. Reproducible
cache objects may be marked redownload-required. Force retirement bypasses the
safe drain requirement and requires both policy allowance and action-time
confirmation.

## Interfaces

MVP:

- Rust CLI built with `clap`;
- S3-compatible object API through an orchestrated object service;
- eventual GUI dashboard and safe operations through `axum` and `yew`;
- local admin credential;
- per-store service credentials.

The GUI stack should stay Rust-native: `axum` for HTTP/API serving and `yew`
for the browser frontend. DASObjectStore should not grow a separate ad hoc UI
framework; GUI delivery should be aligned with the sibling Monas and Synoptikon
surfaces in `../monas` and `../mnemosyne`.

Initial SMB/NFS scope:

- read-only exports of settled/protected data;
- not a primary write path.

Future:

- full setup wizard;
- notification sinks;
- optional store-level encryption;
- coarse disk zones;
- DASObjectStore-native parity or erasure policies.

## Mnemosyne Integration

DASObjectStore is public-core first. Mnemosyne integration lives behind an adapter.

Initial integration target:

- export Mneion-compatible storage definition snippets;
- support Synoptikon/Mneion development stores;
- preserve the Mnemosyne boundary where Limen mediates artefact ingress/egress
  and public contracts remain object-style.

Later integration target:

- `dasobjectstore-mnemosyne` adapter crate/module;
- CLI commands to export, register, and verify storage context against Mneion.

## Platform Plan

Initial full platform:

- Linux, including GB10-style development hosts.

Initial macOS beta:

- attach and inspect a DASObjectStore pool;
- read settled objects;
- expose read-only export where feasible.

Docker/Compose is the default deployment path for supporting services, with
native/systemd support allowed where it improves reliability.

## License

DASObjectStore is intended to be licensed under the Mozilla Public License 2.0.
