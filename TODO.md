# DASObjectStore TODO

Status: Draft  
Source roadmap: [ROADMAP.md](ROADMAP.md)  
Purpose: discrete implementation tasks suitable for CODEX agents or senior
developers

Current status: Milestones 1-18 record the delivered appliance foundation, not
market completion. Milestones 19-24 and the architecture-remediation backlog
contain active debt. The new market/integration campaign below is the primary
dependency order; historical milestone checklists remain as implementation
evidence and detailed source tasks.

## Working Rules

- Keep changes surgical and tied to one task or closely related task group.
- Prefer small modules and tests with each implementation task.
- Update this file when tasks are completed, split, or superseded.
- Keep persistent metadata, CLI behavior, and compatibility-impacting changes
  documented before merging implementation.

## Current External Blockers (2026-07-12)

- DASServer/Garage hardware, appliance credentials, and deployment access are
  unavailable while travelling. This blocks appliance/provider soak, package
  install/reboot/remount/device replacement/full-disk acceptance, packaged
  telemetry-loop verification, and real-world multi-HDD/S3 performance runs.
  Continue with macOS unit/contract tests and the dedicated generated-data root
  instead; do not retry SSH or appliance commands until access is restored.
- The public paired-session HTTPS completion authentication contract is not
  approved. Daemon-side completion authorization is tested, but no public
  bearer/renewal-token semantics should be exposed without the security
  decision.
- Folder one-root adoption semantics and native-versus-provider-backed S3
  gateway choice remain product decisions listed above; implementation should
  not infer them from a path or profile name.
- The local Playwright screenshot runner now reaches the fixture server but
  timed out waiting for the authenticated ``.dos-topbar`` in this environment;
  Rust/Yew, wasm, JavaScript syntax, and source-contract checks pass. Treat
  desktop/mobile artifact execution as environment-gated until the local Web
  asset/bootstrap runtime is available; do not claim visual acceptance from
  this run.

## Market and Mnemosyne Integration Campaign

Work one campaign gate at a time. A checked campaign item must meet the
repository Definition of Done; a design or accepted request alone is not
completion.

### Decisions required

- [ ] Approve the public HTTPS authentication contract for a paired remote
  client to declare provider upload completion. A renewal token is currently
  available and daemon-side write-grant authorization is tested, but treating
  it as a bearer credential is a product/security decision.
- [ ] Decide whether one ``folder`` root maps one-to-one to one logical
  ObjectStore and define whether direct user edits are forbidden, detected as
  drift, or eligible only through explicit reconcile/adopt.
- [ ] Decide native embedded versus provider-backed S3 gateway support for
  folder and drive profiles; catalogue/daemon authority is mandatory either way.
- [ ] Approve logical quota accounting at full object-version size independent
  of physical deduplication, with physical amplification reported separately.

### Gate 0: Re-baseline and close release-critical appliance debt

- [x] Reconcile every unchecked item in historical Milestones 12 and 19-24
  into this campaign as implemented, locally actionable, externally blocked, or
  superseded; remove stale claims that the product is complete through M18.
  Historical checklists now cross-reference the active campaign gates; hardware
  acceptance, public upload authentication, and future design-language work
  remain explicitly open rather than being presented as delivered.
- [~] Finish daemon-owned remote upload completion so provider success is not
  reported before SSD-first ingest, checksum, placement, and catalogue commit.
  Resolve and document the public paired-session completion authentication
  contract before exposing the endpoint. Internal admission, transfer,
  cancellation, and progress modules are delivered; catalogue completion and
  public paired-session authorization remain open.
- [~] Finish resumable/cancellable reconciliation with per-key manifests,
  collision/malformed-key reporting, provider progress, and restart recovery.
  Local manifest/checkpoint planning and cancellation are delivered; stable
  provider rediscovery, byte-range restart, and appliance acceptance remain.
  - [x] Add a versioned provider-independent per-key manifest/resume planner
    with safe key normalization, collision/malformed-key outcomes, atomic
    durable in-progress checkpoints and restart-planner tests; stable manifest
    rediscovery, true byte-range restart, and appliance/provider acceptance
    remain.
  - [x] Integrate manifest checkpoints into the Garage transfer path, replace
    aggregate `aws s3 sync` with safe per-key downloads, and expose per-key
    progress through the daemon job stream; interrupted clients leave durable
    in-progress checkpoints while stable manifest rediscovery and true
    byte-range restart remain open.
  - [x] Add explicit administrator cancellation tokens for an active
    reconciliation job; cancellation is checked between provider transfers and
    leaves the durable in-progress manifest available for later rediscovery.
  - [x] Add a provider-independent completion-commit gate to the daemon remote
    upload worker; a successful provider transfer is not reported complete when
    the injected manifest/catalogue handoff fails. Concrete catalogue wiring
    and public paired-session authorization remain open.
  - [ ] Complete appliance/provider soak acceptance (blocked while the
    DASServer, Garage appliance, and deployment credentials are unavailable).
- [~] Reserve bounded daemon/control-plane capacity and make HTTPS liveness,
  login, static assets, cancellation, and degraded cached status responsive
  during blocked or saturated ingest. Daemon lanes, bounded GUI bridges, strict
  cancellation, configured runtime resource-policy injection, and the daemon/
  CLI file-ingest emergency control, authenticated Web action bridging, and a
  compact one-shot TUI action snapshot are delivered; async HTTP bridging and
  appliance soak acceptance remain.
  - [x] Load the versioned ingest resource policy from daemon configuration,
    inject its CPU/memory/socket/I/O budget into packaged local ingest, and
    retain a backward-compatible safe default when the field is absent. The
    same gate is threaded through Garage reconciliation; explicit HTTP
    bridging and host-level control-plane telemetry remain open.
- [ ] Close telemetry device mapping, warm-up/missing-reason, package-loop, and
  appliance acceptance gaps without fabricating continuity.
- [x] Remove temporary production module-size exceptions through owned,
  test-preserving splits; keep dispatcher and public façades narrow. The
  exception file is empty and the module-size guard passes. The previously
  unexcepted
  daemon ``server/request_handler/storage.rs`` violation has been split into
  storage reconciliation/helpers modules; the guard now passes against the
  reviewed baseline. The request-handler façade is now also below budget;
  no production exceptions remain.
  - [x] Keep package-asset regression tests aligned with the authoritative
    Debian dependency contract, including `udisks2` and `awscli`, so the local
    workspace test baseline does not mask packaging drift.
  - [x] Keep the checked-in product manifest version synchronized with the
    workspace package version so Mnemosyne contract tests detect real drift.
  - [x] Remove stale daemon and GUI ``home_aggregator.rs`` entries after the
    module-size guard confirmed those modules are below budget; no GUI
    exceptions remain.
  - [x] Split GUI API product workspace view models and bootstrap projections
    into `crates/dasobjectstore-gui-api/src/workspaces_product.rs`; preserve
    public JSON types and workspace-route contracts.
  - [x] Split shared Web API request/response contracts into
    `crates/dasobjectstore-gui-web/src/api_contracts.rs`; preserve wasm/test
    decoding and client-facing JSON shapes, then remove the final Web API
    module-size exception.
- [x] Implement the daemon-owned enclosure preparation executor with typed
  validation, command-runner injection, ext4/xfs planning, and atomic fsync'd
  role markers; keep destructive execution behind the existing confirmation
  and existing-data acknowledgement gates.
- [x] Route `disk prepare-das` through the daemon executor and remove normal
  appliance preparation writes from the CLI; preserve dry-run reports,
  confirmation behavior, and machine-readable daemon response fields.
- [ ] Complete the Mnemosyne design-language/Web workflow tasks in Milestone 24
  after storage contracts stabilize.

### Gate 1: Profile, backend, manifest, and compatibility contracts

- [x] Add compatibility-sensitive domain types for ``folder``, ``drive``, and
  ``appliance`` deployment profiles; keep host mode orthogonal as per-user,
  system, or integrated authority. The additive `DeploymentProfile` and
  `HostMode` enums use stable snake-case wire names and are not yet persisted
  into existing store metadata.
- [ ] Decide and document profile creation/adoption semantics, including whether
  one folder root maps exactly to one ObjectStore and how existing data drifts.
- [ ] Define a capability-based backend contract for validation, reservation,
  staging, durable finalization, reads, enumeration, verification, health,
  reconciliation, and removal.
  - [x] Add the shared `ObjectStoreBackend` trait, typed object/health/error
    records, and explicit capability flags for every operation; concrete folder,
    drive, and appliance implementations remain subsequent gates.
- [ ] Version portable manifest and placement contracts so logical identity,
  hierarchy, versions, hashes, provenance, protection, and backend locations do
  not encode mandatory appliance SSD/HDD assumptions.
  - [x] Add a versioned `ObjectStoreManifest` with explicit folder root,
    drive filesystem/device identity, or appliance pool backend references;
    legacy metadata remains untouched and profile/backend mismatches are
    rejected. Add the separate strict ``portable_object_catalogue.v1``
    companion contract for logical object versions, digests, provenance,
    lifecycle/protection state, and profile-neutral placements; private
    catalogue authority and daemon transaction wiring remain open.
  - [x] Define the minimal profile-neutral `ObjectCatalogueAuthority` batch
    contract and adapt the durable folder catalogue to it; shared SQLite,
    appliance, and daemon transaction wiring remain open.
  - [x] Prove authority batches are all-or-none across conflicts and restart;
    a conflicting existing version cannot partially add a new record.
  - [x] Adapt the SSD-backed drive profile to the same authority contract and
    fail closed on drive-guard loss for catalogue reads and mutations.
  - [x] Centralize checked logical used-byte accounting through the authority
    contract; FolderBackend now uses the shared overflow-safe helper.
  - [x] Route folder adoption catalogue commits through the shared authority
    seam; richer daemon transaction and SQLite/object-service integration stay
    open.
- [ ] Define protection policies independently from profiles: local-only,
  reproducible, externally replicated, appliance protected, and future
  multi-site protection.
  - [x] Add the profile-independent `ProtectionPolicy` vocabulary and carry it
    in the versioned portable ObjectStore manifest; physical profile selection
    no longer implies protection semantics.
- [ ] Document compatibility and migration rules before changing persistent
  metadata, public APIs, CLI behavior, or existing appliance pools.
  - [x] Document version-1 manifest compatibility, fail-closed future-schema
    handling, untouched legacy metadata, explicit adoption/migration writes,
    and identity-over-path rules; strict decode rejects unknown fields and
    future schemas before interpretation. Migration implementation remains
    gated until a new persistent field is approved.
- [ ] Put existing appliance placement behind the backend contract with
  regression evidence showing unchanged ingress, repair, and export behavior.

### Gate 2: Universal capacity and transactional admission

- [ ] Extend every ObjectStore policy with logical capacity limit, mandatory
  backend reserve, warning threshold, and critical threshold; require a finite
  limit for ``folder``.
  - [x] Add serde-compatible `CapacityPolicy` fields and strict validation to
    the shared `StorePolicy` model while retaining unbounded legacy defaults
    until profile creation supplies a finite limit.
- [ ] Add a transactional quota ledger and capacity reservations so concurrent,
  streaming, versioned, and multipart uploads cannot overbook the same bytes.
  - [x] Add a core transactional `CapacityReservationLedger` with reserve,
    commit, release, overflow, duplicate-ID, and backend-reserve enforcement
    tests; daemon/S3/multipart admission wiring remains open.
- [ ] Admit against the strictest of logical quota, outstanding reservations,
  backend usable space after reserve, SSD staging, and copy amplification.
  - [x] Add a pure core admission evaluator that reports logical/backend/SSD
    availability and rejects the strictest failed constraint with copy
    amplification; SSD staging is now an explicit daemon-derived input so
    policy-permitted direct ingress bypasses only SSD free-space checks.
    Daemon/S3 call-site integration remains open.
- [ ] Charge each logical object version at full logical size even when physical
  content is deduplicated; report physical staging/replication separately.
  - [x] Add a typed core logical-object-version charge and ledger reservation
    entry point that always accounts the full version size independently of
    content dedupe, copy count, or staging; daemon/catalogue call-site wiring
    and physical telemetry remain open.
- [ ] Define over-quota behavior: preserve reads, verified deletion, repair, and
  cleanup; reject new ingress; never delete data when a quota is lowered.
  - [x] Add derived pressure states and atomic quota-policy updates to the core
    ledger. Lowering a limit preserves usage and existing reservations, marks
    the ledger ``over_quota``, and rejects only new reservations; deletion
    accounting and daemon wiring remain follow-up work.
  - [x] Add checked used-byte debits for verified folder deletion, with
    underflow protection and capacity recovery tests; repair and catalogue
    transaction wiring remain separate.
  - [x] Require folder staging bytes to match the reservation exactly before
    commit, preventing logical-used/accounted-size drift during adoption.
- [ ] Add optional SubObject budgets whose reservations atomically update both
  child and parent allocations.
  - [x] Add a core hierarchical SubObject capacity ledger with atomic
    parent/child reservation, commit, and release behavior; daemon registry
    persistence and transport wiring remain open.
- [ ] Expose used, reserved, available, backend free, amplification, thresholds,
  and admission-block reason through daemon API, CLI, TUI, Web, and adapters.
  - [x] Add a read-only daemon ``capacity_status`` transport response backed by
    the registry policy, persisted ledger, and daemon-owned statvfs probes;
    authorized readers receive pressure and explicit block reasons without
    mutating reservations. The CLI exposes it as ``store capacity`` (including
    ``--json``); TUI rendering and the authenticated Web route are delivered,
    while profile-specific adapters remain follow-up work.
  - [x] Render the daemon-owned capacity snapshot in the embedded TUI with
    logical used/reserved/available bytes, backend and SSD availability, copy
    amplification, thresholds, and admission-block reasons; the authenticated
    Web adapter now uses the bounded daemon bridge, while external adapter
    contracts remain pending.
  - [x] Add an optional Web ``capacity_status`` detail with explicit
    unavailable fallback and old-payload compatibility; live values are
    obtained through the authenticated bounded daemon bridge when available.
  - [x] Add the authenticated dashboard store-capacity route through the
    shared bounded daemon bridge, preserving typed busy/circuit/deadline
    responses; add the typed Web client getter/path helper; appliance-backed
    acceptance remains.
  - [x] Add transport-neutral daemon capacity admission request/decision DTOs
    with stable snake_case reasons, observed-capacity fields, and direct-ingress
    SSD fields optional; SSD-first/direct behavior is derived from the typed
    ingress origin rather than caller-supplied booleans. Transport routes and
    live store-state wiring remain.
  - [x] Add a daemon-owned ledger evaluation helper that derives logical usage
    and outstanding reservations from the live reservation ledger while taking
    backend/SSD free-space observations from daemon probes; caller-supplied
    usage cannot override the ledger.
  - [x] Add an atomic daemon evaluate-and-reserve helper keyed by the validated
    client request ID; rejected requests leave the ledger unchanged while
    admitted logical versions create a transactional reservation. Transport
    route integration remains open.
  - [x] Add an authenticated, read-only daemon capacity-admission transport
    route with typed client plumbing, stable API errors, and fail-closed
    orchestration when live ledger/probe state is unavailable; live registry,
    filesystem probes, and ingest/S3/multipart reservation wiring remain open.
  - [x] Add the daemon-owned file-backed capacity provider: it reads current
    store policy from the registry, restores bounded ledgers, probes backend
    and SSD free space, atomically persists admitted reservations, and fails
    closed for missing bounded ledgers or probe/persistence errors; configured
    copy counts remain daemon-authoritative. S3/multipart completion and
    stale-reservation scheduling remain open.
  - [x] Add explicit provider commit/release lifecycle operations with
    snapshot rollback when durable persistence fails; transfer workers still
    need to carry reservation IDs through their completion/failure paths.
  - [x] Wire the daemon-owned remote S3 transfer worker to retain the job ID as
    the reservation ID, admit before invoking the byte-transfer adapter, and
    commit or release the reservation after transfer and catalogue completion;
    admission rejection is persisted as a failed daemon job and provider
    lifecycle failures remain fail-closed. Multipart and catalogue accounting
    remain separate follow-up work; explicit stale-reservation maintenance is
    available below.
  - [x] Route typed S3/multipart byte-transfer adapters through the same
    capacity admission lifecycle; regression coverage proves rejection occurs
    before adapter invocation and admitted transfers commit or release their
    reservation. Concrete multipart API/catalogue accounting remains open.
  - [x] Wire local file ingest through the same daemon provider boundary: each
    non-skipped file reserves its size with the verified ingress origin before
    source/staging or direct-HDD work, commits after durable metadata settlement,
    and releases outstanding reservations on failure. Dry runs and skipped
    existing objects do not reserve; multipart adapters still need to pass the
    provider through their own orchestration paths.
  - [x] Pass the daemon-owned capacity provider through Garage S3
    reconciliation into the local ingest worker, so staged provider downloads
    reserve with ``remote_s3`` origin and settle through the same commit/release
    lifecycle; appliance credentials and provider soak remain blocked.
  - [x] Fail closed before local writes when a capacity-enabled ingest request
    supplies a copy-count override that differs from the daemon ObjectStore
    policy; this preserves daemon-authoritative redundancy rather than charging
    a reservation for a client-selected copy count. The legacy no-provider
    executor tests retain their existing explicit override coverage.
  - [x] Scope local-ingest reservation IDs with a client request ID or stable
    source-path digest, preventing unrelated same-second jobs from colliding
    while preserving deterministic retries for the same source.
  - [x] Extend the daemon decision DTO with raw backend free space, policy
    thresholds, and copy-amplification basis points so adapters can render the
    observed block reason without recomputing physical policy.
- [~] Add concurrency, crash/restart, multipart expiry, quota-change, dedupe,
  and full-filesystem tests before enabling new profile writes.
  - [x] Add a concurrent reservation regression using the transactional core
    ledger; eight workers contend for a bounded quota and cannot overbook it.
  - [x] Add a schema-versioned, strict capacity-ledger snapshot/restore
    contract that preserves used bytes and outstanding reservations across a
    restart boundary; daemon file persistence is complete and legacy snapshots
    remain loadable. Explicit stale-reservation expiry is now covered below;
    lease policy and scheduling remain open.
  - [x] Add daemon atomic JSON persistence around that snapshot contract with
    file and directory ``fsync``/rename ordering plus corrupt-state rejection;
    live-store registry wiring remains open. Durable reservation timestamps,
    deterministic expiry, legacy-snapshot retention, and provider persistence
    rollback tests are now in place; no background scheduler is enabled until
    a lease/renewal policy is approved.
  - [x] Add durable reservation creation timestamps with schema-v2 emission,
    schema-v1 compatibility, deterministic boundary expiry, and a provider
    maintenance API that atomically persists reclaimed bytes. Unknown-age
    legacy reservations are retained; automatic expiry and renewal remain
    intentionally disabled pending an approved lease policy.
  - [x] Exercise the file-backed provider against a real macOS filesystem
    fixture, including statvfs backend/SSD observations, admission, and commit;
    appliance-scale full-disk acceptance remains blocked on DASServer access.
    Crash/restart persistence, multipart expiry, dedupe, and full-filesystem
    fixtures are otherwise covered above; multipart expiry/renewal remains
    lease-policy gated.

### Gate 3: Bounded folder profile

- [ ] Implement system-service and programmatic create/adopt for one explicitly
  bounded directory, including idempotent DEB/RPM provisioning hooks.
  - [x] Enforce a finite logical capacity limit when opening a folder backend;
    idempotent directory/namespace creation is covered locally, while package
    hooks and explicit adopt/reconcile semantics remain open.
- [ ] Finalize ingress on the same filesystem using private temporary files,
  in-flight checksum, file ``fsync``, atomic rename, directory ``fsync``, then
  transactional manifest/catalogue commit.
  - [x] Add the macOS-safe `FolderBackend` staging/finalization path with
    in-flight SHA-256, private same-filesystem temporary files, file and
    directory sync, atomic rename, capacity reservation commit, and focused
    read/verify/enumerate/remove tests; finalized adoption and verified removal
    now update the private catalogue transactionally. Removal persists the
    catalogue deletion before unlinking the payload, preserving payload and
    logical accounting when the catalogue write fails and restoring the record
    if unlinking fails.
  - [x] Run the folder backend regression against the dedicated generated-data
    root `/Users/stephen/.dasobjectstore-codex-validation` on macOS; the test
    removes only its uniquely named child after completion.
- [x] Preserve user-visible hierarchy while reserving and protecting the
  ``.dasobjectstore`` namespace. Folder objects remain under the private
  namespace with relative catalogue locations that preserve nested keys;
  read-only inspection excludes that namespace and never adopts user files.
- [ ] Reject symlink escape, hard-link ambiguity, devices, sockets, FIFOs,
  unsafe keys, unsupported names, and files changed during import.
  - [x] Harden `FolderBackend` namespace/parent traversal against symlink
    escapes and reject non-regular entries during enumeration; unsafe key and
    symlink regression tests pass on macOS.
  - [x] Classify hard-linked user files as unsafe and add a file-specific
    stable-source staging primitive that re-hashes the source after copying;
    macOS tests cover hard-link inspection and unchanged source adoption.
  - [x] Add fd/path identity checks during enumerate/verify/finalize and keep
    staged reservations recoverable after tampering; macOS tests cover stable
    checksums, path identity, hard-linked managed objects, and staged-object
    recovery.
  - [x] Add a deterministic post-read mutation fixture before enabling
    resumable adoption; generic stream staging still cannot promise
    source-file stability without a file-specific import path.
  - [x] Tighten private namespace/object/staging directories to Unix ``0700``
    and payload files to ``0600`` without changing the user-selected root;
    local permission regression coverage passes on macOS.
- [ ] Add read-only inspection followed by resumable adoption/reconciliation;
  report unmanaged drift without silently accepting it as authoritative.
  - [x] Add a read-only `FolderBackend::inspect_user_tree` report for unmanaged
    hierarchy and unsafe entries; no adoption or authority change occurs during
    inspection, and resumable reconciliation remains open.
  - [x] Convert the read-only inspection into a resumable reconciliation plan
    with manifest entries and normalized download actions; unsafe paths remain
    report-only and no user file is adopted or mutated.
  - [x] Make that report-only plan revision-aware and restart-safe: stable
    source identity revisions are required before Complete/InProgress entries
    can skip or resume; changed/replaced files reset to Download and wrong-store
    checkpoints fail closed. Adoption execution and catalogue authority remain
    open.
  - [x] Preserve provider resume safety when listings expose stable ETags by
    carrying the ETag as the source revision; providers without a revision
    remain fail-safe and replan rather than guessing continuity.
  - [x] Add an explicit folder adoption executor that copies unmanaged files
    through stable-source verification, private staging, checksum/finalization,
    and restart-safe manifest checkpoints. User files remain untouched and
    failed attempts are checkpointed without silently becoming authoritative;
    catalogue transaction handoff remains a separate shared metadata task.
  - [x] Add a versioned, private folder-profile catalogue snapshot with
    idempotent conflict-checked commits and file/directory fsync+rename; folder
    adoption commits the finalized record before its Complete checkpoint.
    Malformed/future-schema/wrong-store/conflict recovery tests are covered;
    verified removal debits logical usage and removes the catalogue record;
    shared SQLite/object-service catalogue integration remains open. Folder
    reopen now derives used-byte accounting from the durable catalogue and
    rejects conflicting supplied accounting before filesystem use.
  - [x] Add a profile-neutral, read-only browser projection over authoritative
    folder catalogue records with bounded prefix/search/page queries. Nested
    keys, sizes, checksums, and private locations are preserved; appliance-only
    object type, lifecycle, and placement fields remain explicitly unknown.
    Profile-aware registry selection and shared SQLite authority remain open.
- [ ] Implement profile-aware browse, download, verify, capacity, health,
  repair, lifecycle, and common S3 operations.
  - [x] Expose folder browse/read/verify, health, and typed capacity snapshots
    from `FolderBackend`; folder-profile catalogue records now reload from the
    private snapshot, while repair/lifecycle/S3 and shared catalogue
    integration remain open.
- [ ] Add per-user host mode with XDG state/runtime paths and a user service;
  do not require root for a user-owned folder and test coexistence with system
  mode.
  - [x] Add pure per-user/system state and runtime path derivation. Per-user
    state falls back beneath HOME, missing XDG runtime remains explicitly
    unavailable (never shared `/tmp`), and socket length/name validation is
    covered locally; service-manager creation and ownership checks remain.
- [ ] Validate package-created, programmatically created, adopted, container-
  mounted, restart/recovery, quota, and hostile-filesystem fixtures.

### Gate 4: Dedicated SSD drive profile

- [ ] Create/adopt only an explicit mount backed by a validated non-rotational
  device; identify it by stable filesystem/device identity rather than name.
  - [x] Require an explicit `DriveMediaKind::Ssd` plus stable filesystem/device
    identity in portable drive manifests; runtime mount/device probing and
    create/adopt orchestration remain open.
  - [x] Add injected platform validation for positively observed SSD media,
    matching filesystem/device identities, safe mounted paths, root-status,
    and writable mode; real diskutil/lsblk observation remains external.
- [ ] Reject the system root and already-claimed devices by default; support a
  documented administrator override for virtual or unusual SSD topology.
  - [x] Reject missing drive device identity and `/` system-root mount hints in
    the portable manifest validator; live mount/claim probing and override
    authorization remain open.
- [ ] Implement reserve, pressure, capacity, SMART/NVMe health, endurance,
  mount-loss, replacement, import/export, and read-only degraded behavior.
- [ ] Reuse folder hierarchy/manifest/S3 semantics while making the single-
  device failure domain explicit in policy, CLI, TUI, and Web.
  - [x] Add a drive backend wrapper that retains the drive manifest while
    delegating hardened folder hierarchy, checksum, durable-finalization, and
    bounded-capacity behavior; an injected runtime guard fails closed before
    filesystem I/O when mount/device state drifts.
  - [x] Expose drive capacity and read-only user-tree inspection through the
    guarded profile boundary, including a fail-closed ``guarded_capacity``
    accessor; SMART/NVMe telemetry and daemon inventory remain
    hardware-dependent.
  - [x] Expose the same bounded, profile-neutral catalogue browser projection
    through the drive guard. It never walks private payloads, keeps the single
    device failure-domain boundary explicit, and fails closed on guard drift;
    catalogue transaction handoff and Web exposure remain shared-profile work.
- [ ] Add Linux package, reboot/remount, device replacement, full-disk,
  corruption, and performance acceptance coverage.

### Gate 5: Unified S3, product APIs, and migrations

- [ ] Decide native embedded gateway versus provider-backed S3 per profile while
  preserving one public S3 contract.
- [ ] Route S3 PUT and multipart completion through quota reservation, daemon
  ingress, durable finalization, and catalogue commit; derive GET/HEAD/list from
  catalogue state rather than provider listings.
- [ ] Add profile/capability discovery and idempotent provisioning APIs so a
  Mnemosyne product requests storage policy without implementing filesystem or
  appliance logic.
  - [x] Add a versioned static profile-capability catalogue DTO with separate
    backend-operation, service, host-mode, protection, and requirement fields;
    runtime store readiness and provisioning routes remain separate. The
    catalogue states a static local failure-domain ceiling (folder/drive 1,
    appliance 3), not current redundancy or external replication.
  - [x] Wire static profile-capability discovery through the typed daemon
    request/response and client boundaries; runtime readiness and provisioning
    remain separate and appliance availability stays explicitly blocked.
  - [x] Expose the same static capability contract through the read-only CLI
    command ``store capabilities`` with human and JSON output; no provisioning
    or runtime health is inferred locally.
- [~] Provide product-owned policy templates and adapters for Synoptikon,
  Mneion, Mnemosyne, and small standalone/package-managed projects.
  - [x] Add a shared `StoragePolicyTemplate` contract carrying explicit
    product ownership, profile, host mode, protection, bounded capacity,
    local-copy count, and typed ingress origin. Validation is fail-closed for
    unsafe slugs, unbounded new templates, invalid capacity, and copy counts
    beyond the profile's local failure-domain ceiling; product defaults,
    provisioning, and concrete adapters remain open.
  - [x] Add a generic Mnemosyne adapter envelope that validates adapter-owned
    template identity and emits a strict versioned shape without product
    defaults, paths, credentials, or provisioning behavior; concrete product
    defaults and provisioning adapters remain open.
- [ ] Implement folder-to-drive, folder/drive-to-appliance, and portable export/
  import jobs preserving IDs, versions, hashes, provenance, and protection.
  - [x] Add a core resumable promotion state machine that retains source
    placement through destination verification and explicit retirement; actual
    copy workers and profile adapters remain. Add atomic schema-versioned
    checkpoint save/load with strict source-retention invariants.
  - [x] Add a daemon folder-to-folder migration worker that verifies the
    source, reserves destination capacity, streams through the bounded folder
    backend, verifies the finalized destination, and leaves source retirement
    pending; retries retain failed staged data and reservations safely.
  - [x] Generalize the migration worker to a guarded dedicated-SSD drive
    destination, preserving the same source-retention and checksum guarantees;
    appliance adapters and catalogue transaction wiring remain open.
  - [x] Commit migration destination records through the same authority before
    marking the destination verified; folder reopen tests prove catalogue and
    logical usage (`used_bytes`) survive restart, while provider/appliance
    adapters remain. Guarded drive migration/reopen tests now prove the same
    authority handoff for the dedicated SSD profile.
- [~] Retain source placements until destination verification and explicit
  retirement confirmation; make interrupted promotion resumable.
  - [x] The core migration state machine and atomic checkpoints retain source
    placement until explicit retirement; daemon copy-worker, catalogue, and
    profile-adapter execution remain.

### Gate 6: Integration and market-readiness acceptance

- [x] Publish a profile-by-host-mode support matrix and upgrade/migration policy.
  - [x] Publish the current profile/host-mode matrix and fail-closed upgrade
    policy in `docs/user/storage-profile-matrix.rst`, distinguishing local
    preview contracts from DASServer/Garage-blocked acceptance.
- [ ] Run package install/upgrade/uninstall, authentication, quota, S3,
  migration, recovery, security, observability, and performance matrices for
  folder, drive, and appliance.
- [ ] Validate generated-data stress tests plus representative Mnemosyne product
  workflows; never use customer/project data in automated acceptance.
- [ ] Require real-world validation readiness, operator runbooks, release notes,
  and no unexplained critical TODO blockers before declaring a profile ready.

## Architecture Remediation Backlog

Status: partially completed in commits `aa4d3463` and `7c56b146`. Keep this
list until every temporary size-budget exception has been removed.

- [x] Split the Web workspace root into page, view-model, shared-component, and
  test modules; retain a small routing façade and shared state-message renderer.
- [x] Split daemon request dispatch into service/admin, storage/telemetry, and
  EasyConnect request-family handlers.
- [x] Split request-handler orchestration, job projection, and shared request
  helpers into focused sibling modules so the public handler façade remains
  within the production module budget; preserve all typed responses and error
  contracts.
- [x] Split daemon storage request reconciliation and registry/path helpers
  into focused sibling modules so the storage dispatcher remains within the
  production module budget; preserve reconciliation job and authorization
  behavior.
- [x] Centralize UTC parsing/formatting in `dasobjectstore-core` and remove the
  duplicated calendar implementations from daemon, GUI API, and remote client.
- [x] Split daemon ingest runtime endpoint discovery, managed-device
  environment, and HDD scheduling from the execution façade.
- [x] Extract daemon ingest pipeline work records and live progress/rate state
  into `runtime/ingest_files/pipeline_state.rs`; preserve SSD-first routing,
  direct-HDD policy, and telemetry tests while continuing the remaining
  execution-engine split.
- [x] Extract bounded SSD-flush/HDD-settlement workers and admission helpers
  into `runtime/ingest_files/pipeline_workers.rs`; preserve queue backpressure,
  cancellation, worker error messages, and SSD-pressure behavior.
- [x] Extract ingest settlement event draining and progress projection into
  `runtime/ingest_files/pipeline_events.rs`; preserve coalesced telemetry,
  metadata commit ordering, and finalization progress states.
- [x] Split GUI dashboard object-service discovery and telemetry projection
  from the home-dashboard assembly façade.
- [x] Add CI enforcement for a 1,000 production-line module budget, with a
  reviewed temporary baseline exception list.
- [x] Replace temporary size-budget exceptions by splitting the CLI runner and
  CLI argument contracts into command-family modules; keep dispatcher-only
  roots and move tests beside their owning modules.
  - [x] Extract the ingest files/directive parser and its conflict-policy
    contract into `crates/dasobjectstore-cli/src/cli/ingest.rs`; keep the root
    dispatcher and existing daemon request behavior unchanged.
  - [x] Extract the remaining ingest queue/status/direct-import contracts into
    the same command-family module without changing parser behavior.
  - [x] Move the ingest parser tests beside the new module; the root CLI test
    module now retains only top-level dispatch coverage.
  - [x] Extract the SubObject argument contracts and parser regression into
    `crates/dasobjectstore-cli/src/cli/subobject.rs`.
  - [x] Extract the Object argument contracts and parser regressions into
    `crates/dasobjectstore-cli/src/cli/object.rs`.
  - [x] Extract the Service argument contracts and parser regressions into
    `crates/dasobjectstore-cli/src/cli/service.rs`, preserving Docker Compose
    defaults and help text.
  - [x] Extract the Store dispatcher, ingest-policy, and contents contracts
    with their parser regressions into `crates/dasobjectstore-cli/src/cli/store.rs`.
  - [x] Document `store objects` and `store list-contents` aliases for the
    read-only ObjectStore contents listing command.
  - [x] Move the remaining Store create/adopt/list/drain/delete/defaults,
    S3-upload, and policy-file contracts into `cli/store.rs` while preserving
    destructive confirmation and hidden registry attributes.
  - [x] Move the remaining Store parser tests beside the new module before
    removing the CLI size exception.
  - [x] Extract the Disk argument contracts and parser regressions into
    `crates/dasobjectstore-cli/src/cli/disk.rs`, preserving destructive
    confirmation and preparation defaults.
  - [x] Extract the Pool argument contracts and parser regressions into
    `crates/dasobjectstore-cli/src/cli/pool.rs`, preserving debug-command
    feature gates and import/repair accessors.
  - [x] Extract the small Object, Service, Mnemosyne, Pool-marker, and probe
    runtime handlers into `crates/dasobjectstore-cli/src/run/command_handlers.rs`;
    preserve the dispatcher and platform cfg behavior.
  - [x] Extract connection-status models, probe projection, preferred-path
    selection, and operator recommendations into
    `crates/dasobjectstore-cli/src/run/connection_status.rs`; preserve
    Thunderbolt preference, topology context, and USB fallback guidance.
  - [x] Extract portable registry mirroring, known-root validation, and
    writer-group registry/ACL access into
    `crates/dasobjectstore-cli/src/run/registry_access.rs`; preserve
    non-Linux no-op behavior and fail-closed Linux group checks.
  - [x] Remove the CLI `run.rs` size-budget exception after extracting its
    local-direct ingest fallback; the CLI runner now passes the 1,000-line
    production-module guard.
  - [x] Extract ingest queue inspection, rendering, and daemon-owned drain
    handling into `crates/dasobjectstore-cli/src/run/ingest_queue.rs`; preserve
    dry-run risk gates, JSON/text output, and daemon mutation ownership.
  - [x] Extract shared live-SQLite path resolution into
    `crates/dasobjectstore-cli/src/run/metadata_paths.rs`; preserve explicit
    override behavior and unknown-store diagnostics for queue and contents
    readers.
  - [x] Move Object export/put disk-root mapping validation beside its runtime
    handlers in `crates/dasobjectstore-cli/src/run/command_handlers.rs`; add
    regressions for malformed IDs, empty paths, and order preservation.
  - [x] Extract the performance report, JSON artifact, chart, and PDF rendering
    helpers into `crates/dasobjectstore-cli/src/run/performance_report.rs`;
    preserve report output and existing regressions.
  - [x] Extract the runtime status endpoint inspection, Docker published-port
    parsing, and status rendering into
    `crates/dasobjectstore-cli/src/run/runtime_status.rs`; preserve the CLI
    status output and keep published-bind parser regressions beside the module.
  - [x] Extract service provisioning, Compose lifecycle, and service-status
    handlers into `crates/dasobjectstore-cli/src/run/service.rs`; keep the
    top-level runner limited to dispatch and shared error handling.
  - [x] Extract pool import/repair/inspection and managed-disk lifecycle
    handlers into `crates/dasobjectstore-cli/src/run/storage_lifecycle.rs`;
    preserve risk gates and read-only pool semantics.
  - [x] Extract read-only `store contents` and `store validate` handlers into
    `crates/dasobjectstore-cli/src/run/store_read.rs`, keeping the runner focused
    on dispatch and shared error handling.
  - [x] Extract read-only `store list` and `store defaults` handlers into the
    same module, keeping registry inspection and policy rendering out of the
    top-level runner.
  - [x] Extract the read-only `store s3-upload` plan renderer into the same
    module, keeping service-layout lookup and AWS command planning out of the
    top-level runner.
  - [x] Extract SubObject create/list/search runtime handlers and report
    helpers into `crates/dasobjectstore-cli/src/run/subobject.rs`.
  - [x] Extract Store create/adopt runtime handlers into
    `crates/dasobjectstore-cli/src/run/store_write.rs`, retaining shared
    validation and registry helpers behind narrow calls.
  - [x] Extract Store ingest-policy read/update runtime handling into the
    same module, preserving typed daemon requests and inventory rendering.
  - [x] Extract health output-mode dispatch into
    `crates/dasobjectstore-cli/src/run/health.rs`, leaving platform health
    projection helpers shared and testable.
  - [x] Extract read-only platform probe dispatch into
    `crates/dasobjectstore-cli/src/run/probe.rs`, keeping probe acquisition and
    presentation behind a small command handler.
  - [x] Extract performance-report artifact rebuild dispatch into the existing
    performance report module, keeping PDF/markdown lifecycle handling out of
    the top-level runner.
  - [x] Extract SSD residency budgeting, bounded batch planning, and admission
    boundary tests into `crates/dasobjectstore-cli/src/run/performance_residency.rs`.
  - [x] Extract live SSD-read/HDD-write rate accounting and its focused
    regression tests into `crates/dasobjectstore-cli/src/run/performance_rates.rs`;
    preserve shared counters across worker clones and sync-time accounting.
  - [x] Extract bounded asynchronous SSD settlement and queue-drain completion
    into `crates/dasobjectstore-cli/src/run/performance_settle.rs`; add coverage
    that `finish` drains every accepted settlement job.
  - [x] Extract the performance execution engine (scenario runners, disk
    scheduler, copy primitives, measurement helpers, orchestration, and
    lifecycle) into focused modules; keep the CLI runner exception open only
    for the remaining platform and ingest command families.
    - [x] Extract performance copy/read primitives, sync-policy dispatch, and
      progress measurement into `crates/dasobjectstore-cli/src/run/performance_io.rs`;
      preserve staged settlement and final-sync accounting tests.
    - [x] Extract the disk-placement scheduler and bounded queue-capacity
      policy into `crates/dasobjectstore-cli/src/run/performance_scheduler.rs`;
      preserve distinct-disk redundancy and single-writer scheduling tests.
    - [x] Extract shared performance scenario job, pending-queue, and active
      HDD-write state into `crates/dasobjectstore-cli/src/run/performance_execution.rs`;
      preserve FIFO backpressure and live TUI/report state coverage.
    - [x] Extract the SSD-only performance scenario into
      `crates/dasobjectstore-cli/src/run/performance_ssd_only.rs`; preserve
      bounded SSD residency batching, write-before-readback ordering, and TUI
      log suppression regressions.
    - [x] Extract the SSD stage-then-drain performance scenario into
      `crates/dasobjectstore-cli/src/run/performance_ssd_stage_then_drain.rs`;
      preserve bounded HDD fan-out, source-read accounting, and batch ordering.
    - [x] Extract the overlapping SSD pipeline scenario into
      `crates/dasobjectstore-cli/src/run/performance_ssd_pipeline.rs`; preserve
      SSD residency admission, FIFO HDD drain, overlap, and distinct-disk
      redundancy regressions.
    - [x] Extract the direct-HDD performance scenario into
      `crates/dasobjectstore-cli/src/run/performance_direct_hdd.rs`; preserve
      bounded placement, split read/write accounting, and live TUI coverage.
    - [x] Extract scenario-matrix execution orchestration into
      `crates/dasobjectstore-cli/src/run/performance_scenarios.rs`; preserve
      file-order sequencing, scenario result ordering, completion TUI frames,
      and report-path context.
    - [x] Extract daemon-backed ingest request submission, request builders,
      TUI streaming, and completion rendering into
      `crates/dasobjectstore-cli/src/run/ingest_client.rs`; preserve normal
      SSD-first and direct-import request contracts.
    - [x] Extract packaged-daemon source canonicalization and Linux ACL planning
      into `crates/dasobjectstore-cli/src/run/ingest_source_access.rs`; preserve
      fail-closed permission handling and the non-Linux no-op.
    - [x] Extract managed-DAS root discovery, marker validation, supported
      enclosure checks, and SSD/HDD root policy into
      `crates/dasobjectstore-cli/src/run/managed_roots.rs`; preserve QNAP
      guard fixtures and environment overrides.
    - [x] Move platform health collection, disk scoring, and Linux/macOS health
      adapters into `crates/dasobjectstore-cli/src/run/health.rs`; preserve
      output contracts and health projection behavior.
    - [x] Move performance report artifact persistence and QR/PDF/metadata
      helpers into `crates/dasobjectstore-cli/src/run/performance_report.rs`;
      preserve JSON validation, chart generation, authoritative policy output,
      and report rebuild contracts.
    - [x] Extract performance-test lifecycle setup, provenance, scenario
      execution, and report assembly into `crates/dasobjectstore-cli/src/run/performance_run.rs`;
      preserve temporary-root cleanup and authoritative artifact behavior.
  - [x] Move Store contents tree/du rendering and aggregation helpers beside
    the `store_read` handlers, keeping output contracts and tests unchanged.
  - [x] Extract daemon storage authorization, telemetry access, and browser
    delegation helpers into `storage_authorization.rs`, keeping storage
    mutation handlers below the production module budget.
  - [x] Split performance workload planning and performance TUI rendering into
    dedicated modules; the production size guard now passes without adding a
    new exception.
  - [x] Move Store drain/delete CLI presentation and daemon request adapters into
    `run/store_write.rs`, leaving the root runner focused on dispatch.
- [ ] Complete the daemon ownership boundary: add daemon request contracts and
  runtime operations for store drain/delete, ingest queue drain, object put,
  disk retirement, and other managed mutations still performed by the CLI.
  Do not redirect a CLI command to an acceptance-only daemon request unless the
  daemon actually performs and reports the requested operation.
  - [x] Route `store drain` through a typed daemon request; keep authorization,
    managed-HDD discovery, metadata mutation, and the full drain report inside
    the daemon while preserving CLI JSON/text output.
  - [x] Make non-dry-run WebUI object-store creation persist the validated
    registry definition before recording the administrator job complete; add a
    regression proving acceptance follows registry persistence.
  - [x] Route ingest queue drain through a typed daemon request so daemon-owned
    metadata paths, authorization, timestamps, and cancellation reporting are
    used instead of CLI-side SQLite mutation.
  - [x] Route normal disk retirement through a typed daemon request so the
    daemon owns the live metadata path, timestamp, authorization, and state
    transition report.
  - [x] Route force disk retirement through a typed daemon request with
    daemon-side administrator authorization, policy allowance, confirmation,
    timestamp, and risk-gated state transition.
  - [x] Route `store delete` through a typed daemon request; keep metadata,
    managed-HDD payload cleanup, host/portable registries, authorization,
    policy allowance, and action-time confirmation inside the daemon.
  - [x] Route `object put` through a typed daemon request so staged SSD/HDD
    placement and metadata mutation execute behind the authenticated daemon
    boundary rather than in the CLI process.
  - [x] Route normal CLI `store create` requests with a writer group through
    the typed daemon creation contract when the packaged daemon socket is
    available; keep explicit registry-path, no-writer-group, portable mirror,
    and `store adopt` behavior as separate migration/test fallbacks until an
    unassigned-writer policy is approved.
- [x] Split remote-upload runtime into admission, transfer/progress, and
  cancellation-cleanup modules; keep shared concurrency/backpressure policy
  single-sourced with normal ingest.
  - [x] Extract cancellation cleanup planning, safe managed-path removal,
    multipart aborts, and cleanup reports into
    `crates/dasobjectstore-daemon/src/runtime/remote_upload/cleanup.rs`.
  - [x] Extract remote-upload admission gates, queue-depth snapshots, transfer
    permits, and backpressure decisions into
    `crates/dasobjectstore-daemon/src/runtime/remote_upload/admission.rs`.
  - [x] Extract transfer progress reporting, telemetry enrichment, short-window
    S3 rate calculation, and progress messages into
    `crates/dasobjectstore-daemon/src/runtime/remote_upload/progress.rs`.
  - [x] Extract the admission-gated transfer worker, typed byte-transfer
    adapter, daemon job lifecycle events, and failure cleanup orchestration
    into `crates/dasobjectstore-daemon/src/runtime/remote_upload/transfer.rs`.
- [x] Split GUI API authentication routes into router/auth, contracts, daemon
  clients, local-group administration, enclosure administration, and reporting
  modules; consolidate repeated confirmation and client-error adapters.
  The production route façade is now composed from dedicated router, contracts,
  daemon-client, identity, validation, parsing, reporting, local-group, and
  enclosure modules; the large test module remains colocated for fixture
  locality and the module-size guard passes.
  - [x] Extract standalone route composition into
    `crates/dasobjectstore-gui-api/src/auth_router.rs`, leaving handlers and
    validation logic behind a narrow routing façade.
  - [x] Extract GUI daemon-client submission adapters into
    `crates/dasobjectstore-gui-api/src/auth_clients.rs`, consolidating
    unavailable-daemon and bad-gateway error mapping.
  - [x] Extract daemon response projections and stable admin/job labels into
    `crates/dasobjectstore-gui-api/src/auth_reporting.rs`.
  - [x] Extract bucket-name normalization and endpoint/enclosure enum parsing
    into `crates/dasobjectstore-gui-api/src/auth_parsing.rs`.
  - [x] Extract the shared authentication/admin request and response DTOs into
    `crates/dasobjectstore-gui-api/src/auth_contracts.rs` without changing JSON
    shapes or route behavior.
  - [x] Extract local-user authority, local-group, and enclosure daemon client
    adapters into `crates/dasobjectstore-gui-api/src/auth_admin_clients.rs`;
    preserve request/error projections and macOS-safe compilation.
  - [x] Extract standalone authentication, session, remote-authentication, and
    EasyConnect route handlers into
    `crates/dasobjectstore-gui-api/src/auth_identity_routes.rs`; preserve
    router visibility, response contracts, and local-password error mapping.
  - [x] Extract administrator request validation, managed-mount rejection,
    client-request-ID checks, and action-specific confirmation markers into
    `crates/dasobjectstore-gui-api/src/auth_validation.rs`; preserve dry-run
    safety gates and field-specific HTTP errors.
- [x] Move object-service Docker status/bind parsing into one shared
  inspection module used by both CLI and GUI API, with one bounded timeout
  policy and parser regressions beside the shared implementation.
- [x] Add password-authenticated `dasobjectstore-remote authenticate HOST
  OBJECTSTORE` over verified HTTPS, with daemon-owned eight-hour scoped Garage
  sessions, redacted default output, explicit JSON connection context, and
  persisted-credential validation.
- [x] Align remote-upload admission with the registry's canonical `s3_bucket`
  export label so writer-group users can authenticate to exported stores.
- [ ] Split global Web CSS by base primitives and feature-owned styles, and
  split screenshot regression runner, fixture server, assertions, and
  per-workspace fixtures into dedicated modules.
  - [x] Move Object Browser controls, hierarchy/table, placement badges,
    download states, and responsive breakpoints into
    `styles/object-browser.css`; register it before shared styles and preserve
    CSS contract coverage. Screenshot runner artifact execution remains open
    behind the documented local Playwright bootstrap blocker.
  - [x] Move Activity grids, queue/task cards, typography, and responsive
    rules into `styles/activity.css`; keep shared card/form primitives in the
    base sheet and preserve feature ownership/order tests.
  - [x] Move enclosure wizard selectors into `styles/enclosures.css` while
    retaining shared form/review primitives in the base sheet.
  - [x] Move remote-upload layout and selection styles into the dedicated
    `styles/remote-upload.css` feature sheet, register it with Trunk, and keep
    CSS contract tests loading the shared and feature-owned sheets together.
  - [x] Move screenshot viewport, role, and page matrices into the shared
    `tools/web-screenshot-fixtures.mjs` module so runner orchestration does not
    own fixture definitions.
  - [x] Move Home telemetry chart and source/gap treatments into
    `styles/home.css`, keeping the global sheet focused on shared primitives and
    loading the feature sheet through Trunk and CSS contract tests.
  - [x] Move Activity report dropzone/progress styles into
    `styles/reporting.css` and add feature-ownership/order contract coverage.
  - [x] Correct the screenshot runner’s footer assertion to accept the approved
    report-style sans-serif footer and reject only an accidental monospace
    treatment; full Playwright artifact execution remains environment-gated.
  - [x] Move authentication shell/form selectors into feature-owned
    ``styles/auth.css`` while retaining shared card/session primitives in the
    base sheet; registration and source-contract coverage preserve load order.
  - [x] Move enclosure inventory, drive detail, and responsive layout rules
    into ``styles/enclosures.css``; mixed/shared primitives remain in the base
    sheet and source tests reject enclosure selector leakage.

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
- [x] Commit each finalized inline-hashed object placement to daemon-owned live
  metadata before reporting ingest completion; fail closed when the store or
  disk catalogue is unavailable.
- [x] Add `dasobjectstore store repair` with read-only inspection by default and
  explicit daemon-authorized metadata rebuild, timestamped backup, atomic
  replacement, size-selected partial-duplicate reporting, and tests.
- [x] Add daemon-owned `dasobjectstore store verify` health checks with optional
  payload hashing, missing/orphan detection, mismatch reporting, and tests.
- [x] Add guarded checksum-based `dasobjectstore store deduplicate`; only
  duplicate metadata rows are removable after confirmation and payload files
  are preserved.
- [x] Support `store contents STORE/PREFIX` folder/file targets with explicit
  directory/file rendering and scoped path output.

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
- [x] Validate Garage 2.3 provisioning syntax on the deployment appliance and
  use its supported short ``-n`` key-name option so registry provisioning can
  complete without a daemon transport failure.
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

## Milestone 12: Managed Daemon and Client Boundary

### Ingress correctness and live operator telemetry follow-up

- [x] Remove strict-conflict pre-copy source hashing from every ingress route. Checksums are calculated while bytes are copied; content-addressed destinations are deduplicated after that in-flight hash is available, so direct NVMe-to-HDD ingest never performs an unconditional prior read.
- [x] Preserve ingress routing invariants: local NVMe/server ingress may use direct-to-HDD only when the store policy permits it; USB-mounted source disks, Web uploads, Remote S3, and other remote ingress stage through the DAS SSD.
- [x] Replace serial redundant-copy settlement with bounded fan-out: direct-ingest reads each source stream once, calculates source and target checksums in flight, and concurrently lands bounded copies on distinct HDDs before per-target `fsync` and atomic placement.
- [x] Make HDD worker admission and disk placement permit concurrent writes to three or four distinct HDDs when capacity and policy allow; worker admission is copy-aware and defaults to up to four distinct HDD target sets, while redundant-copy jobs remain bounded by complete distinct disk sets.
- [x] Add daemon-provided short-window source-read, SSD-write, and aggregate HDD-write rates to ingest telemetry; stale and fsync/rename-finalizing rates report zero while existing per-target rates remain authoritative.
- [x] Extend daemon ingest events with target-disk assignment before copying begins and bytes written. HDD placement now emits assigned target records at zero bytes before writes; keep the phase-rate fields above and use short-window rates so stalled transfers are distinguishable from active ones.
- [x] Complete queue/worker event coverage for scan, source-read, SSD-stage, HDD-write, verification, and finalization so every progress frame reports each queue and worker lane rather than only inferred source/HDD depths. The daemon now populates each lane and the TUI names all queue/worker lanes explicitly.
- [x] Model direct-copy durability finalization explicitly: direct HDD targets now emit separate `fsync` and atomic-rename states with durations and zero current byte-write rate while finalizing. Direct metadata commits remain daemon-owned follow-up work.
- [x] Replace lifetime-average per-HDD rates with short-window sampled rates and retain both current and completed-copy summaries. A full-size transfer waiting in `fsync` must display zero current write rate plus its finalization state, not an apparently active 54 MiB/s average.
- [x] Decouple daemon progress/socket reporting from the I/O hot path with byte/time coalescing that preserves phase and target-assignment transitions; the embedded TUI retains the latest snapshot and redraws byte-only updates at a bounded cadence.
- [x] Update the embedded TUI to render all active HDD targets, copy numbers, source and destination throughput, and pipeline queue depths. It now renders daemon source/SSD/aggregate-HDD phase rates explicitly; remaining checksum-conflict wording is tracked in the operator-route work.
- [x] Add TUI regression coverage for multiple active disk assignments and non-zero per-disk rates.
- [x] Add deterministic fan-out overlap coverage proving one source reader feeds at least two concurrent physical-disk writers while preserving per-target outputs.
- [x] Add executor route-plan regression coverage proving Remote/Web/USB external origins select SSD-first under a direct-capable store policy.
- [x] Add full-pipeline coverage proving external-origin SSD-first behavior through staged settlement under a direct-capable policy.
- [x] Add deterministic performance coverage for no pre-copy hash on normal direct ingress and bounded high-frequency progress delivery; existing fan-out and staged external fixtures cover byte-path correctness.
- [x] Keep the guarded legacy `direct-to-HDD` import on the same inline-hash
  copy path: an expected digest is now an optional post-copy check, so callers
  without trusted source metadata do not trigger a strict pre-copy read.
- [ ] Add appliance sustained external-origin throughput and direct-ingest
  no-precopy soak acceptance. Broader performance/soak acceptance remains
  tracked under Web availability. Deferred while travelling without DAS host
  access; when resumed, verify the appliance is quiescent and do not overlap
  the generated-data ``codex`` acceptance with production or performance work.

### Local source classification and direct-HDD operator intent

- [x] Replace the normal `ingest files` client's hard-coded `UsbMountedDisk` origin with daemon-owned source classification. The submitted path is only a hint: the daemon verifies mount and device topology, distinguishing local server NVMe/SATA paths from USB/removable, NFS/SMB/FUSE, and other remote sources. Unknown or unverifiable sources remain SSD-first.
- [x] Add the daemon-owned, auditable `ingest_mode` status/update API. It patches only the existing store definition, validates the complete resulting policy, supports dry-run responses, and requires the exact action-time confirmation `confirm direct hdd ingest` for `DirectToHdd`; operators do not edit registry files.
- [x] Expose the policy API through an authenticated daemon-backed `dasobjectstore store ingest-policy` command. Unix peer credentials are checked by the daemon; only root, `sudo`, or `dasobjectstore-admin` peers may update policy, and the daemon records the resolved actor instead of trusting a client-supplied identity.
- [x] Replace the transitional Web configure planner with an authenticated Web action that forwards the logged-in local administrator identity through the daemon boundary. The ObjectStore dashboard now exposes the current landing mode and applies only the daemon-owned ingest-mode patch; registry files remain hidden.
- [x] Reconcile the Web service peer's daemon group membership in packaged deployments. Debian/RPM post-install now create `dasobjectstore-admin` and add the shared Web/daemon service user; the daemon accepts this peer only when the authenticated Web route supplies a non-blank resolved actor, while ordinary CLI peers still require root/`sudo`/`dasobjectstore-admin` membership.
- [x] Make normal local-folder ingest eligible for direct-to-HDD only through the explicit `ingest direct-import` route. Ordinary `ingest files` now submits a `LocalServerSsdFirst` hint, while direct-import submits `LocalServerDirectImport`; the daemon still verifies topology, store policy, and external/removable/network origins fail closed to SSD-first.
- [x] Add an ingest preflight/plan event and CLI/TUI rendering before source content bytes are read. The event reports the source path and verified-local versus external/unverified topology class, classified origin, store `ingest_mode`, selected landing mode, and the exact routing reason; authoritative mount/device identifiers remain a follow-up enrichment.
- [x] Add focused route-selection tests covering a server-local `/home` path direct to HDD when policy and operator intent allow it; `SsdFirst` policy overriding that path; and USB/removable, NFS/SMB/FUSE, and unknown paths remaining SSD-first. Include a regression proving ordinary `ingest files` no longer serializes every path as USB.
- [x] Enrich the preflight event with authoritative mount point, filesystem, backing-device, and major:minor identifiers from daemon source classification, including explicit unknown/unavailable values when topology verification cannot resolve them.
- [x] Make Debian/RPM external-source access package-managed: enable a
  root-owned traversal watcher for standard udisks mount roots and generate a
  udev mount policy that gives FAT/exFAT/NTFS volumes read-only
  `dasobjectstore` group access at mount time. Unsupported filesystems remain
  explicit CLI diagnostics rather than receiving unsafe broad permissions.
- [ ] Add an appliance performance acceptance run using a server-local NVMe source and a policy-approved direct-HDD store. Verify that no SSD ingest stage is entered, a bounded one-read fan-out uses distinct HDDs, and the preflight/TUI route explanation matches the daemon decision. Acceptance is gated on a quiescent appliance: do not overlap production ingest, repair/drain, or another performance run. The repeatable sequence is a small `performance-test --scenario direct-hdd --hdd-concurrency 1,2,3,4` run for per-disk/SSD-stage evidence, followed by a `ingest direct-import --dry-run --tui` against the same server-local source to capture the daemon preflight route; only then run the bounded non-dry-run fixture and archive its JSON/TUI evidence.
- [x] Document the supported operator workflow for inspecting a store's ingest policy, requesting a policy-allowed direct local ingest, and interpreting an SSD-first fallback. Keep external/removable-source staging and data-loss safeguards explicit in `docs/user/ingesting-files.rst`.

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

### Web availability under heavy ingest and I/O pressure

- [x] Split the daemon Unix-socket control plane from long-running ingest execution. The listener continues accepting health, inventory, status, and cancellation requests while an ingest streams progress by using bounded, separately admitted ingest and control execution lanes rather than holding the sole request handler for the transfer lifetime.
- [x] Reserve a separate bounded priority-control lane for administrator
  cancellation requests so cancellation capacity cannot be consumed by routine
  queries or new ingest submissions; saturated lanes return the typed
  `server_busy` response. Async HTTP bridging, deadlines, circuit breaking,
  and appliance soak acceptance remain follow-up work below.
- [x] Add the first bounded async GUI-to-daemon bridge for the read-only
  ObjectStore browser listing: cap blocking workers, return typed `429`/`503`
  overload/deadline responses, and retain a bridge permit until a timed-out
  synchronous socket call actually returns.
- [x] Give control, query, and cancellation requests reserved daemon capacity
  and priority over new ingest work. The Unix listener has independently bounded
  routine-control, priority-cancellation, and ingest lanes (8/2/2); saturated
  lanes return the typed `server_busy` response. Async HTTP bridging,
  deadlines, circuit breaking, and appliance soak acceptance remain separate
  follow-up items.
- [~] Replace synchronous daemon Unix-socket calls in Axum handlers with an
  async bridge and bounded blocking pool. The identified browser, activity,
  remote-auth, administrator, and archive paths now have deadlines, bounded
  permits, typed overload/degraded responses, and a circuit breaker with a
  single half-open probe. GUI bridge transports now opt into bounded idle
  deadlines (reset by progress frames), so stalled socket workers return and
  release bridge capacity; default CLI/long-ingest transports remain
  unbounded. Async HTTP bridging and appliance soak acceptance remain open.
- [x] Route ObjectStore file and folder download authorization/location lookups
  through the shared bounded daemon bridge; preserve typed `429`/`503`
  overload/deadline responses and release control capacity before payload
  streaming. Folder archive data-plane admission remains a separate follow-up.
- [x] Bound folder archive `spawn_blocking` workers with a separate two-worker
  semaphore held until each tar stream exits; saturated archive requests return
  a typed `429` without consuming daemon control capacity.
- [x] Route the Activity workspace daemon job-list lookup through the shared
  bounded bridge and retain a 200/degraded workspace with an actionable warning
  when the daemon is busy or exceeds its deadline.
- [x] Route the standalone remote-authentication pairing/approval/exchange
  transaction through the shared bounded bridge so a stalled daemon cannot pin
  an Axum worker; preserve typed overload and deadline responses.
- [x] Route Web administrator job status and cancellation requests through the
  bounded bridge, preserving daemon errors and typed busy/deadline responses;
  cancellation now has a bounded Web-side worker path as well as the daemon's
  priority lane.
- [x] Route Web ObjectStore creation through the bounded bridge, preserving
  daemon-owned mutation and typed overload/deadline failures before reporting
  the accepted job to the browser.
- [x] Route Web endpoint-inventory upsert through the bounded bridge, preserving
  daemon-owned validation/mutation and typed overload/deadline failures.
- [x] Route Web ObjectStore ingest-policy updates through the bounded bridge,
  preserving daemon-owned policy validation, confirmation, and typed
  overload/deadline failures.
- [x] Route Web enclosure-preparation submissions through the bounded bridge,
  preserving daemon-owned destructive-operation confirmation and existing-data
  acknowledgement gates.
- [x] Route Web local-group creation and membership assignment through the
  bounded bridge, keeping the daemon as mutation authority and updating the
  local group registry only after accepted non-dry-run responses.
- [x] Keep Web administrator cancellation on a dedicated bounded priority
  bridge/circuit so routine query or mutation degradation cannot suppress the
  emergency cancellation path.
- [x] Harden the shared bridge circuit state with generation/epoch-gated
  closed/open/half-open transitions, single-probe recovery, stale-completion
  protection, and transport-versus-domain error tests; object-browser socket
  transport failures now contribute to degraded-circuit state without treating
  daemon validation errors as outages.
- [x] Bound performance-report PDF rebuilds to a separate two-worker
  `spawn_blocking` semaphore held until rendering completes; saturated requests
  return a typed `429` without running the renderer.
- [ ] Keep HTTPS liveness, static Web assets, login/session renewal, and a minimal cached appliance-status page independent of daemon round trips. Expose daemon-dependent pages as `degraded` with the last successful snapshot and retry guidance rather than making the whole WebUI uncontactable.
  - [x] Add the public `/api/v1/liveness` contract as a daemon-independent
    readiness probe with stable service/version/instance metadata; the
    authenticated cached-status route is covered below, while daemon-owned
    freshness and appliance acceptance remain open.
  - [x] Preserve the last successful Home dashboard snapshot across a failed
    refresh and render a retryable stale-data warning instead of replacing
    operator telemetry with a blank error state; cold-start failures remain
    explicit transport errors until a first snapshot exists.
  - [x] Add a route-level regression proving daemon-independent liveness stays
    HTTP 200 while the daemon-backed Activity round trip is degraded; the
    Activity response remains a typed 200 workspace with warnings and retryable
    state rather than blocking the health surface.
  - [x] Bound standalone static-asset reads behind a four-permit async lane and
    add explicit no-cache index/unfingerprinted and immutable fingerprinted
    asset cache headers; route regressions cover both cache policies. Daemon
    telemetry freshness and appliance acceptance remain open.
  - [x] Add authenticated ``/api/v1/dashboard/status`` with a bounded in-process
    last-successful snapshot, explicit ``stale``/retry metadata, and fail-closed
    cold-start behavior; appliance-backed soak and telemetry freshness remain.
  - [x] Add the typed Web client response contract, WASM getter, and path helper
    for cached dashboard status; existing Home-page loading remains unchanged.
- [ ] Add daemon-owned ingest admission and dynamic backpressure that reserves CPU, memory, socket workers, and I/O capacity for the Web/control plane. In sustained disk-pressure conditions, throttle or pause low-priority source reads and HDD settlement before control-plane latency is affected.
  - [x] Add a typed daemon admission decision that combines source-read error/
    pressure backpressure with adaptive worker scheduling and reports run,
    throttle, or block plus the limiting reason and schedule snapshot. Runtime
    resource reservations, live host telemetry, and call-site wiring remain.
  - [x] Add a transactional daemon resource gate for CPU, memory, socket-worker,
    and I/O-worker reservations with fail-closed over-budget admission and
    automatic lease release; runtime policy injection and live telemetry remain.
  - [x] Wire packaged local file ingest through the shared resource gate before
    source enumeration, preserving automatic release on dry-run, failure, and
    successful settlement; dynamic policy injection is complete for the
    packaged path.
- [ ] Package the Web server and storage daemon in distinct systemd resource domains with explicit CPU, memory, and I/O protection. The Web server must retain a protected service budget; ingest may be constrained per SSD/HDD device when PSI, queue latency, or control-plane latency crosses policy thresholds.
- [ ] Emit and retain live availability telemetry: HTTP accept queue/active requests and latency, daemon socket queue/active handlers, control-plane deadline/circuit-breaker counts, cgroup memory, per-device queue latency, and CPU/I/O PSI. Surface the current throttle/degraded reason in both the WebUI and TUI.
  - [x] Surface the optional daemon ingest admission action, limiting reason,
    source-read worker count, HDD queue depth, and verification parallelism in
    the TUI; Web bridge and live host telemetry remain.
- [x] Add the daemon-owned and CLI emergency file-ingest control contract for
  `pause`, `throttle`, and `resume`. It requires exact action-time
  confirmation, allows dry-run preview, gates both SSD-first and direct-HDD
  source reads between objects, and leaves in-flight checksum/fsync/rename
  work untouched. The control is process-local (restart returns to `running`)
  and provider-specific S3 workers retain their separate admission gate;
  authenticated Web action wiring now uses the reserved daemon bridge and
  typed Web client contract; the compact ``ingest control --tui`` snapshot is
  covered by parser and renderer tests, while interactive keyboard controls
  and live daemon state refresh remain open.
- [ ] Add deterministic regressions with a deliberately blocked ingest handler and a saturated I/O fixture: HTTPS liveness/static assets and login remain responsive, daemon-backed pages fail fast with typed degraded responses, cancellation remains accepted, and no HTTP accept queue grows unbounded.
  - [x] Add a deterministic local bridge-saturation regression proving the
    daemon-independent health and liveness routes remain HTTP 200 while a
    bounded daemon bridge worker is blocked; bridge capacity is retained until
    that worker releases. Full appliance I/O saturation, static/login soak,
    and accept queue measurements remain hardware/deployment acceptance work.
  - [x] Add deterministic local coverage that acquires all four static-asset
    read permits and proves requests fail fast with HTTP 503, then recover with
    the documented no-cache index response; the same fixture proves local
    login remains HTTP 200 while an unrelated daemon bridge worker is blocked.
    Full static/login soak, saturated-I/O fixtures, and accept-queue
    measurements remain hardware/deployment acceptance work.
  - [x] Add a deterministic authenticated Web cancellation regression proving
    the reserved priority bridge accepts cancellation while the routine admin
    bridge is saturated; full daemon/I/O and accept-queue measurements remain
    appliance acceptance work.
  - [x] Add a deterministic daemon-backed object-browser regression proving a
    saturated bridge returns the typed HTTP 429 response before invoking the
    client; appliance I/O saturation and accept-queue measurements remain
    acceptance work.
- [ ] Run an appliance soak acceptance test using direct NVMe source reads plus multi-HDD settlement at the configured maximum. Record p95/p99 Web health and dashboard latency, PSI, disk queue latency, and recovery after throttling; fail the release if the WebUI cannot serve its liveness endpoint within the control-plane SLO.
- [x] Document operator triage for an ingest-pressure incident, including SSD
  pressure, queue/verification indicators, safe daemon throttle/pause/resume
  actions, expected degraded WebUI behavior, and escalation evidence. The
  runbook explicitly preserves in-flight durability and does not prescribe
  killing ingest or restarting the daemon; appliance p95/PSI evidence remains
  part of the blocked soak acceptance.

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

### Critical viewer delivery follow-up

- [x] Recover the appliance empty live SQLite index without touching payload
  files: 1,047 objects and 1,063 placements were reconstructed into a degraded
  catalogue; the original zero-byte file is preserved as
  `live.sqlite.empty-20260710`. Hashes remain unverified by design.

- [x] Make an omitted browser prefix a tested root-tree request: after selecting
  an ObjectStore, render all immediate folders and files without requiring a
  name, path, or search value.
- [ ] Make daemon-owned upload completion atomically register each object path,
  size, checksum, lifecycle state, and readable managed/provider location in
  the ObjectBrowser catalogue before reporting the upload as complete. Blocked
  on a public API security decision: the remote client has a paired-session
  renewal token but no defined HTTPS completion endpoint or approved bearer
  authorization scheme for declaring a provider upload complete. The daemon
  now has tested paired-session completion authorization; resume once the
  public endpoint/authentication contract is approved.
- [~] Add a guarded, resumable reconciliation operation for already-uploaded
  S3/object-service keys missing from the ObjectBrowser catalogue; report
  collisions, malformed keys, and inaccessible objects without silently
  overwriting metadata. ``store repair STORE --reconcile-s3 --apply`` now
  performs a safe SSD-first Garage import and catalogue registration with
  per-key manifests, collision reporting, and durable checkpoints; remaining
  work is stable cross-job manifest rediscovery, byte-range continuation,
  non-Garage providers, and appliance acceptance.
- [~] Make `store repair --reconcile-s3` terminate naturally with a persisted
  terminal job state and final CLI response. Garage reconciliation now forwards
  normal coalesced SSD/HDD ingest events, preserves existing live metadata
  rather than attempting an unsafe filtered rebuild, records success/failure
  terminal Repair jobs, and marks interrupted nonterminal jobs failed on daemon
  restart. Garage now also reports per-key progress and checks administrator
  cancellation between provider transfers while preserving in-progress
  checkpoints. Remaining work is cancellation during a blocked provider copy,
  true byte-level/range resume, stable rediscovery after restart, and appliance
  acceptance rather than a new transfer after daemon restart.
- [ ] Extend daemon-authorized Web download to stream a verified
  provider-backed object when no settled managed-HDD payload is available,
  preserving existing public/read/write authorization and safe disposition
  headers.
- [ ] Show explicit browser diagnostics for a genuinely empty store versus
  uncatalogued backend objects, including catalogue count, backend count, last
  reconciliation time, and actionable failure details.
- [ ] Add end-to-end appliance acceptance coverage for upload, root-tree
  refresh, folder navigation, individual download, and content/checksum
  verification; cover both managed-HDD and provider-backed uploads.
- [ ] Document the operator recovery workflow for backend objects absent from
  the browser catalogue and the acceptance path proving an uploaded file can
  be browsed and downloaded.

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
  Blocked until Web direct-upload byte-transfer execution and concrete
  Synoptikon/Mneion submission clients exist.
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
- [x] Add a daemon runtime easyconnect AWS CLI upload-job executor that
  constructs remote-upload job metadata and the daemon AWS CLI/S3
  byte-transfer plan before running both through the admission-gated worker.
- [x] Add the daemon API DTOs, typed daemon client helper, and request-handler
  route that submit easyconnect AWS CLI upload jobs into the daemon runtime
  executor.
- [x] Wire the remote client/local-agent easyconnect upload command to call the
  daemon submit route so SSD staging, S3/object-service intake, HDD landing
  workers, and verification cannot grow without bounds end-to-end.
- [x] Add a daemon runtime cancellation cleanup plan for remote upload jobs,
  covering partial SSD-staged objects, failed S3 multipart uploads, abandoned
  sessions, expired pairings, and interrupted browser handoffs.
- [x] Add a daemon runtime cleanup worker facade that executes remote-upload
  cancellation cleanup plans and reports per-action success or failure without
  stopping at the first failed cleanup action.
- [x] Wire remote upload transfer-worker execution to the cleanup worker facade
  so failed transfer jobs can run a cancellation cleanup plan and return the
  cleanup report to daemon callers.
- [x] Add concrete cleanup workers for partial SSD-staged objects, failed S3
  multipart uploads, abandoned sessions, expired pairings, and interrupted
  browser handoff records.
- [x] Add a typed daemon remote-upload progress telemetry payload for source
  scan count, staged bytes, S3 transfer rate, SSD queue depth, HDD landing
  queue depth, active per-HDD writers, verification state, and session-renewal
  status.
- [x] Wire the remote easyconnect/AWS CLI submit path to populate source scan
  count and staged-byte remote-upload progress telemetry from the client-side
  source inventory.
- [x] Wire the daemon remote-upload S3 transfer worker to derive
  S3 transfer-rate telemetry from byte progress and progress timestamps.
- [x] Wire the daemon remote-upload S3 transfer worker to populate non-zero
  SSD stage and HDD landing queue-depth telemetry from the admission gate
  snapshot.
- [x] Wire daemon ingest telemetry into remote-upload progress for active HDD
  writer count and pending verification state.
- [x] Wire the remote easyconnect/AWS CLI submit path to populate
  session-renewal status telemetry from paired session renewal metadata.
- [x] Add Web progress rendering for remote uploads that remains accurate when
  the browser refreshes, disconnects, or reconnects while the paired CLI agent
  continues transfer.
- [x] Add remote CLI progress rendering for easyconnect uploads using the same
  daemon job/event model as normal CLI ingest and embedded TUI views.
- [x] Add remote-client tests for easyconnect pairing success, expired pairing,
  denied login, missing/expired paired upload sessions, and renewal telemetry
  states.
- [x] Add server-side easyconnect API contract tests for revocation request
  validation, eight-hour renewal policy, active-upload renewal responses, and
  standalone local-user grant filtering.
- [x] Add standalone easyconnect auth-context route tests for invalid, expired,
  and revoked persisted local sessions.
- [x] Add remote-upload runtime tests proving a failed paired upload can report
  active-upload renewal progress, clean abandoned session state, release S3
  admission capacity, and preserve the failed daemon job record.
- [x] Add a first-class persisted easyconnect paired-session store with tests
  for revocation, renewal token rotation, expiry, actor matching, and
  per-ObjectStore write permission checks.
- [x] Wire the persisted easyconnect paired-session store into daemon revoke
  and renew routes with persisted revocation, expiry extension, and renewal
  token rotation tests.
- [x] Wire the persisted easyconnect paired-session store into daemon
  easyconnect create/exchange routes and remote-upload ObjectStore listing
  authorization.
- [x] Add tests for ObjectStore listing through a remote upload session,
  including non-writer denial, read-only/locked store denial, and missing writer
  group diagnostics.
- [x] Add tests for browser/agent coordination, drag/drop folder expansion,
  local path privacy, agent unreachable state, and user cancellation before
  transfer begins.
- [x] Add S3 upload integration tests or fakes for multipart transfer,
  interrupted transfer cleanup, credential expiry, and derived store/bucket
  routing.
- [x] Add daemon ingest policy tests proving remote/Web/S3 origins stage to SSD
  and local server origins use direct-to-HDD placement when safe.
- [x] Add scheduler tests for the HDD worker formula across 1, 2, 3, 4, 5, and
  8 HDD enclosures, including one-writer-per-HDD and redundancy placement
  constraints.
- [x] Update `docs/user/remote-upload.rst` with easyconnect setup, browser
  authentication, ObjectStore selection, drag-and-drop upload, session renewal,
  cancellation, and recovery behavior.
- [x] Update `docs/user/ingesting-files.rst`, `docs/user/object-stores.rst`,
  and `docs/user/web-interface.rst` with the simplified ingress-origin rules:
  local server ingest writes direct to HDD, while S3/Web/remote upload stages to
  SSD first.
- [x] Update packaging docs and Makefile notes for `make remote`, `make
  remote-deb`, and `make remote-rpm` so remote easyconnect dependencies and
  browser-launch expectations are explicit.
- [x] Add operator documentation for the default HDD landing concurrency rule,
  per-HDD writer exclusivity, SSD pressure behavior, and how to diagnose slow
  remote uploads.

## Milestone 23: Appliance Telemetry, Home Dashboard Graphs, and floundeR Time-Series Contracts

- [x] Define a versioned appliance telemetry JSON schema covering timestamped
  CPU usage, memory usage, disk capacity, per-disk read/write IO counters,
  enclosure/disk identity, DASObjectStore Web/session user counts, and
  collection quality/missing-data markers.
- [x] Choose and document the managed telemetry state location under the
  appliance-owned state tree, including file ownership, permissions, atomic
  write strategy, recovery from partial writes, and migration behavior for
  future schema versions.
- [x] Implement daemon-owned telemetry collection as a managed service loop
  rather than a Web/API side effect, with configurable sampling cadence and
  initial supported cadences around 6 seconds and 30 seconds.
- [x] Add platform collectors for CPU and memory usage on supported Linux
  appliance hosts, with unit tests using fixture `/proc` or command-output data
  rather than relying on live host state.
- [~] Add per-enclosure disk capacity collection for every disk physically
  associated with known DAS enclosures:
  - [x] Collect capacity for managed HDD roots declared by
    `.dasobjectstore/device.env`, preserving disk ID, label, mount path, role,
    filesystem/device marker data, and any marker-provided enclosure ID.
  - [x] Preserve marker-provided bay labels in capacity telemetry and daemon
    API summaries so current operators can correlate known bays while the
    authoritative physical enclosure/bay registry is still pending.
  - [ ] Tie capacity samples to the future physical enclosure/bay registry so
    every disk physically associated with a known DAS enclosure carries the
    authoritative enclosure association in each sample.
    Blocked until the physical enclosure/bay registry is implemented.
- [~] Add per-enclosure disk IO collection for read bytes/s, write bytes/s,
  read operations/s, write operations/s, queue or await signals where available,
  and explicit missing-counter reasons when the host cannot provide a metric:
  - [x] Add Linux `/proc/diskstats` parsing and managed-HDD marker matching for
    per-disk IO rate calculation using fixture data.
  - [x] Wire disk IO counters into the daemon telemetry service loop with
    retained previous samples and cadence-aware rate calculation.
  - [x] Preserve marker-provided bay labels in disk IO telemetry, current IO
    summaries, and per-disk IO series for stable current grouping.
  - [ ] Tie disk IO samples to the future physical enclosure/bay registry so
    per-enclosure IO grouping uses authoritative hardware association.
    Blocked until the physical enclosure/bay registry is implemented.

### Live Disk IO and throughput-card production follow-up

- [ ] Verify the packaged daemon telemetry loop is running on the appliance,
  writes samples at its configured cadence, and uses the active managed-HDD
  root rather than a development/default path; expose its last successful
  collection time and failure reason to operators.
- [ ] Resolve every managed HDD mount to the block device actually represented
  in `/proc/diskstats`, including partitions, device-mapper/LVM paths, MD RAID,
  USB bridge names, and stable `/dev/disk/by-*` aliases; do not depend only on
  a marker's basename when it cannot match the kernel counter name.
  - [x] Resolve explicit `diskstats_device` markers and stable `/dev/disk/by-*`
    or `/dev/disk/by-path` aliases through a fixtureable sysfs root before
    reporting `device_missing`; preserve the remaining partition/device-mapper/
    MD/USB topology work as an open extension.
- [ ] Validate managed-HDD device markers during enclosure preparation and
  telemetry collection. Emit a per-disk diagnostic when the marker has no
  usable block-device mapping, the device is absent from `/proc/diskstats`, or
  counter access is denied.
- [x] Model first-sample warm-up separately from unavailable telemetry: retain
  the first counter snapshot, report `first_sample_warmup` for per-disk IO, and
  never present a zero or missing rate as a confirmed idle disk.
- [ ] Propagate structured per-disk IO missing reasons, sample age, mapped
  device name, and collection status through the daemon and Home API so the
  Disk IO card identifies the affected disk and corrective action instead of
  only stating that telemetry is unavailable.
  - [x] Add optional mapped device, missing-reason, and current-sample timestamp
    fields to daemon disk-IO summaries and series points, preserving decoding of
    older responses.
  - [x] Surface warm-up and missing-device diagnostics, including disk and
    mapped device identity, in the Home Disk IO card; mixed valid/missing disks
    retain valid totals while showing an elevated diagnostic.
  - [x] Carry sample timestamp/age and per-disk identity/rates through the Home
    wire view, along with collection quality and raw missing-data markers.
- [ ] Make the Home throughput chart explicitly distinguish retained Disk IO
  samples, legacy throughput-file fallback, no observed IO, and telemetry
  collection failure. Preserve chart gaps and show a linked diagnostic rather
  than silently rendering an empty graph.
  - [x] Add explicit `source` and optional `message` fields to throughput
    summaries: daemon disk-IO retention, legacy throughput-file fallback, and
    unavailable state are distinguishable while legacy payloads decode safely.
  - [x] Render the provenance as a visible Home chart badge and source-specific
    line treatment (solid daemon, amber legacy fallback, fixture/unavailable
    dashed states), retaining the optional diagnostic message; preserving
    invalid-sample gaps and linked failure diagnostics remains open.
  - [x] Preserve invalid daily samples as fixed-position chart gaps, split SVG
    lines at missing intervals, and show a non-interpolating gap diagnostic;
    the broader appliance integration fixture matrix remains open.
- [ ] Add appliance integration coverage using managed marker, mount, sysfs,
  and `/proc/diskstats` fixtures for SATA, partition, USB, and device-mapper
  paths; assert first-sample warm-up, later non-zero rates, unavailable-device
  diagnostics, Disk IO card values, and throughput-chart points.
  - [x] Add a macOS/Linux-safe collector fixture matrix for direct SATA,
    partition, stable USB `by-id`, device-mapper `by-path`, and missing-device
    mappings. It asserts warm-up, mapped names, non-zero second-sample rates,
    and explicit `device_missing`; authoritative enclosure topology and live
    packaged-loop acceptance remain blocked.
- [x] Add an operator runbook for restoring Home telemetry: verify daemon loop
  health, marker/device mapping, `/proc/diskstats` visibility, sample state
  ownership, and the distinction between an idle disk and unavailable data.
  The macOS-safe guide is `docs/user/telemetry-troubleshooting.rst`; packaged
  daemon-loop and authoritative enclosure-topology acceptance remain blocked
  on appliance access.
- [x] Add active-user/session telemetry for local Web sessions and remote-agent
  sessions, including total active sessions, distinct logged-in users, and
  administrator/non-administrator counts where policy permits exposure.
- [x] Implement bounded JSON retention so telemetry cannot grow without limit,
  with retention/downsampling policy sufficient for 1 hour, 1 day, 10 day, and
  3 month chart windows.
- [x] Add daemon tests for telemetry cadence, bounded retention, atomic rewrite,
  corrupt JSON recovery, missing metric markers, and preservation of
  enclosure/disk identity across samples.
- [x] Expose authenticated telemetry API routes for current summaries,
  downsampled time-series windows, per-disk IO series, capacity history,
  session/user history, available time windows, and missing-data intervals.
- [x] Add API tests proving telemetry windows are downsampled consistently,
  unauthorized users cannot access protected telemetry, missing data is not
  interpolated, and response sizes remain bounded for 3 month windows.
- [x] Extend the Home dashboard API payload so existing Capacity, Throughput,
  and Memory Stress cards consume telemetry-backed summaries where available.
- [x] Add Home dashboard cards for IO, logged-in users, and CPU usage, with
  compact operator wording, stable card dimensions, and no dependence on
  placeholder/fallback text once telemetry is available.
- [x] Implement a global Home telemetry time-window control with 1 hour, 1 day,
  10 days, and 3 months options that applies consistently to all telemetry
  charts on the page.
  Completed by adding a Home telemetry-window query contract, filtering
  daemon-backed Home telemetry summaries by the selected window, and rendering a
  browser-side segmented window control above the Home metric grid.
- [x] Ensure telemetry charts update on cadence without jitter: stable chart
  containers, stable axes/labels, bounded redraw work, no card resizing, and no
  text overlap on desktop or mobile.
  Completed by adding fixed-cadence Home telemetry refresh, decoding bounded
  daily throughput points, and rendering a fixed-viewBox SVG chart with stable
  axes, bounded labels, and an explicit empty-sample state.
- [x] Define reusable floundeR data contracts for Mnemosyne appliance
  telemetry: line charts with missing-data gaps, point/step summaries,
  capacity bands, per-disk IO traces, and small-multiple chart layouts.
  Completed by adding the versioned Mnemosyne floundeR appliance telemetry
  contract module with chart layout, axis, series, point-quality, missing
  interval, capacity-band, per-device, and small-multiple DTOs.
- [x] Implement floundeR rendering support for scientifically correct missing
  intervals so absent samples, service restarts, unknown devices, and
  unavailable counters are shown as gaps or labelled missing intervals rather
  than interpolated lines.
  Completed by adding floundeR render-plan DTOs that split observed series into
  non-interpolated segments and emit labelled gap intervals for missing
  samples, service restarts, unknown devices, and unavailable counters.
- [x] Ensure the floundeR telemetry chart contract can be used both by the Web
  dashboard and by Grammateus formal reports without DASObjectStore-specific
  hard-coding.
  Completed by adding a product-neutral floundeR chart contract wrapper with
  explicit Web dashboard, Grammateus report, and API export audiences plus a
  conversion path from DASObjectStore appliance telemetry payloads.
- [x] Add Yew DTO/component tests for CPU, memory, IO, capacity, throughput,
  and active-user charts with full data, sparse data, missing intervals,
  changing time windows, and per-disk series.
  Completed by adding Web workspace DTO/component-helper tests for full Home
  telemetry cards, non-default telemetry windows, per-disk IO identity, sparse
  and unavailable telemetry states, and invalid/missing throughput chart
  samples.
- [x] Add screenshot or DOM regression coverage proving the Home telemetry
  cards and charts do not jitter, overlap, or resize unexpectedly across
  desktop and mobile layouts.
  Completed by adding a Home telemetry DOM/CSS regression contract test that
  pins the metric grid, time-window controls, fixed SVG chart frame, chart
  labels/points, text wrapping, and desktop/mobile responsive breakpoints.
- [x] Update `docs/user/web-interface.rst` with the Home telemetry cards,
  time-window control, missing-data interpretation, update cadence, and
  administrator/operator expectations.
  Completed by documenting the Home telemetry cards, the 1 hour/1 day/10
  days/3 months selector, missing and sparse sample interpretation, the
  30-second browser refresh cadence, and read-only operator versus
  administrator expectations.
- [x] Update `docs/standalone-service.md` with telemetry state file location,
  retention policy, ownership, cadence configuration, and how to reset or
  inspect telemetry safely.
  Completed by documenting packaged telemetry ownership, config validation,
  supported cadences, bounded retention, read-only inspection commands, safe
  history reset steps, and daemon log checks for collection/write failures.
- [x] Add cross-product notes for floundeR documenting the generalized
  telemetry chart grammar so Monas, Synoptikon, Mnematikon, and future
  Mnemosyne products can reuse the same plotting semantics.
  Completed by adding a product-neutral floundeR telemetry chart grammar note
  and registering `mnemosyne.flounder.telemetry_chart_contract.v1` in the
  public format registry.

## Milestone 24: Mnemosyne Design Language Alignment

This milestone supersedes the **visual and interaction assumptions** of the
completed Milestones 19, 20, and 22. Their completed work remains valid for
daemon ownership, API boundaries, authorization, and data loading; do not
reopen those concerns. The current Web console must now conform to the central
Mnemosyne design canon in `../mnemosyne_design_language/docs/brief.md` and
`../mnemosyne_design_language/docs/interface-patterns.md`.

The implementation rule is: **operational data stays on the page; a user
performs creation, qualification, editing, and confirmation in a transient,
contextual task pane.** A task pane is triggered by click or keyboard, never by
pointer hover alone. It receives focus when opened, supports Escape for a
non-destructive close, and restores focus to its trigger when closed.

### 24.1 Shared tokens, assets, footer, and task-pane primitive

- [x] Import the approved Mnemosyne assets into
  `crates/dasobjectstore-gui-web/assets/` from
  `../mnemosyne_design_language/assets/branding/` without redrawing them:
  `mnemosyne-biosciences-logo-master-mono.png`,
  `mnemosyne-biosciences-logo-icon-black.png`, and
  `mnemosyne-biosciences-partial.png`. Register every file with Trunk in
  `crates/dasobjectstore-gui-web/index.html`. Preserve source identity with a
  checksum or byte-comparison test/documented provenance; do not make the
  browser fetch a sibling-repository path at runtime.
  - [x] Import all three approved marks into the repository, register each as a
    Trunk asset, and pin each source SHA-256 in the Web workspace contract test.
- [x] Replace the current near-black/monospace `.dos-product-footer` treatment
  in `crates/dasobjectstore-gui-web/styles.css` and
  `src/components/footer.rs` with the Mnemosyne footer contract:
  - use `#1c2b0b` as the footer surface, not the current near-black;
  - render the approved company wordmark on the left, reversed
    non-destructively for the dark surface;
  - retain compact DASObjectStore product/version/provenance text as secondary
    content, not as the footer identity;
  - render exactly one `aria-hidden` partial mark, oversized and cropped at the
    lower-right edge, behind but never underneath readable text; and
  - keep the footer in the application flex shell so it reaches the viewport
    bottom on short pages and follows content on long pages.
  Do not use the partial mark as a button, repeated card motif, spinner, or
  status icon. Retain the current footer component as the one shared source of
  truth; do not introduce page-specific footer copies.
- [x] Add and use semantic CSS variables for Mnemosyne footer/provenance,
  interaction, and status roles. The current teal action treatment may remain
  the primary interaction colour, but Mnemosyne green is reserved for company
  provenance/footer and must not become a generic success badge or action
  colour. Use explicit text plus colour for every state; contract tests pin the
  variables and their use in the shared stylesheet.
- [x] Add a reusable Yew `TaskPane` component under
  `crates/dasobjectstore-gui-web/src/components/` or extend the existing
  `InspectorDrawer` only if it gains the full task contract: title, selected
  context, close button, focus management, Escape handling, labelled form
  region, footer actions, and an optional review/confirmation step. Model open
  state as one explicit enum (for example `Closed | Create | Edit(Id) |
  Review`) rather than multiple unrelated booleans. A small anchored pane is
  permitted for one-step low-risk work; use a side sheet on desktop and a
  full-height sheet on narrow screens for the workflows below. The shared
  component now implements focus-on-open, trigger-focus restoration, Escape
  close, labelled form content, selected context, and footer actions; page
  migrations to it remain in the workflow tasks below.
- [x] Update the shared Web CSS and component tests so panes, footer, tables,
  status chips, and responsive layouts share primitives rather than adding
  page-local variants. Dense tables and Object Browser tables now use the
  shared table wrapper/base, every reusable widget has owned status/capacity/
  segmented/icon/risky/inspector styling, and host-safe source contracts pin
  semantic attributes, form-submit prevention, stylesheet order, and mobile
  breakpoints under the production module-size budget.

### 24.2 Local Access: users first, qualification and groups in one task flow

- [x] Refactor `crates/dasobjectstore-gui-web/src/workspace/users_groups.rs` so
  the primary content is a comparable **Users** table/structured list, not the
  permanent `create_local_group` and `assign_local_user_to_group` dashed form
  cards. Each row must show: local user, registration/qualification state,
  current access or tenant groups, administrator state where applicable, and a
  scoped action. Keep group policy in a secondary Groups section/tab rather
  than presenting it as a competing dashboard card. The Users table is now
  primary and the former permanent mutation cards are gone.
- [x] Add one primary `Add user` action in the Local Access page header or
  users-table toolbar. It opens a `TaskPane` above the existing table and has
  this sequence: (1) select/identify an existing local user, (2) record or
  select the access qualification the appliance policy requires, (3) select
  one or more access/tenant groups, and (4) review and apply. The pane title,
  labels, and review must identify the user being changed. The pane supports
  one or more selected groups and reports contextual partial failures.
- [x] Do **not** create Unix/OS users from the browser. `Add user` means adding
  or qualifying an already OS-recognised/local-account user for
  DASObjectStore access. Preserve daemon-side authorization and the existing
  local-group action routes. If the current workspace DTO cannot show each
  user’s memberships/qualification, extend the daemon/API contract and
  `UsersGroupsWorkspaceResponse` with an authoritative per-user mapping; do
  not infer all users’ memberships from `current_user`. The response now
  carries per-user qualification state, groups, and sudo-derived administrator
  state from the server-side local authority provider.
- [x] Move group creation behind a secondary action inside the Add-user flow or
  the Groups context. Creating a group must refresh/select the new group in the
  user task flow. Mapping a user to a group must not be represented as an
  independent dashboard object or permanent form card. It now opens from the
  secondary Groups context in a task pane.
- [x] Preserve the existing policy and safety semantics: non-administrators see
  the table and a clearly explained disabled/unavailable action; administrator
  submissions still go through the daemon-backed create/assign routes; success
  updates the source table; failures remain in the task pane with the user and
  target group context. Do not introduce confirmation phrases or acknowledgement
  checkboxes for ordinary group assignment unless the daemon policy marks that
  action consequential. The current daemon contract still requires the
  existing action-time acknowledgement marker, which remains in the pane review.

### 24.3 Endpoints: inventory first, add/edit only in a contextual pane

- [x] Refactor `crates/dasobjectstore-gui-web/src/endpoints.rs` so the endpoint
  inventory is the primary table/list. Add a page-level `Add endpoint` action
  and a row-level `Edit` action. Remove the always-visible
  `render_endpoint_upsert_card` form from loading, empty, and populated
  inventory states.
- [x] Implement add/edit as a `TaskPane` with explicit sections in this order:
  endpoint identity and display name; endpoint kind and service URL;
  validation state/evidence; optional ObjectStore/governance binding; then
  review and submit. Pre-fill edit fields from the selected endpoint. Keep
  binding fields hidden until binding is intentionally enabled.
- [x] Preserve the current authenticated daemon/API upsert contract, dry-run
  behaviour, and high-impact live confirmation. The inventory view must not
  expose the confirmation phrase. Show it only in the live-update review step,
  with the endpoint ID, URL, binding, and impact summary visible immediately
  above it.
- [x] On success, close the pane or show a success state and refresh/update the
  corresponding row. On failure, keep the pane open with editable values and
  an inline error. Loading, empty, permission-denied, and transport-error
  states must still have a clear inventory heading and the appropriate action
  affordance; do not regress to a form-only page.

### 24.4 Remote Upload: explicit ObjectStore selection is mandatory

- [x] Remove `RemoteUpload` from the global `PRIMARY_NAVIGATION` and
  `INTEGRATED_PRIMARY_NAVIGATION` arrays in
  `crates/dasobjectstore-gui-web/src/workspace.rs`. A generic remote-upload
  page must not be reachable as an unscoped primary workspace; target-scoped
  entry from ObjectStores remains open.
- [x] Add an `Upload` action to each writable, authorized ObjectStore row/card
  in `src/workspace/object_stores.rs`. The action selects that exact store and
  opens the remote-upload pane or a target-scoped workspace. Its visible title
  must be `Upload to {ObjectStore display name}` and its context must show the
  selected store’s writer group, object type, capacity/warnings, and ingress
  policy before file selection. The action selects the exact store and opens
  the target-scoped pane; the pane now renders the target name, writer group,
  object type, used/free capacity, and paired-agent ingress policy before the
  dropzone.
- [x] Change `RemoteUploadPageProps`, the Web state in `src/app.rs`, and the
  remote-upload API contract so a target store ID is required. The server must
  reject a missing, unauthorised, non-writable, or disabled target; do not rely
  on the browser’s default selection for authorization. Do not silently select
  the first writable store.
  - [x] Require ``store_id`` on the daemon-independent Web workspace route,
    filter the response to that target, and render no file dropzone until the
    explicit target is present. Missing targets return HTTP 400; authorization
    and writable-state filtering remain server-owned.
- [x] Refactor `src/workspace/remote_upload.rs` so its file/folder dropzone,
  handoff summary, and confirm action are not rendered until an explicit,
  authorized target is present. If no writable store exists, show an explanatory
  empty state with a route/action back to ObjectStores; do not show an active
  dropzone beside a store catalogue.
  - [x] Render target title/context and the file dropzone only after the
    target-scoped response contains an authorized writable store; include
    writer group, object type, capacity, and paired-agent landing policy.
  - [x] Make the component boundary target-required as well as the route:
    `RemoteUploadPageProps` now accepts only a non-empty `String`, the app
    renders a target-required empty state when no ObjectStore was selected,
    and target query values are percent-encoded through the shared Web helper.
  - [x] Add a direct `Back to ObjectStores` action to the target-required
    empty state so the target-scoped flow does not strand the operator.
- [ ] Keep all existing remote-agent pairing, path privacy, S3 credential,
  SSD-first ingress, daemon job, cancellation, and renewal behaviour. This is
  a presentation/context refactor, not permission or transfer-policy
  relaxation.

### 24.5 Verification and documentation

- [ ] Update `crates/dasobjectstore-gui-web/src/workspace/tests.rs` and the
  visual runner under `tools/web-screenshot-regression.mjs`. The runner’s old
  Local Access assertions still expect separate dry-run preview controls that
  no longer match the current Yew screen; replace them with the canonical
  users-table/task-pane assertions and fixture data that matches the live DTOs.
  - [x] Add host-side source/contract coverage for the users-first inventory,
    per-user authority fields, target pane steps, and non-admin action gating.
  - [x] Update the Web Interface and Local Access documentation for the
    users-first table and task-pane workflow; Playwright artifact execution
    remains environment-gated.
  - [x] Replace the legacy screenshot fixture users/groups selectors and add
    endpoint inventory/upsert fixture responses plus users-first workflow
    assertions; full Playwright artifact execution remains environment-gated.
  - [x] Add a focused source contract proving target-scoped Remote Upload does
    not expose a dropzone before an explicitly authorized writable target and
    does not silently select the first writable store; visual artifact
    execution remains environment-gated.
- [ ] Add focused component/API tests for: footer content on login and each
  authenticated shell state; one decorative partial mark only; keyboard open,
  Escape close, and focus return for every task pane; Local Access per-user
  memberships/qualification; endpoint add/edit prefill and confirmation gate;
  and remote-upload rejection without an explicit target.
- [ ] Add desktop and 390 px mobile visual/DOM regression coverage for the
  closed and open Local Access, Endpoints, and target-scoped Remote Upload
  workflows. Assert no overlap, horizontal overflow, hidden primary form,
  unreadable footer text, or visible upload dropzone before target selection.
- [x] Update `docs/user/web-interface.rst` and `docs/user/remote-upload.rst`
  with the task-pane interaction, Local Access qualification flow, endpoint
  inventory/add-edit workflow, and ObjectStore-first upload flow. State that
  the browser never creates OS users or mutates managed storage directly.

**Milestone 24 completion gate:** run focused Rust/Yew tests and the updated
visual regression runner; inspect desktop and mobile artifacts; compare the
footer against the approved Grammateus reference; and confirm that the only
staged changes are the worker’s scoped task. Do not mark the milestone complete
until all five sub-sections are complete and the application no longer contains
the permanent Local Access/group-mapping or endpoint-administration form cards.

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
