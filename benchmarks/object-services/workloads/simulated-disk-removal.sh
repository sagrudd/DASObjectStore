#!/usr/bin/env sh
set -eu

provider="${1:?provider required}"
object_bytes="${DASOBJECTSTORE_REMOVAL_OBJECT_BYTES:-67108864}"
bucket="${DASOBJECTSTORE_BENCH_BUCKET:-dasobjectstore-bench-disk-removal}"
object_prefix="${DASOBJECTSTORE_BENCH_OBJECT_PREFIX:-disk-removal-object}"
restart_settle_seconds="${DASOBJECTSTORE_RESTART_SETTLE_SECONDS:-3}"
output_root="${DASOBJECTSTORE_BENCH_OUTPUT_DIR:-benchmarks/output/object-services}"
dry_run="${DASOBJECTSTORE_BENCH_DRY_RUN:-0}"

. "$(dirname "$0")/lib.sh"

require_positive_integer "object size" "$object_bytes"
require_positive_integer "restart settle seconds" "$restart_settle_seconds"
configure_provider_s3 "$provider" "simulated-disk-removal"

compose_file="$(provider_compose_file "$provider")"
service_name="$(provider_service_name "$provider")"
data_path="$(provider_data_path "$output_root" "$provider")"
workload_dir="$output_root/$provider/workloads/simulated-disk-removal"
removed_dir="$workload_dir/removed-data"
payload_path="$workload_dir/upload.bin"
removed_download_path="$workload_dir/removed-download.bin"
restored_download_path="$workload_dir/restored-download.bin"
report_path="$workload_dir/report.tsv"
object_key="$object_prefix.bin"
state_removed=0

cleanup_removed_data() {
  if [ "$state_removed" = "1" ]; then
    docker_compose "$compose_file" down >/dev/null 2>&1 || true
    safe_rm_rf_benchmark_path "$data_path"
    mv "$removed_dir/data" "$data_path"
    docker_compose "$compose_file" up -d "$service_name" >/dev/null 2>&1 || true
    state_removed=0
  fi
}

on_exit() {
  status="$?"
  cleanup_removed_data
  exit "$status"
}

trap on_exit EXIT

if [ "$dry_run" = "1" ]; then
  echo "provider=$provider"
  echo "endpoint=$endpoint"
  echo "compose_file=$compose_file"
  echo "service_name=$service_name"
  echo "data_path=$data_path"
  echo "removed_dir=$removed_dir"
  echo "bucket=$bucket"
  echo "object_bytes=$object_bytes"
  echo "payload_path=$payload_path"
  echo "report_path=$report_path"
  exit 0
fi

require_command "aws" "aws CLI is required for S3 benchmark workloads"
require_compose_command "Docker Compose is required for simulated disk removal benchmarks"

mkdir -p "$workload_dir"
ensure_sparse_file "$payload_path" "$object_bytes"
source_hash="$(hash_file "$payload_path")"

aws_s3 create-bucket --bucket "$bucket" >/dev/null 2>&1 || true
aws_s3 put-object --bucket "$bucket" --key "$object_key" --body "$payload_path" >/dev/null

safe_rm_rf_benchmark_path "$removed_dir"
docker_compose "$compose_file" down >/dev/null
mkdir -p "$removed_dir"
mv "$data_path" "$removed_dir/data"
state_removed=1
mkdir -p "$data_path"
docker_compose "$compose_file" up -d "$service_name" >/dev/null
sleep "$restart_settle_seconds"

removed_read_state="absent"
if aws_s3 get-object --bucket "$bucket" --key "$object_key" "$removed_download_path" >/dev/null 2>&1; then
  removed_hash="$(hash_file "$removed_download_path")"
  if [ "$source_hash" != "$removed_hash" ]; then
    echo "simulated disk removal returned corrupt object" >&2
    exit 65
  fi
  removed_read_state="complete"
fi

docker_compose "$compose_file" down >/dev/null
cleanup_removed_data
sleep "$restart_settle_seconds"

restore_start="$(start_epoch)"
aws_s3 get-object --bucket "$bucket" --key "$object_key" "$restored_download_path" >/dev/null
restore_end="$(start_epoch)"

restored_hash="$(hash_file "$restored_download_path")"
if [ "$source_hash" != "$restored_hash" ]; then
  echo "simulated disk removal restore returned checksum mismatch" >&2
  exit 65
fi

{
  echo "provider	bucket	object_key	bytes	removed_read_state	restore_seconds	sha256"
  echo "$provider	$bucket	$object_key	$object_bytes	$removed_read_state	$((restore_end - restore_start))	$source_hash"
} > "$report_path"

echo "$report_path"
