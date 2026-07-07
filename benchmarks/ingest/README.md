# Ingest Benchmarks

This harness scaffolds Milestone 18 ingress performance and recovery scenarios.
It is intentionally separate from the daemon and TUI implementation: the scripts
define scenario contracts, expected profiling fields, and report locations
without mutating managed DAS roots by themselves.

Generated output belongs under `benchmarks/output/ingest/`, which is ignored by
Git.

## Scope

The scenario matrix covers:

- `small-file`: many small regular files, stressing scan, metadata, queue, and
  verification overhead.
- `large-file`: a smaller number of large files, stressing sustained
  source-to-SSD staging and sequential HDD fan-out.
- `mixed-file`: a representative blend of small metadata-heavy files and large
  payload files.
- `slow-hdd`: healthy source/SSD input with constrained HDD settlement,
  proving that HDD pressure is classified without unbounded memory growth.
- `full-ssd`: SSD reserve pressure, proving source reads throttle before the
  staging device crosses the safety reserve.
- `interrupted-import`: interruption during an active import, proving recovery
  and reconcile time after restart or reconnect.

## Local Use

Check the script contract without requiring a running daemon:

```sh
benchmarks/ingest/scripts/preflight.sh --offline
```

Run offline smoke validation:

```sh
benchmarks/ingest/scripts/smoke-test.sh
```

Inspect a scenario plan without creating output:

```sh
DASOBJECTSTORE_INGEST_BENCH_DRY_RUN=1 \
  benchmarks/ingest/scripts/run.sh mixed-file
```

Create a scaffold report for one scenario:

```sh
benchmarks/ingest/scripts/run.sh mixed-file
```

Run every scenario scaffold:

```sh
benchmarks/ingest/scripts/run-matrix.sh
```

When daemon telemetry and the TUI entry point are available, set
`DASOBJECTSTORE_INGEST_BENCH_COMMAND` to an external runner command. The harness
exports scenario variables before invoking that command and records its exit
status and wall-clock seconds. The external runner is responsible for creating
fixtures, submitting ingest jobs, applying interruption or pressure controls,
and collecting daemon/TUI telemetry.

## Profiling Contract

Each real benchmark run should collect these fields in addition to scenario
metadata:

- CPU: total CPU percent, hash CPU percent, verify CPU percent, and whether CPU
  is the current bottleneck.
- Memory: resident set size, configured memory budget, buffer pool usage, queue
  depth by stage, and peak-to-steady growth classification.
- SSD: source-to-SSD staging bytes per second, staged bytes, SSD used/free
  bytes, reserve bytes, pressure state, throttle state, and block state.
- HDD: per-target write bytes per second, fan-out concurrency, backlog bytes,
  retry count, and saturated target identifiers.
- Verification: verification bytes per second, verified bytes/files, retry
  count, failure count, and whether verification is limiting finalization.
- Recovery: interrupted job ID, journal records reconciled, staged bytes reused,
  bytes rewritten, and time to reach a monitorable or completed state.

The scaffold `metrics.tsv` uses stable column names for these measurements.
Values remain `not_collected` until a real telemetry runner supplies them.

## Acceptance Targets

Milestone 18 acceptance should be evaluated by hardware class rather than one
global throughput number. A report must state the source device, SSD model,
HDD count/model, filesystem, CPU, RAM, copy count, verification policy, and
resource policy before declaring pass/fail.

Minimum acceptance gates:

- Sustained source-to-SSD staging remains close to the lower of source-read and
  SSD-write capability until documented SSD, RAM, or safety-reserve pressure
  requires throttling.
- HDD fan-out keeps all selected healthy target disks busy when staged data is
  available, unless placement policy or safety limits intentionally cap
  concurrency.
- Verification throughput is measured separately from write throughput and does
  not hide failed, retried, or pending verification work behind completed byte
  counts.
- Memory growth is bounded by the configured buffer and queue policy. Peak
  resident memory may rise during discovery and fan-out, but it must settle
  without tracking total import size.
- Slow HDD and full SSD scenarios classify the active bottleneck correctly and
  apply backpressure before uncontrolled queue growth.
- Interrupted imports recover to a clear daemon-visible state, preserve already
  valid staged or written work when safe, and report reconcile duration.

## Reports

Use `reports/report-template.md` for human-readable benchmark notes. Raw
per-scenario scaffold outputs are TSV files under:

```text
benchmarks/output/ingest/<scenario>/<run-id>/
```
