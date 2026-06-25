# DASObjectStore TODO

Status: Draft  
Source roadmap: [ROADMAP.md](ROADMAP.md)  
Purpose: discrete implementation tasks suitable for CODEX agents or senior
developers

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
- [x] Add `dasobjectstore ingest status`.
- [x] Add `dasobjectstore ingest queue --json`.
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
- [ ] Produce first benchmark report and recommend MVP object service.
  Blocked until Garage and RustFS workload reports exist under
  `benchmarks/output/object-services/`.

## Milestone 9: S3 Service Orchestration

- [x] Create `dasobjectstore-object-service` crate with provider trait.
- [ ] Implement provider for selected MVP object service.
  Blocked until Milestone 8 selects the MVP provider.
- [x] Generate Docker/Compose configuration from store and pool policy.
- [x] Generate per-store service credentials.
- [ ] Persist credential references without leaking secrets into normal logs.
- [ ] Map store definitions to bucket/service layout.
- [ ] Add `dasobjectstore service render-compose`.
- [ ] Add `dasobjectstore service up`.
- [ ] Add `dasobjectstore service down`.
- [ ] Add `dasobjectstore service status --json`.
- [ ] Add integration test using local generated Compose where available.
- [ ] Document macOS Docker Desktop limitations for service orchestration.

## Milestone 10: Health, Drain, Repair, and Disk Retirement

- [ ] Define health score inputs and weighting.
- [ ] Implement SMART ingestion where available on Linux.
- [ ] Implement best-effort SMART/health ingestion on macOS.
- [ ] Implement IO error and checksum failure health signals.
- [ ] Implement USB reset/disconnect event ingestion where feasible.
- [ ] Implement benchmark drift signal ingestion.
- [ ] Implement disk health state transitions.
- [ ] Block new protected placement on suspect disks.
- [ ] Implement protected-store evacuation planner.
- [ ] Implement evacuation executor with copy verification.
- [ ] Implement reproducible-cache opportunistic evacuation.
- [ ] Add `dasobjectstore disk retire <disk-id>`.
- [ ] Add `dasobjectstore disk drain <disk-id>`.
- [ ] Add `dasobjectstore disk replace <old-disk-id> --with <new-disk-id>`.
- [ ] Add force-retire flow with policy allowance and action-time
  confirmation.
- [ ] Add health summary, verbose, and JSON output.
- [ ] Add tests for suspect disk evacuation.
- [ ] Add tests for insufficient capacity during drain.

## Milestone 11: macOS Development and Read/Export Path

- [ ] Implement macOS read-only pool inspection from metadata snapshots.
- [ ] Implement macOS clean-pool read-only attach flow.
- [ ] Implement macOS dirty-pool read-only default import flow.
- [ ] Implement settled object export command for macOS.
- [ ] Add `dasobjectstore pool import --read-only`.
- [ ] Add `dasobjectstore pool repair --dry-run`.
- [ ] Add macOS fixture tests for metadata inspection.
- [ ] Add cross-platform test that reads metadata generated by Linux fixtures on
  macOS.
- [ ] Document macOS limits for Docker Desktop, service management, SMART,
  filesystem support, permissions, and performance.

## Milestone 12: Web UI, Read-Only Exports, and Mnemosyne Adapter Draft

- [ ] Create `axum` GUI API scaffold with clear separation from core domain
  logic.
- [ ] Create `yew` frontend scaffold intended for delivery through `../monas`
  and Synoptikon in `../mnemosyne`.
- [ ] Add dashboard view model for pool status.
- [ ] Add dashboard view model for disk health.
- [ ] Add dashboard view model for ingest and destage queues.
- [ ] Add dashboard view model for warnings and required actions.
- [ ] Add safe Web UI actions for health check, service start/stop, and read-only
  import where supported.
- [ ] Add read-only SMB export recipe generation.
- [ ] Add read-only NFS export recipe generation.
- [ ] Add optional managed read-only export task for Linux if safe and
  well-bounded.
- [ ] Create `dasobjectstore-mnemosyne` adapter crate/module.
- [ ] Implement Mneion-compatible storage definition export.
- [ ] Implement Mneion-compatible binding snippet export.
- [ ] Add `dasobjectstore mnemosyne export`.
- [ ] Document a bioinformatics reference workflow using `reproducible_cache`
  and `generated_data`.
- [ ] Add README section linking to the reference workflow.

## Cross-Cutting Tasks

- [ ] Keep CLI examples synchronized between `README.md`,
  `docs/requirements.md`, `ROADMAP.md`, and this file.
- [ ] Keep JSON/schema-like formats versioned before implementation lands.
- [ ] Add test fixtures whenever platform command output parsing is introduced.
- [ ] Add negative tests for risky operation gates.
- [ ] Keep documentation for data-loss risks adjacent to commands that can
  trigger those risks.
- [ ] Review file sizes before each milestone completion and split modules that
  have grown too broad.
