# Changelog

All notable DASObjectStore release changes are recorded here.

This project follows semantic versioning. Patch and minor version bumps may be
made automatically for compatible work; major version bumps require explicit
agreement before landing.

## 0.72.60 - 2026-07-13

- Add writable ObjectStore upload actions that carry the selected target into
  the target-scoped remote-upload pane.

## 0.72.59 - 2026-07-13

- Remove the unscoped Remote Upload workspace from primary navigation while
  target-scoped ObjectStore upload entry is completed.

## 0.72.58 - 2026-07-13

- Add read-only daemon and CLI capacity status with ledger pressure,
  reservation, backend/SSD availability, and explicit admission-block reasons.

## 0.72.57 - 2026-07-13

- Add an operator runbook for restoring Home telemetry, distinguishing warm-up,
  missing-device diagnostics, idle samples, and stale service state.

## 0.72.56 - 2026-07-13

- Add fixture coverage for SATA, partition, USB alias, device-mapper alias,
  warm-up, non-zero rates, and missing-device telemetry diagnostics.

## 0.72.55 - 2026-07-13

- Add a route-level availability regression proving liveness remains healthy
  while daemon-backed Activity degrades with typed warnings.

## 0.72.54 - 2026-07-13

- Preserve the last successful Home dashboard snapshot across failed refreshes
  with explicit stale-data and retry guidance.

## 0.72.53 - 2026-07-13

- Consolidate Web table and reusable-widget styling into shared responsive
  primitives with semantic source contracts and safe task-pane form handling.

## 0.72.52 - 2026-07-13

- Keep bounded-folder catalogue visibility and logical usage coherent after
  verified removal; catalogue failure leaves the payload and accounting
  untouched, while payload failure restores the catalogue record.

## 0.72.51 - 2026-07-13

- Restore bounded-folder logical usage from the durable catalogue on reopen and
  reject mismatched caller accounting before filesystem access.

## 0.72.50 - 2026-07-13

- Harden folder catalogue recovery with unique temporary names and fail-closed
  malformed/schema/identity/conflict regression tests.

## 0.72.49 - 2026-07-12

- Correct Web screenshot regression validation for the approved sans-serif
  Mnemosyne footer contract.

## 0.72.48 - 2026-07-12

- Add the shared Yew TaskPane primitive with explicit workflow modes,
  focus/escape behavior, labelled forms, and responsive side-sheet styling.

## 0.72.47 - 2026-07-12

- Add shared Web semantic interaction and status tokens with regression
  coverage, keeping Mnemosyne green reserved for provenance/footer surfaces.

## 0.72.46 - 2026-07-12

- Add explicit bounded-folder adoption execution with stable-source checks,
  durable finalization, capacity settlement, restart-safe checkpoints, and a
  private idempotent folder catalogue snapshot.

## 0.72.45 - 2026-07-12

- Add durable capacity-reservation timestamps, schema-v1 compatibility, and a
  caller-scheduled stale-reservation expiry sweep with rollback-safe provider
  persistence.

## 0.72.44 - 2026-07-12

- Deliver the Mnemosyne footer contract with local wordmark/partial assets,
  green provenance surface, responsive flex-shell layout, and regression tests.

## 0.72.43 - 2026-07-12

- Reject capacity-enabled local-ingest copy overrides that differ from the
  daemon ObjectStore policy before source reads begin.

## 0.72.42 - 2026-07-12

- Pass daemon-owned capacity reservations through Garage S3 reconciliation
  into the local ingest settlement lifecycle.

## 0.72.41 - 2026-07-12

- Wire local file ingest to daemon-owned per-object capacity reservations with
  verified ingress-origin admission and commit/release cleanup.

## 0.72.40 - 2026-07-12

- Wire remote S3 transfer jobs to daemon-owned capacity reservations, committing
  or releasing each job reservation across transfer and catalogue outcomes.

## 0.72.39 - 2026-07-12

- Add durable capacity-reservation commit and release operations with
  snapshot rollback when persistence fails.

## 0.72.38 - 2026-07-12

- Import the approved Mnemosyne partial branding asset as a local Trunk file
  with pinned SHA-256 provenance coverage.

## 0.72.37 - 2026-07-12

- Install a daemon-owned registry-backed capacity provider with persisted
  reservations and backend/SSD filesystem probes; bounded stores fail closed
  until their ledger is initialized, and daemon policy owns the copy count.

## 0.72.36 - 2026-07-12

- Expose an authenticated, read-only capacity-admission daemon route with
  typed client plumbing and fail-closed unavailable-provider errors.

## 0.72.35 - 2026-07-12

- Add read-only CLI profile-capability discovery with human and JSON output.

## 0.72.34 - 2026-07-12

- Move Activity workspace grids, queues, and task cards into a feature-owned
  responsive stylesheet with CSS ownership and order tests.

## 0.72.33 - 2026-07-12

- Move Object Browser CSS into a feature-owned responsive stylesheet with
  contract coverage while preserving shared status-pill styling.

## 0.72.32 - 2026-07-12

- Preserve safe provider reconciliation resumes by carrying stable S3 ETags as
  source revisions when listings provide them.

## 0.72.31 - 2026-07-12

- Make folder reconciliation checkpoints revision-aware so changed or legacy
  revision-less sources cannot be skipped or resumed as authoritative.

## 0.72.30 - 2026-07-12

- Add daemon atomic, fsync-ordered capacity-ledger JSON persistence with
  corrupt-state and future-schema rejection.

## 0.72.29 - 2026-07-12

- Add strict schema-versioned capacity-ledger snapshot and restore contracts
  for restart-safe reservation persistence.

## 0.72.28 - 2026-07-12

- Expose static folder/drive/appliance capability discovery through the typed
  daemon request, response, and client boundaries.

## 0.72.27 - 2026-07-12

- Add a read-only folder inspection bridge that emits resumable reconciliation
  manifest entries without adopting user files or unsafe paths.

## 0.72.26 - 2026-07-12

- Gate remote-upload completion on a daemon-owned manifest/catalogue handoff,
  preserving failed state and permit release when that handoff rejects.

## 0.72.25 - 2026-07-12

- Extend the migration worker to guarded dedicated-SSD drive destinations
  while retaining source data through verification and retirement approval.

## 0.72.24 - 2026-07-12

- Add a daemon folder-to-folder migration worker with source verification,
  bounded destination admission, durable finalization, and retirement-pending
  safety semantics.

## 0.72.23 - 2026-07-12

- Add a strict Mnemosyne adapter envelope for product-owned storage-policy
  templates without embedding defaults or provisioning behavior.

## 0.72.22 - 2026-07-12

- Add concurrent capacity-reservation regression coverage proving the
  transactional ledger cannot overbook a bounded logical quota.

## 0.72.21 - 2026-07-12

- Add an atomic daemon capacity evaluate-and-reserve helper keyed by client
  request ID, preserving unchanged ledgers for rejected requests.

## 0.72.20 - 2026-07-12

- Extend capacity admission decisions with backend free space, policy
  thresholds, and copy-amplification observations for presentation adapters.

## 0.72.19 - 2026-07-12

- Add a daemon capacity-admission helper that reads logical usage and
  outstanding reservations from the live reservation ledger.

## 0.72.18 - 2026-07-12

- Add atomic parent/child SubObject capacity reservations and commits while
  preserving the standalone store ledger contract.

## 0.72.17 - 2026-07-12

- Add a typed logical object-version capacity charge so deduplicated versions
  are still accounted at full logical size before physical placement.

## 0.72.16 - 2026-07-12

- Align Debian package-asset regression expectations with the authoritative
  dependency contract, restoring a green workspace test baseline.
- Synchronize the checked-in product manifest version with the workspace
  release version.

## 0.72.15 - 2026-07-12

- Add a shared, bounded `StoragePolicyTemplate` contract for product-owned
  profile requests, including typed ingress and local-copy validation without
  hardcoding product defaults or provisioning behavior.

## 0.72.14 - 2026-07-12

- Mark migration source-retention execution as partial: core state/checkpoints
  are delivered while daemon copy/catalogue workers remain open.

## 0.72.13 - 2026-07-12

- Reconcile TODO campaign gate markers so partial delivery and external
  blockers are visible alongside completed sub-slices.

## 0.72.12 - 2026-07-12

- Publish the profile-by-host-mode support matrix and fail-closed migration
  policy for local preview and DASServer-blocked paths.

## 0.72.11 - 2026-07-12

- Add atomic schema-versioned migration checkpoint save/load with strict
  source-retention invariants.

## 0.72.10 - 2026-07-12

- Add deterministic folder source-mutation regression coverage for stable-file
  validation before adoption.

## 0.72.9 - 2026-07-12

- Add a resumable profile-promotion state machine that retains source
  placement until verification and explicit retirement confirmation.

## 0.72.8 - 2026-07-12

- Derive capacity SSD-staging requirements from typed ingress origins rather
  than caller-supplied booleans.

## 0.72.7 - 2026-07-12

- Make SSD staging an explicit capacity-admission input so approved direct
  ingress bypasses only the SSD-free constraint.

## 0.72.6 - 2026-07-12

- Harden folder private namespace permissions to owner-only directories and
  payload files without changing user-selected roots.

## 0.72.5 - 2026-07-12

- Declare static local failure-domain ceilings in profile capability discovery,
  without implying current placement or external replication.

## 0.72.4 - 2026-07-12

- Add a versioned static profile-capability catalogue for product adapters,
  separating backend operations from service requirements and runtime health.

## 0.72.3 - 2026-07-12

- Add pure per-user/system folder host-path derivation with explicit missing
  runtime handling and bounded socket-path validation.

## 0.72.2 - 2026-07-12

- Add strict v1 ObjectStore manifest decoding with fail-closed future-schema
  handling and dedicated compatibility/migration documentation.

## 0.72.1 - 2026-07-12

- Document manifest compatibility and migration rules and add a future-schema
  rejection regression test.

## 0.72.0 - 2026-07-12

- Expose guarded drive-profile capacity snapshots and read-only user-tree
  inspection through the dedicated SSD backend boundary.

## 0.71.99 - 2026-07-12

- Add a guarded drive backend that reuses hardened folder storage semantics
  while failing closed on injected mount or identity drift.

## 0.71.98 - 2026-07-12

- Add injected SSD drive-profile validation for stable identities, safe mounts,
  root exclusion, and writable media checks without probing hardware.

## 0.71.97 - 2026-07-12

- Require folder staged byte counts to match capacity reservations before
  commit, preventing usage-accounting drift.

## 0.71.96 - 2026-07-12

- Add checked used-byte debits for stable folder deletion with underflow
  protection and capacity recovery.

## 0.71.95 - 2026-07-12

- Add derived capacity pressure states and non-destructive quota-policy updates
  that preserve existing usage while rejecting new over-quota reservations.

## 0.71.94 - 2026-07-12

- Add transport-neutral daemon capacity admission request and decision
  contracts with stable rejection reasons and optional direct-ingress SSD
  observations.

## 0.71.93 - 2026-07-12

- Validate folder object fd/path identity and stable size while hashing, and
  keep staged reservations recoverable when finalization detects tampering.

## 0.71.92 - 2026-07-12

- Reject hard-linked folder import sources and classify hard-linked user files
  as unsafe; add stable-source staging that rechecks content after copying.

## 0.71.91 - 2026-07-12

- Keep folder-backend enumeration locations aligned with finalized catalogue
  locations while preserving nested user-visible object hierarchy.

## 0.71.90 - 2026-07-12

- Add strictest-constraint capacity admission evaluation across logical quota,
  backend reserve, SSD staging, and copy amplification.

## 0.71.89 - 2026-07-12

- Record the current DASServer/Garage access and unresolved public-auth/profile
  decisions in TODO so automation can skip repeated blockers and drain local
  work safely.

## 0.71.88 - 2026-07-12

- Reject drive manifests without stable device identity or with a system-root
  mount hint, preserving explicit SSD/profile safety boundaries.

## 0.71.87 - 2026-07-12

- Require explicit non-rotational SSD media classification in drive manifests,
  alongside stable filesystem and device identities.

## 0.71.86 - 2026-07-12

- Expose typed folder capacity snapshots alongside browse, read, verify, and
  health operations for profile-aware admission inspection.

## 0.71.85 - 2026-07-12

- Add read-only folder hierarchy inspection that reports unmanaged and unsafe
  entries without silently adopting user files.

## 0.71.84 - 2026-07-12

- Harden folder backend namespace and parent traversal against symlink escapes
  and reject non-regular filesystem entries during enumeration.

## 0.71.83 - 2026-07-12

- Require finite logical capacity when opening a folder backend, preventing
  unbounded new folder stores while preserving legacy appliance defaults.

## 0.71.82 - 2026-07-12

- Add a bounded folder backend that hashes in flight, fsyncs and atomically
  finalizes same-filesystem files, and exposes safe read/verify/enumerate/remove
  operations through the shared backend contract.

## 0.71.81 - 2026-07-12

- Move Home telemetry chart styling into a feature-owned CSS sheet while
  retaining shared global primitives and CSS contract coverage.

## 0.71.80 - 2026-07-12

- Close the GUI authentication route decomposition milestone after verifying
  dedicated router, contract, client, identity, validation, parsing, reporting,
  local-group, and enclosure modules with the size guard.

## 0.71.79 - 2026-07-12

- Add the shared capability-based `ObjectStoreBackend` contract and typed
  backend records for validation, reservation, staging, durable finalization,
  reads, enumeration, verification, health, reconciliation, and removal.

## 0.71.78 - 2026-07-12

- Add profile-independent protection policy names to portable ObjectStore
  manifests for local-only, reproducible, external, and appliance protection.

## 0.71.77 - 2026-07-12

- Add a versioned portable ObjectStore manifest with explicit folder, drive,
  and appliance backend references while preserving legacy metadata semantics.

## 0.71.76 - 2026-07-12

- Add validated logical-capacity policy fields and a transactional core
  reservation ledger while retaining legacy unbounded policy defaults for
  compatibility during profile rollout.

## 0.71.75 - 2026-07-12

- Add compatibility-sensitive `DeploymentProfile` and orthogonal `HostMode`
  domain vocabulary for folder, drive, and appliance rollout contracts.

## 0.71.74 - 2026-07-12

- Add a daemon-independent `/api/v1/liveness` readiness contract for Web
  health checks without changing authenticated dashboard dependencies.

## 0.71.73 - 2026-07-12

- Preserve invalid Home throughput samples as fixed-position gaps and split the
  SVG chart into non-interpolating segments with an explicit gap diagnostic.

## 0.71.72 - 2026-07-12

- Make Home throughput provenance visible with source badges and distinct line
  treatments for daemon telemetry, legacy fallback, fixtures, and unavailable
  data while retaining diagnostics for follow-up gap handling.

## 0.71.71 - 2026-07-12

- Bound idle GUI daemon socket calls with progress-aware cancellation so a
  stalled bridge worker releases its capacity without changing CLI or
  long-running ingest transport behavior.

## 0.71.70 - 2026-07-12

- Label Home throughput summaries by daemon telemetry, legacy file fallback, or
  unavailable state and preserve diagnostics through the Web response contract.

## 0.71.69 - 2026-07-12

- Carry Home Disk IO collection quality and raw missing-data markers alongside
  the per-disk diagnostics, with optional fields for legacy clients.

## 0.71.68 - 2026-07-12

- Add per-disk Home Disk IO rows with mapped identity, rates, missing reasons,
  and deterministic sample age while preserving aggregate totals and legacy
  response decoding.

## 0.71.67 - 2026-07-12

- Propagate mapped disk identity, missing reasons, and sample timestamps through
  daemon disk-IO summaries and surface warm-up/missing-device diagnostics in the
  Home Disk IO card without discarding valid disk totals.

## 0.71.66 - 2026-07-12

- Resolve managed-HDD telemetry markers through fixtureable sysfs and stable
  device aliases before reporting missing `/proc/diskstats` mappings.

## 0.71.65 - 2026-07-12

- Distinguish first-sample disk-IO warm-up from daemon-startup or unavailable
  telemetry so operators do not mistake an expected initial gap for an idle or
  failed disk.

## 0.71.64 - 2026-07-12

- Harden the Web daemon bridge circuit state against stale completions and
  classify object-browser transport failures separately from daemon validation
  errors while retaining single-probe recovery.

## 0.71.63 - 2026-07-12

- Keep Web administrator cancellation on an independent bounded daemon bridge
  so routine circuit degradation cannot suppress emergency cancellation.

## 0.71.62 - 2026-07-12

- Bound Web performance-report PDF rebuilds to a dedicated two-worker blocking
  lane with typed overload responses so report rendering cannot exhaust Axum
  or daemon bridge capacity.

## 0.71.61 - 2026-07-12

- Add single-probe half-open semantics to the daemon bridge circuit breaker so
  cooldown recovery cannot stampede a stalled daemon.

## 0.71.60 - 2026-07-12

- Add a bounded best-effort daemon bridge circuit breaker: repeated transport
  deadlines/worker failures produce typed degraded responses until cooldown,
  without tripping on normal request errors or capacity saturation.

## 0.71.59 - 2026-07-12

- Route Web local-group creation and membership assignment through the bounded
  daemon bridge while preserving post-acceptance registry updates.

## 0.71.58 - 2026-07-12

- Route Web enclosure-preparation submissions through the bounded daemon bridge
  while preserving daemon-owned destructive-operation safety gates.

## 0.71.57 - 2026-07-12

- Route Web ObjectStore ingest-policy updates through the bounded daemon bridge
  while preserving daemon-owned validation and direct-HDD confirmation gates.

## 0.71.56 - 2026-07-12

- Route Web endpoint-inventory upsert through the bounded daemon bridge while
  preserving daemon-owned validation and typed overload/deadline failures.

## 0.71.55 - 2026-07-12

- Route Web ObjectStore creation through the bounded daemon bridge while
  preserving daemon-owned mutation and typed overload/deadline failures.

## 0.71.54 - 2026-07-12

- Route Web administrator job status and cancellation through the bounded
  daemon bridge, preserving typed overload/deadline responses and cancellation
  priority across the Web and daemon boundaries.

## 0.71.53 - 2026-07-12

- Route standalone remote-authentication pairing, approval, and exchange
  through the bounded daemon bridge with typed busy/deadline failures.

## 0.71.52 - 2026-07-12

- Route the Activity workspace daemon job-list lookup through the shared
  bounded bridge while preserving a degraded 200 response with actionable
  warnings when daemon capacity or deadlines prevent a live snapshot.

## 0.71.51 - 2026-07-12

- Bound folder archive generation to two concurrent blocking workers, retaining
  archive permits until tar streams finish and returning typed overload errors
  when archive capacity is saturated.

## 0.71.50 - 2026-07-12

- Route ObjectStore file and folder download lookups through the bounded GUI
  daemon bridge, preserving typed overload/deadline responses and releasing
  control capacity before payload streaming.

## 0.71.49 - 2026-07-12

- Rebaseline historical TODO/roadmap claims around reconciliation, module-size
  exceptions, and control-plane capacity; document the current Garage
  checkpoint/cancellation limits and the remaining appliance/byte-range gaps.

## 0.71.48 - 2026-07-12

- Add the first bounded async GUI-to-daemon bridge for ObjectStore browser
  listings: synchronous socket work runs on a capped blocking pool, has a
  typed overload response, and is subject to an overall deadline.

## 0.71.47 - 2026-07-12

- Reserve a bounded Unix-socket priority lane for administrator cancellation
  requests so routine control queries and ingest submissions cannot exhaust the
  cancellation capacity; saturated lanes return the typed `server_busy` error.

## 0.71.46 - 2026-07-12

- Add administrator cancellation tokens to active Garage reconciliation jobs;
  cancellation is checked between provider transfers while durable in-progress
  manifests remain available for restart.

## 0.71.45 - 2026-07-12

- Wire durable reconciliation manifests into Garage S3 reconciliation:
  enumerate keys safely, reject malformed/colliding paths before transfer,
  download per key with atomic checkpoints, and stream per-key progress through
  the daemon job path.

## 0.71.44 - 2026-07-12

- Correct the roadmap baseline to record that the production module-size guard
  passes without exceptions and that reconciliation now has a durable manifest
  foundation while provider transfer integration remains open.

## 0.71.43 - 2026-07-12

- Add a versioned provider-independent reconciliation manifest/resume planner
  with safe key normalization, collision and malformed-key reporting, atomic
  durable checkpoints, and restart coverage; Garage worker integration remains
  a follow-up.

## 0.71.42 - 2026-07-12

- Route `disk prepare-das` through the daemon-owned enclosure executor and
  remove destructive preparation writes from the normal CLI path; preserve
  dry-run command plans and action-time safety gates.

## 0.71.41 - 2026-07-12

- Add the daemon-owned enclosure preparation executor with typed validation,
  command-runner injection, ext4/xfs planning, and atomic fsync'd role
  markers; CLI routing remains a separate follow-up.

## 0.71.40 - 2026-07-12

- Split the Web API administration request/response contracts into a focused
  module so all production modules pass the enforced size guard without
  exceptions.

## 0.71.39 - 2026-07-12

- Split shared Web API request/response contracts into a dedicated module,
  preserving wasm/test decoding and removing the final Web API size exception.

## 0.71.38 - 2026-07-12

- Split GUI API product workspace view models and bootstrap projections into a
  focused module, preserving public JSON types and route contracts.

## 0.71.37 - 2026-07-12

- Extract GUI administrator request validation, managed-mount rejection,
  client-request-ID checks, and action-specific confirmation markers into a
  focused validation module while preserving dry-run safety gates and errors.

## 0.71.36 - 2026-07-12

- Extract GUI standalone authentication, session, remote-authentication, and
  EasyConnect handlers into a focused identity-routes module; preserve router
  visibility and local-password error contracts.

## 0.71.35 - 2026-07-12

- Extract GUI API local-user authority, local-group, and enclosure daemon
  client adapters into a focused authentication-admin module while preserving
  request/error projections and route behavior.

## 0.71.34 - 2026-07-12

- Remove stale daemon and GUI module-size exception entries after guard
  validation; the reviewed baseline now lists only the three active GUI
  exceptions.

## 0.71.33 - 2026-07-12

- Move Object export/put disk-root mapping validation beside its command
  handlers and add malformed-ID, empty-path, and order-preservation tests.

## 0.71.32 - 2026-07-12

- Extract shared live-SQLite path resolution into the CLI metadata-path module
  while preserving explicit override behavior and unknown-store diagnostics.

## 0.71.31 - 2026-07-12

- Extract ingest queue inspection, rendering, and daemon-owned drain handling
  into a focused CLI module while preserving dry-run risk gates and output
  contracts.

## 0.71.30 - 2026-07-12

- Extract the hidden local-direct ingest fallback into a focused CLI module and
  remove the CLI runner's temporary module-size exception; preserve source
  collection, placement, authorization, and progress contracts.

## 0.71.29 - 2026-07-12

- Extract portable registry mirroring, known-root validation, and writer-group
  registry/ACL access into the CLI registry-access module while preserving
  fail-closed Linux group checks and non-Linux no-op behavior.

## 0.71.28 - 2026-07-12

- Extract host connection-status assessment, probe projection, preferred-path
  selection, and operator recommendations into the CLI connection module while
  preserving Thunderbolt preference and USB fallback guidance.

## 0.71.27 - 2026-07-12

- Move platform health collection, disk scoring, and OS-specific adapters into
  the CLI health module while preserving output contracts.

## 0.71.26 - 2026-07-12

- Extract managed-DAS roots, marker validation, supported-enclosure checks, and
  SSD/HDD root policy into a focused CLI module.

## 0.71.25 - 2026-07-12

- Extract packaged-daemon source canonicalization and Linux ACL planning into
  a focused ingest source-access module with fail-closed errors.

## 0.71.24 - 2026-07-12

- Extract daemon-backed ingest request submission, builders, TUI streaming, and
  completion rendering into a focused CLI module.

## 0.71.23 - 2026-07-12

- Mark the CLI performance execution-engine decomposition complete; remaining
  runner size work is now isolated to platform and ingest command families.

## 0.71.22 - 2026-07-12

- Extract performance-test lifecycle setup, provenance, scenario execution,
  and report assembly into a focused run module.

## 0.71.21 - 2026-07-12

- Move performance report persistence and QR/PDF/metadata helpers into the
  report module while preserving artifact and authoritative-policy contracts.

## 0.71.20 - 2026-07-12

- Extract performance scenario-matrix execution orchestration into a focused
  module while preserving ordering, result aggregation, and TUI context.

## 0.71.19 - 2026-07-12

- Extract the direct-HDD performance scenario into a focused module while
  preserving bounded placement, split timing, and live TUI accounting.

## 0.71.18 - 2026-07-12

- Extract the overlapping SSD pipeline performance scenario into a focused
  module while preserving bounded residency admission and HDD-drain overlap.

## 0.71.17 - 2026-07-12

- Extract the SSD stage-then-drain performance scenario into a focused module,
  preserving bounded HDD fan-out, source-read accounting, and batch ordering.

## 0.71.16 - 2026-07-12

- Extract the SSD-only performance scenario into a focused execution module,
  preserving bounded residency batches and SSD write/readback telemetry.

## 0.71.15 - 2026-07-12

- Extract shared CLI performance job, queue, and active HDD-write state into a
  focused execution module while preserving FIFO backpressure and telemetry.

## 0.71.14 - 2026-07-12

- Extract CLI performance disk placement and bounded queue-capacity scheduling
  into a focused module while preserving distinct-disk redundancy behavior.

## 0.71.6 - 2026-07-12

- Extract CLI performance live-rate accounting into a dedicated module with
  focused coverage for idle gaps and HDD-only sync time.
- Refresh the store-contents tree regression assertions to match its explicit
  file/directory labels.

## 0.71.7 - 2026-07-12

- Extract bounded asynchronous SSD settlement and queue-drain completion into a
  dedicated CLI performance module with multi-job regression coverage.

## 0.71.8 - 2026-07-12

- Split daemon storage reconciliation and registry/path helpers into focused
  request-handler modules; the production module-size guard now passes against
  the reviewed baseline.

## 0.71.9 - 2026-07-12

- Split request-handler orchestration, job projection, and shared request
  helpers so the daemon request-handler façade is within the production module
  budget without changing typed response or error contracts.

## 0.71.10 - 2026-07-12

- Extract daemon ingest pipeline work records and live progress/rate state into
  a focused module while preserving SSD-first/direct-HDD and telemetry tests.

## 0.71.11 - 2026-07-12

- Extract bounded SSD-flush/HDD-settlement workers and admission helpers into
  a focused daemon ingest module while preserving backpressure and cancellation
  behavior.

## 0.71.12 - 2026-07-12

- Extract ingest settlement event draining and progress projection into a
  focused daemon module while preserving metadata and telemetry ordering.

## 0.71.13 - 2026-07-12

- Extract CLI performance copy/read primitives and sync-policy dispatch into a
  focused module while preserving staged settlement and final-sync accounting.

## 0.71.4 - 2026-07-12

- Re-baseline the roadmap, requirements, architecture, and active TODO campaign
  around bounded folder, dedicated-SSD drive, and tiered appliance profiles.
- Make universal transactional capacity admission, portable backend contracts,
  profile-aware S3, migration, and Mnemosyne integration explicit delivery
  gates while preserving the completed appliance milestone history.

## 0.71.5 - 2026-07-12

- Extract CLI performance SSD residency budgeting and bounded batch admission
  into a dedicated module with capacity-boundary regression coverage.

## 0.71.3 - 2026-07-11

- Complete scoped Garage reconciliation without replacing the live metadata
  index after normal SSD-first ingest has already registered recovered objects.
  The repair job now reaches a durable successful terminal state while existing
  catalogue rows are preserved.

## 0.71.2 - 2026-07-11

- Persist terminal success and failure records for Garage reconciliation repair
  jobs, and mark nonterminal jobs interrupted after daemon restart.

## 0.71.1 - 2026-07-10

- Stream daemon reconciliation phase events to ``store repair --reconcile-s3``
  clients so Garage recovery no longer starts without operator-visible evidence.

## 0.71.0 - 2026-07-10

- Add guarded ``dasobjectstore store repair STORE --reconcile-s3`` recovery
  for uncatalogued Garage objects. The daemon downloads through private SSD
  staging, checksums and settles via normal RemoteS3 ingest, then runs the
  existing metadata repair flow; it never registers a bucket listing as data.

## 0.70.0 - 2026-07-10

- Add daemon-owned `store verify` health checks with optional in-flight payload
  hashing, missing/orphan payload detection, size/hash mismatch reporting, and
  duplicate placement findings.
- Add guarded `store deduplicate` checksum scanning that records verified hashes
  and removes only duplicate metadata rows after explicit confirmation; payload
  files are never deleted automatically.
- Allow `store contents STORE/PREFIX` targets and label directory/file entries
  explicitly in text and JSON output.

## 0.69.75 - 2026-07-10

- Wire `store repair` through the daemon request-family dispatcher so the
  released CLI command reaches the storage handler.

## 0.69.74 - 2026-07-10

- Persist completed ingest placements and inline checksums to live metadata.
- Add daemon-owned `store repair` dry-run/apply recovery for payloads left
  behind by interrupted or historically incomplete metadata commits, preserving
  timestamped SQLite backups and refusing to claim hash verification.
- Recover and document the appliance metadata index after the live SQLite file
  was found empty while managed HDD payloads remained present.

## 0.69.73 - 2026-07-10

- Fix remote-upload admission to accept the canonical `s3_bucket` export label
  emitted by the ObjectStore registry and dashboard.

## 0.69.72 - 2026-07-10

- Move Store contents tree/du rendering into the CLI `store_read` module and
  keep the top-level runner focused on dispatch.

## 0.69.71 - 2026-07-10

- Add password-authenticated remote ObjectStore access with verified HTTPS,
  store-scoped eight-hour Garage connection contexts, explicit JSON secret
  output, and persisted-credential validation.

## 0.69.70 - 2026-07-10

- Add `store objects` and `store list-contents` aliases for the ObjectStore
  contents listing command.

## 0.69.69 - 2026-07-10

- Extract Web screenshot viewport, role, and workspace fixture matrices into a
  dedicated module shared by the visual runner.

## 0.69.68 - 2026-07-10

- Extract GUI authentication bucket normalization and endpoint/enclosure enum
  parsing into a dedicated validation helper module.

## 0.69.67 - 2026-07-10

- Split remote-upload Web styles into a feature-owned Trunk stylesheet while
  preserving shared CSS contract coverage.

## 0.69.66 - 2026-07-10

- Extract GUI daemon response projections and stable administration/job labels
  into `auth_reporting.rs`.

## 0.69.65 - 2026-07-10

- Extract GUI authentication daemon-client submission adapters into
  `auth_clients.rs` and consolidate their unavailable-client error mapping.

## 0.69.64 - 2026-07-10

- Extract standalone GUI API route composition into `auth_router.rs`, keeping
  authentication and administration handlers separate from router assembly.

## 0.69.63 - 2026-07-10

- Extract performance-report artifact rebuild dispatch into the dedicated
  performance report module.

## 0.69.62 - 2026-07-10

- Extract CLI platform probe dispatch into a dedicated read-only probe module.

## 0.69.61 - 2026-07-10

- Extract CLI health output-mode dispatch into a dedicated health runner
  module while preserving shared platform projections.

## 0.69.60 - 2026-07-10

- Extract Store ingest-policy read/update handling into the CLI Store write
  module while preserving typed daemon requests and output formats.

## 0.69.59 - 2026-07-10

- Extract Store create/adopt runtime handlers into a dedicated CLI write
  module while preserving validation and registry behavior.

## 0.69.58 - 2026-07-10

- Extract SubObject CLI runtime handlers and registry report helpers into a
  dedicated command-family module.

## 0.69.57 - 2026-07-10

- Route CLI `object put` through a typed daemon request so staged placement and
  metadata mutation remain behind the authenticated daemon boundary.

## 0.69.56 - 2026-07-10

- Route `store delete` through a typed daemon request with daemon-owned
  metadata cleanup, registry cleanup, authorization, policy allowance, and
  action-time confirmation.

## 0.69.55 - 2026-07-10

- Extract the read-only CLI `store s3-upload` plan renderer into the dedicated
  store-read runner module.

## 0.69.54 - 2026-07-10

- Extract read-only CLI `store list` and `store defaults` handlers into the
  dedicated store-read runner module.

## 0.69.53 - 2026-07-10

- Extract read-only CLI store inspection and policy-validation handlers into a
  dedicated runner module.

## 0.69.52 - 2026-07-10

- Route force disk retirement through a typed daemon-owned request with
  administrator authorization, policy allowance, confirmation, and risk-gated
  metadata mutation.

## 0.69.51 - 2026-07-10

- Route normal disk retirement through a typed daemon-owned request with
  administrator authorization and daemon-selected metadata/timestamp state.

## 0.69.50 - 2026-07-10

- Route ingest queue drain through a typed daemon-owned request with explicit
  authorization, daemon timestamps, and queue-drain reporting.

## 0.69.49 - 2026-07-10

- Persist non-dry-run WebUI object-store creation requests in the daemon-owned
  registry before reporting the administrator job complete.

## 0.69.48 - 2026-07-10

- Route `store drain` through a typed daemon-owned request with daemon-side
  authorization, managed-disk discovery, execution, and report handling.

## 0.69.47 - 2026-07-10

- Isolate remote-upload transfer execution and daemon job lifecycle handling
  behind a dedicated runtime module.

## 0.69.46 - 2026-07-10

- Extract GUI API authentication and administration contracts into a dedicated
  module while preserving route payloads and validation behavior.

## 0.69.45 - 2026-07-10

- Isolate remote-upload progress reporting, telemetry enrichment, and transfer
  rate calculation from the runtime façade.

## 0.69.44 - 2026-07-10

- Isolate remote-upload admission gates, queue snapshots, and backpressure
  permits from transfer execution.

## 0.69.43 - 2026-07-10

- Isolate daemon remote-upload cancellation cleanup and multipart-abort
  handling behind a dedicated runtime module.

## 0.69.42 - 2026-07-10

- Package external-source access for Debian/RPM installations with managed
  udisks mount permissions and automatic read-only traversal preparation.

## 0.69.41 - 2026-07-10

- Extract performance report, JSON artifact, chart, and PDF rendering helpers
  into `crates/dasobjectstore-cli/src/run/performance_report.rs`.

## 0.69.40 - 2026-07-10

- Extract the small Object, Service, Mnemosyne, Pool-marker, and platform probe
  runtime handlers into `crates/dasobjectstore-cli/src/run/command_handlers.rs`.

## 0.69.39 - 2026-07-10

- Extract the Pool command contracts and parser regressions into
  `crates/dasobjectstore-cli/src/cli/pool.rs`, preserving debug-command gates
  and import/repair behavior.

## 0.69.38 - 2026-07-10

- Extract the Disk command contracts and parser regressions into
  `crates/dasobjectstore-cli/src/cli/disk.rs`, preserving destructive
  confirmation and preparation defaults.

## 0.69.37 - 2026-07-10

- Colocate the remaining Store parser regressions with the extracted command
  contracts in `crates/dasobjectstore-cli/src/cli/store.rs`.

## 0.69.36 - 2026-07-10

- Complete extraction of the remaining Store create/adopt/list/drain/delete,
  defaults, validation, and S3-upload CLI contracts into the command-family
  module.

## 0.69.35 - 2026-07-10

- Extract the Store dispatcher, ingest-policy, and contents CLI contracts with
  colocated parser coverage; destructive store command contracts remain in the
  next split slice.

## 0.69.34 - 2026-07-10

- Extract Service CLI argument contracts and colocate Docker Compose parser
  regressions in the command-family module.

## 0.69.33 - 2026-07-10

- Extract Object CLI argument contracts and colocate their parser regressions
  in the command-family module.

## 0.69.32 - 2026-07-10

- Extract SubObject CLI argument contracts and colocate their parser regression
  in the command-family module.

## 0.69.31 - 2026-07-10

- Move ingest parser regression tests beside the extracted CLI ingest command
  family, leaving root CLI tests focused on dispatcher behavior.

## 0.69.30 - 2026-07-10

- Complete extraction of the ingest status, queue, drain, and direct-import
  argument contracts into the CLI ingest command-family module.

## 0.69.29 - 2026-07-10

- Extract the ingest files/directive argument parser into its own CLI command
  family module without changing daemon request behavior or wire contracts.

## 0.69.28 - 2026-07-10

- Document the authenticated ingest-policy workflow, explicit server-local
  direct-import route, preflight fallback interpretation, and copy-aware HDD
  worker admission for operators.

## 0.69.27 - 2026-07-10

- Enrich ingest preflight route explanations with the daemon-resolved mount
  point, filesystem, backing-device source, and major:minor identifier, while
  reporting explicit unknown values when topology verification is unavailable.

## 0.69.26 - 2026-07-10

- Emit a daemon preflight route explanation before source content is read and
  render it in CLI and embedded TUI progress, including source topology,
  classified origin, store ingest mode, landing mode, and routing reason.

## 0.69.25 - 2026-07-10

- Require explicit operator intent for direct local landing: normal
  `ingest files` is now SSD-first, while `ingest direct-import` remains the
  policy-gated direct-HDD route after daemon topology verification.

## 0.69.24 - 2026-07-10

- Package the `dasobjectstore-admin` peer group and add the Web/daemon service
  user to it so daemon-side policy mutation can verify the trusted Web process
  boundary without trusting a browser-supplied administrator claim.

## 0.69.23 - 2026-07-10

- Replace the planner-only Web ObjectStore configure action with an
  authenticated ingest-policy endpoint and dashboard control that reports the
  current landing mode and forwards the logged-in administrator identity.

## 0.69.22 - 2026-07-10

- Add the daemon-backed `store ingest-policy` CLI command for policy inspection
  and authenticated updates; Unix peer credentials now gate policy mutation to
  local administrators.

## 0.69.21 - 2026-07-10

- Add a daemon-owned object-store ingest-policy update contract that preserves
  the existing policy, validates the resulting store policy, supports dry runs,
  and requires explicit confirmation before enabling direct-HDD ingest.

## 0.69.20 - 2026-07-10

- Add deterministic high-frequency progress coverage proving byte-only daemon
  events are bounded before socket delivery while preserving the latest frame.

## 0.69.19 - 2026-07-10

- Exercise USB, Web, and Remote S3 origins through the staged SSD executor
  path under a direct-capable policy, with explicit SSD-stage and SSD-flush
  progress assertions.

## 0.69.18 - 2026-07-10

- Add daemon executor route-plan coverage proving USB, Web, and Remote S3
  submissions select SSD-first under a direct-capable store policy.

## 0.69.17 - 2026-07-10

- Add executor-level external-ingress regression coverage proving USB, Web,
  and Remote S3 remain SSD-first even when the target store permits direct HDD.

## 0.69.16 - 2026-07-10

- Render daemon-provided source-read, SSD-write, and aggregate HDD-write
  phase rates explicitly in the embedded ingest TUI.

## 0.69.15 - 2026-07-10

- Add deterministic metadata fan-out coverage proving multiple physical-disk
  writers overlap while the source is read once and every target is verified.

## 0.69.14 - 2026-07-10

- Add multi-target TUI regression coverage for simultaneous HDD assignments,
  copy numbers, and non-zero per-disk rates while retaining existing one-read,
  no-preflight-hash, and ingress-policy tests.

## 0.69.13 - 2026-07-10

- Populate ingest worker telemetry for source reads, SSD staging, HDD writing,
  and durability finalization on every progress frame. The TUI queue panel now
  names all scan/source/SSD/HDD/verification lanes and worker activity.

## 0.69.12 - 2026-07-10

- Emit an explicit HDD-placement progress transition after target reservation,
  including every assigned disk/copy at zero bytes before the first write.
  Existing coalescing preserves this assignment transition for the TUI.

## 0.69.11 - 2026-07-10

- Add daemon-provided short-window source-read, SSD-write, and aggregate HDD
  write rates to ingest progress telemetry, with stale and finalization rates
  reported as zero while existing per-target rates remain authoritative.

## 0.69.10 - 2026-07-10

- Make default HDD settlement admission copy-aware and permit up to four
  concurrent distinct HDD target sets. Three- and four-disk pools no longer
  default to two writers; redundant-copy jobs remain bounded by the number of
  complete, distinct disk sets available.

## 0.69.9 - 2026-07-10

- Separate bounded daemon Unix-socket lanes for long-running ingest and
  control requests. Active ingest streams can no longer monopolize socket
  acceptance or the capacity reserved for status, inventory, and cancellation
  work; exhausted lanes return a typed ``server_busy`` response.

## 0.69.8 - 2026-07-10

- Let normal local-folder ingest provide a local-server hint, then verify its
  mount and device topology in the daemon. Only a verified, non-removable
  local block device may remain eligible for policy-approved direct HDD
  landing; USB, network, FUSE, virtual, and unknown sources fail closed to
  SSD-first.

## 0.69.7 - 2026-07-10

- Replace lifetime-average active-HDD write rates with bounded short-window
  samples that decay to zero when copying stalls and remain zero in durability
  finalization states.

## 0.69.6 - 2026-07-10

- Restore CLI and Debian-package builds after the direct-HDD durability update
  by rendering the explicit HDD ``fsync`` and atomic-rename progress stages.

## 0.69.5 - 2026-07-10

- Expose direct-HDD per-target ``fsync`` and atomic-rename finalization states
  in daemon ingest events, including their measured durations. The embedded TUI
  now identifies the active finalization state and reports zero current write
  rate while the target is not copying bytes.

## 0.69.4 - 2026-07-10

- Keep only the latest byte-progress snapshot between 100 ms embedded-TUI
  redraws, while rendering pipeline, HDD-target, and terminal transitions
  immediately.

## 0.69.3 - 2026-07-10

- Classify mounted-disk ingest as USB SSD-first and require the target store's
  direct-to-HDD policy for every server-local direct-import request.

## 0.69.2 - 2026-07-10

- Coalesce daemon ingest progress by a 1 MiB or 100 ms cadence before it reaches
  Unix-socket clients, while immediately preserving pipeline transitions, HDD
  target assignments, and terminal progress frames.
- Read each direct-to-HDD source once and fan out bounded concurrent writes to
  distinct HDD targets, with in-flight checksums and per-target `fsync` before
  atomic placement.

## 0.69.1 - 2026-07-10

- Make normal file ingest bypass pre-copy strict conflict hashing so direct
  server-to-HDD paths calculate checksums only while copying; retain
  ``--strict`` as the explicit preflight deduplication mode.

## 0.69.0 - 2026-07-09

- Preserve marker-provided DAS bay labels in daemon appliance telemetry,
  current capacity and disk IO API summaries, and per-disk IO series while the
  authoritative physical enclosure/bay registry remains future work.

## 0.68.0 - 2026-07-09

- Add a product-neutral floundeR telemetry chart contract for shared Web
  dashboard, API export, and Grammateus report consumers without
  DASObjectStore-specific chart hard-coding.

## 0.67.0 - 2026-07-09

- Add floundeR telemetry render-plan support that splits observed series into
  non-interpolated segments and emits labelled gaps for missing samples,
  service restarts, unavailable counters, unknown devices, and collection
  intervals.

## 0.66.0 - 2026-07-09

- Add versioned Mnemosyne floundeR appliance telemetry data contracts covering
  line charts with gaps, point and step summaries, capacity bands, per-disk IO
  traces, small multiples, missing intervals, and per-device metadata.

## 0.65.0 - 2026-07-09

- Add a stable Home throughput telemetry chart with fixed axes, bounded labels,
  and an empty-sample state so telemetry refreshes do not resize dashboard
  cards or overlap text.
- Refresh the selected Home telemetry window on a fixed browser cadence while
  preserving the existing authenticated dashboard load/error states.

## 0.64.0 - 2026-07-09

- Add a global Home telemetry window control for 1 hour, 1 day, 10 days, and
  3 months, with the Home API filtering daemon-backed telemetry summaries by
  the selected window.
- Render the Home telemetry window selector in the Web console and include the
  selected window in the throughput metric state.

## 0.63.0 - 2026-07-09

- Add telemetry-backed Home dashboard cards for Disk IO, CPU usage, and
  logged-in users, with explicit unavailable states when daemon appliance
  telemetry has not been written yet.

## 0.62.5 - 2026-07-09

- Skip strict direct-import duplicates before HDD copy even when a previous
  interrupted run left the content-addressed payload on disk without a live
  metadata row, while preserving inline hashing for genuinely new files.

## 0.62.4 - 2026-07-09

- Prefer daemon appliance telemetry for the Web Home dashboard Capacity,
  seven-day Throughput, and Memory Stress cards when telemetry samples are
  available, while preserving the existing filesystem, throughput JSON, and
  ``/proc/meminfo`` fallbacks.
- Bound Home dashboard Docker object-service probes so a missing or stalled
  Docker CLI cannot block the Web dashboard route.

## 0.62.3 - 2026-07-09

- Skip existing objects before direct-to-HDD or SSD ingest work when the
  configured conflict policy can prove the incoming file already matches
  recorded metadata, avoiding expensive duplicate HDD copies that fail only at
  final placement.

## 0.62.2 - 2026-07-09

- Fix the standalone Web ObjectStore browser so browse, file download, and
  folder archive requests delegate the authenticated local browser user to the
  daemon instead of authorizing as the ``dasobjectstore`` service account.
- Restrict object-browser delegated authorization to root or the packaged
  daemon service peer and add regression coverage for trusted service
  delegation and rejected non-service impersonation.

## 0.62.1 - 2026-07-09

- Fix server-local ``ingest direct-import`` so checksum calculation happens
  inline during HDD copy rather than as a producer-side source prehash that
  starves the HDD worker pool.
- Add direct-to-HDD object copy support that streams to a temporary HDD payload,
  calculates SHA-256 during the write, and renames into the content-addressed
  object path once the hash is known.
- Update direct-import progress accounting so live TUI displays HDD write
  activity and active disk transfers instead of a misleading source-read
  hashing phase.

## 0.62.0 - 2026-07-09

- Persist Garage store-scoped credentials in a daemon-owned managed registry
  with private file permissions and auditable issued, reused, and rotated
  events.
- Reuse persisted Garage credentials during repeated ``dasobjectstore service
  provision`` runs instead of minting replacement keys from the ObjectStore
  registry every time.
- Add ``dasobjectstore service provision --rotate-credentials`` for explicit
  credential rotation, with provisioning output that reports credential registry
  path and issued/reused/rotated counts without exposing S3 secret material.
- Package the protected object-service state directory for DEB/RPM installs and
  document the credential custody and rotation workflow for S3 uploads.
- Wire daemon appliance telemetry to report managed-HDD capacity and Linux
  `/proc/diskstats` read/write IO rates using retained cadence-aware samples.
- Add appliance session telemetry for active standalone Web sessions, remote
  easyconnect agents, distinct logged-in users, and local administrator/operator
  session counts where host group data is readable.
- Bound the appliance telemetry JSON history with raw samples for the last hour,
  one-minute buckets through one day, ten-minute buckets through ten days, and
  hourly buckets through three months.
- Preserve corrupt telemetry JSON for operator inspection before starting a
  fresh schema-valid history, with regression coverage for recovery, atomic
  rewrite cleanup, missing-data markers, and disk/enclosure identity retention.
- Add an authenticated daemon appliance telemetry API command that returns
  current summaries, fixed-point chart series, available windows, per-disk IO,
  capacity/session history, and missing-data intervals from the managed state.
- Downsample appliance telemetry API chart series by requested window and add
  regression coverage for authorization, missing-data gaps, and bounded
  three-month responses.

## 0.61.2 - 2026-07-09

- Add active HDD transfer telemetry to daemon ingest progress events, including
  file index, target disk, copy number, transferred bytes, total bytes, and
  transfer rate for each active HDD settlement worker.
- Render an HDD Landing pane in the upload/direct-import TUI so server-local
  direct imports expose the same per-disk active transfer visibility expected
  from benchmark runs.
- Show HDD worker active/idle counts in the upload TUI queue summary to make
  ``--hdd-workers`` concurrency visible during live imports.

## 0.61.1 - 2026-07-09

- Decouple Garage Docker Compose rendering from static ObjectStore bucket lists
  and default Garage key environment variables so adding an ObjectStore does not
  require rebuilding or restarting the object-service container.
- Add ``dasobjectstore service provision`` to apply the live ObjectStore
  registry through the daemon, creating Garage buckets and grants against the
  running service.
- Document Garage bucket provisioning and S3 credential custody as
  DASObjectStore-managed operations rather than manual Docker Compose shell
  environment workflows.

## 0.61.0 - 2026-07-09

- Rework ``dasobjectstore ingest direct-import`` so it materially mirrors
  ``ingest files`` for endpoint, source directory, object type, copies,
  ``--hdd-workers``, conflict policy, ``--tui``, and ``--dry-run`` options,
  while submitting an explicit daemon local direct-to-HDD ingress origin.
- Remove the legacy single-object direct-import CLI shape that required
  ``--disk-id``, ``--destination``, ``--expected-sha256``, ``--policy-file``,
  and a confirmation phrase.
- Add a stable ``local_server_direct_import`` ingress-origin wire value so the
  daemon cannot silently route explicit direct imports through SSD staging.
- Update README and user/reference documentation for server-local direct-HDD
  directory imports and remove stale direct-import JSON output documentation.

## 0.60.9 - 2026-07-09

- Simplify Web Local Access group creation and user-to-group mapping by removing
  dry-run preview buttons and visible confirmation phrase entry, replacing both
  flows with explicit acknowledgement checkboxes while preserving daemon-side
  administrator markers.
- Update Local Access terminology to "data access account or tenant group" and
  "Map user to tenant group" to align the DASObjectStore page with Prosopikon
  tenant-access language.
- Pick up Prosopikon shared role-template and local-access editor widget exports
  for the Web console dependency set.

## 0.60.8 - 2026-07-09

- Add server-side easyconnect API contract coverage for session revocation,
  eight-hour renewal semantics, rotated renewal-token responses, and standalone
  local-user ObjectStore grant filtering.
- Add standalone easyconnect auth-context route regressions for invalid,
  expired, and revoked persisted local sessions.
- Add remote-upload runtime regression coverage for failed paired uploads that
  report active-upload renewal progress, clean abandoned session state, release
  S3 admission capacity, and preserve failed daemon job status.
- Add a file-backed remote easyconnect paired-session store for daemon runtime
  persistence with revocation, renewal token rotation, expiry, actor matching,
  and per-ObjectStore write authorization coverage.
- Wire daemon remote easyconnect session revocation and renewal commands to the
  persisted paired-session store, including expiry extension and rotated
  renewal-token responses.
- Add daemon-backed remote easyconnect pairing create, approve, and exchange
  handling that persists paired sessions and filters ObjectStore inventory
  through persisted remote-upload session grants.
- Tighten remote-upload ObjectStore inventory authorization with denial tests
  for non-writer sessions, non-S3/read-only stores, and missing writer-group
  diagnostics.
- Add remote-client easyconnect regression coverage for successful, denied, and
  expired pairing callbacks, paired upload sessions, renewal metadata states,
  and local rejection of expired sessions before stored credentials are used.
- Render remote easyconnect daemon upload job events in
  ``dasobjectstore-remote upload --submit-to-daemon`` with percent, byte/unit
  counters, stage, and daemon message/failure details, while allowing
  ``--no-progress`` to suppress intermediate rows.
- Render daemon-recorded remote-upload progress in Web Activity task rows so
  browser refresh, disconnect, and reconnect recover current transfer stage,
  byte counters, percent complete, and daemon messages from persisted job state.
- Populate remote-upload progress with session-renewal status telemetry from
  paired easyconnect session renewal metadata.
- Populate remote-upload progress with active HDD writer count and pending
  verification-state telemetry derived from daemon ingest telemetry.
- Populate remote-upload progress with non-zero SSD stage and HDD landing
  queue-depth telemetry from the daemon admission gate snapshot.
- Derive remote-upload S3 transfer-rate telemetry from daemon byte progress
  timestamps when transfer producers do not provide an explicit rate.
- Wire easyconnect/AWS CLI remote-upload submissions to populate source scan
  count and staged-byte telemetry from the remote client's source inventory.
- Add a typed daemon remote-upload progress telemetry payload covering source
  scan count, staged bytes, S3 transfer rate, SSD/HDD queue depths, active HDD
  writers, verification state, and session-renewal status.
- Add a concrete daemon remote-upload cancellation cleanup runtime for managed
  SSD stage cleanup, local session/pairing/browser-handoff state cleanup, and
  AWS CLI multipart aborts with managed-root path containment checks.
- Wire remote upload transfer-worker execution to cancellation cleanup plans so
  failed transfers can return daemon-visible cleanup reports without holding S3
  admission capacity during cleanup.
- Include Garage bootstrap environment variables in rendered production Compose
  YAML so the selected Garage image can start with ``--default-bucket`` using a
  project-local ``.env`` file rather than failing after container creation.

## 0.60.7 - 2026-07-09

- Add a daemon runtime cleanup worker facade for remote-upload cancellation
  plans so cleanup execution reports per-action success or failure and
  continues after non-blocking cleanup failures.
- Add a typed daemon runtime cleanup plan for cancelled or interrupted remote
  uploads, covering partial SSD staging, failed multipart uploads, abandoned
  sessions, expired pairings, and interrupted browser handoffs.
- Surface S3-compatible object-service status in the Web Home dashboard,
  including active state, bind address, port, service state, and a remote-ready
  endpoint URL for Mnemosyne ecosystem clients.
- Extend the top-level runtime status healthcheck payload with object-service
  remote readiness and remote URL fields.
- Align daemon-managed Garage service defaults with production remote upload by
  reporting a non-loopback object-service endpoint.

## 0.60.6 - 2026-07-09

- Wire ``dasobjectstore-remote upload --submit-to-daemon`` through the typed
  easyconnect daemon upload route, including source-byte accounting, redacted
  display arguments, and AWS session environment handoff.
- Add a typed daemon API, client helper, and request-handler route for
  easyconnect AWS CLI remote-upload jobs so submissions reach the
  admission-gated runtime executor.
- Make rendered object-service Compose bind addresses explicit, defaulting the
  CLI render path to ``0.0.0.0`` for remote-upload endpoints while retaining a
  configurable ``--bind-address`` override for loopback-only deployments.
- Improve ``dasobjectstore status`` object-service reporting so Docker
  loopback-only port bindings are detected and surfaced as unsuitable for
  remote upload clients.

## 0.60.5 - 2026-07-09

- Add a daemon runtime easyconnect AWS CLI upload-job executor that constructs
  remote-upload jobs and S3 transfer plans before running them through the
  admission-gated worker.
- Add a daemon AWS CLI remote-upload byte-transfer adapter that runs concrete
  S3-compatible transfer commands through the admission-gated worker and
  records completion byte progress.
- Fix packaged standalone Web login by storing the Prosopikon-backed
  DASObjectStore local auth registry under the writable appliance state
  directory, ``/var/lib/dasobjectstore/auth``, instead of Prosopikon's
  product-level default root.

## 0.60.4 - 2026-07-09

- Add a typed daemon remote-upload byte-transfer adapter so concrete
  S3/object-service transfer implementations run through the admission-gated
  worker and shared job-progress recorder.
- Add live byte-progress reporting to the daemon remote-upload transfer worker
  so concrete byte-transfer implementations can persist intermediate job
  progress while admission capacity is held.
- Add daemon remote-upload queue observers that derive SSD staging, HDD
  landing, and verification depths from daemon ingest telemetry before
  admission decisions.
- Add a daemon remote-upload S3 transfer worker facade that acquires admission
  capacity, records running/final job states, executes a byte-transfer closure,
  and releases capacity after completion or failure.
- Fix the standalone web server startup panic caused by duplicate
  ``/api/v1/workspaces/remote-upload`` route registration during Axum router
  assembly.
- Include remote-upload daemon jobs in GUI activity and admin job rendering so
  the latest daemon job model remains buildable and visible through the web
  API.

## 0.60.3 - 2026-07-09

- Add daemon job registry persistence for remote-upload S3 transfer summaries
  so transfer attempts can be queried through the common job status/list path.
- Add the stable ``remote_upload`` daemon job kind and map remote-upload S3
  transfer job summaries into the shared daemon job event model for completed,
  waiting, rejected, and failed transfers.
- Add a daemon remote-upload S3 transfer job wrapper that records job identity,
  target ObjectStore, source bytes, admission/transfer outcome, and runtime
  queue state while enforcing the central admission gate before transfer code
  runs.
- Include Prosopikon in ``make pull`` sibling repository discovery and add a
  Web packaging preflight that verifies the local Prosopikon checkout exposes
  the required ``auth`` and ``pam`` features before Trunk starts, avoiding the
  misleading crates.io ``prosopikon-core`` feature-resolution failure during
  ``make deb`` and ``make rpm``.

## 0.60.2 - 2026-07-09

- Add a daemon remote-upload S3 transfer execution wrapper that acquires an
  admission permit before invoking worker transfer code and releases capacity
  after success or transfer failure.
- Add an RAII-style daemon remote-upload S3 transfer permit so concrete upload
  workers can reserve bounded transfer capacity and release it safely on
  completion or failure.
- Add a daemon runtime remote-upload admission gate that tracks active S3
  transfers plus SSD staging, HDD landing, and verification queue depths before
  accepting more remote intake.
- Wire remote-upload admission decisions into the daemon request/response,
  request handler, and typed client boundary so upload executors can ask the
  daemon before admitting more intake.
- Add daemon remote-upload admission decisions for SSD pressure, S3 transfer
  concurrency, SSD staging, HDD landing, and verification queue limits.
- Promote ingress-origin classification into the core domain crate and include
  ``remote_s3``/``ssd_first`` in remote S3 upload plans and CLI output.
- Advertise shared ``web_upload``, ``remote_s3``, ``synoptikon``, and
  ``mneion`` ingress placement contracts through Web remote-upload and
  Mnemosyne/Synoptikon binding payloads.
- Update default HDD landing worker fan-out to
  ``max(managed_hdd_count - 2, 2)`` capped by available HDDs, with one worker
  only for one-HDD/degraded cases.
- Reject duplicate managed HDD disk IDs in the daemon settlement scheduler so
  active writers and redundant object copies remain tied to distinct physical
  disks.
- Add a shared remote-upload backpressure policy and surface it through
  easyconnect handoff, Web remote-upload workspace payloads, and remote S3
  upload plans.
- Carry typed ingress origin through daemon file-ingest requests, default
  legacy requests to ``local_server``, and have the daemon runtime use the
  request origin when deciding SSD-first versus direct-to-HDD landing.
- Route local-server daemon file ingest through direct-to-HDD landing when the
  target store policy explicitly uses ``DirectToHdd``, hashing sources before
  daemon-selected verified HDD settlement and avoiding SSD payload staging.
- Add daemon ingress-origin classification with stable ``remote_s3``,
  ``web_upload``, ``local_server``, ``synoptikon``, and ``mneion`` names, and
  advertise ``ssd_first`` landing mode for remote easyconnect upload handoffs.
- Route ``dasobjectstore-remote upload`` through paired easyconnect ObjectStore
  grants so users pass ObjectStore names, the client derives bucket routing,
  and appliance-issued session credentials are used for S3 transfer planning.
- Define the remote easyconnect browser-to-local-agent upload handoff contract
  with loopback-only agent URLs, explicit confirmation text, relative-path
  privacy, byte-total validation, and named unreachable/cancelled failure
  states.
- Add a Web ``Remote Upload`` drag/drop selection panel that captures browser
  file/folder metadata, target ObjectStore choice, byte totals, folder counts,
  largest-file summaries, and an explicit paired-agent handoff state.
- Delegate standalone DASObjectStore authentication registry, session, token,
  and local PAM plumbing to Prosopikon while preserving existing DAS route
  response contracts.
- Mark DEB/RPM native PAM dependencies with the Prosopikon authentication
  dependency marker for package infrastructure.

## 0.60.1 - 2026-07-09

- Add the authenticated Web ``Remote Upload`` workspace with easyconnect
  ObjectStore visibility filtering, writer readiness, capacity warning, export
  state, and upload-allowed summaries for paired remote agents.
- Add ``dasobjectstore ingest files --hdd-workers`` and daemon-side validation
  so operators can explicitly set HDD settlement fan-out while preventing more
  concurrent workers than managed HDDs.
- Default normal daemon file ingest to detected managed HDD count minus two,
  bounded to at least one worker, so SSD staging can continue while bounded HDD
  settlement drains concurrently instead of falling back to a stale single
  worker policy.

## 0.60.0 - 2026-07-09

- Add daemon-owned remote easyconnect ObjectStore grant filtering so remote
  upload sessions only list ObjectStores readable by the authenticated actor
  and only grant upload rights when daemon writer authorization allows ingest.

## 0.59.1 - 2026-07-09

- Rename the standalone Users/Groups console to Local Access, declare the
  DASObjectStore Prosopikon authentication framework as `Hybrid`, expose
  device-token readiness metadata, and start rendering Prosopikon-owned local
  user/group/membership selector widgets in the Web console.

## 0.59.0 - 2026-07-09

- Set remote easyconnect upload sessions to an eight-hour default contract,
  define renewal timing and token-rotation semantics, expose the policy through
  daemon and remote-client contracts, and redact stored renewal tokens from
  remote client support output.

## 0.58.0 - 2026-07-09

- Add server-side remote easyconnect pairing/session API contracts and typed
  daemon client methods for discovery, pairing creation, browser approval,
  session exchange, revocation, and renewal.

## 0.57.0 - 2026-07-09

- Implement the client-side ``dasobjectstore-remote easyconnect`` browser
  launch flow with loopback callback binding, timeout handling, ``--no-browser``
  fallback, one-time pairing result capture, and redacted exchange-code output.

## 0.56.0 - 2026-07-09

- Add paired-appliance remote client configuration storage for easyconnect
  sessions, including temporary upload credentials, expiry and renewal metadata,
  default ObjectStore selection, preservation across ``config set``, and
  redacted ``config show`` output.

## 0.55.1 - 2026-07-09

- Reframe the standalone Web Users/Groups console as a Prosopikon-aware local
  appliance capability mapping view while preserving the existing
  ``workspaces/users-groups`` API route.

## 0.55.0 - 2026-07-09

- Add the public ``dasobjectstore-remote easyconnect <host-or-ip>`` contract
  command with stable discovery URL, browser-login URL, local callback/polling
  lifecycle, failure-state, and JSON output semantics for the planned
  browser-approved remote upload pairing flow.

## 0.54.25 - 2026-07-09

- Improve Web ObjectStore browser placement fidelity with SSD landing,
  verified settled HDD, external endpoint, pending, degraded, missing,
  redownload-required, and unavailable state summaries.

## 0.54.24 - 2026-07-09

- Render DASObjectStore performance report figures as container-generated
  ggplot2 PNG assets and embed them in the formal Grammateus PDF instead of
  printing chart alt text or raw benchmark JSON.

## 0.54.23 - 2026-07-09

- Add the Web ObjectStores browser panel with authenticated daemon-backed
  object metadata loading, endpoint selection, breadcrumbs, folder navigation,
  sortable file tables, readiness/lifecycle badges, and placement badges.

## 0.54.22 - 2026-07-09

- Remove full benchmark JSON payloads from formal performance report bodies and
  replace them with concise reproducibility provenance so chart sections render
  without thousands of pages of raw JSON.

## 0.54.21 - 2026-07-09

- Add daemon-authorized ObjectStore folder archive downloads with verified
  managed-HDD preflight, bounded streaming ``tar.gz`` generation, and standalone
  Web API headers that expose source byte and file counts before the stream.

## 0.54.20 - 2026-07-09

- Promote performance recommendation rationale into its own report section so
  it does not visually crowd the recommendation table in formal PDFs.

## 0.54.19 - 2026-07-09

- Add explicit spacing before performance report rationale text so the
  recommendation table remains visually separated in formal PDFs.

## 0.54.18 - 2026-07-09

- Add daemon-authorized individual ObjectStore file download resolution and a
  standalone Web API route that streams verified settled HDD copies with safe
  download headers.
- Compact the visible DASObjectStore performance report metadata and early
  recommendation tables so long identifiers, hashes, paths, and quantitative
  values do not overflow formal PDF table bounds.

## 0.54.17 - 2026-07-09

- Reduce and constrain the Mnemosyne Biosciences login wordmark so the brand
  lockup reads as a balanced signature rather than dominating the appliance
  login pane.

## 0.54.16 - 2026-07-09

- Add first-class ObjectStore reader-group and authenticated public-read policy
  fields, and enforce them for daemon-backed ObjectBrowser metadata access.
- Align DASObjectStore performance report metadata with the shared Grammateus
  ``dasobjectstore-performance`` template so rebuilt PDFs from existing JSON
  artifacts render with the required Run ID, QR provenance, and signature table.

## 0.54.15 - 2026-07-09

- Fix the packaged Grammateus report wrapper to invoke the
  ``grammateus/report:0.8.1`` provider image through its configured
  ``grammateus_markdown_pdf`` entrypoint instead of passing a duplicate command
  argument.

## 0.54.14 - 2026-07-09

- Declare Docker Buildx as a formal report-provider dependency because the
  Grammateus/floundeR provider build uses Docker named build contexts.
- Document the Debian and Docker CE package-name differences for Buildx so
  appliance installs can repair the provider build path reproducibly.

## 0.54.13 - 2026-07-09

- Add a `make report-provider` build target that initialises the formal
  Grammateus/floundeR PDF report container through the Grammateus-owned
  `grammateus_report_provider` installer.
- Run the report-provider initialisation target before `make deb` and
  `make rpm` package assembly so report-enabled appliance builds do not depend
  on a later ad hoc Docker pull.
- Prewarm the packaged report provider during DEB/RPM configuration when the
  Grammateus installer is available, and report the exact repair command when
  it is not.

## 0.54.12 - 2026-07-09

- Move Web performance-report rebuild scratch space out of systemd private
  ``/tmp`` and into ``/var/lib/dasobjectstore/report-rebuild`` so Docker can
  bind-mount the renderer inputs and outputs.
- Package the report rebuild scratch directory through tmpfiles and DEB/RPM
  post-install setup.

## 0.54.11 - 2026-07-09

- Add the packaged ``dasobjectstore`` service user to the ``docker`` group
  during DEB/RPM configuration so Web performance-report rebuilds can launch the
  Grammateus renderer container.
- Improve the packaged report-renderer wrapper diagnostics when the Web service
  cannot access the Docker API socket.

## 0.54.10 - 2026-07-09

- Package a DASObjectStore-owned ``gnostikon-workflow-control`` compatibility
  wrapper for Grammateus PDF rendering so local DEB/RPM installs no longer
  depend on a noninstallable external renderer package.
- Keep Docker/container runtime as the formal report-rendering dependency and
  teach the CLI to prefer the packaged wrapper before falling back to a
  developer-provided ``gnostikon-workflow-control`` command.

## 0.54.9 - 2026-07-09

- Add ``gnostikon-workflow-control`` to Mnemosyne sibling discovery for
  ``make pull`` so the Grammateus report handoff source is kept current with
  DASObjectStore development checkouts.
- Declare ``gnostikon-workflow-control`` and the Docker command/container
  runtime as required DEB/RPM runtime dependencies for the supported Web
  performance-report rebuild workflow.
- Clarify CLI and user documentation for formal performance PDF rendering so
  missing Grammateus handoff dependencies are treated as packaging/install
  faults rather than optional developer tooling.

## 0.54.8 - 2026-07-09

- Add a per-process GUI API health instance identifier so the Web console can
  detect server restarts even when the product version is unchanged.
- Tighten the connected Web session heartbeat to poll server health every five
  seconds, require the server version and instance identity to match the
  authenticated session, and immediately clear tokens/unmount authenticated
  pages when the server is unreachable, restarted, changed version, or rejects
  the session.

## 0.54.7 - 2026-07-09

- Refresh the Web Users/Groups workspace after successful live local group
  creation or user assignment so writer groups and assignment controls update
  immediately without a browser reload.
- Reset stale local group creation form state after successful submission and
  seed the assignment form with the newly created group for a clearer operator
  journey.

## 0.54.6 - 2026-07-09

- Fix Web build warnings by retaining the session heartbeat interval for effect
  cleanup and removing an unnecessary mutable PDF download blob option binding.

## 0.54.5 - 2026-07-09

- Simplify Web ObjectStore creation so operators choose store name, writer
  group, mounted enclosure, object type, class, redundancy, export mode, and
  visibility while S3 bucket, SSD root, retention, capacity behavior, and
  writeable state are derived from product policy.
- Add mounted enclosure choices to the ObjectStores dashboard payload and block
  Web creation until an administrator has at least one mounted DAS enclosure to
  anchor the store.
- Default minimal Web/API ObjectStore create requests to generated-data,
  ``naive`` object type, retain-until-deleted retention, writer-group writeable
  state, and a bucket name derived from the store name.

## 0.54.4 - 2026-07-09

- Make Web local group creation idempotent for host groups that already exist,
  adopting them instead of failing a live `groupadd` operation.
- Reconcile successful live local group creation and membership assignment into
  `/opt/dasobjectstore/groups.json` so existing OS writer groups become visible
  to ObjectStore policy and the Users/Groups workspace.

## 0.54.3 - 2026-07-09

- Hide Web enclosure preparation controls unless the API can advertise a valid
  unprepared DAS enclosure candidate for an administrator session.
- Block Web enclosure preparation requests against mount roots that already
  contain DASObjectStore managed enclosure metadata; deliberate destructive
  re-preparation remains a CLI-only workflow.

## 0.54.2 - 2026-07-09

- Invalidate standalone Web browser session tokens on server startup so a
  service restart forces users to sign in again.
- Add Web session heartbeat handling that automatically logs out invalidated
  sessions and shows a disconnected message when the server is unreachable,
  clearing the message when the Web API responds again.

## 0.54.1 - 2026-07-09

- Refine the Web login page Mnemosyne Biosciences wordmark sizing and
  centering so the brand pane reads as a composed identity lockup rather than a
  large illustration.

## 0.54.0 - 2026-07-09

- Add an authenticated Activity Reporting card with drag-and-drop benchmarking
  JSON upload for rebuilding formal DASObjectStore performance reports.
- Stream rebuilt performance reports back to the browser as PDF downloads using
  the existing `dasobjectstore performance-report` Grammateus rendering path.
- Add bounded report-rebuild request handling, schema validation, renderer
  timeout handling, Web upload/download helpers, and documentation for the
  Activity reporting workflow.

## 0.53.5 - 2026-07-09

- Add the full Mnemosyne Biosciences wordmark to the Web login page using a
  trimmed packaged asset and responsive login-page sizing.
- Keep the compact Mnemosyne icon treatment for authenticated navigation while
  extending screenshot regression checks to validate the login wordmark.

## 0.53.4 - 2026-07-09

- Replace the Web UI boxed placeholder brand mark with the Mnemosyne
  Biosciences icon asset in the login lockup and authenticated top bar.
- Extend Web screenshot regression checks to assert the packaged Mnemosyne
  brand icon is present and visibly rendered.

## 0.53.3 - 2026-07-09

- Fix standalone Web administrator detection so sudo-derived authority is
  resolved for the authenticated local session username rather than the Web
  service process account.
- Serve standalone dashboard admin affordances through authenticated routes so
  Enclosure and ObjectStore Web controls reflect the logged-in local user's
  sudo status and group membership.

## 0.53.2 - 2026-07-09

- Extend the Web screenshot regression harness into role-aware end-to-end
  workflow coverage for viewer and administrator sessions, including enclosure
  preparation, ObjectStore creation, SubObject planning, local group
  administration, Activity task visibility, and Bioinformatics readiness.
- Add deterministic mocked daemon responses for Web workflow planning,
  administrator job acceptance/status, local group dry-run/live submissions,
  and API-derived Bioinformatics readiness/context cards.

## 0.53.1 - 2026-07-09

- Document administrator Web workflow operation, permission boundaries, daemon
  audit expectations, failed-job recovery, and Bioinformatics readiness
  semantics for operators.

## 0.53.0 - 2026-07-09

- Add Bioinformatics derivation-source API records for ObjectStore metadata,
  SubObject metadata, object-type assignments, endpoint/export state, and
  Mneion/Mnemosyne governance bindings.
- Render derivation-source cards generically in the Yew Bioinformatics page so
  readiness evidence is API-owned rather than hard-coded in browser workflow
  paths.
- Document the Bioinformatics derivation-source contract as the handoff point
  for live ObjectStore/SubObject and Mneion metadata aggregation.

## 0.52.0 - 2026-07-09

- Add Bioinformatics context views for sequencing run provenance, object
  lineage, basecalling readiness, genome/transcriptome workflow handoff, and
  Mnemosyne governance binding state.
- Extend the Bioinformatics Web/API contract with read-only context cards for
  provenance, lineage, workflow handoffs, and project/governance bindings.
- Document that Bioinformatics context cards are informational and API-owned
  until ObjectStore/SubObject metadata and Mneion bindings drive live state.

## 0.51.0 - 2026-07-09

- Replace the Bioinformatics Web placeholder with object-type readiness cards
  for BAM, CRAM, POD5, FASTQ/FASTQ.GZ, FASTA, VCF/BCF, GFF/GTF, and ENA/SRA
  data families.
- Extend the Bioinformatics workspace API contract with readiness-card
  metadata covering category, state, workflow intent, handoff target, and
  required metadata while preserving existing supported-object-type fields.
- Document the Bioinformatics readiness-card surface and its API-owned boundary
  before provenance and lineage-derived states are added.

## 0.50.0 - 2026-07-09

- Promote the Endpoints workspace into standalone Web primary navigation and
  render registry-backed endpoint inventory cards from the authenticated API.
- Add Yew endpoint-administration controls for endpoint identity, validation
  state, active ObjectStore/governance-domain bindings, dry-run/live
  confirmation, daemon acceptance results, and permission-denied handling.
- Document the browser-side Endpoints workflow for daemon-owned endpoint
  inventory updates.

## 0.49.0 - 2026-07-09

- Add a daemon `upsert_endpoint_inventory` request/response for administrator
  endpoint inventory creation and updates, including registry persistence and
  endpoint-validation administrator job recording.
- Add a standalone authenticated Web API route for sudo-authorized local
  administrators to submit endpoint inventory upserts with dry-run/live
  confirmation gating.
- Map endpoint-validation daemon jobs into Activity and document the daemon-owned
  endpoint inventory administration workflow.

## 0.48.0 - 2026-07-09

- Add a JSON-backed endpoint inventory registry at
  `/opt/dasobjectstore/endpoints.json`, with `DASOBJECTSTORE_ENDPOINTS_PATH`
  override support.
- Connect the Endpoints workspace and Activity endpoint-validation tasks to the
  shared registry-backed inventory source.
- Surface missing, unreadable, and invalid endpoint registry states as explicit
  Web warnings instead of silently returning an empty fixture inventory.

## 0.47.0 - 2026-07-09

- Add a metadata reader for live pool repair Activity state from SSD
  `live.sqlite`, including `Repairing` and `Degraded` pool events.
- Render live repair metadata as Web Activity task rows with operator warnings
  for blocked write/repair review states.
- Add Activity task mapping for endpoint-validation states from the shared
  endpoint inventory contract and document the remaining persistent endpoint
  registry gap.

## 0.46.0 - 2026-07-09

- Connect the Web Activity workspace API to live SSD ingest queue metadata,
  including queued, active, complete, failed, and cancelled ingest rows.
- Derive Activity destage summaries from live ingest queue rows so HDD
  settlement state is visible without browser-side storage mutation.
- Document Activity source warnings for unavailable daemon job and ingest queue
  metadata feeds.

## 0.45.0 - 2026-07-09

- Add a transport-neutral daemon `job_list` request/response and typed client
  method backed by the persistent administrator job registry.
- Connect the Web Activity workspace API to live daemon administrator jobs,
  mapping enclosure preparation, ObjectStore creation, local administration,
  service, ingest, and repair-oriented job kinds into Activity task rows.
- Preserve Activity rendering when the daemon socket is unavailable by returning
  the category view with an explicit daemon-activity warning.

## 0.44.0 - 2026-07-09

- Promote Activity into the primary Web navigation and render daemon activity
  categories, ingest/destage queue summaries, active task rows, warnings, and
  empty states from the shared Activity workspace API.
- Extend the Activity workspace contract with administrator, enclosure
  preparation, ObjectStore/SubObject creation, ingest, destage, repair, and
  endpoint-validation categories.
- Add Web/API regression coverage for Activity response decoding, route
  construction, navigation, and category-state summaries.

## 0.43.0 - 2026-07-09

- Add Users/Groups Web forms for daemon-backed local group creation and local
  user-to-group assignment, with dry-run preview, exact confirmation phrase,
  accepted-result rendering, and error display.
- Add typed frontend API requests and responses for the existing standalone
  local group administration routes.
- Add Web regression coverage for local group admin payload decoding and
  form readiness/confirmation gates.

## 0.42.0 - 2026-07-08

- Promote Users/Groups into the primary Web navigation for standalone
  local-user appliances while keeping Synoptikon/Monas integrated hosts
  host-authority gated.
- Add a read-only Users/Groups Web page backed by the existing authenticated
  workspace route, showing host mode, current OS authority, product-local
  users, local groups, writer groups, administrator readiness, and warnings.
- Add Web regression coverage for host-aware navigation, Users/Groups API path
  construction, payload decoding, and summary card derivation.

## 0.41.0 - 2026-07-08

- Project Web-submitted ObjectStore creation requests into the same typed store
  service definition used by CLI registry paths, and reject unsupported or
  invalid store policy combinations before daemon submission.
- Add Web/API regression coverage proving ObjectStore creation forwards the
  expected registry/domain shape and rejects invalid policy vocabulary.
- Add Web action-plan regression coverage proving SubObject creation matches the
  CLI SubObject registry definition shape.

## 0.40.0 - 2026-07-08

- Add an explicit existing-data acknowledgement gate to enclosure preparation
  requests so destructive DAS preparation cannot be submitted through the Web,
  daemon API, or CLI action-plan path without acknowledging that selected media
  may already contain data.
- Add ``disk prepare-das --acknowledge-existing-data`` and include it in Web
  action-plan output for confirmed enclosure preparation.
- Add regression tests for daemon validation, standalone Web risk gates, GUI
  action planning, and Enclosures wizard retry/cancellation state preservation.

## 0.39.0 - 2026-07-08

- Add a Web SubObject creation workflow on the ObjectStores page with parent
  ObjectStore selection, nested SubObject parent entry, SSD-root review,
  object-type inheritance/override, S3 routing mode, and registry prefix
  preview.
- Extend the GUI action-plan request contract with SubObject review policy
  fields and validate object type plus S3 routing before returning a
  ``subobject_create`` plan.
- Add GUI API and Web regression coverage for SubObject planning with review
  policy fields, invalid policy rejection, and browser registry preview text.

## 0.38.0 - 2026-07-08

- Add a Web ObjectStore configuration workflow for existing stores, including
  store selection, redundancy, writer group, public/writeable policy,
  retention, capacity behavior, export mode, store class, and SSD-root review.
- Add a distinct ``store_configure`` GUI action-plan contract with conservative
  validation for store class, copy count, retention, capacity behavior, and
  export mode before administrators can review policy changes.
- Add GUI API and Web regression coverage for ObjectStore configuration action
  catalog entries, route planning, invalid policy rejection, and browser review
  summaries.

## 0.37.0 - 2026-07-08

- Add a transport-neutral daemon ObjectStore creation command, validation
  contract, typed client method, request-handler dispatch, and persisted
  administrator job kind.
- Convert the Web ObjectStore creation card from plan-only review to an
  administrator-gated daemon submission workflow with exact confirmation
  phrase, accepted job metadata, and audit context.
- Add daemon, GUI API, and Web regression coverage for ObjectStore creation
  validation, command routing, confirmation gating, request forwarding, and
  browser route contracts.

## 0.36.0 - 2026-07-08

- Add browser-side ObjectStore creation controls for store identity, writer
  group, enclosure anchor, object type, redundancy, public/writeable flags,
  store class, capacity behavior, retention, S3/export mode, bucket, and SSD
  root.
- Wire the ObjectStore creation form to the existing GUI action-plan endpoint
  so administrators can review the generated `store create` plan before the
  daemon submission workflow is added.
- Add Web regression coverage for ObjectStore bucket normalization, required
  planning fields, and policy-review summaries.

## 0.35.0 - 2026-07-08

- Render live daemon administrator job status in the Enclosures preparation
  wizard after a confirmed enclosure-preparation submission.
- Add Web wizard controls for manual status refresh, cancellation requests, and
  retry reset while preserving selected enclosure media.
- Add progress-state helpers and regression tests for daemon job terminal
  states, percentage display, and byte/unit progress text.

## 0.34.0 - 2026-07-08

- Add a daemon-owned file-backed administrator job registry under the daemon
  state directory for persisted job status and cancellation state.
- Wire the packaged daemon to record accepted administrator jobs and serve
  generic job status/cancellation from the registry.
- Record accepted service, enclosure-preparation, and local group
  administration jobs as completed daemon job summaries until asynchronous job
  execution is introduced.
- Add regression coverage for registry persistence, cancellation semantics, and
  request-handler status/cancel integration.

## 0.33.0 - 2026-07-08

- Add transport-neutral daemon commands and typed client methods for generic
  administrator job status and cancellation.
- Expose standalone authenticated Web routes for administrator job status and
  cancellation so enclosure preparation progress can be polled through a
  daemon-owned boundary.
- Add Web and daemon regression coverage for administrator job command names,
  typed client forwarding, local administrator gating, blank cancel-reason
  rejection, status forwarding, and cancellation forwarding.

## 0.32.0 - 2026-07-08

- Add authenticated standalone Web submission for Enclosures preparation at
  ``/api/v1/workspaces/enclosures/prepare``, gated by local session and
  sudo-derived administrator authority.
- Wire the Enclosures preparation wizard to submit confirmed daemon jobs and
  display accepted job metadata or clear daemon failure messages.
- Add Web risk-gate coverage for missing sessions, non-admin users, unsupported
  empty HDD selections, missing destructive format allowance, daemon submission
  failures, and successful daemon-client forwarding.

## 0.31.0 - 2026-07-08

- Add transport-neutral daemon DTOs for Web-submitted enclosure preparation,
  including SSD/HDD media, mount root, filesystem, owner, administrator actor,
  destructive format allowance, and the required confirmation marker.
- Add typed ``DaemonClient::prepare_enclosure`` support plus request-handler
  dispatch coverage so Web administrator workflows can submit through the daemon
  boundary instead of mutating devices directly.
- Add validation coverage for absolute device paths, duplicate HDD devices,
  safe local names, format allowance, and the exact ``confirm prepare das``
  marker.

## 0.30.0 - 2026-07-08

- Add a Web Enclosures preparation wizard that selects detected SSD/HDD media,
  reviews destructive formatting risk, requires the ``confirm prepare das``
  phrase, and requests a daemon-owned preparation plan.
- Add the ``enclosure_prepare`` GUI action plan contract with server-side
  validation for SSD device, HDD devices, explicit format allowance, and the
  destructive confirmation phrase.
- Document the current preparation-plan boundary while leaving daemon job
  submission and progress rendering as the next administrator workflow slice.

## 0.29.1 - 2026-07-08

- Clarify the Web interface documentation for live-data dashboard behavior,
  canonical navigation surfaces, placeholder removal, daemon-owned mutation
  boundaries, and the shared Mnemosyne footer standard.

## 0.29.0 - 2026-07-08

- Add a Trunk-backed Playwright screenshot regression harness for login, Home,
  Enclosures, ObjectStores, and Bioinformatics pages at desktop and mobile
  widths.
- Add `make web-screenshots` to build the real WebAssembly app, serve mocked
  authenticated API payloads, capture screenshot artifacts, and fail on missing
  footer/navigation or major layout overlaps.
- Document the Web screenshot regression workflow and artifact location.

## 0.28.0 - 2026-07-08

- Add a reusable Web `DasObjectStoreFooter` component with Mnemosyne product
  footer wording, version display, monospaced dark styling, 2026 attribution,
  and `https://mnemosyne.co.uk` link.
- Apply the shared footer to the login and authenticated Web console surfaces,
  replacing separate hard-coded footer fragments.
- Add footer CSS tokens and regression coverage for the required app states.

## 0.27.0 - 2026-07-08

- Reconcile the Web console navigation boundary so legacy Stores and
  Users/Groups routes remain compatibility API helpers without standalone Yew
  holder components.
- Add regression coverage proving primary navigation uses the redesigned Home,
  Enclosures, ObjectStores, and Bioinformatics routes rather than legacy holder
  routes.
- Document the canonical Web console surfaces and the compatibility status of
  legacy Stores and Users/Groups workspace routes.

## 0.26.0 - 2026-07-08

- Add a server-side DASObjectStore groups registry reader for
  `/opt/dasobjectstore/groups.json`, with explicit missing, unreadable, and
  invalid-registry warning states.
- Expose managed writer groups and current-user membership through the
  authenticated Users/Groups workspace payload.
- Add ObjectStore writer-policy readiness to Web dashboard cards so stores show
  whether their writer group is known and whether current-user membership is
  confirmed.

## 0.25.0 - 2026-07-08

- Replace the Web ObjectStores dashboard bootstrap fixture route with a live
  registry-backed aggregator.
- Populate ObjectStore cards from the system store registry and live SQLite
  usage metadata, including writer group, object type, copy count, used
  capacity, object count, S3/export state, warnings, and last-ingest time.
- Document the ObjectStores page live-data sources and usage-warning behavior.

## 0.24.0 - 2026-07-08

- Add a live Web Enclosures `Add enclosure` affordance payload that reports
  administrator capability, supported DAS discovery, daemon readiness, disabled
  reasons, and the next operator step.
- Replace the static Yew `Add enclosure` card with enabled/disabled rendering
  driven by the live dashboard payload.
- Document the Enclosures page readiness gate and daemon-owned administrator
  workflow boundary.

## 0.23.0 - 2026-07-08

- Extend Web enclosure slot payloads with role, mount path, device path,
  filesystem, SMART warning count, and daemon-managed action availability.
- Replace the selected-enclosure slot rows with compact drive cards for SSD and
  HDD members in the Yew detail panel.

## 0.22.0 - 2026-07-08

- Replace the Web Enclosures dashboard bootstrap fixture route with a live
  managed-root aggregator that reports SSD/HDD root membership, capacity,
  mounted drive counts, inferred QNAP TL-D800C identity for `qnap-*` managed
  disks, and initial slot detail payloads.
- Add Web API regression coverage proving the Enclosures dashboard no longer
  reports the old pending fixture warning.

## 0.21.0 - 2026-07-08

- Replace the Web Home dashboard bootstrap fixture route with a live API
  aggregator for managed SSD/HDD roots, capacity, drive counts, store registry
  cards, Linux memory pressure, optional SMART warnings, and optional seven-day
  throughput telemetry.
- Add Web API regression coverage proving the Home dashboard no longer reports
  the old bootstrap "Inventory pending" state.

## 0.20.1 - 2026-07-08

- Require formal ``gnostikon-workflow-control``/Grammateus rendering for
  ``dasobjectstore performance-test`` PDF reports instead of emitting degraded
  pandoc or built-in fallback PDFs.
- Add ``dasobjectstore performance-report`` to rebuild branded Mnemosyne PDF
  reports, chart SVGs, metadata, and provenance QR payloads from existing
  performance-test JSON artifacts.
- Expand the DASObjectStore performance report metadata envelope to include the
  canonical document identifier, test identifier, version/state, device,
  operator, timestamp, run ID, test status, operator signature, and
  cryptographic signature fields.

## 0.20.0 - 2026-07-08

- Add ``dasobjectstore performance-test --file_order`` with ``fifo``,
  ``size_asc``, ``size_desc``, ``time_asc``, and ``time_desc`` upload order
  policies; ``size_desc`` is now the default benchmark order.
- Allow repeated or comma-delimited file-order sweeps so one benchmark run can
  compare FIFO and largest-first landing against the same scenario matrix.
- Record file orders in performance-test reproduction payloads, JSON scenario
  rows, tidy plot data, authoritative recommendations, and PDF reports.

## 0.19.2 - 2026-07-08

- Keep ``performance-test --tui`` responsive during large HDD landing by
  emitting time-cadenced copy progress and heartbeat updates while final
  ``sync_all()`` settlement is in progress.
- Label active HDD landing rows as copying or settling instead of leaving
  large-file transfers at ``pending`` until the next byte threshold is reached.
- Add CLI regression coverage for active HDD landing state text and split-copy
  settling progress events.

## 0.19.1 - 2026-07-08

- Replace generic Home attention copy with operator cards derived from the
  daemon Home payload for drive health, ingest pressure, destage backlog,
  capacity pressure, memory stress, DAS enclosure warnings, SMART warnings,
  ObjectStore/service readiness, and empty inventory states.
- Extend the Home dashboard Web contract with backward-compatible optional
  ingest and destage queue summaries.
- Add Web regression coverage for Home attention-card signal mapping and clear
  all-good copy.

## 0.19.0 - 2026-07-08

- Add a shared Yew API page loading model for the redesigned Web console
  covering loading, success, empty, permission-denied, transport-error, and
  stale-data states.
- Wire the Bioinformatics page to fetch the daemon-backed product workspace
  payload instead of rendering a static placeholder card.
- Add Web tests for the shared loading-state contract and Bioinformatics
  workspace response decoding.

## 0.18.2 - 2026-07-08

- Remove fixture fallback helper APIs from the redesigned Yew Web console and
  stop rendering synthetic zero-valued Home metrics while authenticated pages
  wait for live daemon dashboard payloads.

## 0.18.1 - 2026-07-08

- Fix ``dasobjectstore performance-test`` direct-to-HDD scenarios so TUI runs
  continuously render queue state, active HDD landing rows, and live HDD write
  rates instead of appearing idle until the scenario completes.

## 0.18.0 - 2026-07-08

- Add per-second Linux block-device IO sampling to ``dasobjectstore
  performance-test`` for the SSD and managed HDD roots used by each scenario.
- Record scenario ``io_samples`` and tidy ``plot_data.io_time_series`` rows in
  the performance JSON artifact for downstream policy analysis and plotting.
- Render per-run IO SVG line charts in the performance report bundle with
  solid write lines and dashed read lines.

## 0.17.0 - 2026-07-08

- Extend the Web ObjectStores dashboard card contract with object type,
  public/private state, and writeable/read-only state.
- Render ObjectStore object type and access state on redesigned Web
  ObjectStores cards.
- Add dashboard serialization and Yew mapping regression coverage for the new
  ObjectStore card fields.

## 0.16.0 - 2026-07-08

- Wire the redesigned Web ObjectStores page to load the authenticated
  ``/products/dasobjectstore/api/v1/dashboard/object-stores`` payload.
- Render ObjectStores loading, empty, permission-denied, and transport-error
  states without using fixture store cards for authenticated pages.
- Add live ObjectStore cards for store class, copy policy, placement,
  capacity, object count, writer group, endpoint mode, last ingest time,
  warning count, and health.

## 0.15.0 - 2026-07-08

- Add ``dasobjectstore performance-test --file_select <random|smaller|larger>``
  for source-folder benchmarks capped with ``--cap``.
- Default capped source-folder sampling to random whole-file selection while
  preserving sorted FIFO execution order for the selected cohort.
- Record the source file-selection policy in reproduction commands, JSON
  artifacts, and performance reports.

## 0.14.0 - 2026-07-08

- Wire the redesigned Web Enclosures page to load the authenticated
  ``/products/dasobjectstore/api/v1/dashboard/enclosures`` payload.
- Render Enclosures loading, empty, permission-denied, and transport-error
  states without using fixture hardware for authenticated pages.
- Add live enclosure cards and detail panels for connection topology, mount
  path, drive counts, capacity, warning counts, enclosure identity, and bay
  membership.

## 0.13.1 - 2026-07-08

- Enforce one active HDD settlement writer per managed HDD in daemon ingest and
  performance-test HDD landing scenarios.
- Select HDD landing workers by projected fractional free space so ingest and
  benchmark placement distribute usage across members without a fixed disk
  preference.
- Keep HDD write concurrency bounded by managed HDD count and add regression
  coverage for active disk reservations.

## 0.13.0 - 2026-07-08

- Wire the redesigned Web Home dashboard to load the daemon-backed
  ``/products/dasobjectstore/api/v1/dashboard/home`` payload after login.
- Render live Home metrics for drive inventory, mounted enclosures, capacity,
  seven-day throughput, memory stress, SMART warnings, and visible
  ObjectStores.
- Replace the Home bootstrapping placeholder with attention cards sourced from
  health, memory, and SMART warning data, with authenticated-session error
  states for dashboard loading failures.

## 0.12.1 - 2026-07-08

- Fix ``ssd-overlap-drain`` performance benchmarking so a full HDD worker
  channel no longer blocks staging the next source file to SSD when the SSD
  residency budget still has capacity.
- Preserve FIFO HDD settlement order with an explicit pending staged-file queue
  while allowing SSD staging and HDD drain to proceed concurrently within the
  safe SSD backlog window.

## 0.12.0 - 2026-07-08

- Add selectable ``dasobjectstore performance-test --scenario`` values so large
  real-world source-folder benchmarks can include only the desired scenario
  classes.
- Add ``--hdd-concurrency`` for explicit HDD worker-count matrices such as
  ``1,3,5`` instead of always sweeping every value through
  ``--max-hdd-concurrency``.
- Record the selected scenario matrix in reproduction metadata, JSON artifacts,
  and PDF reports so authoritative recommendations reflect only measured
  benchmark permutations.

## 0.11.1 - 2026-07-08

- Change generated ``dasobjectstore performance-test`` workloads to create all
  random source files up front under ``--tmp-dir`` before measured SSD/HDD
  benchmark phases begin.
- Remove generated source files on normal completion or cancellation, including
  when ``--keep-temp`` is used for benchmark objectstore inspection.

## 0.11.0 - 2026-07-08

- Add the redesigned Web operator navigation with Home, Enclosures,
  ObjectStores, and Bioinformatics pages after standalone login.
- Add Mnemosyne Biosciences login/footer branding and a top bar with the
  current username and logout control.
- Add Web dashboard API payloads for Home, Enclosures, and ObjectStores,
  including health, drive, capacity, throughput, memory, SMART warning,
  enclosure, object-store, and admin-create metadata.
- Document the redesigned Web operator flows and admin-only daemon submission
  boundaries.

## 0.10.11 - 2026-07-08

- Allow the packaged Web service to execute the root-owned local-auth helper
  with its setuid transition intact by setting `NoNewPrivileges=false`.
- Document and test the systemd setting required for PAM-backed local Web login
  on the standalone appliance.

## 0.10.10 - 2026-07-08

- Add a root-owned local authentication helper for packaged standalone Web UI
  logins so PAM can verify OS-local passwords while the Web service remains
  unprivileged.
- Install and harden the helper under
  `/usr/libexec/dasobjectstore/dasobjectstore-local-auth-helper` with
  `root:dasobjectstore` ownership and `4750` mode during Debian and RPM package
  configuration.
- Document the packaged local-auth helper and extend package asset regression
  coverage for the helper binary and permissions.

## 0.10.9 - 2026-07-08

- Correct performance benchmark rate attribution so SSD read rates are measured
  only while SSD bytes are actively read during HDD drain work.
- Charge HDD destination write rates to active `write_all()` work plus final
  file settlement, without blending in unrelated source-read or idle elapsed
  time.
- Add regression coverage for idle-rate stability, sync-only HDD settlement
  accounting, split copy progress attribution, and SSD-read report rollups.

## 0.10.8 - 2026-07-08

- Document native PAM/libclang packaging prerequisites in the generated Debian
  control metadata and RPM spec.
- Add Debian/RPM build preflight checks that report the required PAM/libclang
  build packages before native Rust compilation starts.

## 0.10.7 - 2026-07-08

- Authenticate standalone Web UI logins against OS-local users through PAM
  instead of requiring users to exist first in the DASObjectStore session
  registry.
- Keep `/opt/dasobjectstore/users.json` as a browser-session token registry and
  auto-create a session record after successful OS password authentication.
- Package a named `/etc/pam.d/dasobjectstore` PAM service and add Debian/RPM
  package regression coverage for the PAM asset and runtime dependencies.

## 0.10.6 - 2026-07-08

- Redirect Trunk build logs to stderr during Web UI asset preparation so Debian
  and RPM package scripts capture only the generated web dist path on stdout.

## 0.10.5 - 2026-07-08

- Fix package asset validation for required strings that begin with hyphens,
  ensuring the strict Web UI packaging checks run correctly on Debian/RPM
  builders.

## 0.10.4 - 2026-07-08

- Make packaged Web UI asset preparation fail loudly when Trunk or the
  `wasm32-unknown-unknown` target is missing instead of silently installing the
  developer placeholder page.
- Force `make web`, `make deb`, and `make rpm` to build and validate real
  WebAssembly, JavaScript, and HTML assets for the operator interface.
- Keep the placeholder Web page behind an explicit
  `prepare-web-dist.sh --allow-fallback` developer escape hatch only.

## 0.10.3 - 2026-07-08

- Move performance-test SSD file settlement onto a bounded background
  `sync_all()` worker so the next SSD staging write can begin after the byte
  stream completes instead of waiting for per-file filesystem durability.
- Keep final-media HDD benchmark writes on the durable `sync_all()` path so
  reported HDD landing rates still include settlement to disk.
- Add a regression test proving SSD staging does not call `sync_all()` on the
  foreground upload path while the background settler still flushes the staged
  file.

## 0.10.2 - 2026-07-08

- Show per-active-file HDD landing rates in the performance-test TUI so each
  concurrent transfer reports its own throughput alongside copied bytes.
- Grow the HDD Landing pane to fit the active transfer set used by typical DAS
  concurrency tests and summarize only when more active transfers remain.
- Remove completed staged-drain transfers from the active HDD landing map and
  keep their byte counters updated while copies progress.

## 0.10.1 - 2026-07-08

- Capture SHA-256 checksums inline during normal source-to-SSD ingest instead
  of re-reading the staged SSD payload in the flush worker.
- Validate HDD settlement and direct-to-HDD copy checksums from the active copy
  stream, avoiding an immediate full destination readback after writing.
- Keep explicit destination rehash support available for audit and repair
  workflows where a second read is intentionally requested.

## 0.10.0 - 2026-07-08

- Add an authenticated standalone DASObjectStore Web UI landing page modeled on
  the Mnematikon connection surface, with local login, stored session recovery,
  session-token verification, and logout.
- Extend `/opt/dasobjectstore/config.json` with an explicit
  `authentication.authority` setting. Packaged standalone appliances default to
  `local_user`; `synoptikon` and `monas` remain integrated host-authority modes.
- Wire `dasobjectstore-server` through the auth-aware GUI router so standalone
  deployments expose `/api/login`, `/api/session`, and `/api/logout` under the
  product mount when local authentication is configured.

## 0.9.4 - 2026-07-08

- Improve the performance-test TUI by separating workload, active HDD landing,
  rates, scenario details, and artifacts into distinct panels.
- Show active HDD landing file/copy details, including target disk, landed
  bytes, total file size, and relative path.
- Report per-disk HDD write rates only for disks with active writes; completed
  historical per-disk averages remain in the PDF/JSON report rather than the
  live TUI active-rate row.

## 0.9.3 - 2026-07-08

- Make SSD-backed performance-test scenarios capacity-aware. ``ssd-only`` now
  writes a measured SSD-resident batch sequentially, then reads that batch back
  sequentially before continuing; ``ssd-stage-then-drain`` stages and drains
  measured resident batches; and ``ssd-overlap-drain`` applies SSD-residency
  backpressure while HDD workers remove staged files as copies complete.
- Update performance-test TUI/report wording so SSD residency bounds reflect
  measured available SSD capacity instead of assuming the complete selected
  dataset fits on SSD.

## 0.9.2 - 2026-07-08

- Split daemon file ingest into a bounded SSD pipeline by default: source
  writes land staged payload bytes on SSD, a bounded side worker syncs and
  calculates the staged checksum, and only synced/checksummed files are queued
  for HDD settlement. This keeps ingress of the next file from blocking on the
  previous file's SSD sync or SHA-256 calculation.
- Add explicit ``ssd-flush`` and checksum-capture progress telemetry so upload
  TUI sessions show why a staged file is not yet eligible for HDD migration.

## 0.9.1 - 2026-07-08

- Remove wasteful per-file SSD readback from ``ssd-stage-then-drain`` and
  ``ssd-overlap-drain`` performance-test staging. SSD read throughput for
  SSD-to-HDD routes is now derived from actual drain copy work instead of a
  synthetic read immediately after each SSD write.

## 0.9.0 - 2026-07-08

- Add packaged standalone Web UI/API service startup with
  ``dasobjectstore-server`` enabled by default through systemd.
- Install ``/opt/dasobjectstore/config.json`` with appliance defaults,
  including ``0.0.0.0:8448`` for the standalone HTTPS listener and TLS asset
  paths under ``/opt/dasobjectstore/tls``.
- Add top-level ``dasobjectstore status`` to report daemon, Web UI, and
  S3-compatible object-service endpoints, including configured ports and
  listener activity.
- Add ``make web`` and route Debian/RPM package builds through packaged Web UI
  asset preparation so the standalone interface is installed with the service.

## 0.8.1 - 2026-07-08

- Keep performance-test TUI SSD write and read rates populated with live
  phase-average values during active file writes and SSD readback instead of
  leaving fields as pending until a scenario completes.
- Show live HDD drain progress while staged and overlapping SSD/HDD benchmark
  routes are settling data, including drained, draining, queued, and pending
  copy-job counts plus interim aggregate HDD write rate.

## 0.8.0 - 2026-07-08

- Add ``dasobjectstore performance-test --redundancy <1|2|3>`` so benchmark
  runs can model one, two, or three physical HDD copies per logical source
  file while keeping the HDD write-worker pool bounded by the requested
  concurrency.
- Teach performance-test HDD scenarios to land redundant copies on distinct
  disks when enough managed HDD members are present, record physical HDD write
  volume and operation counts, and expose bounded FIFO queue capacity in the
  recommendation JSON.
- Add tidy ``plot_data`` rows to performance-test JSON artifacts for
  Grammateus/floundeR bar charts covering strategy landing rate, elapsed time,
  HDD write volume, HDD write operations, and per-disk HDD write rate.

## 0.7.1 - 2026-07-08

- Split performance-test SSD/HDD benchmarking into explicit
  ``ssd-stage-then-drain`` and ``ssd-overlap-drain`` routes so separated
  staging/drain behavior can be compared against real-world overlapping SSD
  ingest and HDD settlement.
- Add overlap evidence to performance-test report and JSON rows, recording
  whether HDD drainage started before all selected files finished SSD staging.
- Expand the performance-test TUI workload panel with separate SSD write,
  SSD read, aggregate HDD write, and per-disk HDD write rate fields.

## 0.7.0 - 2026-07-08

- Add ``dasobjectstore performance-test --cap <SIZE>`` for source-folder
  workloads so large extant datasets can be benchmarked as a deterministic FIFO
  prefix without staging the entire tree.
- Expand the performance-test TUI with scenario objective and SSD residency
  bounds so operators can distinguish SSD-only, SSD-first FIFO drain, and
  direct-to-HDD scenarios during long runs.
- Record source cap and discovered source totals in performance-test
  reproduction metadata and recommendation JSON artifacts.

## 0.6.0 - 2026-07-08

- Add ``dasobjectstore performance-test --authoritative`` to persist the
  benchmark recommendation under the daemon state directory for use after the
  next daemon restart.
- Teach daemon file ingest to load the authoritative performance policy and use
  the benchmark-selected HDD settlement worker count while keeping remote and
  external-disk ingress SSD-first.

## 0.5.0 - 2026-07-08

- Add remote-only packaging targets, ``make remote-deb`` and
  ``make remote-rpm``, for distributing the standalone
  ``dasobjectstore-remote`` upload client without installing the appliance
  daemon or managed storage service assets.

## 0.4.6 - 2026-07-08

- Render performance-test TUI snapshots through ratatui's in-memory backend and
  write the composed screen directly, avoiding crossterm cursor-position probes
  in wrapped or scripted terminal sessions.

## 0.4.5 - 2026-07-08

- Replace the performance-test TUI terminal-size fallback with direct Unix
  window-size detection so wrapped terminal sessions do not trigger
  cursor-position probe failures.

## 0.4.4 - 2026-07-08

- Make the performance-test TUI viewport robust when terminal size probing is
  unavailable, while still using the available full terminal dimensions in
  normal interactive shells.

## 0.4.3 - 2026-07-08

- Make the `performance-test --tui` dashboard use the full terminal viewport
  instead of a fixed-size frame.
- Add live performance-test TUI updates during long SSD write/readback and
  SSD-pipeline staging operations so the active phase, file, bytes, and current
  operation rate are visible while work is running.

## 0.4.2 - 2026-07-08

- Make `performance-test --report` PDF-only, requiring a `.pdf` path and using
  temporary Markdown only as an internal renderer source that is removed after
  PDF generation.
- Update performance-test examples, TUI labels, and recommendation JSON
  artifacts so the PDF is the only human report artifact.

## 0.4.1 - 2026-07-08

- Keep `performance-test --tui` rendering isolated from benchmark progress log
  lines so ratatui frames are not corrupted by per-file or shell-style output.
- Make performance-test generated writes, source copies, and readback checks
  Ctrl-C aware at chunk granularity so cancellation stops large-file work
  promptly and lets temporary benchmark roots be cleaned.
- Close benchmark worker queues and join HDD worker threads before returning
  cancellation errors, avoiding detached workers after Ctrl-C.

## 0.4.0 - 2026-07-08

- Add the standalone `dasobjectstore-remote` CLI for remote computers to list
  accessible S3-backed object stores and upload files or folders through the
  DASObjectStore object-service endpoint.
- Support remote client configuration, AWS profile based credentials, and
  Mneion, Synoptikon, or local-password credential-helper flows with password
  capture that does not echo to the terminal.
- Package `dasobjectstore-remote` in DEB/RPM artifacts and document remote
  client setup, object-store listing, uploads, and the credential-helper
  contract.

## 0.3.8 - 2026-07-08

- Allow `dasobjectstore performance-test` to benchmark extant source folders via
  `--source`, preserving recursive relative paths and FIFO source order while
  testing SSD-only, SSD-first HDD drain, and direct-to-HDD landing strategies.
- Record workload provenance, optional source path, and total source bytes in
  the performance recommendation JSON so future ingress planning can distinguish
  generated benchmarks from real dataset benchmarks.
- Ensure the temporary benchmark objectstore cleanup guard is active for
  completion, command errors, and Ctrl-C cancellation after the active file
  operation returns.

## 0.3.7 - 2026-07-08

- Add operator-facing `performance-test` CLI surface for embedded TUI rendering
  and optional JSON benchmark artifacts, and document administrative runs up to
  `--max-hdd-concurrency 5`.
- Rework `performance-test` as an administrative, scenario-based benchmark
  covering SSD-only landing, SSD-first FIFO HDD drainage, and direct-to-HDD
  landing without writing the same logical file to every disk for a primary
  measurement.
- Emit `dasobjectstore.performance_test.recommendation.v1` JSON so future
  ingress planning can consume the recommended strategy, HDD concurrency, and
  per-disk assigned byte/rate measurements.

## 0.3.6 - 2026-07-08

- Prefer `grammateus_markdown_pdf` for `dasobjectstore performance-test` PDF
  artifacts when it is available, using the standardized
  `dasobjectstore-performance` report template with mandatory Mnemosyne
  metadata, QR provenance payload, and signature fields.
- Retain `pandoc` and the built-in fallback PDF renderer so benchmark execution
  still completes on hosts without an external report renderer.

## 0.3.5 - 2026-07-08

- Add `dasobjectstore performance-test` for generated-file SSD write/read and
  concurrent HDD settlement benchmarking with Markdown, QR, and PDF report
  artifacts.
- Add report contracts and templates for Mnemosyne Biosciences branded,
  reproducible ingest performance evidence.

## 0.3.4 - 2026-07-07

- Pipeline folder ingest so source-to-SSD staging can run concurrently with
  FIFO HDD settlement, bounded by a small staged-object queue to protect SSD
  capacity.
- Surface concurrent SSD/HDD worker activity, HDD queue depth, and SSD pressure
  in upload progress telemetry and the embedded `--tui` view.
- Pause source ingress under high or critical SSD pressure while queued HDD
  settlement drains.

## 0.3.3 - 2026-07-07

- Distinguish logical source data size from replicated SSD/HDD work in ingest
  progress events and the embedded upload TUI, so `Data` matches the source
  dataset while `Work` reports the full storage pipeline IO.

## 0.3.2 - 2026-07-07

- Keep upload rate information visible in `dasobjectstore ingest files --tui`
  by rendering current and average transfer rate on the visible transfer row.

## 0.3.1 - 2026-07-07

- Make `dasobjectstore store contents` tolerate older or empty live SQLite
  metadata files without contents tables by rendering an empty contents snapshot
  instead of failing with a missing-table error.

## 0.3.0 - 2026-07-07

- Add `dasobjectstore store contents` to inspect logical object-store contents
  from live metadata with du-style aggregate sizes, tree output, JSON output,
  depth limits, and regex filtering.

## 0.2.0 - 2026-07-07

- Add a top-level Makefile with standard build, test, check, clean, DEB, RPM,
  and combined package targets for Ubuntu and AlmaLinux development.
- Add native RPM package generation through `packaging/rpm/build-rpm.sh`,
  reusing the shared Linux daemon service, sysusers, tmpfiles, and packaged
  runtime configuration assets.
- Skip hidden files and hidden directories during folder ingest so transient
  dotfiles in source trees do not abort daemon-backed uploads.

## 0.1.12 - 2026-07-07

- Keep `dasobjectstore ingest queue` read-only for normal users; older live
  SQLite files without `ingest_jobs` now render an empty queue instead of
  attempting schema repair against service-owned metadata.

## 0.1.11 - 2026-07-07

- Keep `dasobjectstored` running when an upload client disconnects during
  streaming progress or final response delivery; broken client pipes are now
  handled as per-client disconnects rather than daemon-fatal errors.
- Render Ctrl-C upload interruption once as a clean cancellation in the
  embedded upload TUI.
- Remove temporary SSD ingest job roots after verified HDD settlement as well
  as after cancelled or failed object puts, preventing settled uploads from
  filling the SSD staging area.
- Apply additive live SQLite schema repair before draining ingest queues so
  older live metadata files gain the `ingest_jobs` table on mutating paths.

## 0.1.10 - 2026-07-07

- Render upload TUI byte counters with binary size units such as MiB, GiB, and
  TiB instead of raw byte integers.
- Show current and average upload speed in the embedded upload TUI using binary
  rate units such as MiB/s.

## 0.1.9 - 2026-07-07

- Detect QNAP TL-D800C enclosures from Linux udev parent hub topology when the
  per-disk block devices only expose the individual drive and ASMedia bridge.
- Group TL-D800C members by the upstream QNAP USB hub path so downstream hub
  branches such as `5.3.*` and `5.4.*` are treated as one physical enclosure
  while other host USB ports remain separate.
- Require production `store create` to map managed HDDs to a supported,
  identifiable DAS enclosure. Initially supported: QNAP TL-D800C.

## 0.1.8 - 2026-07-07

- Detect QNAP TL-D800C USB DAS enclosures on Linux through udev metadata and
  group physically associated disks by their shared USB enclosure path.
- Show enclosure vendor, product, and bridge hints in pretty probe output.

## 0.1.7 - 2026-07-07

- Repair managed SSD/HDD root ownership and modes during package install or
  upgrade so existing object directories remain writable by `dasobjectstored`.
- Prepare Linux source ACLs before daemon-backed file ingest so the service can
  traverse private user home directories and read the selected import tree.

## 0.1.6 - 2026-07-07

- Remove the public `--live-sqlite-path` requirement from `store drain`; the
  store name now resolves live metadata from the managed SSD root.
- Scope `ingest queue` by store name and add pretty output by default, with
  JSON still available through `--json`.
- Add `ingest drain-queue` to cancel active queued ingest jobs for a store with
  administrative confirmation while preserving queue rows for auditability.

## 0.1.5 - 2026-07-07

- Fix Linux package builds for the upload Ctrl-C handler by initializing
  `sigaction` portably across libc targets.

## 0.1.4 - 2026-07-07

- Replace the embedded upload TUI renderer with Ratatui/Crossterm to reduce
  screen jitter during long-running ingest operations.
- Add upload heartbeat rendering so elapsed time continues to advance while the
  daemon is between progress frames.
- Propagate Ctrl-C from `dasobjectstore ingest files --tui` to the daemon as a
  cancellation signal and remove active partial SSD ingest job roots and partial
  HDD destination files.
- Extend daemon ingest progress events with per-stage byte counters and clearer
  SSD-settling/HDD-migration status for the upload TUI.

## 0.1.3 - 2026-07-07

- Ensure package upgrades grant the `dasobjectstore` daemon group read access to
  JSON configuration and registry files under `/etc/dasobjectstore`, so
  daemon-owned file ingest can read store definitions.

## 0.1.2 - 2026-07-07

- Wire `dasobjectstored` to handle `SubmitIngestFiles` requests by resolving
  store/SubObject endpoints, discovering managed HDD roots, staging source files
  through the SSD, and writing verified HDD copies.
- Add `dasobjectstore ingest files --tui` as an embedded upload view driven by
  daemon progress events while file ingest runs.
- Remove the standalone `dasobjectstore-tui` command surface and packaging path;
  terminal graphics are embedded niceties for long-running CLI actions.
- Relax the packaged systemd service to `ProtectHome=read-only`, allowing the
  daemon to read user-provided source paths when Linux filesystem permissions
  allow it.
- Surface daemon API error responses directly in the client, so unwired daemon
  commands report their daemon error code and message instead of a generic
  unexpected-response error.
- Corrected `TODO.md` entries that incorrectly marked production daemon ingest
  dispatch and peer-credential authorization as complete.

## 0.1.1 - 2026-07-07

- Established the first maintained semantic version for the Rust workspace,
  CLI, daemon, GUI/API, Mnemosyne adapter, object-service, platform, metadata,
  and TUI crates.
- Updated packaged/product metadata to advertise `0.1.1` instead of the
  pre-release placeholder `0.0.0`.
- Documented the release discipline in `AGENTS.md` and the versioning policy.
