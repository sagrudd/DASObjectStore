#!/usr/bin/env sh
set -eu

script_dir="$(dirname "$0")"
offline=0

case "${1:-}" in
  "")
    ;;
  --offline)
    offline=1
    ;;
  *)
    echo "usage: $0 [--offline]" >&2
    exit 64
    ;;
esac

. "$script_dir/matrix.sh"

failures=0

record_failure() {
  echo "$1" >&2
  failures=$((failures + 1))
}

require_command() {
  command_name="$1"
  if command -v "$command_name" >/dev/null 2>&1; then
    echo "ok command: $command_name"
  else
    record_failure "missing command: $command_name"
  fi
}

require_file() {
  path="$1"
  if [ -f "$path" ]; then
    echo "ok file: $path"
  else
    record_failure "missing file: $path"
  fi
}

require_executable() {
  path="$1"
  if [ -x "$path" ]; then
    echo "ok executable: $path"
  else
    record_failure "missing executable bit: $path"
  fi
}

require_file "benchmarks/ingest/README.md"
require_file "benchmarks/ingest/reports/report-template.md"
require_file "$script_dir/matrix.sh"
require_executable "$script_dir/run.sh"
require_executable "$script_dir/run-matrix.sh"
require_executable "$script_dir/smoke-test.sh"

dry_run_tmpdir="$(mktemp -d)"
cleanup() {
  rm -rf "$dry_run_tmpdir"
}
trap cleanup EXIT

for scenario in $scenarios; do
  if ! DASOBJECTSTORE_INGEST_BENCH_DRY_RUN=1 \
    DASOBJECTSTORE_INGEST_BENCH_OUTPUT_DIR="$dry_run_tmpdir" \
    "$script_dir/run.sh" "$scenario" >/dev/null; then
    record_failure "dry-run failed: scenario=$scenario"
  fi
done

if [ "$offline" -eq 0 ]; then
  require_command date
  require_command mkdir
  require_command mktemp
  require_command rm
else
  echo "offline preflight: external command checks skipped"
fi

if [ "$failures" -gt 0 ]; then
  echo "ingest benchmark preflight failed: $failures issue(s)" >&2
  exit 69
fi

echo "ingest benchmark preflight passed"
