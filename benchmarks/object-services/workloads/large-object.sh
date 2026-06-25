#!/usr/bin/env sh
set -eu

provider="${1:?provider required}"

output_root="${DASOBJECTSTORE_BENCH_OUTPUT_DIR:-benchmarks/output/object-services}"
object_bytes="${DASOBJECTSTORE_LARGE_OBJECT_BYTES:-1073741824}"
bucket="${DASOBJECTSTORE_BENCH_BUCKET:-dasobjectstore-bench-large}"
object_key="${DASOBJECTSTORE_BENCH_OBJECT_KEY:-large-object.bin}"
dry_run="${DASOBJECTSTORE_BENCH_DRY_RUN:-0}"

case "$object_bytes" in
  ''|*[!0-9]*)
    echo "DASOBJECTSTORE_LARGE_OBJECT_BYTES must be a positive integer" >&2
    exit 64
    ;;
esac

if [ "$object_bytes" -eq 0 ]; then
  echo "DASOBJECTSTORE_LARGE_OBJECT_BYTES must be greater than zero" >&2
  exit 64
fi

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
    echo "unsupported provider for large-object workload: $provider" >&2
    exit 64
    ;;
esac

export AWS_DEFAULT_REGION="$region"
payload_dir="$output_root/$provider/workloads/large-object"
payload_path="$payload_dir/upload.bin"
download_path="$payload_dir/download.bin"
report_path="$payload_dir/report.tsv"

if [ "$dry_run" = "1" ]; then
  echo "provider=$provider"
  echo "endpoint=$endpoint"
  echo "bucket=$bucket"
  echo "object_key=$object_key"
  echo "object_bytes=$object_bytes"
  echo "payload_path=$payload_path"
  echo "download_path=$download_path"
  echo "report_path=$report_path"
  exit 0
fi

if ! command -v aws >/dev/null 2>&1; then
  echo "aws CLI is required for S3 benchmark workloads" >&2
  exit 69
fi

mkdir -p "$payload_dir"

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

aws_s3 create-bucket --bucket "$bucket" >/dev/null 2>&1 || true

upload_start="$(start_epoch)"
aws_s3 put-object --bucket "$bucket" --key "$object_key" --body "$payload_path" >/dev/null
upload_end="$(start_epoch)"

download_start="$(start_epoch)"
aws_s3 get-object --bucket "$bucket" --key "$object_key" "$download_path" >/dev/null
download_end="$(start_epoch)"

source_hash="$(hash_file "$payload_path")"
download_hash="$(hash_file "$download_path")"

if [ "$source_hash" != "$download_hash" ]; then
  echo "large-object checksum mismatch after download" >&2
  exit 65
fi

{
  echo "provider	bucket	object_key	bytes	upload_seconds	download_seconds	sha256"
  echo "$provider	$bucket	$object_key	$object_bytes	$((upload_end - upload_start))	$((download_end - download_start))	$source_hash"
} > "$report_path"

echo "$report_path"
