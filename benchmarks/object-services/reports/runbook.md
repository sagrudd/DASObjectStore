# Object Service Benchmark Runbook

Purpose: produce the first complete Garage and RustFS workload set for the
Milestone 8 provider-selection report.

## Preconditions

- Docker with Compose support is installed and running.
- AWS CLI is installed and available as `aws`.
- `sha256sum` or `shasum` is available for payload verification.
- No previous benchmark provider containers are running on ports `3900`, `9000`,
  or `9001`.
- Generated output may be deleted and recreated under
  `benchmarks/output/object-services/`.

## 1. Validate the Harness

Check the script and provider/workload contract without requiring external
services:

```sh
benchmarks/object-services/scripts/preflight.sh --offline
```

Check local tool dependencies before a real run:

```sh
benchmarks/object-services/scripts/preflight.sh
```

If the local preflight fails, fix the missing command or Docker Compose issue
before starting provider containers.

## 2. Start Each Provider

Start both providers before running the complete matrix. When debugging a
single workload, it is fine to start only the provider under test.

Garage:

```sh
docker compose -f benchmarks/object-services/providers/garage/compose.yml up -d
docker compose -f benchmarks/object-services/providers/garage/compose.yml ps
```

RustFS:

```sh
docker compose -f benchmarks/object-services/providers/rustfs/compose.yml up -d
docker compose -f benchmarks/object-services/providers/rustfs/compose.yml ps
```

## 3. Run Workloads

Run a single workload while developing or debugging:

```sh
benchmarks/object-services/scripts/run.sh garage large-object
```

Run the complete matrix when both providers are ready:

```sh
benchmarks/object-services/scripts/run-matrix.sh
```

The complete matrix must generate Garage and RustFS outputs for every workload
listed in `benchmarks/object-services/scripts/matrix.sh`.

## 4. Verify Report Inputs

After the matrix completes, verify that every required TSV report exists:

```sh
benchmarks/object-services/scripts/check-report-inputs.sh
```

The selection report must not be produced until this check passes.

## 5. Stop Providers

Garage:

```sh
docker compose -f benchmarks/object-services/providers/garage/compose.yml down
```

RustFS:

```sh
docker compose -f benchmarks/object-services/providers/rustfs/compose.yml down
```

## 6. Produce the Selection Report

Use `benchmarks/object-services/reports/report-template.md` and the scoring
rules in `benchmarks/object-services/reports/scoring-rubric.md`.

The report must:

- treat any reliability hard-gate failure as provider-disqualifying;
- score performance only for providers that pass every reliability gate;
- state whether Garage, RustFS, or neither is selected for MVP integration;
- record environment details, Docker image tags, and configuration overrides;
- keep raw benchmark outputs under `benchmarks/output/object-services/` and out
  of version control.
