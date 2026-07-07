#!/usr/bin/env sh
set -eu

script_dir="$(dirname "$0")"

sh -n "$script_dir"/*.sh

"$script_dir/preflight.sh" --offline >/dev/null

tmpdir="$(mktemp -d)"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

DASOBJECTSTORE_INGEST_BENCH_OUTPUT_DIR="$tmpdir" \
  DASOBJECTSTORE_INGEST_BENCH_RUN_ID=smoke \
  "$script_dir/run-matrix.sh" >/dev/null

for scenario in small-file large-file mixed-file slow-hdd full-ssd interrupted-import; do
  metrics_path="$tmpdir/$scenario/smoke/metrics.tsv"
  scenario_path="$tmpdir/$scenario/smoke/scenario.tsv"
  if [ ! -f "$metrics_path" ] || [ ! -f "$scenario_path" ]; then
    echo "missing smoke output for scenario: $scenario" >&2
    exit 65
  fi
  grep -q 'bottleneck_classification' "$metrics_path"
  grep -q "scenario	$scenario" "$scenario_path"
done

grep -q 'interrupted-import' benchmarks/ingest/README.md
grep -qi 'supported terminal' docs/user/tui-operations.rst

echo "ingest benchmark smoke test passed"
