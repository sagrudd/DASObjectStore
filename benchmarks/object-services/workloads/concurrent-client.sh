#!/usr/bin/env sh
set -eu

provider="${1:?provider required}"
client_count="${DASOBJECTSTORE_CONCURRENT_CLIENTS:-4}"
object_bytes="${DASOBJECTSTORE_CONCURRENT_OBJECT_BYTES:-65536}"
object_count="${DASOBJECTSTORE_CONCURRENT_OBJECT_COUNT:-25}"
bucket="${DASOBJECTSTORE_BENCH_BUCKET:-dasobjectstore-bench-concurrent}"
object_prefix="${DASOBJECTSTORE_BENCH_OBJECT_PREFIX:-concurrent-object}"
output_root="${DASOBJECTSTORE_BENCH_OUTPUT_DIR:-benchmarks/output/object-services}"
dry_run="${DASOBJECTSTORE_BENCH_DRY_RUN:-0}"

. "$(dirname "$0")/lib.sh"

require_positive_integer "client count" "$client_count"
require_positive_integer "object size" "$object_bytes"
require_positive_integer "object count" "$object_count"

case "$provider" in
  garage|rustfs) ;;
  *)
    echo "unsupported provider for concurrent-client workload: $provider" >&2
    exit 64
    ;;
esac

workload_dir="$output_root/$provider/workloads/concurrent-client"
summary_path="$workload_dir/summary.tsv"

if [ "$dry_run" = "1" ]; then
  echo "provider=$provider"
  echo "clients=$client_count"
  echo "object_bytes=$object_bytes"
  echo "object_count_per_client=$object_count"
  echo "bucket=$bucket"
  echo "object_prefix=$object_prefix"
  echo "summary_path=$summary_path"
  exit 0
fi

mkdir -p "$workload_dir/logs"
rm -f "$workload_dir/pids.tsv"

index=1
while [ "$index" -le "$client_count" ]; do
  client_id="$(printf 'client-%04d' "$index")"
  client_prefix="$object_prefix-$client_id"
  client_workload="concurrent-client/$client_id"
  client_log="$workload_dir/logs/$client_id.log"

  "$(dirname "$0")/s3-roundtrip.sh" "$provider" "$client_workload" "$object_bytes" "$object_count" "$bucket" "$client_prefix" > "$client_log" 2>&1 &
  echo "$index $! $client_id $client_log" >> "$workload_dir/pids.tsv"
  index=$((index + 1))
done

status=0
while read -r _ pid client_id client_log; do
  if wait "$pid"; then
    :
  else
    status=1
    echo "concurrent-client failed: $client_id; see $client_log" >&2
  fi
done < "$workload_dir/pids.tsv"

rm -f "$workload_dir/pids.tsv"

if [ "$status" -ne 0 ]; then
  exit 65
fi

{
  echo "provider	clients	object_count_per_client	object_bytes"
  echo "$provider	$client_count	$object_count	$object_bytes"
} > "$summary_path"

echo "$summary_path"
