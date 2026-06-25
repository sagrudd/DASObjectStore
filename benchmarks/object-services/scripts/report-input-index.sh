#!/usr/bin/env sh
set -eu

output_root="${DASOBJECTSTORE_BENCH_OUTPUT_DIR:-benchmarks/output/object-services}"
script_dir="$(dirname "$0")"

. "$script_dir/matrix.sh"

file_size() {
  wc -c < "$1" | tr -d ' '
}

echo "# Object Service Benchmark Input Index"
echo
echo "Output root: \`$output_root\`"
echo
echo "| Provider | Workload | Status | Bytes | Path |"
echo "| --- | --- | --- | ---: | --- |"

for provider in $providers; do
  for workload in $workloads; do
    report_path="$(expected_report_path "$output_root" "$provider" "$workload")"
    if [ -s "$report_path" ]; then
      status="present"
      bytes="$(file_size "$report_path")"
    else
      status="missing"
      bytes="0"
    fi
    echo "| $provider | $workload | $status | $bytes | \`$report_path\` |"
  done
done
