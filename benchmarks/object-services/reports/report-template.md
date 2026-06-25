# Object Service Benchmark Report

Status: Draft
Benchmark date: `YYYY-MM-DD`
Rubric: `benchmarks/object-services/reports/scoring-rubric.md`

## Summary

Recommendation: `none | garage | rustfs`

Decision:

- Garage: `eligible | failed hard gate | not run`
- RustFS: `eligible | failed hard gate | not run`

Rationale:

- `One to three concise bullets explaining the recommendation.`

## Environment

| Field | Value |
| --- | --- |
| Host OS | `...` |
| CPU | `...` |
| Memory | `...` |
| Docker version | `...` |
| Filesystem | `...` |
| DAS hardware | `none | model/details` |
| SSD ingest path | `...` |
| HDD destage root | `...` |
| Benchmark output root | `benchmarks/output/object-services` |

## Provider Builds

| Provider | Image | Compose file | Notes |
| --- | --- | --- | --- |
| Garage | `dxflrs/garage:v2.3.0` | `providers/garage/compose.yml` | `...` |
| RustFS | `rustfs/rustfs:1.0.0-beta.8-glibc` | `providers/rustfs/compose.yml` | `...` |

## Configuration Overrides

| Variable | Value |
| --- | --- |
| `DASOBJECTSTORE_BENCH_OUTPUT_DIR` | `...` |
| `DASOBJECTSTORE_SSD_INGEST_PATH` | `...` |
| `DASOBJECTSTORE_HDD_ROOT_PATH` | `...` |
| `DASOBJECTSTORE_*` workload overrides | `...` |
| `AWS_*` overrides | `...` |

## Reliability Gates

| Gate | Garage | RustFS | Raw reports |
| --- | --- | --- | --- |
| Large object integrity | `pass/fail/not run` | `pass/fail/not run` | `...` |
| Small object integrity | `pass/fail/not run` | `pass/fail/not run` | `...` |
| Concurrent client integrity | `pass/fail/not run` | `pass/fail/not run` | `...` |
| Crash/restart recovery | `pass/fail/not run` | `pass/fail/not run` | `...` |
| Interrupted write safety | `pass/fail/not run` | `pass/fail/not run` | `...` |
| Metadata recovery | `pass/fail/not run` | `pass/fail/not run` | `...` |
| Disk pressure safety | `pass/fail/not run` | `pass/fail/not run` | `...` |
| Simulated disk removal safety | `pass/fail/not run` | `pass/fail/not run` | `...` |
| SSD ingest/HDD destage compatibility | `pass/fail/not run` | `pass/fail/not run` | `...` |

Hard-gate failures:

- `Provider/workload/failure reason, or "none".`

## Performance Score

Score only providers that passed every reliability gate.

| Category | Points | Garage | RustFS | Basis |
| --- | ---: | ---: | ---: | --- |
| Large object throughput | 25 | `...` | `...` | `...` |
| Small object throughput | 20 | `...` | `...` | `...` |
| Concurrent client throughput | 20 | `...` | `...` | `...` |
| Recovery time after crash/restart | 10 | `...` | `...` | `...` |
| Metadata recovery time | 10 | `...` | `...` | `...` |
| Disk pressure behavior | 5 | `...` | `...` | `...` |
| SSD ingest/HDD destage throughput | 10 | `...` | `...` | `...` |
| Total | 100 | `...` | `...` | `...` |

## Workload Notes

| Workload | Garage notes | RustFS notes |
| --- | --- | --- |
| `large-object` | `...` | `...` |
| `small-object` | `...` | `...` |
| `concurrent-client` | `...` | `...` |
| `crash-restart-ingest` | `...` | `...` |
| `interrupted-write` | `...` | `...` |
| `metadata-recovery` | `...` | `...` |
| `disk-full` | `...` | `...` |
| `simulated-disk-removal` | `...` | `...` |
| `ssd-ingest-hdd-destage` | `...` | `...` |

## Residual Risks

- Long-duration soak testing is not covered by this milestone.
- Real DAS USB reset, SMART, and filesystem edge cases may differ from local
  benchmark simulation.
- `Any run-specific risks go here.`

## Final Recommendation

`State the selected MVP object service or state that no provider is selected.`

Next steps:

- `Follow-up actions required before Milestone 9 integration.`
