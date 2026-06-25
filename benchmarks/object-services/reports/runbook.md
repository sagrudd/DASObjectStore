# Object Service Benchmark Runbook

Purpose: produce the first complete Garage and RustFS workload set for the
Milestone 8 provider-selection report.

## Preconditions

- Docker with Compose support is installed and running.
- AWS CLI is installed as `aws`, or Docker can run the default
  `amazon/aws-cli:2` container image.
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

When `aws` is not installed locally, the workload scripts run S3 commands via
Docker using `${DASOBJECTSTORE_AWS_CLI_IMAGE:-amazon/aws-cli:2}`. Local
`127.0.0.1` endpoints are rewritten to `host.docker.internal` for the
containerized CLI, and Docker's `host-gateway` mapping is added for Linux
hosts.

## 2. Start Each Provider

Start both providers before running the complete matrix. When debugging a
single workload, it is fine to start only the provider under test.

Garage:

```sh
benchmarks/object-services/scripts/provider.sh garage up
benchmarks/object-services/scripts/provider.sh garage ps
```

RustFS:

```sh
benchmarks/object-services/scripts/provider.sh rustfs up
benchmarks/object-services/scripts/provider.sh rustfs ps
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

Generate a Markdown input inventory for the report appendix:

```sh
benchmarks/object-services/scripts/report-input-index.sh
```

Generate a draft report from the template, environment snapshot, and raw input
inventory:

```sh
benchmarks/object-services/scripts/draft-report.sh \
  > benchmarks/object-services/reports/YYYY-MM-DD-provider-selection.md
```

Capture a Markdown environment snapshot for the report:

```sh
benchmarks/object-services/scripts/environment-snapshot.sh
```

## 5. Stop Providers

Garage:

```sh
benchmarks/object-services/scripts/provider.sh garage down
```

RustFS:

```sh
benchmarks/object-services/scripts/provider.sh rustfs down
```

## 6. Produce the Selection Report

Use `benchmarks/object-services/reports/report-template.md` and the scoring
rules in `benchmarks/object-services/reports/scoring-rubric.md`.

`benchmarks/object-services/scripts/draft-report.sh` can produce the initial
Markdown shell with an environment snapshot and raw input inventory, but the
recommendation, scoring, workload notes, and residual risks must still be
reviewed and completed by a developer.

The report must:

- treat any reliability hard-gate failure as provider-disqualifying;
- score performance only for providers that pass every reliability gate;
- state whether Garage, RustFS, or neither is selected for MVP integration;
- record environment details, Docker image tags, and configuration overrides;
- include or link the raw input inventory generated from the benchmark output
  tree;
- keep raw benchmark outputs under `benchmarks/output/object-services/` and out
  of version control.
