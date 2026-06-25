#!/usr/bin/env sh
set -eu

provider="${1:?provider required}"
object_bytes="${DASOBJECTSTORE_DESTAGE_OBJECT_BYTES:-67108864}"
bucket="${DASOBJECTSTORE_BENCH_BUCKET:-dasobjectstore-bench-destage}"
object_prefix="${DASOBJECTSTORE_BENCH_OBJECT_PREFIX:-destage-object}"
ssd_ingest_path="${DASOBJECTSTORE_SSD_INGEST_PATH:-/tmp/dasobjectstore-bench/ssd}"
hdd_root_path="${DASOBJECTSTORE_HDD_ROOT_PATH:-/tmp/dasobjectstore-bench/hdd}"
output_root="${DASOBJECTSTORE_BENCH_OUTPUT_DIR:-benchmarks/output/object-services}"
dry_run="${DASOBJECTSTORE_BENCH_DRY_RUN:-0}"

. "$(dirname "$0")/lib.sh"

require_positive_integer "object size" "$object_bytes"
configure_provider_s3 "$provider" "ssd-ingest-hdd-destage"

workload_dir="$output_root/$provider/workloads/ssd-ingest-hdd-destage"
ssd_workload_dir="$ssd_ingest_path/$provider/ssd-ingest-hdd-destage"
hdd_object_dir="$hdd_root_path/$provider/settled/$bucket"
ssd_payload_path="$ssd_workload_dir/upload.bin"
hdd_object_path="$hdd_object_dir/$object_prefix.bin"
report_path="$workload_dir/report.tsv"
object_key="$object_prefix.bin"

if [ "$dry_run" = "1" ]; then
  echo "provider=$provider"
  echo "endpoint=$endpoint"
  echo "bucket=$bucket"
  echo "object_bytes=$object_bytes"
  echo "ssd_payload_path=$ssd_payload_path"
  echo "hdd_object_path=$hdd_object_path"
  echo "report_path=$report_path"
  exit 0
fi

require_s3_cli

mkdir -p "$workload_dir" "$ssd_workload_dir" "$hdd_object_dir"
ensure_sparse_file "$ssd_payload_path" "$object_bytes"
source_hash="$(hash_file "$ssd_payload_path")"

ingest_start="$(start_epoch)"
aws_s3 create-bucket --bucket "$bucket" >/dev/null 2>&1 || true
aws_s3 put-object --bucket "$bucket" --key "$object_key" --body "$ssd_payload_path" >/dev/null
ingest_end="$(start_epoch)"

destage_start="$(start_epoch)"
aws_s3 get-object --bucket "$bucket" --key "$object_key" "$hdd_object_path" >/dev/null
destage_end="$(start_epoch)"

destage_hash="$(hash_file "$hdd_object_path")"
if [ "$source_hash" != "$destage_hash" ]; then
  echo "SSD ingest to HDD destage checksum mismatch" >&2
  exit 65
fi

{
  echo "provider	bucket	object_key	bytes	ssd_payload_path	hdd_object_path	ingest_seconds	destage_seconds	sha256"
  echo "$provider	$bucket	$object_key	$object_bytes	$ssd_payload_path	$hdd_object_path	$((ingest_end - ingest_start))	$((destage_end - destage_start))	$source_hash"
} > "$report_path"

echo "$report_path"
