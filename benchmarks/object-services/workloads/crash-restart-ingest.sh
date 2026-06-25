#!/usr/bin/env sh
set -eu

provider="${1:?provider required}"
object_bytes="${DASOBJECTSTORE_CRASH_OBJECT_BYTES:-1073741824}"
bucket="${DASOBJECTSTORE_BENCH_BUCKET:-dasobjectstore-bench-crash-restart}"
object_prefix="${DASOBJECTSTORE_BENCH_OBJECT_PREFIX:-crash-restart-object}"
restart_delay_seconds="${DASOBJECTSTORE_RESTART_DELAY_SECONDS:-2}"
restart_settle_seconds="${DASOBJECTSTORE_RESTART_SETTLE_SECONDS:-3}"
output_root="${DASOBJECTSTORE_BENCH_OUTPUT_DIR:-benchmarks/output/object-services}"
dry_run="${DASOBJECTSTORE_BENCH_DRY_RUN:-0}"

. "$(dirname "$0")/lib.sh"

require_positive_integer "object size" "$object_bytes"
require_positive_integer "restart delay seconds" "$restart_delay_seconds"
require_positive_integer "restart settle seconds" "$restart_settle_seconds"
configure_provider_s3 "$provider" "crash-restart-ingest"

compose_file="$(provider_compose_file "$provider")"
service_name="$(provider_service_name "$provider")"
workload_dir="$output_root/$provider/workloads/crash-restart-ingest"
payload_path="$workload_dir/upload.bin"
interrupted_download_path="$workload_dir/interrupted-download.bin"
post_restart_download_path="$workload_dir/post-restart-download.bin"
report_path="$workload_dir/report.tsv"
upload_log="$workload_dir/interrupted-upload.log"
interrupted_key="$object_prefix-interrupted.bin"
post_restart_key="$object_prefix-post-restart.bin"

if [ "$dry_run" = "1" ]; then
  echo "provider=$provider"
  echo "endpoint=$endpoint"
  echo "compose_file=$compose_file"
  echo "service_name=$service_name"
  echo "bucket=$bucket"
  echo "object_bytes=$object_bytes"
  echo "restart_delay_seconds=$restart_delay_seconds"
  echo "restart_settle_seconds=$restart_settle_seconds"
  echo "payload_path=$payload_path"
  echo "report_path=$report_path"
  exit 0
fi

require_command "aws" "aws CLI is required for S3 benchmark workloads"
require_compose_command "Docker Compose is required for crash/restart benchmarks"

mkdir -p "$workload_dir"
ensure_sparse_file "$payload_path" "$object_bytes"
source_hash="$(hash_file "$payload_path")"

aws_s3 create-bucket --bucket "$bucket" >/dev/null 2>&1 || true

upload_start="$(start_epoch)"
aws_s3 put-object --bucket "$bucket" --key "$interrupted_key" --body "$payload_path" > "$upload_log" 2>&1 &
upload_pid="$!"

sleep "$restart_delay_seconds"
docker_compose "$compose_file" restart "$service_name" >/dev/null
sleep "$restart_settle_seconds"

if wait "$upload_pid"; then
  interrupted_upload_status=0
else
  interrupted_upload_status=$?
fi
upload_end="$(start_epoch)"

post_restart_start="$(start_epoch)"
aws_s3 put-object --bucket "$bucket" --key "$post_restart_key" --body "$payload_path" >/dev/null
aws_s3 get-object --bucket "$bucket" --key "$post_restart_key" "$post_restart_download_path" >/dev/null
post_restart_end="$(start_epoch)"

post_restart_hash="$(hash_file "$post_restart_download_path")"
if [ "$source_hash" != "$post_restart_hash" ]; then
  echo "post-restart checksum mismatch after download" >&2
  exit 65
fi

interrupted_verified="not_applicable"
if [ "$interrupted_upload_status" -eq 0 ]; then
  aws_s3 get-object --bucket "$bucket" --key "$interrupted_key" "$interrupted_download_path" >/dev/null
  interrupted_hash="$(hash_file "$interrupted_download_path")"
  if [ "$source_hash" != "$interrupted_hash" ]; then
    echo "interrupted upload completed but checksum verification failed" >&2
    exit 65
  fi
  interrupted_verified="true"
fi

{
  echo "provider	bucket	bytes	interrupted_upload_status	interrupted_verified	upload_seconds	post_restart_seconds	sha256"
  echo "$provider	$bucket	$object_bytes	$interrupted_upload_status	$interrupted_verified	$((upload_end - upload_start))	$((post_restart_end - post_restart_start))	$source_hash"
} > "$report_path"

echo "$report_path"
