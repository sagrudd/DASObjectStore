# Architecture

Status: Draft  
Scope: high-level crate and module boundaries for the Rust workspace

## Design Intent

DASObjectStore is a Rust-first system with clear boundaries between domain
logic, platform probing, persistent metadata, service orchestration, CLI, and
adapters.

The architecture should keep the public core useful without Mnemosyne while
leaving a clean adapter path for Monas and Synoptikon integration.

## Deployment Profiles and Host Modes

The storage backend SHALL be selected by explicit deployment profile rather
than appliance-specific branching throughout clients and services:

- ``folder`` owns one bounded managed directory;
- ``drive`` owns one dedicated validated SSD filesystem;
- ``appliance`` owns SSD landing plus managed HDD placement.

Backends SHALL expose narrow capabilities for validation, capacity reservation,
staged writing, durable atomic finalization, readable placement, verification,
health, reconciliation, and safe removal. Domain/service code SHALL decide from
capabilities such as atomic rename, redundancy, repairability, telemetry, and
external-provider support.

Host mode is independent: a per-user daemon uses XDG-owned state/socket paths;
a system daemon owns shared folder, drive, and appliance deployments. Neither
mode permits clients to mutate managed roots directly.

The portable manifest model SHALL represent logical objects separately from
backend-specific placements and SHALL carry protection state honestly. Folder
and drive profiles are single failure domains unless an explicit external
protection policy is satisfied.

Capacity admission is a daemon-owned transaction spanning logical quota,
outstanding reservations, backend reserve, staging, and copy amplification.
Filesystem free-space observation audits the transaction; it does not replace
the quota ledger.

## Server/Client Boundary

DASObjectStore is an enterprise server/client appliance, not a CLI tool that
directly mutates managed disks.

The managed storage authority SHALL be `dasobjectstored`. It owns disk
discovery, managed mount validation, SSD ingest staging, HDD placement,
destage, health-state mutation, disk retirement, repair, object-service
orchestration, and persistent metadata writes.

The `dasobjectstore` CLI is a client. It may parse operator intent, authenticate
to the daemon, stream file bytes, submit local import jobs, and render progress.
It SHALL NOT write directly into managed DAS roots during normal operation.

The standalone HTTPS server, Yew GUI, Synoptikon product mount, and future
Mneion integrations SHALL use the same daemon-owned API/job boundary. Smaller
single-host deployments MAY package the components together, but the storage
mutation actor boundary must remain explicit in code, tests, documentation, and
packaging.

The preferred local daemon transport is a Unix-domain socket with peer
credential inspection on Linux. HTTPS remains the browser and remote appliance
surface. Authorization decisions should combine local group policy, standalone
sessions, or Synoptikon-provided actor context with store-level writer/admin
policy.

Remote S3 writes follow the same authority boundary. The accepted direct
ingress decision places a feature-gated DASObjectStore S3 gateway in front of
Garage, authenticates an exact managed credential-to-store/bucket binding, and
streams bytes through the daemon provider protocol into a store-private managed
SSD namespace. Garage remains available in legacy mode and as a recovery source;
it is not authoritative for direct-object placement. See
[DASObjectStore-owned direct S3 ingress](direct-s3-ingress-adr.md).

## Workspace Crates

### `dasobjectstore-core`

Owns domain types and state machines.

Responsibilities:

- typed identifiers;
- pool, disk, enclosure, store, object, ingest, health, and repair states;
- store policy types;
- placement policy inputs and outputs;
- domain validation that does not require OS calls or persistence.

Must not depend on:

- CLI parsing;
- SQLite or filesystem persistence;
- platform command execution;
- object-service provider implementations;
- Mnemosyne-specific contracts.

### `dasobjectstore-cli`

Owns command-line parsing and client command dispatch.

Responsibilities:

- `clap` command definitions;
- user-facing argument parsing;
- command output formatting;
- daemon client request construction;
- local progress rendering from daemon job events;
- developer-only local execution fallbacks where explicitly marked unsafe.

Must not own domain rules or normal managed-storage mutation. CLI code should
translate user intent into requests against `dasobjectstored` or lower-level
crates only for tests and explicitly documented developer fallbacks.

### `dasobjectstore-daemon`

Owns the managed storage runtime.

Responsibilities:

- daemon configuration and lifecycle;
- Unix socket and internal API boundary;
- authorization from peer credentials, local sessions, or Synoptikon actor
  context;
- job submission, cancellation, progress, and audit;
- execution of ingest, direct reproducible import, destage, drain, retire,
  repair, service orchestration, and metadata mutation;
- systemd/package integration points for managed state directories and logs.

Must not expose raw POSIX storage paths as tenant-facing contracts.

### `dasobjectstore-platform`

Owns host and hardware inspection boundaries.

Responsibilities:

- Linux and macOS disk probing;
- enclosure/topology observation;
- platform-specific command/API adapters;
- structured probe output;
- fixture-backed parsing tests.

Must not decide long-term placement policy or mutate persistent pool metadata
directly.

Probe results are best-effort observations. Known USB bridge and SMART
limitations are documented in [Platform Probing Notes](probing.md).
macOS development and read/export limits are documented in
[macOS Development and Read/Export Notes](macos-development.md).

### `dasobjectstore-metadata`

Owns persistent pool metadata.

Responsibilities:

- live SQLite metadata schema;
- metadata migrations;
- canonical manifest export/import;
- append-only placement log export/import;
- dirty attach and clean eject markers;
- recovery-oriented snapshot handling.

Must treat metadata formats as compatibility-sensitive public surfaces.

### `dasobjectstore-object-service`

Owns object-service orchestration boundaries.

Responsibilities:

- object-service provider traits;
- generated Docker/Compose configuration;
- per-store service credential references;
- service status inspection;
- provider-specific integration for the selected MVP object service.

Must not hide object placement or protection state from core/metadata.

Docker/Compose behavior, including macOS Docker Desktop limits, is documented in
[Service Orchestration Notes](service-orchestration.md).

### `dasobjectstore-gui-api`

Owns GUI-facing HTTP/API scaffolding and client adapters for the daemon.

Responsibilities:

- `axum` router construction;
- GUI view models for pool, disk, ingest, health, queue, and warning views;
- safe API action boundaries for operations exposed through the GUI;
- daemon client calls for storage-mutating actions;
- serialization contracts consumed by future Yew views.

Must not duplicate core storage rules or mutate persistent metadata directly.

### `dasobjectstore-gui-web`

Owns Yew frontend scaffolding for GUI delivery.

Responsibilities:

- Yew root components;
- Monas and Synoptikon mount-path metadata;
- frontend-facing view composition over GUI API contracts.

Must stay deployable through sibling Monas and Synoptikon surfaces rather than
introducing an unrelated frontend host.

### `dasobjectstore-mnemosyne`

Owns optional Mnemosyne, Monas, and Synoptikon integration.

Responsibilities:

- Mneion-compatible storage definition export;
- Mneion-compatible binding snippet export;
- future registration and verification commands;
- alignment with Monas and Synoptikon delivery surfaces.

Must not make Mnemosyne a runtime dependency of the public core.

## Future GUI Boundary

The eventual GUI should stay Rust-native:

- `axum` for GUI-facing HTTP/API surfaces;
- `yew` for frontend views;
- delivery aligned with sibling Monas and Synoptikon surfaces.

GUI code should consume view models produced from domain and metadata services
rather than duplicating storage rules.

## Dependency Direction

Preferred dependency direction:

```text
CLI / GUI / adapters
  -> daemon API client
    -> daemon runtime
      -> object-service / platform / metadata
    -> core
```

`dasobjectstore-core` should remain the lowest-level project crate.

Platform and metadata crates may depend on core types, but core must not depend
on platform or metadata implementation.

Adapters may depend on public core and metadata types, but public core must not
depend on adapters.

## Testing Boundaries

Each crate should test the behavior it owns:

- core: pure domain and state transition tests;
- CLI: command shape and output behavior;
- daemon: authorization, job lifecycle, progress events, and managed mutation
  execution;
- platform: fixture-based parser tests;
- metadata: schema, migration, snapshot, and recovery tests;
- object-service: rendered configuration and provider behavior tests;
- adapter: exported contract fixtures.

Hardware-dependent tests should be opt-in and clearly marked.

## File Size and Churn

Modules should be split when they start mixing responsibilities such as parsing,
domain validation, persistence, service orchestration, or presentation.

Small files with explicit names are preferred over broad modules with hidden
cross-cutting behavior.
