#!/usr/bin/env sh
set -eu

provider="${1:?provider required}"
object_bytes="${DASOBJECTSTORE_METADATA_RECOVERY_OBJECT_BYTES:-67108864}"
bucket="${DASOBJECTSTORE_BENCH_BUCKET:-dasobjectstore-bench-metadata-recovery}"
object_prefix="${DASOBJECTSTORE_BENCH_OBJECT_PREFIX:-metadata-recovery-object}"
restart_settle_seconds="${DASOBJECTSTORE_RESTART_SETTLE_SECONDS:-3}"
output_root="${DASOBJECTSTORE_BENCH_OUTPUT_DIR:-benchmarks/output/object-services}"
dry_run="${DASOBJECTSTORE_BENCH_DRY_RUN:-0}"

. "$(dirname "$0")/lib.sh"

require_positive_integer "object size" "$object_bytes"
require_positive_integer "restart settle seconds" "$restart_settle_seconds"
configure_provider_s3 "$provider" "metadata-recovery"

compose_file="$(provider_compose_file "$provider")"
service_name="$(provider_service_name "$provider")"
workload_dir="$output_root/$provider/workloads/metadata-recovery"
snapshot_dir="$workload_dir/snapshot"
payload_path="$workload_dir/upload.bin"
download_before_path="$workload_dir/download-before.bin"
download_after_path="$workload_dir/download-after.bin"
report_path="$workload_dir/report.tsv"
object_key="$object_prefix.bin"

copy_state_to_snapshot() {
  safe_rm_rf_benchmark_path "$snapshot_dir"
  mkdir -p "$snapshot_dir"

  case "$provider" in
    garage)
      cp -R "$output_root/garage/meta" "$snapshot_dir/meta"
      cp -R "$output_root/garage/data" "$snapshot_dir/data"
      ;;
    rustfs)
      cp -R "$output_root/rustfs/data" "$snapshot_dir/data"
      ;;
  esac
}

restore_state_from_snapshot() {
  case "$provider" in
    garage)
      safe_rm_rf_benchmark_path "$output_root/garage/meta"
      safe_rm_rf_benchmark_path "$output_root/garage/data"
      cp -R "$snapshot_dir/meta" "$output_root/garage/meta"
      cp -R "$snapshot_dir/data" "$output_root/garage/data"
      ;;
    rustfs)
      safe_rm_rf_benchmark_path "$output_root/rustfs/data"
      cp -R "$snapshot_dir/data" "$output_root/rustfs/data"
      ;;
  esac
}

if [ "$dry_run" = "1" ]; then
  echo "provider=$provider"
  echo "endpoint=$endpoint"
  echo "compose_file=$compose_file"
  echo "service_name=$service_name"
  echo "bucket=$bucket"
  echo "object_bytes=$object_bytes"
  echo "snapshot_dir=$snapshot_dir"
  echo "payload_path=$payload_path"
  echo "report_path=$report_path"
  exit 0
fi

require_command "aws" "aws CLI is required for S3 benchmark workloads"
require_command "docker" "Docker Compose is required for metadata recovery benchmarks"

mkdir -p "$workload_dir"
ensure_sparse_file "$payload_path" "$object_bytes"
source_hash="$(hash_file "$payload_path")"

aws_s3 create-bucket --bucket "$bucket" >/dev/null 2>&1 || true
aws_s3 put-object --bucket "$bucket" --key "$object_key" --body "$payload_path" >/dev/null
aws_s3 get-object --bucket "$bucket" --key "$object_key" "$download_before_path" >/dev/null

before_hash="$(hash_file "$download_before_path")"
if [ "$source_hash" != "$before_hash" ]; then
  echo "metadata-recovery pre-snapshot checksum mismatch" >&2
  exit 65
fi

docker compose -f "$compose_file" down >/dev/null
copy_state_to_snapshot
restore_state_from_snapshot
docker compose -f "$compose_file" up -d "$service_name" >/dev/null
sleep "$restart_settle_seconds"

recovery_start="$(start_epoch)"
aws_s3 get-object --bucket "$bucket" --key "$object_key" "$download_after_path" >/dev/null
recovery_end="$(start_epoch)"

after_hash="$(hash_file "$download_after_path")"
if [ "$source_hash" != "$after_hash" ]; then
  echo "metadata-recovery post-restore checksum mismatch" >&2
  exit 65
fi

{
  echo "provider	bucket	object_key	bytes	recovery_seconds	sha256"
  echo "$provider	$bucket	$object_key	$object_bytes	$((recovery_end - recovery_start))	$source_hash"
} > "$report_path"

echo "$report_path"
