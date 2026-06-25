#!/usr/bin/env sh
set -eu

script_dir="$(dirname "$0")"
dry_run="${DASOBJECTSTORE_BENCH_DRY_RUN:-0}"

. "$script_dir/matrix.sh"

for provider in $providers; do
  for workload in $workloads; do
    echo "running benchmark workload: provider=$provider workload=$workload"
    "$script_dir/run.sh" "$provider" "$workload"
  done
done

if [ "$dry_run" = "1" ]; then
  echo "dry run complete; benchmark report readiness check skipped"
  exit 0
fi

"$script_dir/check-report-inputs.sh"
