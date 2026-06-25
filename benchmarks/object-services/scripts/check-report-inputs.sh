#!/usr/bin/env sh
set -eu

output_root="${DASOBJECTSTORE_BENCH_OUTPUT_DIR:-benchmarks/output/object-services}"
script_dir="$(dirname "$0")"

. "$script_dir/matrix.sh"

missing=0

for provider in $providers; do
  for workload in $workloads; do
    report_path="$(expected_report_path "$output_root" "$provider" "$workload")"
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
