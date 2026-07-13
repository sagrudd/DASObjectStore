# DASObjectStore Roadmap

Status: Active product-integration campaign
Tracked TODO status: Milestones 1-18 record the delivered appliance foundation;
Milestones 19-24 contain partially delivered console, browser, upload,
telemetry, and design work. Milestones 25-31 define the dependency-ordered
campaign for multi-profile deployment and Mnemosyne-wide integration.
Scope: a common managed object-storage service ranging from bounded local
folders and dedicated SSDs to tiered DAS appliances
Target platforms: Linux full support, macOS development/read-export support

## Product Direction and Campaign Gates

DASObjectStore is no longer defined only as a mixed-disk appliance. It SHALL
offer one object, manifest, ingress, authorization, S3, and lifecycle contract
across three deployment profiles:

- ``folder``: one explicitly bounded managed directory on a single host;
- ``drive``: one dedicated, validated SSD filesystem on a single host;
- ``appliance``: the existing SSD-ingest and managed-HDD placement system.

Storage profile and host authority are separate. A folder may be managed by a
per-user daemon or system service; drive and appliance profiles normally use a
system service. All profiles remain daemon-owned and capability-driven.

### Approved architecture decisions (2026-07-13)

The storage authority is DASObjectStore. It owns disk paths, mount/profile
validation, service lifecycle, buckets, credentials, catalogue state, placement,
quotas, and health. Monas, AlleleAnchor, Synoptikon, and Mneion consume
versioned storage definitions, immutable object IDs, manifests, checksums, and
typed metadata; they do not receive private host paths or manage storage
lifecycle.

The ``folder`` profile maps one bounded root to one logical ObjectStore. Direct
unmanaged edits are reported as drift, and reconcile/adopt is explicit and
confirmed. The ``drive`` profile requires one exclusively dedicated SSD with
stable device/filesystem identity; mixed-use or HDD-backed drive profiles are
outside the contract. Every ObjectStore has a logical quota measured in full
object-version bytes, with transactional restart-safe reservations and
separate physical-amplification reporting. Lowering a quota never deletes
data.

S3 exposure is provider-neutral. Garage remains the local compatibility
provider behind the object-service interface, while no consumer becomes a
storage authority. Standalone administrator authority is local OS/sudo-derived;
paired clients use scoped credentials. The approved application-authentication
contract supports long-lived service identities, short-lived scoped access
tokens, and one-time upload-completion capabilities; renewal tokens are never
accepted as storage-operation bearer credentials. See
`docs/application-authentication.md` for the full contract.

Development self-signing is a local-only test aid. It is explicitly enabled,
limited to loopback/local-Docker generated-data stores, bounded in scope and
expiry, and rejected for appliance/production listeners. It must never appear
in RPM or DEB contents: no development key, issuer, configuration, or
installation switch may be packaged.

The canonical macOS development path is ``deploy/local-docker``. It is a
single-node compatibility profile with daemon-owned nested Garage lifecycle,
and may use ``$HOME/.dasobjectstore-codex-validation`` as a generated-data-only
root below 1 TiB. AlleleAnchor's local FileStore and Docker/Nextflow stages are
consumer-side substitutes or environment gates; only an exported scoped S3
configuration crosses the boundary. This profile makes no appliance durability,
multi-disk, SMART, repair, or throughput claim.

Market/integration readiness requires these campaign gates, in order:

1. stabilize daemon ownership, ingress completion, metadata durability, and
   control-plane availability already owed by the appliance implementation;
2. define versioned backend-capability, portable-manifest, protection-state,
   capacity, reservation, and migration contracts;
3. deliver the bounded folder profile, including safe adoption and S3 access;
4. deliver the dedicated-SSD drive profile with mount identity and health;
5. make CLI, Web, package, Synoptikon, Mneion, and product adapters profile-aware;
6. validate migration, recovery, quotas, S3 semantics, and realistic workloads.

An implementation is not integration-ready merely because its route or UI is
present. The relevant TODO must be implemented, tested, documented, committed,
pushed, and ready for real-world validation.

### Application identity and token authority

The approved contract is to support long-lived application identities without
shipping long-lived, broadly scoped bearer access tokens. The ordered delivery
slice is:

1. register daemon-owned service principals with owner, purpose, allowed
   ObjectStores/namespaces, operations, ingress origin, optional byte limits,
   expiry, and audit metadata;
2. add rotatable asymmetric-key or certificate identities and a short-lived
   access-token exchange (normally 5–15 minutes);
3. issue single-use upload-completion capabilities bound to the upload,
   ObjectStore, expected size/checksum, audience, expiry, and nonce;
4. enforce daemon-side provider verification, reservation state, atomic
   catalogue commit, idempotent retries, replay protection, revocation,
   rotation, and audit events; and
5. publish contract fixtures for Synoptikon, Mneion, AlleleAnchor, Mnemosyne,
   and standalone integrations without exposing host paths or secrets.

The packaged CLI now provides a request-file bridge for these daemon-owned
operations (`application-auth register-identity`, `register-key`, `revoke`,
and `exchange`). It accepts only the versioned path-free request DTOs; private
keys stay with the caller's signer and all authorization, proof verification,
confirmation, and issuance decisions remain inside `dasobjectstored`.

Profile consumers can also query a daemon-authorized readiness projection. The
versioned response combines binding-root state, folder drift findings, and the
daemon-owned capacity status while remaining path-free; `store
profile-readiness` exposes the same result for operators. Missing capacity or
unreadable roots remain explicit not-ready reasons rather than optimistic
success.

Development self-signing is implemented only as a feature-gated local helper
for workspace and generated-data tests. It is explicit, short-lived, and
honors each request's bounded TTL,
loopback/local-Docker only, and constrained to synthetic validation stores. It
is a test-build facility, not an application authority, and is forbidden from
RPM/DEB contents including keys, issuers, configuration, and enablement
switches. The package-asset test executes the shared payload guard against
safe and forbidden fixture trees, covering marker/path and compiled-payload
regressions without building a package locally; private-key PEM material and
development enablement markers are rejected while the public contract remains
allowed. The core
exchange-request contract validates active key membership, bounded proof shape,
scope, and lifetime. Core issuance now requires an explicit proof verifier
implementation and rejects unverified proofs. The daemon now ships a
ring-backed Ed25519/P-256 verifier bound to registered public-key material and
fingerprints, and its daemon socket now performs proof-verified short-lived
issuance without persisting bearer tokens. The standalone Web API now dispatches
the canonical proof-bearing exchange route through the daemon; mTLS transport
verification remains a listener concern. Native DEB/RPM build scripts also compile the
daemon with `--no-default-features`, making the package boundary explicit.
Non-secret v1 JSON fixtures for each credential and capability shape are
checked into the core crate for consumer adapter contract tests.
The daemon now also provides a state-scoped, expiry-pruned single-use replay
registry for completion capabilities and a ring-backed proof verifier bound to
registered public keys. Its completion helper verifies provider state before
consuming, releases on catalogue failure, and returns an idempotent replay
result; live catalogue wiring remains.
The daemon API now publishes a confirmation-bound, path-free revocation
request/response contract for identities and individual public keys, and
authenticated administrator dispatch applies atomic identity/key deactivation.
Credential registration and revocation now append atomically persisted,
reason-digest audit events without secrets or paths; mTLS transport verification
remains listener work.

### Current delivered baseline

The repository already contains the Rust workspace and domain model, portable
metadata foundations, Linux/macOS probing, store policy, appliance SSD/HDD
ingress and placement, Garage orchestration, daemon/client APIs, CLI/TUI, Web
shell and administrator workflows, object browsing/download, EasyConnect
sessions, telemetry foundations, and Mnemosyne adapter/design work. Completed
historical checklists remain below and in TODO as evidence. The bounded folder
and guarded-drive profile seams now also provide catalogue-authoritative S3
read/write/delete/verify/health operations, capacity reconciliation, and a
versioned path-free list transport projection; authenticated HTTP gateway and
multipart dispatch remain deliberately separate. An authenticated profile-S3
HEAD transport now exposes bounded logical key/version/size/checksum metadata
through the same daemon bridge without revealing backend paths.
The same authenticated route family now exposes provider-neutral bounded-profile
health state and message fields; physical SMART/NVMe and appliance topology
remain deployment-gated.

Folder and dedicated-drive backends now reject a manifest whose store identity
does not match the opened backend before any reservation, read, or catalogue
operation can proceed. Appliance placement still remains behind the same
capability contract and awaits deployment validation.

### Open technical debt

The active baseline still has release-relevant gaps: provider upload completion
is not yet an atomic catalogue transaction; Garage reconciliation now uses
durable provider-independent manifest/checkpoint planning plus a per-key Garage
transfer worker with progress and administrator cancellation checks between
provider transfers. Retry jobs now rediscover the newest incomplete manifest
for the same store/prefix scope without following symlinks, and resumed keys
request and append only the missing byte suffix after validating the partial
file. Non-Garage providers and appliance soak acceptance remain open;
daemon/CLI file-ingest pause/throttle/
resume is now available between source objects and through an authenticated Web
admin action (the compact ``ingest control --tui`` acknowledgement is now
available; interactive keyboard controls and live state refresh remain);
control/Web capacity is not fully
reserved under ingest; telemetry missing markers now include actionable
warm-up, counter-reset, permission, and device-mapping details, while stable
device mapping and appliance acceptance remain incomplete; and UI/design work
remains. The module-size guard now passes with no
exceptions. Hardware-only acceptance is deferred while travelling without DAS
access, but offline design, domain, metadata, API, test, and packaging work
should continue.

The profile creation/adoption semantics are now approved and documented: one
bounded folder root maps to one logical ObjectStore, unmanaged edits are
read-only drift until an explicit confirmed reconcile/adopt, and no profile
authority is inferred from a path or name. Protection and compatibility rules
are likewise explicit and profile-independent. New bounded ObjectStores now
initialize their daemon-owned quota ledger before
the store registry definition is published, so creation cannot expose a finite
quota without durable admission state. Per-profile manifest/root registration
and probe-path selection now have a daemon-owned binding registry; explicit
typed create/adopt binding registration transport is now available. Folder
backend opening, quota-ledger initialization, and explicit store-definition
publication are delivered for the supported local path; the CLI exposes the
same validated create/adopt workflow, and bounded CapacityPolicy is initialized
before binding persistence. Full create/adopt orchestration remains the next
capacity/profile seam for live drive/appliance identity probing, cross-file
rollback, and hardware-backed backends. The binding registry path is state-scoped
with an explicit environment override; strict
missing-binding rejection is deferred until local-Docker bootstrap can persist
container-visible roots rather than host-only paths. The provider now exposes an
explicit fail-closed requirement mode for profile-aware callers while legacy
appliance fallback remains compatible. Programmatic bounded-folder creation now
bootstraps the daemon-private namespace and an empty durable catalogue
idempotently without adopting unmanaged user files; explicit adopt/reconcile
reporting and checkpointed source-preserving adoption are delivered, while live
drive opening still requires platform identity probing.
Profile create/adopt responses now carry redacted unmanaged/unsafe drift counts
so callers can require explicit reconciliation without receiving private host
paths; a separately authenticated inspection transport remains open.

The canonical local Docker profile now renders successfully against the
dedicated macOS validation root and both generated Compose documents pass
configuration parsing. Starting the daemon, nested Garage, and the AlleleAnchor
S3 smoke remains Docker-daemon/file-sharing gated.

## Historical Appliance MVP Definition

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
- console users have a supported TUI for file ingress planning, execution,
  reconnect, pressure/bottleneck inspection, and completion review.
- current Synoptikon and Mneion conventions are treated as mutable design inputs
  when a better integrated storage architecture requires coordinated changes
  across affected Mnemosyne software.

Milestones 1-18 delivered a substantial appliance foundation, but their
acceptance and later integration debts remain open. The active campaign gates
take precedence over historical milestone numbering when selecting new work.

The Web interface has a coherent Yew shell and API contracts, but several
current surfaces are holder implementations rather than completed operator
workflows. The Home, Enclosures, ObjectStores, Users/Groups, Stores/SubObject,
and Bioinformatics pages must now be finished against live daemon-backed data
and action routes before the console can be considered operational.

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
- reconcile the Mnematikon-style local auth store with the appliance charter for
  local OS users and sudo-derived administrator status before enabling broader
  standalone administrator workflows;
- disable local auth routes in Synoptikon-integrated mode;
- ensure risky operations still require operation-level confirmation after
  login;
- serve the Yew bundle from the standalone server.

Exit criteria:

- standalone HTTPS starts on `https://127.0.0.1:8448` by default;
- integrated mode rejects local login endpoints and relies on host context;
- authentication tests cover login, session expiry, logout, and integrated
  session behavior;
- the standalone administrator model is explicit: either OS-local sudo users are
  authoritative, or the documented host-mode decision explains why product-local
  users supersede that requirement;
- package/service docs state the permanent port policy.

Current delivery note: the daemon-independent liveness route is covered, and
the Home Web workspace now retains its last successful telemetry snapshot with
explicit stale-data/retry guidance after a refresh failure; cold-start failures
remain visible rather than fabricating appliance state. Full cached appliance
status, static/login saturation tests, and appliance acceptance remain open. A
route-level regression also proves liveness remains HTTP 200 while a daemon-
backed Activity request degrades into a warning-bearing workspace response.

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
- implement Users/Groups workspace for standalone administration where host mode
  allows local user and group management;
- expose ObjectStore and SubObject creation/configuration through Web UI routes
  and Yew surfaces when the existing CLI/domain APIs are stable enough;
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

## Milestone 18: Parallel Ingress Operations and Embedded TUI Views

Goal: make file ingress fast, reliable, observable, and operable from a normal
CLI session as well as the Web UI and Synoptikon-facing adapters.

Priority: this milestone hardens the existing SSD ingest, HDD settlement, daemon
job, CLI progress, and Web Activity work into a supported operations surface. It
SHALL not reintroduce direct CLI mutation of managed DAS roots; all normal
ingest execution remains daemon-owned.

Scope:

- implement a daemon-owned parallel ingress pipeline with distinct stages for
  path scanning, source read, SSD staging, checksum/manifest capture, HDD
  placement, per-target HDD write queues, verification, and finalization;
- prioritize streaming source files to SSD staging while bounded backpressure
  protects SSD capacity, RAM, HDD backlog, verification backlog, and error
  rates;
- use available CPU cores and memory headroom through explicit resource policy,
  bounded buffers, queue limits, and safety reserves;
- distribute staged payloads to final HDD locations through parallel per-disk or
  per-target queues so one slow disk does not stall the whole job;
- emit shared daemon telemetry for file counts, MiB/GiB/TiB data volume,
  staged/written/verified fractions, worker counts, queue depths, SSD pressure,
  HDD pressure, CPU/memory use, bottleneck classification, verification state,
  and throughput trend;
- add durable ingest journals/manifests so interrupted jobs can be resumed,
  cancelled, retried, or reconciled without silent data loss;
- implement supported embedded terminal views for long-running CLI actions that
  can plan, describe, confirm, monitor, control, and summarize ingest jobs
  without introducing a standalone TUI command surface;
- ensure embedded CLI TUI views, the standard CLI progress renderer, Yew
  Activity view, and Synoptikon adapters consume the same daemon job model and
  event stream;
- add benchmark/profiling coverage for small-file, large-file, mixed-file,
  slow-HDD, full-SSD, and interrupted-import scenarios.

Exit criteria:

- before import, users can see file count and total import volume scaled to MiB,
  GiB, or TiB;
- during import, users can see SSD-staged, HDD-written, and verified fractions;
- embedded terminal views show resource policy, active workers, queue depths,
  SSD/HDD pressure, bottleneck stage, throughput trend, verification status, and
  warnings without requiring log inspection;
- interrupted jobs can be resumed or reconciled from the daemon journal;
- benchmark evidence shows configured resource policies can be saturated without
  unbounded memory growth or unverified persistence claims;
- embedded CLI terminal views and Web views agree on job state because both
  consume the same backend events.

## Milestone 19: Web Console Live Data and Grammateus-Aligned Design

Goal: turn the current Yew holder interface into a live, polished operations
console while locking the Mnemosyne Biosciences report-style footer across
every Web page.

Priority: this milestone is the immediate Web completion slice. The existing
top-level navigation, login shell, and dashboard contracts are retained, but
fallback fixtures and placeholder cards must be replaced by authenticated,
daemon-backed data. The page footer must mirror the Mnemosyne Biosciences
Grammateus/Mnematikon presentation style: dark, compact, monospaced,
version-bearing, Mnemosyne-linked, and present on login and authenticated pages.

Scope:

- replace Home dashboard fallback metrics with live daemon/API values for drive
  count, mounted DAS enclosures, total/used/free capacity, seven-day throughput,
  memory stress, SMART warnings, object-store count, and required actions;
- replace Enclosures page empty holder cards with detected supported DAS
  enclosure cards, TL-D800C identity where available, topology, bay/drive
  membership, SSD/HDD role assignment, SMART warnings, and detail panels;
- replace ObjectStores empty holder cards with live store registry cards,
  writer-group membership, public/writeable state, object counts, used
  capacity, object type, redundancy, and service/export state;
- wire Yew pages to fetch the existing `/dashboard/*` and product workspace
  payloads rather than rendering static fallback functions;
- reconcile the legacy operations workspaces with the redesigned Home,
  Enclosures, ObjectStores, and Bioinformatics navigation so there is one
  coherent product surface;
- implement a reusable DASObjectStore footer component matching the
  Mnemosyne/Grammateus report footer style and apply it to login and all
  authenticated pages;
- add visual and component regression tests for desktop and mobile layouts,
  including footer fidelity, top-bar behavior, card density, empty states, and
  permission-denied states.

Exit criteria:

- loading the Web UI after login shows live daemon-backed values rather than
  "pending" fixture text when the daemon can provide data;
- every top-level page has the Mnemosyne Biosciences footer in the approved
  style and includes product version/provenance information;
- the Home, Enclosures, and ObjectStores pages can be used to understand the
  appliance without reading CLI JSON;
- screenshots prove the footer, top bar, card grid, and empty/error states are
  stable on desktop and mobile widths.

## Milestone 20: Web Administrator Workflows and Bioinformatics Readiness

Goal: complete the currently advertised Web workflows for administrators,
writer groups, ObjectStores/SubObjects, and bioinformatics orchestration.

Priority: this milestone follows Milestone 19. It converts the current action
holders into confirmed, daemon-submitted workflows with risk gates and audit
metadata. No Web workflow may directly mutate managed DAS roots.

Scope:

- implement the Enclosures "Add enclosure" workflow: supported DAS detection,
  SSD/HDD identification, data-loss review, format/prepare confirmation, daemon
  job submission, progress, cancellation, and result review;
- implement ObjectStore creation and configuration from the Web UI using
  `/opt/dasobjectstore/groups.json`, supported object types, enclosure
  anchoring, redundancy, public/writeable policy, store class, and S3 export
  options;
- implement SubObject creation/configuration surfaces for nested prefixes and
  object-service routing once the backing registry action plan is accepted;
- expose Users/Groups as a first-class navigation surface when host mode allows
  local administration, with current OS authority, group creation, local
  user-to-group assignment, and writer-policy readiness;
- expose administrator endpoint-inventory workflows for validated DAS,
  NAS/NFS, S3-compatible, and Mnemosyne-governed storage endpoints through the
  same daemon-owned registry and audit boundary used by Activity;
- implement authenticated action planning, confirmation, submission, progress,
  failure, and audit review for all risky Web administrator workflows;
- replace the Bioinformatics placeholder with workflow-readiness cards for BAM,
  CRAM, POD5, FASTQ/FASTQ.GZ, FASTA, VCF/BCF, GFF/GTF, ENA/SRA datasets,
  sequencing run provenance, object lineage, and downstream analysis handoff;
- add Web Activity views that show submitted admin and ingest jobs using the
  same daemon job/event model as the CLI embedded TUI.

Exit criteria:

- administrators can prepare a supported DAS enclosure and create an
  ObjectStore from the Web UI without shell-only procedures;
- non-administrators see useful inventory and explicit permission-denied
  states without seeing unsafe controls as available actions;
- ObjectStore and SubObject workflows create the same registry/domain records
  as the CLI paths and are covered by API/Yew tests;
- endpoint inventory records can be created or updated through authenticated
  administrator Web workflows without browser-side mutation of
  `/opt/dasobjectstore/endpoints.json`;
- Bioinformatics pages identify workflow-ready datasets and expose clear
  handoff state for basecalling and genome/transcriptome analysis workflows;
- all risky operations are gated, auditable, daemon-owned, and recoverable.

## Milestone 21: ObjectStore Web File Browser and Download Workflows

Goal: provide a high-quality Web file browser for ObjectStores so users can
inspect imported folder hierarchies, understand where data is physically
stored, and download individual files or whole folders without shell access.

Priority: this milestone follows the live ObjectStore inventory work. It turns
ObjectStore cards into browsable data surfaces using standard filesystem
metaphors while preserving DASObjectStore placement, permission, and streaming
boundaries. The browser must feel native to the Mnemosyne/DASObjectStore Web
console: compact, professional, fast with large trees, and clear about storage
location and durability.

Scope:

- add daemon/API contracts for ObjectStore tree browsing, including folder
  nodes, file nodes, object type, object size, modification/import timestamps,
  checksum state, copy count, and the disk IDs/labels on which each file copy
  resides;
- implement paged and searchable object-tree queries so large ObjectStores with
  many thousands of files remain responsive and do not require loading the
  entire tree into the browser;
- implement a Yew ObjectStore file browser using familiar filesystem metaphors:
  breadcrumb navigation, expandable folder hierarchy, sortable file lists,
  size columns, disk placement badges, object-type badges, empty-folder states,
  loading/error/permission states, and keyboard-accessible selection;
- expose authenticated download routes for individual files, with policy checks
  against ObjectStore visibility, writer/read group membership, public state,
  and object lifecycle state before streaming any bytes;
- expose authenticated folder archive downloads that stream a `tar.gz` archive
  for a selected folder prefix without staging the complete archive on SSD or
  HDD, and with cancellation-aware cleanup for interrupted downloads;
- display physical placement honestly: for each file show the disk or disks
  holding settled copies, degraded/missing-copy warnings, and whether the file
  is still on SSD, fully settled to HDD, redownload-required, or unavailable;
- add operator-focused performance safeguards: pagination limits, bounded API
  response size, server-side filtering, lazy folder expansion, streaming
  backpressure, range/download headers where practical, and archive-size
  preflight estimates before folder download;
- add tests for tree construction from metadata, permissions, file download,
  folder archive generation, interrupted archive cleanup, large-tree paging,
  and Web rendering of dense folder/file listings.

Exit criteria:

- selecting an ObjectStore opens a polished browsable tree that mirrors the
  imported folder hierarchy and scales to production-sized cohorts;
- users can download individual files and `tar.gz` archives of whole folders
  through the Web interface, subject to the same permissions and lifecycle rules
  as CLI/API access;
- file rows clearly show size, object type, checksum/readiness, and the disk or
  disks where data is physically stored;
- browser interactions remain responsive with large ObjectStores and do not
  require the browser to hold the full object inventory in memory;
- API, daemon, archive, permission, and Yew regression tests cover the primary
  and failure paths.

## Milestone 22: Remote Easyconnect Uploads and Ingress Policy Simplification

Goal: make remote uploads from laboratory, analysis, and laptop workstations as
simple as authenticating to the DAS appliance in a browser, selecting an
ObjectStore, and dragging local files or folders into a browser-assisted upload
surface powered by the `dasobjectstore-remote` CLI agent.

Priority: this milestone builds on the existing `make remote` client packaging
and Web ObjectStore work. It is a product workflow, not a developer diagnostic:
users with data on remote computers should not have to manually configure S3
credentials, bucket names, temporary policy files, or daemon socket details
before uploading to the DAS.

Scope:

- add `dasobjectstore-remote easyconnect <host-or-ip>` so a remote workstation
  can discover a DASObjectStore appliance such as `192.168.1.192`, open the
  system browser to the appliance Web authentication flow, and bind the local
  remote CLI agent to an authenticated upload session;
- implement a browser-mediated device/session authorization flow with local OS
  browser launch, short-lived one-time pairing tokens, secure local callback or
  polling fallback, and a renewable session suitable for at least eight hours
  of upload work;
- support standalone local-user authentication on the appliance first, while
  keeping the flow compatible with later Synoptikon/Mneion identity providers;
- expose an authenticated remote upload page listing the ObjectStores available
  to the user, including writer/readiness state, object type, capacity signals,
  and whether uploads are currently allowed;
- implement drag-and-drop file and folder selection in the Web page while the
  actual byte transfer is performed by the paired `dasobjectstore-remote`
  process on the remote workstation;
- drive uploads through intended S3-compatible object-service capabilities
  where practical, using credentials/session material issued by the appliance
  and not requiring the user to type S3 bucket names or keys;
- ensure remote uploads and direct Web uploads are staged to the ObjectStore
  SSD first, then settled to HDD through the daemon-owned ingress pipeline;
- simplify server-side ingress placement policy: local server-side ingress uses
  direct-to-HDD writing, while S3, Web, and remote-agent ingress always stages
  to SSD before HDD settlement;
- define the default HDD landing concurrency as
  `max(number_of_hdds_in_enclosure - 2, 2)`, subject to one active writer per
  physical HDD and never placing redundant copies on the same disk;
- apply the same landing-worker rule for SSD destage and local direct-to-HDD
  ingress, with bounded queues, visible backpressure, and clear behavior when
  the enclosure has too few eligible HDDs to satisfy policy safely;
- add resumable/cancellable remote upload semantics so interrupted browser
  sessions, closed laptops, expired sessions, and network loss do not leave
  orphaned partial objects or ambiguous metadata;
- add Web, remote CLI, daemon, object-service, and documentation coverage for
  large folders, large files, many small files, session renewal, permission
  denial, and upload cancellation.

Exit criteria:

- `dasobjectstore-remote easyconnect 192.168.1.192` opens the appliance browser
  login flow and pairs the remote CLI without exposing passwords or S3 keys on
  screen;
- the authenticated remote upload session lasts for eight hours by default and
  can renew safely during long ingress operations;
- users can select an ObjectStore from the browser and drag-drop files or
  folders for upload, while the remote CLI agent performs the actual transfer;
- remote and Web uploads land on SSD first and then settle to HDD using the
  daemon job model, telemetry, cancellation, and cleanup semantics;
- local server-side ingest bypasses SSD staging only when it is truly local to
  the DAS appliance and policy permits direct-to-HDD writes;
- landing concurrency is deterministic from enclosure HDD count, never assigns
  two active writers to the same HDD, and preserves redundancy rules;
- user documentation explains the easyconnect workflow, authentication/session
  model, ObjectStore selection, drag-and-drop behavior, ingress placement rules,
  failure states, and recovery expectations.

## Milestone 23: Appliance Telemetry, Home Dashboard Graphs, and floundeR Time-Series Contracts

Goal: turn the Home dashboard into a live appliance observability surface with
scientifically defensible telemetry charts for CPU, memory, IO, capacity,
throughput, and active users, while extracting general floundeR plotting
contracts that can be reused across the broader Mnemosyne product family.

Priority: this milestone builds on the Web console Home dashboard and the DAS
enclosure identity model. Operators need to understand current system stress,
historical IO behavior, and active-user state without shell access. The charts
must be useful during real ingestion and benchmarking, must update without
screen jitter, and must treat missing development data honestly rather than
drawing misleading interpolated lines.

Scope:

- add a daemon-owned telemetry collector for appliance CPU usage, memory usage,
  active DASObjectStore/Web sessions, disk capacity, and per-disk IO for every
  disk physically associated with known DAS enclosures;
- store telemetry in a bounded, appropriately sized JSON time-series file under
  the managed appliance state tree, with explicit schema versioning, retention
  limits, atomic writes, and crash-tolerant recovery from partial/corrupt files;
- support configurable sampling cadence, initially allowing practical values
  such as 6 seconds for high-resolution operator views and 30 seconds for
  lower-overhead long-running appliance monitoring;
- expose telemetry API contracts for recent samples, downsampled historical
  windows, current point-in-time summaries, missing-data intervals, and
  enclosure/disk identity labels;
- add Home dashboard cards for IO, logged-in users, and CPU usage alongside
  existing Capacity, Throughput, and Memory Stress cards;
- ensure cards whose values overlap telemetry use the telemetry stream as the
  authoritative source and include compact sparkline or chart summaries where
  appropriate;
- implement global Web time-window controls for telemetry graphs with at least
  `1 hour`, `1 day`, `10 days`, and `3 months` windows, applied consistently to
  all Home dashboard telemetry charts;
- update charts at the telemetry cadence without layout shift, card resizing,
  text overlap, or jitter, using stable dimensions and bounded redraw work;
- define and implement floundeR general-purpose chart contracts for appliance
  time-series and interval data, including line charts with gaps, stepped or
  point summaries where scientifically more appropriate, capacity bands,
  per-disk IO traces, and small-multiple views;
- ensure floundeR chart semantics avoid false continuity: missing samples,
  service downtime, unknown devices, and unavailable counters must be shown as
  gaps or explicitly labelled missing intervals rather than interpolated lines;
- produce Web-consumable chart artifacts or data contracts that can be rendered
  efficiently in Yew while remaining compatible with formal report generation
  in Grammateus/floundeR;
- add tests for telemetry sampling, JSON retention and truncation, corrupt-file
  recovery, cadence configuration, per-enclosure disk filtering, downsampling,
  missing-data handling, Web chart DTOs, and stable dashboard rendering.

Exit criteria:

- the daemon continuously records bounded telemetry for CPU, memory, capacity,
  per-enclosure disk IO, and active users without unbounded JSON growth;
- the Home dashboard exposes Capacity, Throughput, Memory Stress, IO, logged-in
  users, and CPU cards backed by the same telemetry model;
- operators can switch all Home telemetry charts between 1 hour, 1 day, 10 day,
  and 3 month windows without page jitter or misleading redraw behavior;
- missing telemetry is represented honestly as gaps or labelled missing
  intervals, never as fabricated continuity;
- floundeR gains reusable, Mnemosyne-wide chart contracts for appliance
  telemetry and scientifically correct missing-data rendering;
- API, daemon, Web, and floundeR regression tests cover normal collection,
  missing/corrupt data, time-window changes, and chart rendering contracts.

Current delivery note: the offline collector matrix now covers direct SATA,
partition, stable USB alias, device-mapper alias, and missing-device fixtures,
including warm-up and non-zero rate transitions. Authoritative enclosure
topology and packaged-loop verification remain appliance-dependent.
The operator recovery runbook is now published in the user guide; it keeps
Home/API as the normal inspection surface, documents read-only state and
marker evidence, and separates idle, warm-up, missing-device, and stale-state
diagnostics from the remaining appliance acceptance gate.

## Milestone 24: Mnemosyne Design Language Alignment (Active Historical Work)

Goal: align shared Web primitives, footer/provenance, contextual task panes,
Local Access, Endpoints, and ObjectStore-scoped remote upload with the approved
Mnemosyne product language. Detailed tasks and visual acceptance remain in
TODO Milestone 24 and must not outrun storage/profile contracts.

Current delivery note: the shared footer now uses the approved local Mnemosyne
wordmark and partial mark, a ``#1c2b0b`` provenance surface, responsive
application-shell layout, and pinned local asset provenance tests. Shared
semantic interaction/status tokens now cover primary actions, focus, warning,
danger, and success states without reusing Mnemosyne green as a generic action.
The shared Yew TaskPane primitive now provides explicit Closed/Create/Edit/Review
state, focus/escape behavior, selected context, labelled form content, and
footer actions. Shared table, status badge, capacity, segmented-control,
icon-button, inspector, and risky-confirmation primitives now have responsive
CSS and host-safe semantic source contracts; page-flow refactors remain open
work in TODO Milestone 24.

Activity report dropzone and progress styles now live in a dedicated
`styles/reporting.css` feature sheet, with registration/order and base-leakage
tests; remaining global CSS and screenshot artifact work stays open.

Enclosure wizard selectors now live in the feature-owned `styles/enclosures.css`
sheet; shared form/review primitives remain in the base sheet with ownership
tests.

Milestone 24.5 now has focused source/component coverage for footer mounting and
decorative-mark ownership, TaskPane keyboard/focus boundaries, Local Access
qualification, endpoint add/edit confirmation, and target-scoped upload
gating; browser visual artifacts remain environment-gated.

The screenshot runner fixture now supplies the authenticated v1 health payload,
tracks the home telemetry and users-first inventory selectors, and includes the
mounted enclosure inventory required by the ObjectStore creation form;
the runner now also awaits asynchronous task-pane closure. Full visual/DOM
acceptance still requires a complete desktop/mobile runner pass; this checkout's
Trunk packaging currently exits without a JavaScript bundle.

The target-scoped Remote Upload Web boundary now requires a concrete
ObjectStore ID in page props and percent-encodes it in the workspace route;
the authenticated shell renders an explicit target-required state instead of
mounting a dropzone without a target. API/permission and visual acceptance
remain part of the open Milestone 24 work.

The no-target state also offers a direct return action to ObjectStores, keeping
the target-scoped workflow navigable without exposing an unscoped upload form.

Remote-upload setup failures now release already-admitted capacity reservations
before returning, with an invalid-job regression; public completion metadata and
authentication remain contract-gated.

Guarded direct-to-HDD imports now use the shared inline-hash copy primitive;
optional source metadata is checked after the single read, preserving the
direct-ingress policy without a strict pre-copy hash pass.

## Milestone 25: Campaign Re-baseline and Compatibility Contracts

Goal: turn the appliance implementation into an explicit multi-profile product
without destabilizing existing pools or metadata.

Scope includes deployment-profile and host-mode decisions, backend capability
contracts, portable placements/protection states, universal capacity policy,
S3 authority, compatibility/versioning, and migration design.

Exit criteria: architecture and requirements are approved; existing appliance
metadata remains readable; public and persistent compatibility boundaries have
tests before profile-specific implementation begins.

Current delivery note: the strict ObjectStore manifest v1 remains unchanged.
The core now provides a separate ``portable_object_catalogue.v1`` companion
contract for logical object versions, digests, provenance,
lifecycle/protection state, and profile-neutral folder/drive/appliance/provider
placements. Unknown/future schema, duplicate-record, and unsafe-path checks
are covered locally, and exporters now validate before emitting JSON; daemon
catalogue authority and profile-adoption wiring remain open pending the shared
metadata decision.

The core now exposes a minimal profile-neutral `ObjectCatalogueAuthority`
batch contract, with the durable folder catalogue implementing it while
SQLite/object-service, appliance, and daemon transaction adapters remain open.
Authority batch conflict and restart tests prove a rejected version cannot
partially persist a companion record.
The backend capability contract now also publishes a stable operation
vocabulary plus missing-operation projection, allowing profile adapters to
explain unsupported operations without maintaining a second field mapping.
The SSD-backed drive profile now delegates the same authority contract through
its validated folder engine and rejects reads or mutations when the drive
runtime guard reports loss of the mounted device.
Logical used-byte accounting now uses one overflow-safe authority helper for
folder-profile initialization; daemon registry/provider reconciliation remains
open.
Folder adoption now uses the same authority batch seam rather than a concrete
catalogue call site; richer daemon transaction and SQLite/object-service
integration remain open.
Folder-to-folder and folder-to-drive migrations now commit the verified
destination record through the shared authority before advancing migration
state; destination reopen coverage proves catalogue-backed logical usage.

## Milestone 26: Appliance Debt and Control-Plane Readiness

Goal: close current daemon ownership, upload completion, reconciliation,
availability, telemetry, module-size, packaging, and soak-test gaps.

Exit criteria: uploads are not complete before catalogue finalization; control
requests retain bounded capacity under ingest; no temporary production module
exceptions remain and the guard passes; appliance-only acceptance blockers are
recorded and repeatable.

The remote-upload completion gate now validates the job and ObjectStore
identifiers before invoking an injected catalogue/manifest commit. Malformed
handoff records therefore fail closed without publishing completion or creating
an unreconcilable catalogue transaction; provider-backed catalogue wiring and
public completion endpoints remain separately gated.

Current delivery note: daemon-independent health/liveness and degraded Activity
responses are covered, including a local bridge-saturation regression for both
health surfaces. Standalone static asset reads now use an async bounded
lane with explicit no-cache index/unfingerprinted and immutable fingerprinted
asset cache headers; a deterministic permit-saturation test proves the lane
fails fast and recovers. Local standalone login remains independent of an
unrelated saturated daemon bridge. The authenticated dashboard status route
now retains a last-successful snapshot and reports stale/retry metadata, while
cold starts fail closed. Authenticated Web cancellation also retains its
reserved priority bridge while routine admin capacity is saturated. Appliance
soak and telemetry freshness remain open; a daemon-backed object-browser
request now has a deterministic typed-429 saturation regression as well.

The daemon API also exposes a typed ingest admission decision combining
source-read pressure/error backpressure with adaptive worker scheduling. It
reports run/throttle/block and the limiting schedule reason; live host resource
telemetry and HTTP bridging remain open.

A transactional resource gate now prevents concurrent daemon jobs from
overbooking CPU, memory, socket-worker, or I/O-worker budgets and releases
leases automatically on scope exit. The packaged daemon now loads the ingest
resource policy from its versioned runtime config (with a safe legacy default)
and injects that policy into local file-ingest reservations; host telemetry
remains open.

Packaged local file ingest now acquires a bounded shared resource lease before
source enumeration and releases it on every completion/error path. The lease
budget is selected from the daemon-configured ingest policy rather than a
hard-coded runtime default. Garage S3 reconciliation uses the same injected
gate before handing staged provider data to local ingest.

The TUI now renders an optional daemon admission action, limiting reason, and
worker schedule alongside live ingest telemetry; Web bridging and host-level
availability counters remain open.

The Web client now has a typed cached-status response and path helper/getter;
the existing Home page remains on its live response until stale-data UX is
intentionally adopted.

The authenticated Web API now exposes a store-scoped capacity-status route
through the bounded daemon bridge, so live logical/backend/SSD admission data
can be consumed without a direct registry read. The Web client now has a typed
getter/path helper; appliance-backed acceptance remains open.

The normal CLI store-creation path now submits a typed daemon request when a
writer group and packaged daemon socket are present. Host-registry mutation is
retained only for explicit registry/test and no-writer-group migration paths;
portable SSD mirroring remains a separate compatibility concern.

The daemon-ownership backlog is now tracked as partial rather than open: store
drain/delete, ingest queue drain, object put, disk retirement, and normal
writer-group creation have typed runtime operations and regression coverage.
CLI migration fallbacks and any remaining managed mutation require their own
daemon runtime operation before they can be marked complete.

The profile-by-host-mode support matrix and fail-closed upgrade/migration
policy are published in `docs/user/storage-profile-matrix.rst`; preview rows
remain distinct from DASServer/Garage acceptance and product-adapter blocks.

## Milestone 27: Universal Capacity and Reservation Policy

Goal: make every ObjectStore explicitly capacity-governed.

Scope includes logical quotas, backend reserve, warning/critical thresholds,
transactional reservations, multipart/concurrent admission, physical
amplification, nested SubObject budgets, over-quota behavior, and observability.

Exit criteria: ``folder`` requires a finite quota; drive/appliance ``unlimited``
means backend-bounded; no concurrent ingress can overbook the same capacity;
quota reduction never deletes data.

Current delivery note: the authenticated read-only capacity-admission daemon
route and typed client contract are in place, and the packaged daemon now
injects a registry-backed provider with persisted ledgers and ``statvfs``
probes. Ingest/S3/multipart reservation completion and catalogue accounting
remain open; explicit stale-reservation maintenance is now delivered below.
The provider also exposes durable commit/release lifecycle operations with
rollback on persistence failure. The daemon-owned remote S3 transfer worker
now retains the job ID as the reservation ID, admits before invoking the typed
byte-transfer adapter, and commits or releases after transfer and catalogue
completion; rejection is persisted as a failed job. Local ingest, multipart,
catalogue accounting remain open. Stale reservations now carry durable
creation timestamps in schema-v2 ledger snapshots (legacy v1 snapshots remain
loadable), and the provider exposes an atomic caller-scheduled expiry sweep.
Typed multipart-style byte-transfer adapters are covered by the same admission
regression contract: rejection happens before adapter invocation, while
success/failure paths commit or release the daemon reservation. A concrete
multipart API and catalogue accounting path is still pending.
Unknown-age legacy reservations are retained; automatic scheduling and renewal
remain open until a lease policy is approved.
Local file ingest now uses the same provider boundary for each non-skipped
object: admission occurs before source/staging or direct-HDD work, durable
settlement commits the reservation, and failed jobs release outstanding IDs.
Garage S3 reconciliation now passes its controller-owned provider into that
worker as well; multipart paths still need explicit provider injection.
The core SubObject capacity ledger now has a strict, schema-versioned snapshot
contract that preserves parent/child reservation links and rejects inconsistent
restart state; daemon registry persistence and transport wiring remain open.
The profile S3 seam now publishes bounded multipart completion metadata
(ordered parts, per-part SHA-256 checksums, total-size and reservation
validation) and a streaming assembler that verifies each declared part;
the assembler now feeds the same transactional PUT lifecycle, with and without
the daemon logical-capacity provider. HTTP dispatch remains.
The transport boundary now also publishes versioned, path-free multipart
completion request and acknowledgement DTOs with the same bounded validation;
all profile-S3 object responses reject unsafe logical keys, zero versions, and
malformed SHA-256 checksums before transport dispatch;
list prefixes apply the same relative-namespace boundary while allowing a
trailing slash for ordinary namespace queries;
the empty prefix remains the valid unfiltered namespace root for list and page
helpers;
provider-neutral runtime list/page, PUT, HEAD, DELETE, and multipart entry
points enforce the same key boundary before backend mutation;
bounded profile streaming now hashes bytes in flight and rejects catalogue
checksum drift before reporting success; the general profile GET reader now
performs the same declared-size and SHA-256 verification while bytes are
consumed;
the daemon socket now dispatches authenticated bounded profile-S3 list requests
through the registered folder binding and daemon-owned capacity policy, while
the standalone authenticated Web route delegates through the same bounded
daemon bridge. Authenticated Web list, HEAD, and health routes now share typed
daemon projections and never expose backend paths; a path-free profile
diagnostics projection and authenticated Web route distinguish empty,
synchronized, uncatalogued, and catalogue-missing states with counts and
reconciliation timestamps. Authenticated Web verification and the read-only
``store profile-verify`` CLI now require daemon-side size/checksum agreement
before reporting success and return no backend location. Multipart HTTP
routing remains separate.
The same path-free profile-readiness projection is now available through the
authenticated Web route ``/api/v1/profile-readiness/stores/{store_id}``, using
the bounded daemon bridge and preserving explicit not-ready reasons.
Static profile-capability discovery is also available through authenticated
Web at ``/api/v1/profile-capabilities``; the response remains a versioned
catalogue and never implies provisioning or live hardware health.
The application access-token contract now publishes one canonical versioned
HTTPS exchange route for clients and adapters, and the standalone Web API
dispatches it through the daemon; listener authentication and mTLS verification
remain deployment-layer work.
Stable profile-S3 route constants now identify bounded object listing and
reservation-bound multipart completion without introducing an HTTP listener;
listener authentication, request routing, and runtime store dispatch remain.
Capacity-enabled local ingest also rejects a client copy-count override before
any source read when it differs from the daemon ObjectStore policy; legacy
standalone executor paths retain their explicit override behavior.
Reservation IDs include the client request identity when supplied, otherwise a
stable source-path digest, preventing unrelated same-second jobs from
colliding while preserving deterministic retries for the same source.

## Milestone 28: Folder ObjectStore Profile

Goal: manage one bounded directory with hierarchical files, portable manifests,
atomic ingress, drift detection, safe adoption, browsing, and common S3 access.

Current delivery note: read-only inspection now has an explicit opt-in adoption
executor. It preserves user files, stages through the hardened folder backend,
verifies and durably finalizes each object, and checkpoints InProgress/Complete/
Failed states atomically for restart-safe retries. A versioned private folder
catalogue snapshot is committed idempotently before Complete. A bounded,
profile-neutral read-only browser projection now queries those authoritative
records without inventing appliance placement/lifecycle fields. Daemon-backed
profile HEAD, verification, capacity, health, readiness, and authenticated Web
capability/readiness routes are also available without exposing private paths;
shared SQLite catalogue authority, provider-backed streaming download,
repair/lifecycle orchestration, and full S3 HTTP integration remain open. The
provider-neutral runtime now also offers a bounded writer-stream helper that
checks catalogue authority and exact byte length before success; HTTP framing
and provider-backed transport remain separate.

Exit criteria: system and per-user deployments can create/adopt, ingest, verify,
reconcile, browse, expose through S3, restart, and recover a folder store without
following symlinks or accepting unmanaged changes silently.

## Milestone 29: Dedicated SSD Drive Profile

Goal: manage one dedicated SSD filesystem with the same logical contracts.

Exit criteria: mount/device identity, non-rotational validation, reserve,
capacity, SMART/NVMe telemetry, ingress, S3, Web operation, replacement, and
import/export are tested; the UI states honestly that this is one failure domain.
The guarded drive backend now exposes the same bounded, profile-neutral
catalogue projection as the folder profile without inventing placement rows;
mount/device, SMART/NVMe, and shared catalogue acceptance remain hardware or
contract gated.

## Milestone 30: Profile-Aware Product and Mnemosyne Integration

Goal: let other Mnemosyne Biosciences products request storage capabilities
without embedding appliance assumptions.

Exit criteria: shared API/schema and adapters support folder, drive, and
appliance; capabilities drive UI/actions; product packages can provision a
bounded store idempotently; Synoptikon/Mneion integration and standalone modes
use the same daemon job model.

## Milestone 31: Migration, Protection, and Market-Readiness Acceptance

Goal: promote data safely between profiles and prove operational behavior.

Exit criteria: folder-to-drive/appliance migration preserves identities,
versions, hashes, and provenance; protection policies are explicit; source data
is retained until confirmed retirement; package, upgrade, quota, S3, recovery,
security, performance, and real-workload acceptance gates pass.

## Post-Campaign Direction

Post-MVP work may include:

- long-duration soak testing for production claims;
- store-level encryption;
- coarse HDD zones such as fast, bulk, and archive;
- DASObjectStore-native parity or erasure policies;
- read/write SMB/NFS ingest semantics;
- richer notifications and Prometheus-style metrics;
- native service management where Docker/Compose is not ideal.
