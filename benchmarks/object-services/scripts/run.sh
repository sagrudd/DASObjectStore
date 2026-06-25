#!/usr/bin/env sh
set -eu

provider="${1:-}"
workload="${2:-}"
script_dir="$(dirname "$0")"

. "$script_dir/matrix.sh"

if [ -z "$provider" ] || [ -z "$workload" ]; then
  echo "usage: $0 <provider> <workload>" >&2
  exit 64
fi

if ! is_supported_provider "$provider"; then
  echo "unsupported provider: $provider" >&2
  exit 64
fi

if ! is_supported_workload "$workload"; then
  echo "unsupported workload: $workload" >&2
  exit 64
fi

case "$workload" in
  large-object)
    exec "$script_dir/../workloads/large-object.sh" "$provider"
    ;;
  small-object)
    exec "$script_dir/../workloads/small-object.sh" "$provider"
    ;;
  concurrent-client)
    exec "$script_dir/../workloads/concurrent-client.sh" "$provider"
    ;;
  crash-restart-ingest)
    exec "$script_dir/../workloads/crash-restart-ingest.sh" "$provider"
    ;;
  interrupted-write)
    exec "$script_dir/../workloads/interrupted-write.sh" "$provider"
    ;;
  metadata-recovery)
    exec "$script_dir/../workloads/metadata-recovery.sh" "$provider"
    ;;
  disk-full)
    exec "$script_dir/../workloads/disk-full.sh" "$provider"
    ;;
  simulated-disk-removal)
    exec "$script_dir/../workloads/simulated-disk-removal.sh" "$provider"
    ;;
  ssd-ingest-hdd-destage)
    exec "$script_dir/../workloads/ssd-ingest-hdd-destage.sh" "$provider"
    ;;
  *)
    echo "workload is not implemented yet: $provider / $workload" >&2
    exit 69
    ;;
esac
