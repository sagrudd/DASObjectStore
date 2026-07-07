# Object Service Benchmark Report

Status: Complete
Benchmark date: `2026-07-07`
Rubric: `benchmarks/object-services/reports/scoring-rubric.md`

## Summary

Recommendation: `garage`

Decision:

- Garage: `eligible`
- RustFS: `eligible`

Rationale:

- Both providers passed every reliability hard gate in the bounded validation
  matrix on the remote DAS host.
- Garage is the better MVP target because its benchmark profile directly
  separates metadata and data directories, matching the DASObjectStore SSD
  metadata plus HDD object-data model.
- RustFS remains a viable comparison provider, but the single-node container
  profile required pre-created bucket directories because S3 `CreateBucket`
  was rejected by the default benchmark credentials.

## Environment

| Field | Value |
| --- | --- |
| Host OS | `Linux` |
| Kernel | `7.0.0-27-generic` |
| Machine | `x86_64` |
| CPU | `12th Gen Intel(R) Core(TM) i9-12900` |
| Memory bytes | `65631334400` |
| Docker version | `Docker version 29.1.3, build 29.1.3-0ubuntu4.1` |
| Docker Compose version | `Docker Compose version 2.40.3+ds1-0ubuntu1` |
| AWS CLI version | `aws-cli/2.31.35 Python/3.14.4 Linux/7.0.0-27-generic source/x86_64.ubuntu.26` |
| AWS CLI backend | `local aws` |
| Hash tool | `sha256sum (uutils coreutils) 0.8.0` |
| Benchmark output root | `benchmarks/output/object-services` |
| SSD ingest path | `/tmp/dasobjectstore-bench/ssd` |
| HDD destage root | `/tmp/dasobjectstore-bench/hdd` |

## Provider Builds

| Provider | Image | Compose file | Notes |
| --- | --- | --- | --- |
| Garage | `dxflrs/garage:v2.3.0` | `providers/garage/compose.yml` | Runs with separate metadata and data bind mounts. Benchmark wrapper provisions buckets and generated keys. |
| RustFS | `rustfs/rustfs:1.0.0-beta.8-glibc` | `providers/rustfs/compose.yml` | Runs as UID/GID `10001`. Benchmark wrapper pre-creates bucket directories for the single-node profile. |

## Configuration Overrides

This was a bounded validation matrix, not a production performance run.

| Variable | Value |
| --- | --- |
| `DASOBJECTSTORE_BENCH_OUTPUT_DIR` | `benchmarks/output/object-services` |
| `DASOBJECTSTORE_SSD_INGEST_PATH` | `/tmp/dasobjectstore-bench/ssd` |
| `DASOBJECTSTORE_HDD_ROOT_PATH` | `/tmp/dasobjectstore-bench/hdd` |
| `DASOBJECTSTORE_LARGE_OBJECT_BYTES` | `1048576` |
| `DASOBJECTSTORE_SMALL_OBJECT_BYTES` | `1024` |
| `DASOBJECTSTORE_SMALL_OBJECT_COUNT` | `2` |
| `DASOBJECTSTORE_CONCURRENT_CLIENTS` | `2` |
| `DASOBJECTSTORE_CONCURRENT_OBJECT_BYTES` | `1024` |
| `DASOBJECTSTORE_CONCURRENT_OBJECT_COUNT` | `2` |
| `DASOBJECTSTORE_CRASH_OBJECT_BYTES` | `1048576` |
| `DASOBJECTSTORE_INTERRUPTED_OBJECT_BYTES` | `1048576` |
| `DASOBJECTSTORE_DISK_FULL_OBJECT_BYTES` | `1024` |
| `DASOBJECTSTORE_DISK_FULL_FILL_BYTES` | `1048576` |
| `DASOBJECTSTORE_METADATA_RECOVERY_OBJECT_BYTES` | `1024` |
| `DASOBJECTSTORE_REMOVAL_OBJECT_BYTES` | `1024` |
| `DASOBJECTSTORE_DESTAGE_OBJECT_BYTES` | `1024` |
| `DASOBJECTSTORE_INTERRUPT_DELAY_SECONDS` | `1` |
| `DASOBJECTSTORE_RESTART_DELAY_SECONDS` | `1` |
| `DASOBJECTSTORE_RESTART_SETTLE_SECONDS` | `1` |

## Reliability Gates

| Gate | Garage | RustFS | Raw reports |
| --- | --- | --- | --- |
| Large object integrity | `pass` | `pass` | `large-object/report.tsv` |
| Small object integrity | `pass` | `pass` | `small-object/report.tsv` |
| Concurrent client integrity | `pass` | `pass` | `concurrent-client/summary.tsv` plus client reports |
| Crash/restart recovery | `pass` | `pass` | `crash-restart-ingest/report.tsv` |
| Interrupted write safety | `pass` | `pass` | `interrupted-write/report.tsv` |
| Metadata recovery | `pass` | `pass` | `metadata-recovery/report.tsv` |
| Disk pressure safety | `pass` | `pass` | `disk-full/report.tsv` |
| Simulated disk removal safety | `pass` | `pass` | `simulated-disk-removal/report.tsv` |
| SSD ingest/HDD destage compatibility | `pass` | `pass` | `ssd-ingest-hdd-destage/report.tsv` |

Hard-gate failures:

- none

## Performance Score

Scores are provisional because the validation matrix used small payloads and
whole-second timing. The score below is sufficient to break the MVP tie, but it
must not be used as a production throughput claim.

| Category | Points | Garage | RustFS | Basis |
| --- | ---: | ---: | ---: | --- |
| Large object throughput | 25 | 25 | 20 | Garage completed the 1 MiB round trip within the one-second timer floor; RustFS recorded 1 second upload time. |
| Small object throughput | 20 | 20 | 10 | Garage recorded 1 second total for two 1 KiB objects; RustFS recorded 2 seconds total. |
| Concurrent client throughput | 20 | 20 | 20 | Both completed two clients with two 1 KiB objects each and matching SHA-256 values. |
| Recovery time after crash/restart | 10 | 10 | 10 | Both post-restart downloads verified in 1 second. |
| Metadata recovery time | 10 | 10 | 10 | Both restored provider state and verified the seed object in 1 second. |
| Disk pressure behavior | 5 | 5 | 5 | Both accepted the bounded write and returned a verified object. |
| SSD ingest/HDD destage throughput | 10 | 10 | 10 | Both ingested in 1 second and destaged within the timer floor. |
| Total | 100 | 100 | 85 | Garage wins the bounded validation score and has better operational fit. |

## Workload Notes

| Workload | Garage notes | RustFS notes |
| --- | --- | --- |
| `large-object` | 1 MiB upload/download verified by SHA-256. | 1 MiB upload/download verified by SHA-256. |
| `small-object` | Two 1 KiB objects verified by SHA-256. | Two 1 KiB objects verified by SHA-256. |
| `concurrent-client` | Two clients completed and per-client reports verified. | Two clients completed and per-client reports verified. |
| `crash-restart-ingest` | Interrupted and post-restart objects verified. | Interrupted and post-restart objects verified. |
| `interrupted-write` | Interrupted object state was `complete`; post-interrupt object verified. | Interrupted object state was `complete`; post-interrupt object verified. |
| `metadata-recovery` | Snapshot/restore verified after provider restart. | Snapshot/restore verified after provider restart after benchmark permission normalization. |
| `disk-full` | Bounded write was accepted and verified. | Bounded write was accepted and verified. |
| `simulated-disk-removal` | Removed-data read state was `absent`; restored object verified. | Removed-data read state was `absent`; restored object verified after benchmark permission normalization. |
| `ssd-ingest-hdd-destage` | Object moved from SSD ingest path to HDD destage root with matching SHA-256. | Object moved from SSD ingest path to HDD destage root with matching SHA-256. |

## Residual Risks

- This run validates correctness and harness operability, not sustained
  throughput, high object counts, or long-duration recovery behavior.
- Real DAS USB reset, SMART, disk-removal, and filesystem-pressure behavior may
  differ from the bounded simulation.
- Garage bucket/key provisioning must be promoted from benchmark wrapper logic
  into the daemon-owned object-service orchestration path.
- RustFS should remain in the benchmark harness as a regression comparator and
  fallback candidate until Garage integration has soak-test evidence.

## Raw Input Inventory

| Provider | Workload | Status | Bytes | Path |
| --- | --- | --- | ---: | --- |
| garage | large-object | present | 200 | `benchmarks/output/object-services/garage/workloads/large-object/report.tsv` |
| garage | small-object | present | 336 | `benchmarks/output/object-services/garage/workloads/small-object/report.tsv` |
| garage | concurrent-client | present | 70 | `benchmarks/output/object-services/garage/workloads/concurrent-client/summary.tsv` |
| garage | crash-restart-ingest | present | 238 | `benchmarks/output/object-services/garage/workloads/crash-restart-ingest/report.tsv` |
| garage | interrupted-write | present | 251 | `benchmarks/output/object-services/garage/workloads/interrupted-write/report.tsv` |
| garage | metadata-recovery | present | 204 | `benchmarks/output/object-services/garage/workloads/metadata-recovery/report.tsv` |
| garage | disk-full | present | 256 | `benchmarks/output/object-services/garage/workloads/disk-full/report.tsv` |
| garage | simulated-disk-removal | present | 219 | `benchmarks/output/object-services/garage/workloads/simulated-disk-removal/report.tsv` |
| garage | ssd-ingest-hdd-destage | present | 397 | `benchmarks/output/object-services/garage/workloads/ssd-ingest-hdd-destage/report.tsv` |
| rustfs | large-object | present | 200 | `benchmarks/output/object-services/rustfs/workloads/large-object/report.tsv` |
| rustfs | small-object | present | 336 | `benchmarks/output/object-services/rustfs/workloads/small-object/report.tsv` |
| rustfs | concurrent-client | present | 70 | `benchmarks/output/object-services/rustfs/workloads/concurrent-client/summary.tsv` |
| rustfs | crash-restart-ingest | present | 238 | `benchmarks/output/object-services/rustfs/workloads/crash-restart-ingest/report.tsv` |
| rustfs | interrupted-write | present | 251 | `benchmarks/output/object-services/rustfs/workloads/interrupted-write/report.tsv` |
| rustfs | metadata-recovery | present | 204 | `benchmarks/output/object-services/rustfs/workloads/metadata-recovery/report.tsv` |
| rustfs | disk-full | present | 256 | `benchmarks/output/object-services/rustfs/workloads/disk-full/report.tsv` |
| rustfs | simulated-disk-removal | present | 219 | `benchmarks/output/object-services/rustfs/workloads/simulated-disk-removal/report.tsv` |
| rustfs | ssd-ingest-hdd-destage | present | 397 | `benchmarks/output/object-services/rustfs/workloads/ssd-ingest-hdd-destage/report.tsv` |

## Final Recommendation

Select Garage as the MVP object service for Milestone 9 implementation.

Next steps:

- Implement the Garage provider as the first selected object-service backend.
- Move bucket provisioning, per-store credential setup, and metadata/data path
  ownership into `dasobjectstored`.
- Keep RustFS benchmark coverage in CI or scheduled validation as an alternate
  provider comparison.
