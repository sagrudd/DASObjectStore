# Architecture

Status: Draft  
Scope: high-level crate and module boundaries for the Rust workspace

## Design Intent

DASObjectStore is a Rust-first system with clear boundaries between domain
logic, platform probing, persistent metadata, service orchestration, CLI, and
adapters.

The architecture should keep the public core useful without Mnemosyne while
leaving a clean adapter path for Monas and Synoptikon integration.

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

Owns command-line parsing and command dispatch.

Responsibilities:

- `clap` command definitions;
- user-facing argument parsing;
- command output formatting;
- delegation into core, metadata, platform, object-service, and adapter crates.

Must not own domain rules. CLI code should translate user intent into calls
against lower-level crates.

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

Owns GUI-facing HTTP/API scaffolding.

Responsibilities:

- `axum` router construction;
- GUI view models for pool, disk, ingest, health, queue, and warning views;
- safe API action boundaries for operations exposed through the GUI;
- serialization contracts consumed by future Yew views.

Must not duplicate core storage rules or mutate persistent metadata directly.

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
