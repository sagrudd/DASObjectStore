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

require_positive_integer() {
  name="$1"
  value="$2"

  case "$value" in
    ''|*[!0-9]*)
      echo "$name must be a positive integer" >&2
      exit 64
      ;;
  esac

  if [ "$value" -eq 0 ]; then
    echo "$name must be greater than zero" >&2
    exit 64
  fi
}

require_positive_integer "object size" "$object_bytes"
require_positive_integer "object count" "$object_count"

case "$provider" in
  garage)
    endpoint="${DASOBJECTSTORE_S3_ENDPOINT:-http://127.0.0.1:3900}"
    region="${AWS_DEFAULT_REGION:-garage}"
    ;;
  rustfs)
    endpoint="${DASOBJECTSTORE_S3_ENDPOINT:-http://127.0.0.1:9000}"
    region="${AWS_DEFAULT_REGION:-us-east-1}"
    export AWS_ACCESS_KEY_ID="${AWS_ACCESS_KEY_ID:-rustfsadmin}"
    export AWS_SECRET_ACCESS_KEY="${AWS_SECRET_ACCESS_KEY:-rustfsadmin}"
    ;;
  *)
    echo "unsupported provider for $workload workload: $provider" >&2
    exit 64
    ;;
esac

export AWS_DEFAULT_REGION="$region"
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

if ! command -v aws >/dev/null 2>&1; then
  echo "aws CLI is required for S3 benchmark workloads" >&2
  exit 69
fi

mkdir -p "$payload_dir" "$download_dir"

file_size() {
  wc -c < "$1" | tr -d ' '
}

if [ ! -f "$payload_path" ] || [ "$(file_size "$payload_path")" != "$object_bytes" ]; then
  rm -f "$payload_path"
  dd if=/dev/zero of="$payload_path" bs=1 count=0 seek="$object_bytes" 2>/dev/null
fi

hash_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

start_epoch() {
  date +%s
}

aws_s3() {
  aws --endpoint-url "$endpoint" s3api "$@"
}

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
