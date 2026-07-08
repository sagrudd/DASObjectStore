# Changelog

All notable DASObjectStore release changes are recorded here.

This project follows semantic versioning. Patch and minor version bumps may be
made automatically for compatible work; major version bumps require explicit
agreement before landing.

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
