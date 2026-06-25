#!/usr/bin/env sh
set -eu

provider="${1:-}"
workload="${2:-}"

if [ -z "$provider" ] || [ -z "$workload" ]; then
  echo "usage: $0 <provider> <workload>" >&2
  exit 64
fi

case "$provider" in
  garage|rustfs) ;;
  *)
    echo "unsupported provider: $provider" >&2
    exit 64
    ;;
esac

case "$workload" in
  large-object|small-object|concurrent-client|crash-restart-ingest|interrupted-write|metadata-recovery|disk-full|simulated-disk-removal|ssd-ingest-hdd-destage) ;;
  *)
    echo "unsupported workload: $workload" >&2
    exit 64
    ;;
esac

case "$workload" in
  large-object)
    exec "$(dirname "$0")/../workloads/large-object.sh" "$provider"
    ;;
  *)
    echo "workload is not implemented yet: $provider / $workload" >&2
    exit 69
    ;;
esac
