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

Provider-specific setup is intentionally added in separate tasks so Garage and
RustFS remain comparable.
