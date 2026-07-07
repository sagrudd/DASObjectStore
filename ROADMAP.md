# DASObjectStore Roadmap

Status: Draft  
Scope: MVP for a DAS-based object store for bioinformatics development  
Target platforms: Linux full support, macOS development/read-export support

## MVP Definition

Priority: bringing DASObjectStore under the Synoptikon umbrella as a formal
Mnemosyne product/plugin is now the primary planning priority. Standalone
operation remains required, but the standalone monolith must not evolve in a way
that conflicts with native Synoptikon/Mneion integration.

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
- DASObjectStore is specified as a formal Synoptikon product/plugin and as a
  standalone HTTPS application.
- `dasobjectstored` is the managed storage authority, and normal CLI/Web/API
  flows submit daemon requests or jobs instead of mutating DAS roots directly.
- the Web GUI design language, host-mode authentication model, and Mneion
  storage endpoint conventions are documented before implementation.
- current Synoptikon and Mneion conventions are treated as mutable design inputs
  when a better integrated storage architecture requires coordinated changes
  across affected Mnemosyne software.

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

## Milestone 12: Managed Daemon and Client Boundary

Goal: make DASObjectStore an enterprise server/client storage appliance rather
than a direct CLI mutator.

Priority: this milestone supersedes additional CLI-local storage mutation work.
It SHALL be implemented before expanding ingest, disk management, or Web
operations beyond scaffolding.

Scope:

- introduce `dasobjectstored` as the daemon-owned storage authority;
- define the local daemon API for health, store inventory, ingest job
  submission, progress events, cancellation, disk management, and service
  orchestration;
- add a daemon client layer reused by CLI, standalone HTTPS, Web UI, and
  Synoptikon integration;
- move normal `dasobjectstore ingest files` behavior to client submission and
  progress rendering;
- keep direct local storage mutation available only as an explicitly hidden
  developer/test fallback until removed;
- enforce writer/admin policy at the daemon boundary using peer credentials,
  local sessions, or Synoptikon actor context;
- package system user, systemd service, Unix socket, runtime directory, state
  directory, logs, and permissions through the DEB;
- document the security boundary so users are not asked to write directly to
  managed DAS disks.

Exit criteria:

- normal non-root ingest succeeds through the daemon without granting the user
  direct write access to managed DAS roots;
- CLI, Web/API, and Synoptikon-facing paths share the same daemon request/job
  model;
- daemon job progress can reproduce the current byte-level ingest progress;
- package installation creates and validates the daemon runtime boundary;
- tests prove that CLI-local direct mutation is not the default storage path.

## Milestone 13: Web UI, Read-Only Exports, and Mnemosyne Adapter Draft

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

## Milestone 14: Formal Mnemosyne Product Plugin

Goal: make DASObjectStore a first-class Synoptikon product/plugin while keeping
the public core standalone.

Priority: this remains the strategic Mnemosyne product milestone, but it
depends on the managed daemon/client boundary from Milestone 12. Product
manifest, catalogue, host-mode, authentication-boundary, and Mneion endpoint
compatibility work SHALL be favored over optional standalone-only enhancements
once the daemon storage authority is established.

Scope:

- add a `mnemosyne.product.manifest.v1` compatible product manifest;
- define dual host support for standalone and `synoptikon_integrated` modes;
- register product API and Web mounts under `/products/dasobjectstore`;
- define required Synoptikon host capabilities, including accounts,
  entitlements, central audit, object-store artifacts, and project context where
  required;
- define the internal product port as catalogue-assigned in Synoptikon;
- keep Mnemosyne/Synoptikon integration isolated in `dasobjectstore-mnemosyne`;
- identify where Synoptikon or Mneion contracts should evolve to make
  DASObjectStore a native storage appliance rather than a bolt-on integration.

Exit criteria:

- the product manifest validates against Mnemosyne product schema expectations;
- Synoptikon integration can generate product UI bootstrap metadata;
- standalone builds do not require Mnemosyne runtime crates;
- integration documentation identifies the exact catalogue entry required in
  `../mnemosyne`;
- any proposed Synoptikon/Mneion changes name the affected repositories,
  contracts, migrations, and tests required to keep the platform coherent.

## Milestone 15: Standalone HTTPS Application and Authentication

Goal: deliver a coherent standalone application where needed, without diverging
from Mnemosyne authentication conventions or bypassing the daemon storage
authority.

Scope:

- define standalone HTTPS default port `8448`;
- implement `axum` server configuration and TLS asset handling;
- connect storage-mutating HTTPS/API routes to `dasobjectstored`;
- add host-mode selection for `standalone` and `synoptikon_integrated`;
- implement local standalone login, logout, session validation, and local user
  storage using the Mnematikon pattern;
- disable local auth routes in Synoptikon-integrated mode;
- ensure risky operations still require operation-level confirmation after
  login;
- serve the Yew bundle from the standalone server.

Exit criteria:

- standalone HTTPS starts on `https://127.0.0.1:8448` by default;
- integrated mode rejects local login endpoints and relies on host context;
- authentication tests cover login, session expiry, logout, and integrated
  session behavior;
- package/service docs state the permanent port policy.

## Milestone 16: Native Mneion Storage Endpoint and External NAS Support

Goal: make DASObjectStore a native storage endpoint across Mneion for DAS-backed
and external NAS-backed storage.

Scope:

- extend Mneion export contracts for DASObjectStore-backed endpoints;
- model endpoint kinds for local DAS, external NAS/NFS, and S3-compatible
  exports;
- support external NAS/NFS endpoints as formal validated storage definitions;
- preserve object-style contracts even when backing storage is NFS or local
  filesystem;
- map DASObjectStore endpoints to Mneion governance-domain storage bindings;
- implement validation flows for NAS reachability, mount semantics, credential
  references, and export safety;
- document how DASObjectStore differs from generic POSIX storage definitions.

Exit criteria:

- `dasobjectstore mnemosyne export` can describe DAS, NFS/NAS, and
  S3-compatible endpoint variants;
- endpoint definitions can be validated without exposing raw paths to product
  contracts;
- governance-domain binding snippets match Mneion storage-binding conventions;
- external NAS endpoints are visible as first-class managed endpoints in the
  Web/API model.

## Milestone 17: Web Operations Console and Design System

Goal: create the contemporary GUI workbench for disk, store, object, and
endpoint operations.

Scope:

- implement a post-login Overview workspace for capacity, ingest pressure,
  destage urgency, endpoint state, and required actions;
- implement Disks workspace for health, USB/SMART warnings, enclosure grouping,
  benchmark drift, migrate, drain, replace, and retire flows;
- implement Stores workspace for create, modify, resize, redundancy, retention,
  endpoint export, and capacity behavior;
- implement Objects workspace for inventory, hashes, copy locations,
  reproducibility source, export/download, repair, and redownload actions;
- implement Endpoints workspace for DAS pools, external NAS/NFS endpoints,
  S3-compatible service state, Mneion export, and governance-domain binding
  readiness;
- define reusable Yew components for dense tables, inspector drawers, status
  badges, capacity bars, segmented controls, and risky-operation confirmation;
- align visual language with Mneion and Mnematikon while remaining usable as a
  standalone customer application.

Exit criteria:

- a user can manage disks, stores, endpoints, and object state through the Web
  UI without reading CLI JSON;
- the UI follows `docs/web-gui-and-mnemosyne-plugin.md`;
- Synoptikon and standalone hosting use the same domain view models;
- risky flows are visibly gated and auditable.

## Post-MVP Direction

Post-MVP work may include:

- long-duration soak testing for production claims;
- store-level encryption;
- coarse HDD zones such as fast, bulk, and archive;
- DASObjectStore-native parity or erasure policies;
- read/write SMB/NFS ingest semantics;
- richer notifications and Prometheus-style metrics;
- native service management where Docker/Compose is not ideal.
