# Reports

Checked-in files in this directory should be report templates or final
benchmark reports. Raw benchmark output belongs under
`benchmarks/output/object-services/`.

Reports must separate hard-gate reliability results from performance results.

- `scoring-rubric.md`: reliability hard gates and performance scoring rules for
  provider comparison.
- `report-template.md`: fill-in template for comparable Garage and RustFS
  benchmark reports.
- `runbook.md`: ordered procedure for producing the complete workload set and
  selection report inputs.

Before producing a provider selection report, run:

```sh
benchmarks/object-services/scripts/check-report-inputs.sh
```

The check fails until each required Garage and RustFS workload has generated its
expected TSV report under `benchmarks/output/object-services/`.

To generate a Markdown inventory of raw report inputs for the selection report,
run:

```sh
benchmarks/object-services/scripts/report-input-index.sh
```

To generate a draft report from the template plus that input inventory, run:

```sh
benchmarks/object-services/scripts/draft-report.sh
```
