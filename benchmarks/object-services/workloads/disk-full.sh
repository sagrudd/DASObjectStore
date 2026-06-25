#!/usr/bin/env sh
set -eu

provider="${1:?provider required}"
object_bytes="${DASOBJECTSTORE_DISK_FULL_OBJECT_BYTES:-67108864}"
fill_bytes="${DASOBJECTSTORE_DISK_FULL_FILL_BYTES:-268435456}"
bucket="${DASOBJECTSTORE_BENCH_BUCKET:-dasobjectstore-bench-disk-full}"
object_prefix="${DASOBJECTSTORE_BENCH_OBJECT_PREFIX:-disk-full-object}"
output_root="${DASOBJECTSTORE_BENCH_OUTPUT_DIR:-benchmarks/output/object-services}"
dry_run="${DASOBJECTSTORE_BENCH_DRY_RUN:-0}"

. "$(dirname "$0")/lib.sh"

require_positive_integer "object size" "$object_bytes"
require_positive_integer "fill bytes" "$fill_bytes"
configure_provider_s3 "$provider" "disk-full"

workload_dir="$output_root/$provider/workloads/disk-full"
payload_path="$workload_dir/upload.bin"
download_path="$workload_dir/download.bin"
filler_path="$workload_dir/filler.bin"
report_path="$workload_dir/report.tsv"
upload_log="$workload_dir/upload.log"
object_key="$object_prefix.bin"

if [ "$dry_run" = "1" ]; then
  echo "provider=$provider"
  echo "endpoint=$endpoint"
  echo "bucket=$bucket"
  echo "object_bytes=$object_bytes"
  echo "fill_bytes=$fill_bytes"
  echo "filler_path=$filler_path"
  echo "payload_path=$payload_path"
  echo "report_path=$report_path"
  exit 0
fi

require_command "aws" "aws CLI is required for S3 benchmark workloads"

mkdir -p "$workload_dir"
ensure_sparse_file "$payload_path" "$object_bytes"
ensure_allocated_file "$filler_path" "$fill_bytes"
source_hash="$(hash_file "$payload_path")"

aws_s3 create-bucket --bucket "$bucket" >/dev/null 2>&1 || true

write_status="accepted"
verify_status="not_run"
upload_start="$(start_epoch)"
if aws_s3 put-object --bucket "$bucket" --key "$object_key" --body "$payload_path" > "$upload_log" 2>&1; then
  upload_end="$(start_epoch)"
  if aws_s3 get-object --bucket "$bucket" --key "$object_key" "$download_path" >/dev/null 2>&1; then
    download_hash="$(hash_file "$download_path")"
    if [ "$source_hash" = "$download_hash" ]; then
      verify_status="verified"
    else
      verify_status="checksum_mismatch"
    fi
  else
    verify_status="download_failed"
  fi
else
  upload_end="$(start_epoch)"
  write_status="rejected"
fi

if [ "$verify_status" = "checksum_mismatch" ]; then
  echo "disk-full write returned corrupt object" >&2
  exit 65
fi

{
  echo "provider	bucket	object_key	object_bytes	fill_bytes	write_status	verify_status	write_seconds	sha256"
  echo "$provider	$bucket	$object_key	$object_bytes	$fill_bytes	$write_status	$verify_status	$((upload_end - upload_start))	$source_hash"
} > "$report_path"

echo "$report_path"
