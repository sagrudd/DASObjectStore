# DASObjectStore TODO

Status: Draft  
Source roadmap: [ROADMAP.md](ROADMAP.md)  
Purpose: discrete implementation tasks suitable for CODEX agents or senior
developers

Current status: the tracked MVP/current-round checklist is functionally complete
through Milestone 18 as of 2026-07-07, with a small number of daemon ingest
hardening items still tracked under Milestone 12. Milestone 12 remains recorded
below as the daemon/client boundary that all normal CLI, HTTPS API, Web UI, TUI,
and Synoptikon-facing storage mutation flows must preserve. New Web console
completion scope is tracked under Milestones 19 and 20 rather than reopening
older checklist claims. ObjectStore file browsing and remote easyconnect upload
planning are tracked under Milestones 21 and 22.

## Working Rules

- Keep changes surgical and tied to one task or closely related task group.
- Prefer small modules and tests with each implementation task.
- Update this file when tasks are completed, split, or superseded.
- Keep persistent metadata, CLI behavior, and compatibility-impacting changes
  documented before merging implementation.

## Milestone 12: Managed Daemon and Client Boundary

- [x] Add a `dasobjectstore-daemon` crate with a small runtime module boundary,
  daemon configuration type, and unit tests for default runtime paths.
- [x] Define the daemon API contract for health summary, store inventory,
  ingest job submission, ingest progress events, job status, and cancellation.
- [x] Add shared request/response DTOs for daemon jobs so CLI, Axum routes, Yew
  view models, and Synoptikon adapters do not duplicate API shapes.
- [x] Add a daemon client abstraction with an in-process test transport and a
  planned Unix-domain socket transport.
- [x] Refactor `dasobjectstore ingest files` so the normal command path builds a
  daemon request and the daemon executes SSD-first local file ingress.
- [x] Render daemon progress events for normal `dasobjectstore ingest files`
  submissions instead of the current synchronous daemon response view.
- [x] Add optional `dasobjectstore ingest files --tui` embedded upload rendering
  for daemon file ingest submissions.
- [x] Move current direct local ingest execution behind an explicit hidden
  developer/test flag or test transport until it can be removed.
- [x] Implement daemon-side local authorization using Linux peer credentials and
  store writer-group policy for the first Linux slice.
- [x] Add package assets for `dasobjectstored`: system user, systemd service,
  socket/runtime directory, state directory, log directory, and permission
  expectations.
- [x] Update DEB validation to ensure managed DAS roots are owned by the daemon
  service identity, not ordinary ingest users.
- [x] Add integration tests proving normal non-root ingest succeeds through the
  daemon without granting direct write permission to managed DAS roots.
- [x] Update user documentation so ingest is described as a client/server job
  submission with byte-level progress, not a local filesystem write.
- [x] Update Synoptikon/Mneion integration docs so all storage-mutating actions
  call the daemon API and inherit the common audit/authentication model.

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
- [x] Add bounded Docker Compose availability and action checks for benchmark
  provider/workload scripts.
- [x] Install Docker, Compose v2, and AWS CLI on the DAS host and pass remote
  benchmark preflight at `192.168.1.192`.
- [x] Pre-create provider output directories before Compose startup so RustFS
  bind mounts do not leave report directories owned by container users.
- [x] Pre-create RustFS benchmark bucket directories during provider bootstrap
  because the single-node container profile rejects S3 `CreateBucket`.
- [x] Run first complete Garage and RustFS workload set.
  Completed on the remote DAS host at `192.168.1.192` on 2026-07-07 with a
  bounded validation matrix; `check-report-inputs.sh` passed against
  `benchmarks/output/object-services/`.
- [x] Produce first benchmark report and recommend MVP object service.
  Report: `benchmarks/object-services/reports/2026-07-07-provider-selection.md`.
  Garage is selected as the Milestone 9 MVP object service.

## Milestone 9: S3 Service Orchestration

- [x] Create `dasobjectstore-object-service` crate with provider trait.
- [x] Add Garage provider implementation with Garage-specific Compose rendering,
  config rendering, and CLI `service render-compose --provider garage` wiring.
- [x] Add daemon API/client contracts for Garage service lifecycle and status
  inspection.
- [x] Implement daemon-owned Garage lifecycle executor and status probe.
  Milestone 8 selected Garage as the MVP provider on 2026-07-07. Added a
  daemon runtime controller with injectable Docker Compose execution and tested
  status parsing on 2026-07-07.
- [x] Wire daemon Garage lifecycle execution into a reusable `dasobjectstored`
  request handler.
- [x] Add the long-running Unix-domain socket listener loop that dispatches
  requests through the `dasobjectstored` request handler.
- [x] Add daemon-owned Garage provisioning command plan and runtime executor
  with secret-redacted diagnostics.
- [x] Wire daemon-owned Garage bucket provisioning and per-store key
  permissions from store registry bindings into the daemon API/job path.
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

## Milestone 13: Web UI, Read-Only Exports, and Mnemosyne Adapter Draft

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

## Milestone 14: Formal Mnemosyne Product Plugin

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

## Milestone 15: Standalone HTTPS Application and Authentication

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
- [x] Reconcile standalone auth with the appliance charter for local OS users
  and sudo-derived administrator status before expanding administrator
  workflows.
- [x] If OS-local auth is selected, add local-user discovery, sudo-rights
  administrator detection, current-user metadata, and auth tests.
- [x] If product-local auth remains authoritative, document the host-mode
  decision and why it supersedes OS-local sudo administrator semantics.
  Not applicable: OS-local authority is selected for standalone appliances, and
  the product-local auth store is documented as a transitional Web session
  layer.

## Milestone 16: Native Mneion Storage Endpoint and External NAS Support

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

## Milestone 17: Web Operations Console and Design System

- [x] Define shared GUI view models for Overview, Disks, Stores, Objects,
  Endpoints, and Activity.
- [x] Implement Overview API route and Yew view for capacity, ingest pressure,
  destage urgency, endpoint state, and required actions.
- [x] Implement Disks API route and Yew view for enclosure grouping, health,
  USB/SMART warnings, benchmark drift, migrate, drain, replace, and retire.
- [x] Implement Stores API route and Yew view for policy creation, modification,
  resizing, redundancy, retention, and export mode.
- [x] Implement Objects API route and Yew view for object inventory, hashes,
  copy locations, reproducibility source, export/download, repair, and
  redownload.
- [x] Implement Endpoints API route and Yew view for DAS pools, external NAS/NFS
  endpoints, S3 service state, Mneion export, and binding readiness.
- [x] Implement Activity API route and Yew view for ingest queue, destage queue,
  repair tasks, audit/provenance, and long-running operations.
- [x] Add reusable Yew components for dense tables, inspector drawers, status
  badges, capacity bars, segmented controls, icon buttons, and risky-operation
  confirmation panels.
- [x] Add visual regression/screenshot checks for the main workspaces on desktop
  and mobile-width layouts.
- [x] Verify the UI does not use a landing-page pattern after login and opens
  directly into the operations Overview.
- [x] Add standalone Users/Groups API and Yew workspace where host mode allows
  local user and group management.
- [x] Add group creation and local-user-to-group assignment surfaces that align
  with daemon writer/admin policy.
- [x] Add ObjectStore creation/configuration surfaces through the Web UI where
  the existing store registry/domain APIs are stable.
- [x] Add SubObject creation/configuration surfaces through the Web UI where the
  existing SubObject registry/domain APIs are stable.
- [x] Add Web UI tests for admin-only access, permission-denied states, group
  management operation surfaces, ObjectStore creation, and SubObject creation.
- [x] Add daemon-backed execution routes for local group creation and
  local-user-to-group assignment once daemon administrator policy APIs are
  available.

## Milestone 18: Parallel Ingress Operations and Embedded TUI Views

- [x] Define the parallel daemon ingress pipeline stages: scan, source read, SSD
  stage, checksum/manifest capture, HDD placement, HDD write, verification, and
  finalization.
- [x] Make streaming source files to SSD staging the first priority while
  maintaining bounded queues and pressure controls.
- [x] Add adaptive worker scheduling that uses available CPU cores for hashing,
  verification, metadata, and coordination without overdriving saturated disks.
- [x] Add configurable resource policy for worker counts, memory budget, SSD
  reserve, HDD queue depth, verification parallelism, and system safety reserve.
- [x] Default resource policy should use available cores and memory headroom for
  standalone performance while preserving explicit safety limits.
- [x] Add bounded memory pools for read/write/verify buffers so high throughput
  does not become unbounded allocation.
- [x] Add per-disk or per-target HDD write queues to distribute staged data
  quickly to final persistence locations.
- [x] Add placement scheduler inputs for target capacity, current queue depth,
  write throughput, health, and failure/pressure state.
- [x] Add backpressure rules that slow source reads only when SSD pressure, RAM
  pressure, HDD backlog, verification backlog, or error rate requires it.
- [x] Extend daemon ingest telemetry with queue depths for scan, source read,
  SSD stage, HDD write, and verification stages.
- [x] Extend daemon ingest telemetry with active/idle worker counts for scan,
  read, stage, write, verify, and finalization workers.
- [x] Extend daemon ingest telemetry with CPU use, memory use, resource policy,
  and bottleneck classification.
- [x] Extend daemon ingest telemetry with current throughput, moving average,
  recent high/low, and trend direction: up, down, or flat.
- [x] Extend daemon ingest telemetry with staged-on-SSD, written-to-HDD, and
  verified byte/file fractions.
- [x] Add crash-safe ingest journal states for planned, staged, written,
  verified, failed, retried, cancelled, and finalized file records.
- [x] Add resume/reconcile behavior for interrupted jobs, including partially
  staged and partially written data.
- [x] Add checksum or content-address manifest capture during SSD staging where
  compatible with the existing object model.
- [x] Add atomic finalization rules so files are not reported as persisted until
  HDD write and verification requirements are satisfied.
- [x] Add daemon API/event fields required by CLI embedded TUI views, Yew, and
  Synoptikon adapters without duplicating progress logic.
- [x] Choose the Rust terminal rendering model for optional embedded views used
  by long-running CLI actions.
- [x] Remove the standalone TUI binary and packaging path; terminal views are
  optional niceties on normal CLI commands, not a separate product surface.
- [x] Implement embedded import planning with target ObjectStore/SubObject
  context, source paths, file count, and data volume scaled to MiB, GiB, or TiB.
- [x] Implement import description metadata capture and confirmation before
  launch where exposed by long-running CLI commands.
- [x] Show resource policy before launch: worker counts, memory budget, SSD
  reserve, HDD queue depth, and verification parallelism.
- [x] Allow administrators to choose automatic resource use or explicit caps for
  cores, memory, SSD reserve, and HDD write concurrency.
- [x] Show live embedded progress for discovered/scanned, staged on SSD, written
  to HDD, and verified data.
- [x] Show active workers, queue depths, current bottleneck classification, and
  whether source-to-SSD streaming is throttled.
- [x] Show SSD pressure with capacity, used/free space, trend, and
  throttle/block state.
- [x] Show HDD write pressure with backlog, write throughput, retries, and
  detected bottlenecks.
- [x] Show verification progress, failures, retries, and final status.
- [x] Show throughput current rate, moving average, recent high/low, and
  up/down/flat trend.
- [x] Provide embedded-view keyboard actions for pause, resume, cancel, retry,
  and job details where the daemon safely supports them.
- [x] Ensure embedded views can attach to an existing running import job after
  reconnecting when the parent command supports attachment.
- [x] Add supported terminal-size behavior for compact and standard console
  layouts.
- [x] Add embedded terminal error states for authentication failure, permission
  denial, lost daemon/event connection, stalled job, SSD pressure, HDD write
  failure, and verification failure.
- [x] Add embedded terminal tests or scripted snapshots for planning, launch
  confirmation, live monitoring, reconnect, and completed summary flows.
- [x] Add benchmark harness for small-file, large-file, mixed-file, slow-HDD,
  full-SSD, and interrupted-import scenarios.
- [x] Add profiling hooks to prove CPU, memory, SSD, HDD, and verification
  bottlenecks are identified correctly.
- [x] Add performance acceptance targets for sustained source-to-SSD staging,
  HDD fan-out, verification throughput, bounded memory growth, and recovery time
  after interruption.
- [x] Document embedded `--tui` command flags, supported terminal sizes, resource
  policy, and operational expectations.

## Milestone 19: Web Console Live Data and Grammateus-Aligned Design

- [x] Audit the Yew redesign surface and remove reliance on
  `fallback_dashboard_metrics`, `fallback_enclosures`, and
  `fallback_object_stores` for authenticated pages once live payload loading is
  in place.
- [x] Add a shared Yew API loading model for the Home, Enclosures, ObjectStores,
  and Bioinformatics pages: loading, success, empty, permission-denied,
  transport-error, and stale-data states.
- [x] Wire `HomeDashboard` to fetch `/products/dasobjectstore/api/v1/dashboard/home`
  or the canonical product workspace route and render live drive count, mounted
  enclosure count, total/used/free capacity, seven-day throughput, memory stress,
  SMART warning count, required actions, and object-store count.
- [x] Replace Home "Live dashboard telemetry is being bootstrapped" copy with
  real attention cards sourced from daemon health, ingest, destage, SMART,
  capacity, and object-service warnings.
- [x] Implement the daemon/API aggregator that populates `HomeDashboardView` or
  the product home workspace from probe, health, store registry, ingest queue,
  destage queue, SMART, throughput, and memory sources instead of
  `bootstrap_fixture`.
- [x] Wire `EnclosuresPage` to fetch live enclosure payloads and render detected
  supported DAS enclosures as cards, including QNAP TL-D800C identity, topology,
  mounted state, SSD/HDD counts, capacity, bay membership, SMART warning count,
  and health state.
- [x] Implement selected-enclosure detail panels with drive cards for SSD and
  HDD members, mounted paths, bay labels, role assignment, capacity, health,
  SMART warnings, and daemon-managed action availability.
- [x] Replace the static Enclosures "Add enclosure" card with a disabled/enabled
  state driven by authenticated administrator capability, supported enclosure
  discovery, and daemon readiness.
- [x] Wire `ObjectStoresPage` to fetch live object-store registry data and render
  cards with name, writer group, object type, public/writeable flags, redundancy,
  object count, used capacity, S3/export state, warning state, and last ingest
  time.
- [x] Load `/opt/dasobjectstore/groups.json` through a daemon/API boundary and
  expose group membership and writer-policy readiness to ObjectStores and
  Users/Groups pages without direct browser filesystem access.
- [x] Reconcile the legacy `workspaces/stores`, `workspaces/users-groups`, and
  operations workspace modules with the redesigned top navigation so there is
  one coherent Web console rather than parallel holder surfaces.
- [x] Implement a reusable `MnemosyneFooter`/`DasObjectStoreFooter` Yew component
  that mirrors the Mnemosyne Biosciences Grammateus/Mnematikon footer style:
  dark compact band, monospaced typography, product version, "Developed by"
  wording, `https://mnemosyne.co.uk` link, and 2026 attribution.
- [x] Apply the shared footer to the login page and every authenticated page,
  replacing the current plain `dos-app-footer` text footer and the separate
  `dos-auth-brand-footer` wording where needed.
- [x] Add CSS tokens for the footer and Mnemosyne report palette so page-level
  styling does not drift from Grammateus report conventions.
- [x] Add Yew/component tests proving the footer renders on disconnected,
  checking-session, connected, busy, and error states.
- [x] Add Playwright or trunk-driven screenshot regression coverage for login,
  Home, Enclosures, ObjectStores, and Bioinformatics at desktop and mobile
  widths, including footer fidelity and no-overlap checks.
- [x] Update `docs/user/web-interface.rst` with the live Web dashboard behavior,
  footer standard, placeholder removal plan, and daemon-owned data boundaries.

## Milestone 20: Web Administrator Workflows and Bioinformatics Readiness

- [x] Implement the Enclosures "Add enclosure" wizard as a real Web workflow:
  detect supported DAS hardware, select SSD landing media, select eligible HDD
  media, show data-loss/format plan, require administrator confirmation, submit
  a daemon preparation job, and render progress/results.
  - [x] Add the browser-side preparation wizard and GUI action-plan handoff for
    detected enclosure media, destructive review, format allowance, and
    confirmation phrase validation.
  - [x] Submit the confirmed plan as a daemon preparation job and render
    daemon job progress, result, failure, cancellation, and retry state.
    - [x] Add authenticated standalone Web submission to the daemon
      ``prepare_enclosure`` client boundary and render accepted job/failure
      state in the Enclosures wizard.
    - [x] Add generic daemon/Web administrator job status and cancellation
      contracts so Web workflows can poll daemon-owned progress and request
      cancellation without browser-side storage mutation.
    - [x] Add a daemon-owned persistent administrator job registry so accepted
      admin jobs can be queried after submission and cancellation requests have
      stable terminal-state semantics.
    - [x] Render live daemon job progress, cancellation, retry, and completed
      result state in the Enclosures wizard using the persistent administrator
      job status route.
- [x] Add API request/response DTOs and daemon client methods for Web-submitted
  enclosure preparation so the browser never mutates devices or managed roots
  directly.
- [x] Add risk-gate tests for enclosure preparation: non-admin denied,
  unsupported DAS denied, existing data requires explicit confirmation, daemon
  job failure shown clearly, and cancellation/retry state preserved.
  - [x] Cover missing session, non-admin, unsupported empty HDD set, missing
    destructive format allowance, daemon failure, and successful daemon-client
    forwarding on the standalone Web submission route.
  - [x] Cover administrator job status/cancel Web risk gates, including
    non-admin denial, blank cancellation reason rejection, status forwarding,
    and cancellation forwarding.
  - [x] Cover daemon administrator job registry persistence and cancellation
    behavior, including completed-job cancellation rejection.
  - [x] Cover existing-data preflight and cancellation/retry preservation when
    the daemon preparation runtime exposes those states.
- [x] Implement ObjectStore creation form controls for store name, writer group,
  enclosure, object type, redundancy, public/writeable state, store class,
  capacity behavior, retention, and S3/export mode.
- [x] Connect ObjectStore creation to the existing action-plan/daemon boundary
  and convert the current `store_create` holder into a confirmation and
  submission workflow with audit metadata.
- [x] Add ObjectStore edit/configuration flows for redundancy, retention,
  writer group, public/writeable policy, export mode, and capacity behavior,
  using the same validation as CLI/domain policy code.
- [x] Implement SubObject creation/configuration UI for nested prefixes, parent
  ObjectStore selection, object type inheritance/override, S3 routing, and
  registry preview before confirmation.
- [x] Add Web tests proving ObjectStore and SubObject creation produce the same
  registry/domain records as CLI paths and reject invalid policy combinations.
- [x] Promote Users/Groups into primary navigation when host mode permits local
  administration, including current OS authority, product-local users, local
  groups, administrator readiness, and writer-policy readiness.
- [x] Implement Users/Groups forms for local group creation and local
  user-to-group assignment against the existing daemon-backed routes, including
  dry-run/preview, confirmation, result, and permission-denied states.
- [x] Extend Web Activity to show administrator jobs, enclosure preparation,
  ObjectStore/SubObject creation, ingest, destage, repair, and endpoint
  validation categories, active task rows, queue summaries, warnings, and empty
  states from the shared Activity workspace contract.
- [x] Connect the Activity workspace API to the live daemon administrator job
  registry so Web Activity renders daemon-recorded enclosure preparation,
  ObjectStore creation, local administration, service, ingest, and
  repair-oriented job rows without browser-side storage mutation.
- [x] Connect the Activity workspace API to live ingest queue metadata and
  queue-derived destage summaries without browser-side storage mutation.
- [x] Connect the Activity workspace API to live repair events from pool
  metadata without browser-side storage mutation.
- [x] Add Activity task mapping for endpoint-validation events from the shared
  endpoint inventory contract without browser-side storage mutation.
- [x] Add a persistent endpoint inventory/validation registry so Activity and
  Endpoints workspaces consume registry-backed endpoint-validation events rather
  than an empty inventory source.
- [x] Add administrator Web and daemon workflows to create/update endpoint
  inventory records from validated NAS/NFS, S3-compatible, and Mnemosyne
  endpoint definitions.
- [x] Add Yew endpoint-administration forms for creating and updating
  registry-backed endpoint records through the authenticated daemon-backed Web
  route, including validation-state review, active binding controls,
  dry-run/live confirmation, result display, and permission-denied states.
- [x] Replace the Bioinformatics placeholder with dataset/workflow readiness
  cards for BAM, CRAM, POD5, FASTQ/FASTQ.GZ, FASTA, VCF/BCF, GFF/GTF, and
  ENA/SRA object types.
- [x] Add Bioinformatics views for sequencing run provenance, object lineage,
  basecalling readiness, genome/transcriptome workflow handoff, and Mnemosyne
  project/governance-domain binding state.
- [x] Add API contracts that allow Bioinformatics readiness to be derived from
  ObjectStore/SubObject metadata, object type assignments, and Mneion export
  bindings without hard-coding workflow-specific paths in Yew.
- [x] Add documentation for administrator Web workflows, Bioinformatics
  readiness semantics, permission boundaries, audit expectations, and recovery
  from failed Web-submitted jobs.
- [x] Add end-to-end Web workflow tests for administrator and non-administrator
  users covering enclosure preparation, ObjectStore creation, SubObject
  creation, group assignment, Bioinformatics readiness, and Activity progress.

## Milestone 21: ObjectStore Web File Browser and Download Workflows

- [x] Define daemon/API DTOs for ObjectStore file browsing: folder nodes, file
  nodes, object type, size, timestamps, checksum/readiness state, lifecycle
  state, copy count, and disk placement for each settled copy.
- [x] Implement metadata-backed ObjectStore tree query logic with prefix
  browsing, breadcrumb paths, server-side filtering/search, sort options,
  pagination, bounded response sizes, and large-tree regression fixtures.
- [x] Add the standalone authenticated API route for listing ObjectStore folders
  and files through the daemon ObjectBrowser client boundary.
- [x] Enforce daemon-authenticated writer-group permissions and object
  lifecycle readiness in the daemon-backed ObjectBrowser API before exposing
  metadata through the daemon request handler.
- [x] Add first-class public/read group fields to store policy/registry data
  and apply them to ObjectBrowser metadata authorization.
- [x] Reuse ObjectBrowser public/read/write policy for individual file download
  authorization.
- [x] Reuse ObjectBrowser public/read/write policy for folder archive download
  authorization when folder download routes are implemented.
- [x] Implement individual file download routes that stream from the selected
  settled copy, report content length where known, use safe content-disposition
  headers, and fail clearly for missing, unsettled, degraded, or unauthorized
  objects.
- [x] Implement folder download as streamed `tar.gz` archive generation for a
  selected folder prefix, with archive-size preflight, bounded memory use,
  cancellation-aware cleanup, and no requirement to stage the full archive on
  SSD or HDD.
- [x] Add Yew ObjectStore file browser page/detail view with breadcrumb
  navigation, expandable folder hierarchy, sortable file table, size and object
  type columns, disk placement badges, lifecycle/readiness badges, and clear
  empty/loading/error/permission states.
- [x] Ensure the file browser design follows the DASObjectStore/Mnemosyne Web
  console style: compact professional cards/tables, minimal icons, dense but
  readable rows, no landing-page treatment, and responsive desktop/mobile
  behavior without text overlap.
- [x] Add Web download controls for file and folder rows, including disabled
  states for unavailable data, confirmation/preflight for large folder archives,
  progress/started feedback, and permission-denied messaging.
- [x] Surface physical placement faithfully in the browser: SSD-only, HDD
  settled, multi-copy disk IDs/labels, degraded/missing-copy warnings,
  redownload-required state, and unavailable objects.
- [x] Add tests for file browser API paging/search/sort, permission denial,
  settled-copy selection, degraded object handling, file download streaming,
  folder `tar.gz` archive contents, interrupted archive cleanup, and large-tree
  response bounds.
- [x] Add Yew/component or screenshot regression coverage for ObjectStore tree
  browsing, dense file lists, placement badges, download controls, empty states,
  mobile layout, and no-overlap rendering.
- [x] Update `docs/user/web-interface.rst` and ObjectStore user docs with
  browser behavior, permission boundaries, download/archive semantics,
  performance limits, and expected failure states.

## Milestone 22: Remote Easyconnect Uploads and Ingress Policy Simplification

- [x] Define the remote easyconnect product contract for
  `dasobjectstore-remote easyconnect <host-or-ip>`, including discovery URL,
  browser launch, pairing lifecycle, local callback/polling fallback, failure
  states, and CLI output.
- [x] Add remote CLI configuration storage for paired DAS appliances, issued
  session credentials, expiry time, renewal metadata, selected default
  ObjectStore, and secure redaction in logs/help output.
- [x] Implement the remote CLI browser-launch flow that opens the appliance Web
  authentication page for a host such as `192.168.1.192` and waits for a
  one-time pairing result without requiring the user to paste passwords or S3
  keys into the terminal.
- [x] Add server-side pairing/session API contracts for remote agents: create
  pairing challenge, approve after authenticated browser login, exchange for a
  remote upload session, revoke session, and renew an active session during
  long uploads.
- [x] Set the default remote upload session lifetime to eight hours and add
  renewal semantics that are safe for long-running ingress jobs without keeping
  passwords in memory longer than required.
- [x] Support standalone local-user authentication for easyconnect first, while
  keeping the API shape ready for Synoptikon/Mneion identity providers.
- [x] Add permission checks so remote upload sessions can list only the
  ObjectStores available to the authenticated user and can write only to stores
  where writer-group policy allows ingest.
- [x] Implement a Web remote-upload page reached after easyconnect login that
  lists accessible ObjectStores with writer readiness, object type, capacity
  warnings, public/export state, and whether uploads are currently allowed.
- [x] Add a polished drag-and-drop file/folder upload panel to the remote-upload
  page, using browser filesystem metadata for selection while delegating actual
  byte transfer to the paired `dasobjectstore-remote` process.
- [x] Define the browser-to-local-agent coordination mechanism for drag/drop
  selections, including local loopback or browser-mediated handoff, explicit
  user confirmation, path privacy, and clear errors when the paired agent is not
  reachable.
- [x] Implement remote CLI upload execution through the intended
  S3-compatible ObjectStore path, using appliance-issued credentials/session
  material and derived bucket/store routing rather than user-entered S3 names.
- [x] Ensure remote-agent uploads and direct Web uploads always stage data to
  the selected ObjectStore SSD before daemon-owned HDD settlement.
  Completed by daemon ingress-origin classification and remote easyconnect
  handoff responses that advertise ``remote_s3``/``web_upload`` as
  ``ssd_first`` landing paths.
- [x] Change server-side/local-appliance ingest policy so ingest performed on
  the DAS server itself uses direct-to-HDD writing when policy permits, rather
  than unnecessarily staging through SSD.
  Completed by routing local-server daemon file ingest with store policy
  ``DirectToHdd`` through source hashing plus direct verified HDD settlement,
  while non-direct policies remain SSD-first.
- [x] Define centralized ingress-origin classification (`local_server`,
  `remote_s3`, `web_upload`, and future Synoptikon/Mneion origins) with stable
  serialized names and deterministic landing-mode tests.
- [x] Wire centralized ingress-origin classification through normal CLI daemon
  submission, daemon API request DTOs, and daemon file-ingest runtime placement
  decisions.
- [x] Wire centralized ingress-origin classification through remote/S3
  object-service upload planning and CLI plan output.
- [x] Wire centralized ingress-origin classification through Web remote-upload
  workspace payloads and Mnemosyne/Synoptikon binding contracts.
- [ ] Wire centralized ingress-origin classification through future Web
  direct-upload execution and concrete Synoptikon/Mneion submission clients
  once those byte-transfer/client paths are implemented.
- [x] Implement the default HDD landing worker rule as
  `max(number_of_hdds_in_enclosure - 2, 2)` for SSD destage and local
  direct-to-HDD ingest, with one active writer per physical HDD and bounded
  behavior when there are too few eligible HDDs.
- [x] Ensure the landing worker scheduler never assigns two active writes to
  the same HDD and never places redundant copies of one object on the same disk.
- [x] Add shared remote-upload backpressure policy contracts to easyconnect
  handoff, Web remote-upload workspace payloads, and remote S3 upload plans.
- [x] Add daemon remote-upload admission decisions for SSD pressure, S3 transfer
  concurrency, SSD staging, HDD landing, and verification queue limits.
- [x] Expose remote-upload admission decisions through daemon request/response,
  request handler, and typed client boundaries.
- [x] Add a daemon runtime remote-upload admission gate that tracks active S3
  transfers plus SSD staging, HDD landing, and verification queue depths before
  accepting more remote intake.
- [x] Add a daemon remote-upload S3 transfer permit guard so upload workers can
  reserve bounded S3 intake capacity and release it safely.
- [x] Add a daemon remote-upload S3 transfer execution wrapper that refuses
  blocked admission before invoking transfer code and releases capacity after
  success or failure.
- [x] Add a daemon remote-upload S3 transfer job wrapper that carries job
  identity, target ObjectStore, source bytes, admission/transfer outcome, and
  runtime queue state while enforcing the central admission gate.
- [x] Add a stable `remote_upload` daemon job kind and map remote-upload S3
  transfer summaries into common daemon job events for complete, waiting,
  rejected, and failed states.
- [x] Add daemon job registry persistence for remote-upload S3 transfer
  summaries so completed, waiting, rejected, and failed transfer attempts can be
  queried through the common job status/list path.
- [x] Add a daemon remote-upload S3 transfer worker facade that acquires
  admission capacity, records running/final job states, executes the provided
  byte-transfer closure, and releases capacity on completion or failure.
- [x] Add daemon remote-upload queue observers that derive SSD staging, HDD
  landing, and verification queue depths from daemon ingest telemetry before
  admission decisions.
- [x] Add live byte-progress reporting to the daemon remote-upload transfer
  worker so concrete byte-transfer implementations can persist intermediate
  progress events while admission capacity is held.
- [x] Add a typed daemon remote-upload byte-transfer adapter so concrete
  S3/object-service implementations run through the admission-gated worker,
  shared byte-progress reporter, and capacity-release path.
- [x] Add a daemon AWS CLI S3 byte-transfer implementation behind the typed
  remote-upload adapter so concrete object-service transfer commands run under
  admission control and record completion byte progress.
- [ ] Wire remote easyconnect upload jobs to construct and run the daemon AWS
  CLI/S3 multipart byte-transfer implementation so SSD staging,
  S3/object-service intake, HDD landing workers, and verification cannot grow
  without bounds end-to-end.
- [ ] Add resumable and cancellable remote upload jobs, including cleanup of
  partial SSD-staged objects, failed S3 multipart uploads, abandoned sessions,
  expired pairings, and interrupted browser tabs.
- [ ] Extend daemon progress/events so remote uploads show source scan count,
  staged bytes, S3 transfer rate, SSD queue depth, HDD landing queue depth,
  active per-HDD writers, verification state, and session-renewal status.
- [ ] Add Web progress rendering for remote uploads that remains accurate when
  the browser refreshes, disconnects, or reconnects while the paired CLI agent
  continues transfer.
- [ ] Add remote CLI progress rendering for easyconnect uploads using the same
  daemon job/event model as normal CLI ingest and embedded TUI views.
- [ ] Add tests for easyconnect pairing success, expired pairing, denied login,
  revoked session, eight-hour expiry, renewal during active upload, and
  standalone local-user permission checks.
- [ ] Add tests for ObjectStore listing through a remote upload session,
  including non-writer denial, read-only/locked store denial, and missing writer
  group diagnostics.
- [ ] Add tests for browser/agent coordination, drag/drop folder expansion,
  local path privacy, agent unreachable state, and user cancellation before
  transfer begins.
- [ ] Add S3 upload integration tests or fakes for multipart transfer,
  interrupted transfer cleanup, credential expiry, and derived store/bucket
  routing.
- [ ] Add daemon ingest policy tests proving remote/Web/S3 origins stage to SSD
  and local server origins use direct-to-HDD placement when safe.
- [ ] Add scheduler tests for the HDD worker formula across 1, 2, 3, 4, 5, and
  8 HDD enclosures, including one-writer-per-HDD and redundancy placement
  constraints.
- [ ] Update `docs/user/remote-upload.rst` with easyconnect setup, browser
  authentication, ObjectStore selection, drag-and-drop upload, session renewal,
  cancellation, and recovery behavior.
- [ ] Update `docs/user/ingesting-files.rst`, `docs/user/object-stores.rst`,
  and `docs/user/web-interface.rst` with the simplified ingress-origin rules:
  local server ingest writes direct to HDD, while S3/Web/remote upload stages to
  SSD first.
- [ ] Update packaging docs and Makefile notes for `make remote`, `make
  remote-deb`, and `make remote-rpm` so remote easyconnect dependencies and
  browser-launch expectations are explicit.
- [ ] Add operator documentation for the default HDD landing concurrency rule,
  per-HDD writer exclusivity, SSD pressure behavior, and how to diagnose slow
  remote uploads.

## Milestone 23: Appliance Telemetry, Home Dashboard Graphs, and floundeR Time-Series Contracts

- [ ] Define a versioned appliance telemetry JSON schema covering timestamped
  CPU usage, memory usage, disk capacity, per-disk read/write IO counters,
  enclosure/disk identity, DASObjectStore Web/session user counts, and
  collection quality/missing-data markers.
- [ ] Choose and document the managed telemetry state location under the
  appliance-owned state tree, including file ownership, permissions, atomic
  write strategy, recovery from partial writes, and migration behavior for
  future schema versions.
- [ ] Implement daemon-owned telemetry collection as a managed service loop
  rather than a Web/API side effect, with configurable sampling cadence and
  initial supported cadences around 6 seconds and 30 seconds.
- [ ] Add platform collectors for CPU and memory usage on supported Linux
  appliance hosts, with unit tests using fixture `/proc` or command-output data
  rather than relying on live host state.
- [ ] Add per-enclosure disk capacity collection for every disk physically
  associated with known DAS enclosures, preserving disk ID, label, mount path,
  role, and enclosure association in each sample.
- [ ] Add per-enclosure disk IO collection for read bytes/s, write bytes/s,
  read operations/s, write operations/s, queue or await signals where available,
  and explicit missing-counter reasons when the host cannot provide a metric.
- [ ] Add active-user/session telemetry for local Web sessions and remote-agent
  sessions, including total active sessions, distinct logged-in users, and
  administrator/non-administrator counts where policy permits exposure.
- [ ] Implement bounded JSON retention so telemetry cannot grow without limit,
  with retention/downsampling policy sufficient for 1 hour, 1 day, 10 day, and
  3 month chart windows.
- [ ] Add daemon tests for telemetry cadence, bounded retention, atomic rewrite,
  corrupt JSON recovery, missing metric markers, and preservation of
  enclosure/disk identity across samples.
- [ ] Expose authenticated telemetry API routes for current summaries,
  downsampled time-series windows, per-disk IO series, capacity history,
  session/user history, available time windows, and missing-data intervals.
- [ ] Add API tests proving telemetry windows are downsampled consistently,
  unauthorized users cannot access protected telemetry, missing data is not
  interpolated, and response sizes remain bounded for 3 month windows.
- [ ] Extend the Home dashboard API payload so existing Capacity, Throughput,
  and Memory Stress cards consume telemetry-backed summaries where available.
- [ ] Add Home dashboard cards for IO, logged-in users, and CPU usage, with
  compact operator wording, stable card dimensions, and no dependence on
  placeholder/fallback text once telemetry is available.
- [ ] Implement a global Home telemetry time-window control with 1 hour, 1 day,
  10 days, and 3 months options that applies consistently to all telemetry
  charts on the page.
- [ ] Ensure telemetry charts update on cadence without jitter: stable chart
  containers, stable axes/labels, bounded redraw work, no card resizing, and no
  text overlap on desktop or mobile.
- [ ] Define reusable floundeR data contracts for Mnemosyne appliance
  telemetry: line charts with missing-data gaps, point/step summaries,
  capacity bands, per-disk IO traces, and small-multiple chart layouts.
- [ ] Implement floundeR rendering support for scientifically correct missing
  intervals so absent samples, service restarts, unknown devices, and
  unavailable counters are shown as gaps or labelled missing intervals rather
  than interpolated lines.
- [ ] Ensure the floundeR telemetry chart contract can be used both by the Web
  dashboard and by Grammateus formal reports without DASObjectStore-specific
  hard-coding.
- [ ] Add Yew DTO/component tests for CPU, memory, IO, capacity, throughput,
  and active-user charts with full data, sparse data, missing intervals,
  changing time windows, and per-disk series.
- [ ] Add screenshot or DOM regression coverage proving the Home telemetry
  cards and charts do not jitter, overlap, or resize unexpectedly across
  desktop and mobile layouts.
- [ ] Update `docs/user/web-interface.rst` with the Home telemetry cards,
  time-window control, missing-data interpretation, update cadence, and
  administrator/operator expectations.
- [ ] Update `docs/standalone-service.md` with telemetry state file location,
  retention policy, ownership, cadence configuration, and how to reset or
  inspect telemetry safely.
- [ ] Add cross-product notes for floundeR documenting the generalized
  telemetry chart grammar so Monas, Synoptikon, Mnematikon, and future
  Mnemosyne products can reuse the same plotting semantics.

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
- [x] Treat current Synoptikon/Mneion conventions as mutable design inputs when
  DASObjectStore requires deeper integration, provided affected software,
  schemas, migrations, tests, and docs are updated coherently.
