#!/usr/bin/env sh
set -eu

provider="${1:?provider required}"
workload="${2:?workload required}"
object_bytes="${3:?object size required}"
object_count="${4:?object count required}"
bucket="${5:?bucket required}"
object_prefix="${6:?object prefix required}"

output_root="${DASOBJECTSTORE_BENCH_OUTPUT_DIR:-benchmarks/output/object-services}"
dry_run="${DASOBJECTSTORE_BENCH_DRY_RUN:-0}"

. "$(dirname "$0")/lib.sh"

require_positive_integer "object size" "$object_bytes"
require_positive_integer "object count" "$object_count"
configure_provider_s3 "$provider" "$workload"
payload_dir="$output_root/$provider/workloads/$workload"
payload_path="$payload_dir/upload.bin"
download_dir="$payload_dir/downloads"
report_path="$payload_dir/report.tsv"

if [ "$dry_run" = "1" ]; then
  echo "provider=$provider"
  echo "endpoint=$endpoint"
  echo "bucket=$bucket"
  echo "object_prefix=$object_prefix"
  echo "object_bytes=$object_bytes"
  echo "object_count=$object_count"
  echo "payload_path=$payload_path"
  echo "download_dir=$download_dir"
  echo "report_path=$report_path"
  exit 0
fi

require_command "aws" "aws CLI is required for S3 benchmark workloads"

mkdir -p "$payload_dir" "$download_dir"
ensure_sparse_file "$payload_path" "$object_bytes"

object_key() {
  index="$1"
  if [ "$object_count" -eq 1 ]; then
    printf '%s.bin\n' "$object_prefix"
  else
    printf '%s-%06d.bin\n' "$object_prefix" "$index"
  fi
}

aws_s3 create-bucket --bucket "$bucket" >/dev/null 2>&1 || true
source_hash="$(hash_file "$payload_path")"

{
  echo "provider	bucket	object_key	bytes	upload_seconds	download_seconds	sha256"
} > "$report_path"

index=1
while [ "$index" -le "$object_count" ]; do
  key="$(object_key "$index")"
  download_path="$download_dir/$key"

  upload_start="$(start_epoch)"
  aws_s3 put-object --bucket "$bucket" --key "$key" --body "$payload_path" >/dev/null
  upload_end="$(start_epoch)"

  download_start="$(start_epoch)"
  aws_s3 get-object --bucket "$bucket" --key "$key" "$download_path" >/dev/null
  download_end="$(start_epoch)"

  download_hash="$(hash_file "$download_path")"
  if [ "$source_hash" != "$download_hash" ]; then
    echo "$workload checksum mismatch after download: $key" >&2
    exit 65
  fi

  echo "$provider	$bucket	$key	$object_bytes	$((upload_end - upload_start))	$((download_end - download_start))	$source_hash" >> "$report_path"
  index=$((index + 1))
done

echo "$report_path"
