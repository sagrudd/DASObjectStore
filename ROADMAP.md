# DASObjectStore Roadmap

Status: Draft  
Scope: MVP for a DAS-based object store for bioinformatics development  
Target platforms: Linux full support, macOS development/read-export support

## MVP Definition

The MVP is complete when DASObjectStore can run coherently on Linux and macOS
development machines as a portable, SSD-ingest-first, mixed-disk object
appliance:

- Linux can create and operate a pool.
- macOS can develop, inspect, attach, and read/export settled objects.
- normal writes are captured through the mandatory SSD ingest path.
- settled objects are placed onto heterogeneous HDDs according to store policy.
- object integrity is hash-verified at ingest and copy time.
- health checks identify risky disks and block unsafe placement.
- Garage and RustFS have been benchmarked under DASObjectStore-relevant
  workloads and one is selected as the MVP object-service target.
- the project can export a Mnemosyne/Mneion-compatible storage definition
  without making Mnemosyne a hard dependency.

## Milestone 1: Workspace, Naming, and Release Baseline

Goal: make the project coherent as `DASObjectStore`.

Scope:

- rename documentation, repository metadata, and CLI examples to
  `DASObjectStore` / `dasobjectstore`;
- keep the public framing as a portable mixed-disk object appliance;
- keep MPL-2.0 licensing;
- establish semantic-versioning notes and contribution rules;
- add initial CI placeholders for Rust formatting, linting, and tests.

Exit criteria:

- repository and documentation consistently use `DASObjectStore`;
- `README.md`, `ROADMAP.md`, `docs/requirements.md`, and `AGENTS.md` agree on
  scope and naming;
- first version policy is documented before code interfaces stabilize.

## Milestone 2: Rust Workspace and Domain Skeleton

Goal: create a small, modular Rust foundation without committing to storage
implementation details too early.

Scope:

- create a Rust workspace;
- add crates/modules for core domain types, CLI, metadata, platform probing,
  and adapters;
- use `clap` for CLI parsing and command documentation;
- define initial state enums for pools, disks, stores, objects, ingest jobs,
  health, and repair;
- add test fixtures for store policies and lifecycle transitions.

Exit criteria:

- `cargo test` runs on macOS and Linux;
- domain types are isolated from CLI and platform-specific code;
- no module mixes CLI parsing, persistence, and placement logic.

## Milestone 3: Cross-Platform Disk and Enclosure Probe

Goal: reliably inspect attached DAS hardware on macOS and Linux.

Scope:

- detect candidate disks and removable/direct-attached storage;
- collect composite disk identity signals;
- infer enclosure grouping from USB topology where possible;
- record user-confirmed enclosure names;
- expose `dasobjectstore probe` and `dasobjectstore health --json`.

Exit criteria:

- Linux probe reports disks, size, serial hints, filesystem/partition hints,
  and USB topology hints;
- macOS probe reports the best available equivalent using native tools/APIs;
- probe output is stable enough to drive later metadata and Web UI work.

## Milestone 4: Portable Metadata Format

Goal: make pool identity and recovery state portable across hosts.

Scope:

- implement SQLite live metadata on the mandatory SSD;
- define canonical manifest and append-only placement log formats;
- implement metadata snapshots replicated onto HDD metadata areas;
- model dirty attach, clean eject, read-only import, repair, and force import
  states;
- document metadata compatibility rules.

Exit criteria:

- a pool can be recognized from disk-borne metadata without hidden host state;
- committed HDD object metadata can be reconstructed from snapshots in tests;
- dirty-state handling has explicit CLI-visible behavior.

## Milestone 5: Store Policy Engine

Goal: make redundancy, placement, retention, and ingestion policy explicit per
store.

Scope:

- implement global pool defaults with per-store overrides;
- support initial store classes:
  `reproducible_cache`, `generated_data`, `critical_metadata`,
  `export_bundle`, and `ingest_staging`;
- implement policy validation for copies, mutability, retention, capacity
  behavior, and enclosure constraints;
- add risk gating for unsafe modes.

Exit criteria:

- invalid or unsafe policies fail with clear diagnostics;
- store policies can be serialized, restored, and validated;
- policy tests cover public cache, generated data, and critical metadata.

## Milestone 6: SSD Ingest Pipeline

Goal: provide the mandatory SSD-first write path.

Scope:

- implement ingest job records;
- compute hashes during ingest;
- model object states from `received_on_ssd` through
  `ssd_eviction_eligible`;
- implement backpressure and priority queue behavior for SSD pressure;
- expose CLI status for ingest and destage queues.

Exit criteria:

- ingest survives process restart without losing committed metadata;
- SSD pressure can pause lower-priority work;
- acknowledged write semantics are store-policy controlled.

## Milestone 7: HDD Placement and Copy Verification

Goal: settle objects from SSD onto heterogeneous HDDs.

Scope:

- implement weighted placement using capacity, health score, performance class,
  write load, and enclosure policy;
- write and verify HDD copies;
- mark objects protected only after policy-satisfying copies are verified;
- support copy-based redundancy for MVP;
- exclude DASObjectStore-native parity/erasure coding from MVP.

Exit criteria:

- generated and critical stores can require multiple verified copies;
- reproducible cache can use one copy and redownload-required semantics;
- placement never knowingly places duplicate protected copies on the same disk.

## Milestone 8: Object Service Benchmark and Selection

Goal: choose the MVP S3-compatible object service from evidence.

Scope:

- build a benchmark harness for Garage and RustFS;
- test large and small object IO;
- test concurrent clients;
- test crash/restart during ingest;
- test interrupted writes and metadata recovery;
- test disk-full behavior and simulated disk removal;
- test compatibility with SSD ingest and HDD destage layout.

Exit criteria:

- Garage and RustFS have comparable benchmark reports;
- reliability failures are treated as hard gates;
- one object service is selected for MVP integration;
- production claims remain blocked until later long-duration soak testing.

## Milestone 9: S3 Service Orchestration

Goal: expose settled object storage through the selected S3-compatible service.

Scope:

- generate Docker/Compose configuration for the selected service;
- manage per-store service credentials;
- map store policy to bucket/service layout;
- expose service status through CLI;
- preserve Docker/Compose as the default path while keeping native service
  support possible later.

Exit criteria:

- Linux can start, stop, and inspect the object service through
  DASObjectStore;
- macOS development can run the service where feasible or consume generated
  configs;
- S3 access is store-aware and credentials are not shared globally.

## Milestone 10: Health, Drain, Repair, and Disk Retirement

Goal: make old-disk failure an expected operating mode.

Scope:

- ingest SMART, IO error, checksum, temperature, USB reset, and benchmark drift
  signals where available;
- assign disk health states;
- block new protected placement on suspect disks;
- automatically evacuate protected stores from suspect disks;
- implement explicit disk retire, drain, replace, and force-retire flows;
- mark reproducible cache objects redownload-required when appropriate.

Exit criteria:

- `dasobjectstore health` gives summary, verbose, and JSON output;
- suspect disks trigger safe placement behavior;
- protected stores must drain before safe removal;
- force operations require policy allowance and action-time confirmation.

## Milestone 11: macOS Development and Read/Export Path

Goal: make macOS a coherent development and portable attach platform.

Scope:

- support macOS pool inspection from disk-borne metadata;
- support read-only attach for clean pools;
- support read-only dirty import by default;
- support settled object export where feasible;
- document macOS limits around Docker Desktop, service management, SMART, and
  filesystem access.

Exit criteria:

- a pool created on Linux can be inspected on macOS;
- settled object metadata and manifests are readable on macOS;
- macOS behavior is explicit rather than pretending to match Linux full
  operation.

## Milestone 12: Web UI, Read-Only Exports, and Mnemosyne Adapter Draft

Goal: complete the coherent MVP surface for users and bioinformatics
development.

Scope:

- add `axum` API scaffolding for GUI-facing pool, disk, ingest, health, queue,
  and warning views;
- add `yew` frontend scaffolding for dashboard views delivered through the
  sibling Monas and Synoptikon surfaces;
- support safe operations through the Web UI where appropriate;
- provide read-only SMB/NFS export recipes or managed exports for settled data;
- export Mneion-compatible storage definition snippets;
- keep Mnemosyne support in an adapter boundary;
- document a bioinformatics reference workflow using public reference cache and
  generated derivative stores.

Exit criteria:

- a user can understand pool health without reading JSON;
- settled data can be browsed/exported read-only;
- Mnemosyne integration does not leak raw POSIX paths into the public contract;
- the MVP can demonstrate a DAS-backed local object store for bioinformatics
  development.

## Post-MVP Direction

Post-MVP work may include:

- long-duration soak testing for production claims;
- store-level encryption;
- coarse HDD zones such as fast, bulk, and archive;
- DASObjectStore-native parity or erasure policies;
- read/write SMB/NFS ingest semantics;
- richer notifications and Prometheus-style metrics;
- deeper `dasobjectstore-mnemosyne` registration and verification commands;
- native service management where Docker/Compose is not ideal.
