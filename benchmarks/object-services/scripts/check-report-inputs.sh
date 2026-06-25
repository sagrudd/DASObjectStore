#!/usr/bin/env sh
set -eu

output_root="${DASOBJECTSTORE_BENCH_OUTPUT_DIR:-benchmarks/output/object-services}"

providers="garage rustfs"
workloads="large-object small-object concurrent-client crash-restart-ingest interrupted-write metadata-recovery disk-full simulated-disk-removal ssd-ingest-hdd-destage"

expected_report_path() {
  provider="$1"
  workload="$2"

  case "$workload" in
    concurrent-client)
      echo "$output_root/$provider/workloads/$workload/summary.tsv"
      ;;
    *)
      echo "$output_root/$provider/workloads/$workload/report.tsv"
      ;;
  esac
}

missing=0

for provider in $providers; do
  for workload in $workloads; do
    report_path="$(expected_report_path "$provider" "$workload")"
    if [ ! -s "$report_path" ]; then
      echo "missing benchmark report: $report_path" >&2
      missing=$((missing + 1))
    fi
  done
done

if [ "$missing" -gt 0 ]; then
  echo "benchmark report inputs are incomplete: $missing missing report(s)" >&2
  exit 66
fi

echo "benchmark report inputs are complete: $output_root"
