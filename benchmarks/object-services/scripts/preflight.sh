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

require_hash_command() {
  if command -v sha256sum >/dev/null 2>&1; then
    echo "ok command: sha256sum"
  elif command -v shasum >/dev/null 2>&1; then
    echo "ok command: shasum"
  else
    record_failure "missing command: sha256sum or shasum"
  fi
}

require_compose_command() {
  timeout_seconds="${DASOBJECTSTORE_BENCH_COMPOSE_CHECK_TIMEOUT_SECONDS:-15}"
  if command -v docker >/dev/null 2>&1 && command_finishes_within "$timeout_seconds" docker compose version; then
    echo "ok command: docker compose"
  elif command -v docker-compose >/dev/null 2>&1 \
    && command_finishes_within "$timeout_seconds" docker-compose version; then
    echo "ok command: docker-compose"
  else
    record_failure "missing command: docker compose or docker-compose"
  fi
}

require_s3_cli() {
  if command -v aws >/dev/null 2>&1; then
    echo "ok command: aws"
  elif command -v docker >/dev/null 2>&1; then
    echo "ok command: docker for containerized AWS CLI"
  else
    record_failure "missing command: aws or docker for containerized AWS CLI"
  fi
}

command_finishes_within() {
  seconds="$1"
  shift

  "$@" >/dev/null 2>&1 &
  command_pid="$!"

  (
    sleep "$seconds"
    kill "$command_pid" >/dev/null 2>&1 || true
  ) &
  timer_pid="$!"

  if wait "$command_pid"; then
    status=0
  else
    status="$?"
  fi

  kill "$timer_pid" >/dev/null 2>&1 || true
  wait "$timer_pid" >/dev/null 2>&1 || true

  return "$status"
}

require_docker_daemon() {
  if ! command -v docker >/dev/null 2>&1; then
    return
  fi

  timeout_seconds="${DASOBJECTSTORE_BENCH_DOCKER_CHECK_TIMEOUT_SECONDS:-15}"
  if command_finishes_within "$timeout_seconds" docker info; then
    echo "ok command: docker daemon"
  else
    record_failure "docker daemon did not respond within ${timeout_seconds}s"
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

require_executable "$script_dir/run.sh"
require_executable "$script_dir/run-matrix.sh"
require_executable "$script_dir/check-report-inputs.sh"
require_executable "$script_dir/garage-credentials.sh"
require_executable "$script_dir/provider.sh"
require_file "$script_dir/matrix.sh"

dry_run_tmpdir="$(mktemp -d)"
cleanup() {
  rm -rf "$dry_run_tmpdir"
}
trap cleanup EXIT

for provider in $providers; do
  require_file "benchmarks/object-services/providers/$provider/compose.yml"
done

for provider in $providers; do
  for workload in $workloads; do
    if ! DASOBJECTSTORE_BENCH_DRY_RUN=1 \
      DASOBJECTSTORE_BENCH_OUTPUT_DIR="$dry_run_tmpdir" \
      "$script_dir/run.sh" "$provider" "$workload" >/dev/null; then
      record_failure "dry-run failed: provider=$provider workload=$workload"
    fi
  done
done

if [ "$offline" -eq 0 ]; then
  require_s3_cli
  require_docker_daemon
  require_command awk
  require_command cp
  require_command date
  require_command dd
  require_command mkdir
  require_command mv
  require_command rm
  require_command wc
  require_hash_command
  require_compose_command
else
  echo "offline preflight: external command checks skipped"
fi

if [ "$failures" -gt 0 ]; then
  echo "benchmark preflight failed: $failures issue(s)" >&2
  exit 69
fi

echo "benchmark preflight passed"
