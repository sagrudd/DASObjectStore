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

Check whether the raw outputs are sufficient for a selection report:

```sh
benchmarks/object-services/scripts/check-report-inputs.sh
```

Provider-specific setup is intentionally added in separate tasks so Garage and
RustFS remain comparable.
