#!/usr/bin/env sh
set -eu

scenario="${1:-}"
script_dir="$(dirname "$0")"

. "$script_dir/matrix.sh"

if [ -z "$scenario" ]; then
  echo "usage: $0 <scenario>" >&2
  exit 64
fi

if ! is_supported_scenario "$scenario"; then
  echo "unsupported ingest benchmark scenario: $scenario" >&2
  exit 64
fi

kind="$(scenario_kind "$scenario")"
file_count="$(scenario_file_count "$scenario")"
total_bytes="$(scenario_total_bytes "$scenario")"
pressure="$(scenario_pressure "$scenario")"
output_root="${DASOBJECTSTORE_INGEST_BENCH_OUTPUT_DIR:-benchmarks/output/ingest}"
run_id="${DASOBJECTSTORE_INGEST_BENCH_RUN_ID:-$(date -u +%Y%m%dT%H%M%SZ)}"
run_dir="$output_root/$scenario/$run_id"

if [ "${DASOBJECTSTORE_INGEST_BENCH_DRY_RUN:-0}" = "1" ]; then
  printf 'scenario=%s kind=%s file_count=%s total_bytes=%s pressure=%s output=%s\n' \
    "$scenario" "$kind" "$file_count" "$total_bytes" "$pressure" "$run_dir"
  exit 0
fi

mkdir -p "$run_dir"

{
  printf 'field\tvalue\n'
  printf 'scenario\t%s\n' "$scenario"
  printf 'kind\t%s\n' "$kind"
  printf 'planned_file_count\t%s\n' "$file_count"
  printf 'planned_total_bytes\t%s\n' "$total_bytes"
  printf 'pressure_target\t%s\n' "$pressure"
  printf 'runner_command\t%s\n' "${DASOBJECTSTORE_INGEST_BENCH_COMMAND:-not_configured}"
} > "$run_dir/scenario.tsv"

{
  printf 'metric\tvalue\n'
  printf 'scenario\t%s\n' "$scenario"
  printf 'result\tscaffold\n'
  printf 'bottleneck_classification\tnot_collected\n'
  printf 'cpu_total_percent\tnot_collected\n'
  printf 'cpu_hash_percent\tnot_collected\n'
  printf 'cpu_verify_percent\tnot_collected\n'
  printf 'memory_rss_peak_bytes\tnot_collected\n'
  printf 'memory_budget_bytes\tnot_collected\n'
  printf 'memory_growth_class\tnot_collected\n'
  printf 'ssd_stage_bytes_per_second\tnot_collected\n'
  printf 'ssd_pressure_state\tnot_collected\n'
  printf 'ssd_throttle_state\tnot_collected\n'
  printf 'hdd_fanout_bytes_per_second\tnot_collected\n'
  printf 'hdd_backlog_bytes\tnot_collected\n'
  printf 'verification_bytes_per_second\tnot_collected\n'
  printf 'verification_failures\tnot_collected\n'
  printf 'recovery_seconds\tnot_applicable\n'
} > "$run_dir/metrics.tsv"

if [ -n "${DASOBJECTSTORE_INGEST_BENCH_COMMAND:-}" ]; then
  export DASOBJECTSTORE_INGEST_BENCH_SCENARIO="$scenario"
  export DASOBJECTSTORE_INGEST_BENCH_KIND="$kind"
  export DASOBJECTSTORE_INGEST_BENCH_FILE_COUNT="$file_count"
  export DASOBJECTSTORE_INGEST_BENCH_TOTAL_BYTES="$total_bytes"
  export DASOBJECTSTORE_INGEST_BENCH_PRESSURE="$pressure"
  export DASOBJECTSTORE_INGEST_BENCH_RUN_DIR="$run_dir"

  started_at="$(date -u +%s)"
  if sh -c "$DASOBJECTSTORE_INGEST_BENCH_COMMAND"; then
    runner_status=0
  else
    runner_status="$?"
  fi
  ended_at="$(date -u +%s)"
  wall_seconds=$((ended_at - started_at))

  {
    printf 'runner_exit_code\t%s\n' "$runner_status"
    printf 'runner_wall_seconds\t%s\n' "$wall_seconds"
  } >> "$run_dir/metrics.tsv"

  exit "$runner_status"
fi

printf 'wrote ingest benchmark scaffold: %s\n' "$run_dir"
