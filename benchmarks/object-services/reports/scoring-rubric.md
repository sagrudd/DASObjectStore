# Object Service Scoring Rubric

Status: Draft
Applies to: Milestone 8 Garage and RustFS benchmark reports

## Decision Rule

A provider is eligible for MVP selection only if it passes every reliability
hard gate. Performance scores are calculated only for eligible providers.

If no provider passes every hard gate, DASObjectStore does not select an MVP
object service from that benchmark run.

## Reliability Hard Gates

Each provider must pass all gates below:

- **Large object integrity:** every completed `large-object` upload/download
  round trip verifies by SHA-256.
- **Small object integrity:** every completed `small-object` upload/download
  round trip verifies by SHA-256.
- **Concurrent client integrity:** every concurrent client completes without
  checksum mismatch or unhandled process failure.
- **Crash/restart recovery:** after service restart, a clean post-restart
  upload/download round trip verifies by SHA-256.
- **Interrupted write safety:** an interrupted client write is either absent or
  complete; retrievable partial/corrupt objects fail the gate.
- **Metadata recovery:** provider state restored from the benchmark snapshot can
  still return the seed object with the expected SHA-256 hash.
- **Disk pressure safety:** under bounded disk pressure, the provider either
  rejects the write cleanly or returns a verified object; corrupt accepted
  writes fail the gate.
- **Simulated disk removal safety:** while data is removed, the provider must
  not return corrupt data; after restore, the seed object must verify.
- **SSD ingest/HDD destage compatibility:** ingest payloads staged under the
  SSD path and settled under the HDD path must match by SHA-256.

Any checksum mismatch, unrecovered provider state, corrupt readable object, or
manual intervention requirement fails the run for that provider.

## Performance Score

Eligible providers are scored out of 100:

- Large object throughput: 25 points.
- Small object throughput: 20 points.
- Concurrent client throughput: 20 points.
- Recovery time after crash/restart: 10 points.
- Metadata recovery time: 10 points.
- Disk pressure behavior: 5 points.
- SSD ingest/HDD destage throughput: 10 points.

Scores are relative within the same benchmark run. The best eligible provider
for each measured performance category receives full points for that category.
Other eligible providers receive proportional points based on throughput or
inverse time as appropriate.

## Reporting Requirements

Reports must include:

- provider image tag and Compose file used;
- host OS, CPU, memory, Docker version, and filesystem;
- benchmark configuration overrides;
- raw report path for each workload;
- reliability gate result for each workload;
- performance score by category;
- final recommendation and residual risks.

Production-readiness claims remain blocked until later long-duration soak
testing, even if one provider wins Milestone 8.
