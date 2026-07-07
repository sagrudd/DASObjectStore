# Object Service Benchmarks

This harness compares candidate S3-compatible object services for the
DASObjectStore MVP.

## Scope

Milestone 8 benchmarks Garage and RustFS against DASObjectStore-specific
workloads:

- SSD-ingest-first write paths;
- large and small object IO;
- concurrent clients;
- crash, restart, interruption, and metadata recovery behavior;
- disk-full and simulated disk-removal behavior;
- compatibility with later HDD destage layout.

Reliability failures are hard gates. Performance results only matter for
providers that preserve object integrity and recover coherently.

## Layout

- `config/`: benchmark configuration examples and local overrides.
- `providers/`: provider-specific Docker/Compose definitions.
- `workloads/`: workload definitions and benchmark drivers.
- `reports/`: checked-in report templates and final comparison reports.
- `scripts/`: thin entrypoints for running a provider/workload pair.

Generated benchmark data belongs under `benchmarks/output/object-services/`,
which is ignored by Git.

## Local Use

Copy `config/example.toml` to a local file before running benchmarks. Keep local
paths and generated output out of version control.

S3 operations use the host `aws` CLI when available. If it is not installed, the
workloads fall back to Docker with
`${DASOBJECTSTORE_AWS_CLI_IMAGE:-amazon/aws-cli:2}`.

Start providers through the wrapper so Garage benchmark credentials and bucket
permissions are generated before workloads run:

```sh
benchmarks/object-services/scripts/provider.sh garage up
benchmarks/object-services/scripts/provider.sh rustfs up
```

```sh
benchmarks/object-services/scripts/run.sh garage large-object
```

Check the benchmark script contract without external services:

```sh
benchmarks/object-services/scripts/preflight.sh --offline
```

Check local tools before running the real matrix:

```sh
benchmarks/object-services/scripts/preflight.sh
```

Docker daemon checks default to 15 seconds. Docker Compose availability checks
also default to 15 seconds, and Compose actions default to 120 seconds. Override
these bounds with `DASOBJECTSTORE_BENCH_DOCKER_CHECK_TIMEOUT_SECONDS`,
`DASOBJECTSTORE_BENCH_COMPOSE_CHECK_TIMEOUT_SECONDS`, and
`DASOBJECTSTORE_BENCH_COMPOSE_TIMEOUT_SECONDS` when remote hardware or image
pulls need a longer window.

Run the complete provider/workload matrix:

```sh
benchmarks/object-services/scripts/run-matrix.sh
```

Follow the full execution procedure in
`benchmarks/object-services/reports/runbook.md` before producing a selection
report.

Check whether the raw outputs are sufficient for a selection report:

```sh
benchmarks/object-services/scripts/check-report-inputs.sh
```

Generate a Markdown inventory of raw report inputs:

```sh
benchmarks/object-services/scripts/report-input-index.sh
```

Generate a draft provider-selection report with environment and raw input
inventory:

```sh
benchmarks/object-services/scripts/draft-report.sh
```

Capture a Markdown environment snapshot:

```sh
benchmarks/object-services/scripts/environment-snapshot.sh
```

Run offline smoke tests for the benchmark harness:

```sh
benchmarks/object-services/scripts/smoke-test.sh
```
