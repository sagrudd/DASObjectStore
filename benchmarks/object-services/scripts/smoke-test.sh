#!/usr/bin/env sh
set -eu

script_dir="$(dirname "$0")"

sh -n "$script_dir"/*.sh "$script_dir"/../workloads/*.sh

provider_tmpdir="$(mktemp -d)"

"$script_dir/preflight.sh" --offline >/dev/null
DASOBJECTSTORE_BENCH_DRY_RUN=1 \
  DASOBJECTSTORE_BENCH_OUTPUT_DIR="$provider_tmpdir" \
  "$script_dir/run-matrix.sh" >/dev/null

DASOBJECTSTORE_BENCH_DRY_RUN=1 \
  DASOBJECTSTORE_BENCH_OUTPUT_DIR="$provider_tmpdir" \
  "$script_dir/provider.sh" garage up | grep -q "benchmark_uid=$(id -u)"
DASOBJECTSTORE_BENCH_DRY_RUN=1 \
  DASOBJECTSTORE_BENCH_OUTPUT_DIR="$provider_tmpdir" \
  "$script_dir/provider.sh" rustfs up | grep -q "output_root=$provider_tmpdir"

empty_tmpdir="$(mktemp -d)"
cleanup_empty() {
  rm -rf "$provider_tmpdir" "$empty_tmpdir"
}
trap cleanup_empty EXIT

missing_count="$(DASOBJECTSTORE_BENCH_OUTPUT_DIR="$empty_tmpdir" "$script_dir/report-input-index.sh" | grep -c '| missing |')"
if [ "$missing_count" -ne 18 ]; then
  echo "expected 18 missing reports in empty fixture tree, found $missing_count" >&2
  exit 65
fi

tmpdir="$(mktemp -d)"
cleanup() {
  rm -rf "$provider_tmpdir" "$empty_tmpdir" "$tmpdir"
}
trap cleanup EXIT

for provider in garage rustfs; do
  for workload in large-object small-object crash-restart-ingest interrupted-write metadata-recovery disk-full simulated-disk-removal ssd-ingest-hdd-destage; do
    mkdir -p "$tmpdir/$provider/workloads/$workload"
    printf 'header\nrow\n' > "$tmpdir/$provider/workloads/$workload/report.tsv"
  done
  mkdir -p "$tmpdir/$provider/workloads/concurrent-client"
  printf 'header\nrow\n' > "$tmpdir/$provider/workloads/concurrent-client/summary.tsv"
done

DASOBJECTSTORE_BENCH_OUTPUT_DIR="$tmpdir" "$script_dir/check-report-inputs.sh" >/dev/null
present_count="$(DASOBJECTSTORE_BENCH_OUTPUT_DIR="$tmpdir" "$script_dir/report-input-index.sh" | grep -c '| present |')"
if [ "$present_count" -ne 18 ]; then
  echo "expected 18 present fixture reports, found $present_count" >&2
  exit 65
fi

DASOBJECTSTORE_BENCHMARK_DATE=2026-06-25 "$script_dir/draft-report.sh" | grep -q '## Raw Input Inventory'
"$script_dir/environment-snapshot.sh" | grep -q '| Host OS |'

fake_bin="$tmpdir/fake-bin"
mkdir -p "$fake_bin"
cat > "$fake_bin/docker" <<'FAKE_DOCKER'
#!/usr/bin/env sh
if [ "$1" = "info" ]; then
  exit 0
fi
if [ "$1" = "compose" ] && [ "$2" = "version" ]; then
  sleep 5
  exit 0
fi
exit 0
FAKE_DOCKER
chmod +x "$fake_bin/docker"
cat > "$fake_bin/docker-compose" <<'FAKE_DOCKER_COMPOSE'
#!/usr/bin/env sh
if [ "$1" = "version" ]; then
  sleep 5
  exit 0
fi
exit 0
FAKE_DOCKER_COMPOSE
chmod +x "$fake_bin/docker-compose"

if PATH="$fake_bin:$PATH" \
  DASOBJECTSTORE_BENCH_DOCKER_CHECK_TIMEOUT_SECONDS=1 \
  DASOBJECTSTORE_BENCH_COMPOSE_CHECK_TIMEOUT_SECONDS=1 \
  "$script_dir/preflight.sh" >/dev/null 2>"$tmpdir/preflight-timeout.err"; then
  echo "expected preflight to fail when docker compose version times out" >&2
  exit 65
fi
grep -q 'missing command: docker compose or docker-compose' "$tmpdir/preflight-timeout.err"

echo "benchmark smoke test passed"
