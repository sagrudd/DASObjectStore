# Changelog

All notable DASObjectStore release changes are recorded here.

This project follows semantic versioning. Patch and minor version bumps may be
made automatically for compatible work; major version bumps require explicit
agreement before landing.

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
