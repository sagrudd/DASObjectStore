# DASObjectStore TODO

Status: Draft  
Source roadmap: [ROADMAP.md](ROADMAP.md)  
Purpose: discrete implementation tasks suitable for CODEX agents or senior
developers

Current priority: bring DASObjectStore under the Synoptikon umbrella as a formal
Mnemosyne product/plugin. Standalone HTTPS operation remains required, but
standalone-only polish is secondary to product manifest, catalogue, host-mode,
authentication-boundary, and Mneion endpoint integration work.

## Working Rules

- Keep changes surgical and tied to one task or closely related task group.
- Prefer small modules and tests with each implementation task.
- Update this file when tasks are completed, split, or superseded.
- Keep persistent metadata, CLI behavior, and compatibility-impacting changes
  documented before merging implementation.

## Milestone 1: Workspace, Naming, and Release Baseline

- [x] Add Rust workspace `Cargo.toml` with placeholder crates for core, CLI,
  platform, metadata, object service orchestration, and Mnemosyne adapter.
- [x] Add `.gitignore` for Rust, editor, macOS, test output, and generated
  benchmark artifacts.
- [x] Add CI workflow for `cargo fmt --check`, `cargo clippy`, and
  `cargo test`.
- [x] Add `docs/versioning.md` describing semantic versioning, pre-1.0
  compatibility expectations, and metadata format version policy.
- [x] Add `docs/architecture.md` with the high-level crate/module boundaries.
- [x] Add `CONTRIBUTING.md` pointing contributors to `AGENTS.md`,
  `ROADMAP.md`, and this TODO.
- [x] Verify repository text uses `DASObjectStore` for project name and
  `dasobjectstore` for CLI examples.

## Milestone 2: Rust Workspace and Domain Skeleton

- [x] Create `dasobjectstore-core` crate for domain types and lifecycle state
  machines.
- [x] Create `dasobjectstore-cli` crate using `clap` with a minimal
  `dasobjectstore --help`.
- [x] Define domain IDs: pool ID, disk ID, enclosure ID, store ID, object ID,
  ingest job ID, and placement ID.
- [x] Define lifecycle enums for pool, disk, store, object, ingest job, health,
  repair, and import mode.
- [x] Define store class and store policy structs matching
  `docs/requirements.md`.
- [x] Add serialization/deserialization tests for domain types.
- [x] Add lifecycle transition tests for valid and invalid object state
  transitions.
- [x] Add CLI command skeletons for `probe`, `health`, `pool`, `disk`, `store`,
  `ingest`, and `mnemosyne`.
- [x] Keep `clap` CLI parsing separate from domain logic and persistence.

## Milestone 3: Cross-Platform Disk and Enclosure Probe

- [x] Create `dasobjectstore-platform` crate with trait-based probe interfaces.
- [x] Define Linux disk inventory command contract using `lsblk --json`.
- [x] Add structured Linux `lsblk --json` disk inventory parser.
- [x] Wire Linux disk inventory parser to command execution.
- [x] Define macOS disk inventory command contract using `diskutil list -plist`.
- [x] Add structured macOS `diskutil list -plist` inventory parser.
- [x] Wire macOS disk inventory parser to command execution.
- [x] Add data model for observed disk identity: size, serial hints, partition
  hints, filesystem hints, removable/direct-attached hints, and transport.
- [x] Add data model for observed enclosure identity: USB topology path, vendor,
  product, bridge hints, and user-assigned name.
- [x] Implement best-effort enclosure grouping from USB topology.
- [x] Add `dasobjectstore probe --json`.
- [x] Add `dasobjectstore probe --pretty`.
- [x] Add fixture-based tests for Linux probe parsing.
- [x] Add fixture-based tests for macOS probe parsing.
- [x] Document known SMART/USB identity limitations for common USB bridge
  behavior.

## Milestone 4: Portable Metadata Format

- [x] Create `dasobjectstore-metadata` crate.
- [x] Define SQLite schema for live SSD metadata.
- [x] Define metadata format version table and migration table.
- [x] Define canonical pool manifest format.
- [x] Define canonical disk manifest format.
- [x] Define append-only placement log format.
- [x] Implement metadata initialization for a new pool on an SSD path.
- [x] Implement metadata snapshot export to HDD metadata directories.
- [x] Implement metadata snapshot import/reconstruction tests.
- [x] Implement dirty-state markers for clean eject, dirty attach, read-only
  import, repair, and force import.
- [x] Add `dasobjectstore pool inspect --metadata-path`.
- [x] Add `dasobjectstore pool mark-clean` and `mark-dirty` developer-only test
  commands behind an explicit debug feature.
- [x] Document metadata compatibility and recovery guarantees.

## Milestone 5: Store Policy Engine

- [x] Implement global pool defaults with per-store overrides.
- [x] Implement built-in store class defaults for `reproducible_cache`,
  `generated_data`, `critical_metadata`, `export_bundle`, and
  `ingest_staging`.
- [x] Implement policy validation for copy count, mutability, retention,
  placement, ingest mode, and capacity behavior.
- [x] Implement policy validation for enclosure-aware placement preferences and
  requirements.
- [x] Implement risk-gating model for direct-to-HDD import, force retire, and
  force read-write import.
- [x] Add `dasobjectstore store validate <policy-file>`.
- [x] Add `dasobjectstore store defaults --class <class>`.
- [x] Add tests for valid public cache policy.
- [x] Add tests for valid generated data policy.
- [x] Add tests for valid critical metadata policy.
- [x] Add tests for invalid and unsafe policy combinations.

## Milestone 6: SSD Ingest Pipeline

- [x] Define ingest job schema in live metadata.
- [x] Implement ingest staging directory layout on SSD.
- [x] Implement streaming hash computation for ingest writes.
- [x] Implement object state transitions from `received_on_ssd` to
  `ssd_eviction_eligible`.
- [x] Implement store-configurable acknowledgement policy.
- [x] Implement SSD capacity measurement and high-watermark policy.
- [x] Implement priority queue and backpressure behavior for SSD pressure.
- [x] Promote HDD destage urgency as SSD pressure rises.
- [x] Add `dasobjectstore ingest status`.
- [x] Add `dasobjectstore ingest queue --json`.
- [x] Add guarded direct-to-HDD import for reproducible cache objects.
- [x] Add crash/restart test for an ingest job before HDD settlement.
- [x] Add crash/restart test for an ingest job after metadata commit.
- [x] Document exactly what is lost if SSD fails before settlement.

## Milestone 7: HDD Placement and Copy Verification

- [x] Define placement candidate model using capacity, health, performance,
  write load, and enclosure constraints.
- [x] Implement weighted placement scorer.
- [x] Implement copy planner for `copies = 1`, `copies = 2`, and `copies = 3`.
- [x] Implement duplicate-copy prevention on the same disk for protected
  objects.
- [x] Implement HDD copy write and post-copy hash verification.
- [x] Implement object protection state update after policy-satisfying copies.
- [x] Implement redownload-required state for reproducible cache objects.
- [x] Add `dasobjectstore object inspect <object-id>`.
- [x] Add placement tests for mixed disk sizes.
- [x] Add placement tests for degraded/suspect disks.
- [x] Add placement tests for enclosure-aware store policy.
- [x] Add copy verification tests with deliberate checksum mismatch.

## Milestone 8: Object Service Benchmark and Selection

- [x] Create `benchmarks/object-services` harness structure.
- [x] Add Docker/Compose setup for Garage benchmark runs.
- [x] Add Docker/Compose setup for RustFS benchmark runs.
- [x] Implement large-object upload/download benchmark.
- [x] Implement small-object upload/download benchmark.
- [x] Implement concurrent-client benchmark.
- [x] Implement crash/restart during ingest benchmark.
- [x] Implement interrupted-write benchmark.
- [x] Implement metadata recovery benchmark.
- [x] Implement disk-full behavior benchmark.
- [x] Implement simulated disk removal benchmark.
- [x] Implement SSD ingest and HDD destage compatibility benchmark.
- [x] Define benchmark scoring rubric with reliability hard gates.
- [x] Write benchmark report template.
- [x] Add benchmark report input readiness check.
- [x] Add full provider/workload matrix runner.
- [x] Add local benchmark preflight check.
- [x] Add benchmark execution runbook.
- [x] Add benchmark report input index generator.
- [x] Add benchmark report draft generator.
- [x] Add benchmark environment snapshot generator.
- [x] Add Docker Compose compatibility helper for benchmark workloads.
- [x] Add benchmark harness smoke test to CI.
- [x] Include environment snapshot in benchmark draft report.
- [x] Support containerized AWS CLI for benchmark workloads.
- [x] Autogenerate Garage benchmark S3 keys and bucket permissions.
- [x] Add bounded Docker daemon responsiveness check to benchmark preflight.
- [ ] Run first complete Garage and RustFS workload set.
  Blocked until Garage and RustFS workload reports exist under
  `benchmarks/output/object-services/`. Automation attempt on 2026-06-25 was
  blocked because local Docker commands hung before provider startup completed.
- [ ] Produce first benchmark report and recommend MVP object service.
  Blocked until the complete Garage and RustFS workload set exists under
  `benchmarks/output/object-services/`.

## Milestone 9: S3 Service Orchestration

- [x] Create `dasobjectstore-object-service` crate with provider trait.
- [ ] Implement provider for selected MVP object service.
  Blocked until Milestone 8 selects the MVP provider.
- [x] Generate Docker/Compose configuration from store and pool policy.
- [x] Generate per-store service credentials.
- [x] Persist credential references without leaking secrets into normal logs.
- [x] Map store definitions to bucket/service layout.
- [x] Add `dasobjectstore service render-compose`.
- [x] Add `dasobjectstore service up`.
- [x] Add `dasobjectstore service down`.
- [x] Add `dasobjectstore service status --json`.
- [x] Add integration test using local generated Compose where available.
- [x] Document macOS Docker Desktop limitations for service orchestration.

## Milestone 10: Health, Drain, Repair, and Disk Retirement

- [x] Define health score inputs and weighting.
- [x] Implement SMART ingestion where available on Linux.
- [x] Implement best-effort SMART/health ingestion on macOS.
- [x] Implement IO error and checksum failure health signals.
- [x] Implement USB reset/disconnect event ingestion where feasible.
- [x] Implement benchmark drift signal ingestion.
- [x] Implement disk health state transitions.
- [x] Block new protected placement on suspect disks.
- [x] Implement protected-store evacuation planner.
- [x] Implement evacuation executor with copy verification.
- [x] Implement reproducible-cache opportunistic evacuation.
- [x] Add `dasobjectstore disk retire <disk-id>`.
- [x] Add `dasobjectstore disk drain <disk-id>`.
- [x] Add `dasobjectstore disk replace <old-disk-id> --with <new-disk-id>`.
- [x] Add force-retire flow with policy allowance and action-time
  confirmation.
- [x] Add health summary, verbose, and JSON output.
- [x] Add tests for suspect disk evacuation.
- [x] Add tests for insufficient capacity during drain.

## Milestone 11: macOS Development and Read/Export Path

- [x] Implement macOS read-only pool inspection from metadata snapshots.
- [x] Implement macOS clean-pool read-only attach flow.
- [x] Implement macOS dirty-pool read-only default import flow.
- [x] Implement settled object export command for macOS.
- [x] Add `dasobjectstore pool import --read-only`.
- [x] Add `dasobjectstore pool repair --dry-run`.
- [x] Add macOS fixture tests for metadata inspection.
- [x] Add cross-platform test that reads metadata generated by Linux fixtures on
  macOS.
- [x] Document macOS limits for Docker Desktop, service management, SMART,
  filesystem support, permissions, and performance.

## Milestone 12: Web UI, Read-Only Exports, and Mnemosyne Adapter Draft

- [x] Create `axum` GUI API scaffold with clear separation from core domain
  logic.
- [x] Create `yew` frontend scaffold intended for delivery through `../monas`
  and Synoptikon in `../mnemosyne`.
- [x] Add dashboard view model for pool status.
- [x] Add dashboard view model for disk health.
- [x] Add dashboard view model for ingest and destage queues.
- [x] Add dashboard view model for warnings and required actions.
- [x] Add safe Web UI actions for health check, service start/stop, and read-only
  import where supported.
- [x] Add read-only SMB export recipe generation.
- [x] Add read-only NFS export recipe generation.
- [x] Add optional managed read-only export task for Linux if safe and
  well-bounded.
- [x] Create `dasobjectstore-mnemosyne` adapter crate/module.
- [x] Implement Mneion-compatible storage definition export.
- [x] Implement Mneion-compatible binding snippet export.
- [x] Add `dasobjectstore mnemosyne export`.
- [x] Document a bioinformatics reference workflow using `reproducible_cache`
  and `generated_data`.
- [x] Add README section linking to the reference workflow.

## Milestone 13: Formal Mnemosyne Product Plugin

- [x] Add `docs/web-gui-and-mnemosyne-plugin.md` covering host modes,
  standalone port policy, authentication posture, Mneion endpoint model, and
  Web UI design language.
- [x] Create `product-manifest.json` for DASObjectStore using
  `mnemosyne.product.manifest.v1`.
- [x] Add manifest validation tests against the Mnemosyne product schema
  expectations in `../mnemosyne`.
- [x] Add product UI bootstrap export support for `/products/dasobjectstore`
  and `/products/dasobjectstore/api`.
- [x] Add host-mode domain model for `standalone` and
  `synoptikon_integrated`.
- [x] Add Synoptikon-integrated host boundary validation for account,
  entitlement, central audit, correlation ID, project context, and storage
  authority.
- [x] Add Monas/standalone host boundary validation for local product root,
  local audit, local hardware workflows, and local state stores.
- [x] Draft the `../mnemosyne/synoptikon-products.toml` catalogue entry for
  DASObjectStore without committing sibling changes from this repository.
- [x] Identify any Synoptikon/Mneion contract changes that would make
  DASObjectStore cleaner as a native storage appliance.
- [x] When a contract change is justified, update DASObjectStore docs with the
  coordinated change plan and affected repositories before implementation.

## Milestone 14: Standalone HTTPS Application and Authentication

- [x] Add standalone server configuration model with default HTTPS port `8448`.
- [x] Add CLI/server entry point for `dasobjectstore-server`.
- [x] Implement TLS asset generation/loading for standalone HTTPS.
- [x] Implement local auth store modeled on Mnematikon: users, registration
  tokens, password hashes, session token hashes, expiry, and logout.
- [x] Add standalone `axum` routes for `/api/register`, `/api/login`,
  `/api/logout`, and `/api/session`.
- [x] Disable local auth routes when host mode is `synoptikon_integrated`.
- [x] Add integrated-session issue/acceptance path for Synoptikon-provided
  actors.
- [x] Add auth middleware/extractors for protected API routes.
- [x] Add tests for login, session expiry, logout, invalid sessions, and
  integrated mode route omission.
- [x] Document packaging/service behavior for `https://127.0.0.1:8448` and
  optional Linux appliance binding to `0.0.0.0:8448`.

## Milestone 15: Native Mneion Storage Endpoint and External NAS Support

- [x] Extend `dasobjectstore-mnemosyne` endpoint model with
  `dasobjectstore_das`, `dasobjectstore_nfs`, and `s3_compatible` variants.
- [x] Add storage-definition export tests for DAS-backed endpoints.
- [x] Add storage-definition export tests for external NAS/NFS endpoints.
- [x] Add validation model for external NAS/NFS endpoint identity, export path,
  credential reference, TLS/CA reference where relevant, and status.
- [x] Add runtime mount/probe plan types for NFS/NAS validation without making
  raw paths tenant-facing contracts.
- [x] Add governance-domain binding export support aligned with Mneion
  storage-binding rules.
- [x] Add API view models for endpoint inventory, validation status, active
  bindings, and degraded endpoint warnings.
- [x] Add CLI or API command to validate a NAS/NFS endpoint definition.
- [x] Document how DASObjectStore-native endpoints differ from generic Mneion
  `posix` storage definitions.
- [x] Identify required Mneion storage-definition schema changes, if any, and
  record the coordinated implementation plan.

## Milestone 16: Web Operations Console and Design System

- [x] Define shared GUI view models for Overview, Disks, Stores, Objects,
  Endpoints, and Activity.
- [x] Implement Overview API route and Yew view for capacity, ingest pressure,
  destage urgency, endpoint state, and required actions.
- [ ] Implement Disks API route and Yew view for enclosure grouping, health,
  USB/SMART warnings, benchmark drift, migrate, drain, replace, and retire.
- [ ] Implement Stores API route and Yew view for policy creation, modification,
  resizing, redundancy, retention, and export mode.
- [ ] Implement Objects API route and Yew view for object inventory, hashes,
  copy locations, reproducibility source, export/download, repair, and
  redownload.
- [ ] Implement Endpoints API route and Yew view for DAS pools, external NAS/NFS
  endpoints, S3 service state, Mneion export, and binding readiness.
- [ ] Implement Activity API route and Yew view for ingest queue, destage queue,
  repair tasks, audit/provenance, and long-running operations.
- [ ] Add reusable Yew components for dense tables, inspector drawers, status
  badges, capacity bars, segmented controls, icon buttons, and risky-operation
  confirmation panels.
- [ ] Add visual regression/screenshot checks for the main workspaces on desktop
  and mobile-width layouts.
- [ ] Verify the UI does not use a landing-page pattern after login and opens
  directly into the operations Overview.

## Cross-Cutting Tasks

- [x] Keep CLI examples synchronized between `README.md`,
  `docs/requirements.md`, `ROADMAP.md`, and this file.
- [x] Keep JSON/schema-like formats versioned before implementation lands.
- [x] Add test fixtures whenever platform command output parsing is introduced.
- [x] Add negative tests for risky operation gates.
- [x] Keep documentation for data-loss risks adjacent to commands that can
  trigger those risks.
- [x] Review file sizes before each milestone completion and split modules that
  have grown too broad.
- [ ] Treat current Synoptikon/Mneion conventions as mutable design inputs when
  DASObjectStore requires deeper integration, provided affected software,
  schemas, migrations, tests, and docs are updated coherently.
