#!/usr/bin/env sh
set -eu

provider="${1:?provider required}"
object_bytes="${DASOBJECTSTORE_INTERRUPTED_OBJECT_BYTES:-1073741824}"
bucket="${DASOBJECTSTORE_BENCH_BUCKET:-dasobjectstore-bench-interrupted}"
object_prefix="${DASOBJECTSTORE_BENCH_OBJECT_PREFIX:-interrupted-object}"
interrupt_delay_seconds="${DASOBJECTSTORE_INTERRUPT_DELAY_SECONDS:-2}"
output_root="${DASOBJECTSTORE_BENCH_OUTPUT_DIR:-benchmarks/output/object-services}"
dry_run="${DASOBJECTSTORE_BENCH_DRY_RUN:-0}"

. "$(dirname "$0")/lib.sh"

require_positive_integer "object size" "$object_bytes"
require_positive_integer "interrupt delay seconds" "$interrupt_delay_seconds"
configure_provider_s3 "$provider" "interrupted-write"

workload_dir="$output_root/$provider/workloads/interrupted-write"
payload_path="$workload_dir/upload.bin"
interrupted_download_path="$workload_dir/interrupted-download.bin"
post_interrupt_download_path="$workload_dir/post-interrupt-download.bin"
report_path="$workload_dir/report.tsv"
upload_log="$workload_dir/interrupted-upload.log"
interrupted_key="$object_prefix-interrupted.bin"
post_interrupt_key="$object_prefix-post-interrupt.bin"

if [ "$dry_run" = "1" ]; then
  echo "provider=$provider"
  echo "endpoint=$endpoint"
  echo "bucket=$bucket"
  echo "object_bytes=$object_bytes"
  echo "interrupt_delay_seconds=$interrupt_delay_seconds"
  echo "payload_path=$payload_path"
  echo "report_path=$report_path"
  exit 0
fi

require_command "aws" "aws CLI is required for S3 benchmark workloads"

mkdir -p "$workload_dir"
ensure_sparse_file "$payload_path" "$object_bytes"
source_hash="$(hash_file "$payload_path")"

aws_s3 create-bucket --bucket "$bucket" >/dev/null 2>&1 || true

upload_start="$(start_epoch)"
aws_s3 put-object --bucket "$bucket" --key "$interrupted_key" --body "$payload_path" > "$upload_log" 2>&1 &
upload_pid="$!"

sleep "$interrupt_delay_seconds"
kill "$upload_pid" >/dev/null 2>&1 || true

if wait "$upload_pid"; then
  interrupted_upload_status=0
else
  interrupted_upload_status=$?
fi
upload_end="$(start_epoch)"

interrupted_object_state="absent"
if aws_s3 get-object --bucket "$bucket" --key "$interrupted_key" "$interrupted_download_path" >/dev/null 2>&1; then
  interrupted_hash="$(hash_file "$interrupted_download_path")"
  if [ "$source_hash" != "$interrupted_hash" ]; then
    echo "interrupted upload produced retrievable object with checksum mismatch" >&2
    exit 65
  fi
  interrupted_object_state="complete"
fi

post_interrupt_start="$(start_epoch)"
aws_s3 put-object --bucket "$bucket" --key "$post_interrupt_key" --body "$payload_path" >/dev/null
aws_s3 get-object --bucket "$bucket" --key "$post_interrupt_key" "$post_interrupt_download_path" >/dev/null
post_interrupt_end="$(start_epoch)"

post_interrupt_hash="$(hash_file "$post_interrupt_download_path")"
if [ "$source_hash" != "$post_interrupt_hash" ]; then
  echo "post-interruption checksum mismatch after download" >&2
  exit 65
fi

{
  echo "provider	bucket	bytes	interrupted_upload_status	interrupted_object_state	interrupted_seconds	post_interrupt_seconds	sha256"
  echo "$provider	$bucket	$object_bytes	$interrupted_upload_status	$interrupted_object_state	$((upload_end - upload_start))	$((post_interrupt_end - post_interrupt_start))	$source_hash"
} > "$report_path"

echo "$report_path"
