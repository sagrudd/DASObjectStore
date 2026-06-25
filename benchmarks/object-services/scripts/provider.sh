#!/usr/bin/env sh
set -eu

provider="${1:-}"
action="${2:-}"
script_dir="$(dirname "$0")"
dry_run="${DASOBJECTSTORE_BENCH_DRY_RUN:-0}"

. "$script_dir/../workloads/lib.sh"

case "$provider" in
  garage|rustfs) ;;
  *)
    echo "usage: $0 <garage|rustfs> <up|down|ps|logs>" >&2
    exit 64
    ;;
esac

case "$action" in
  up|down|ps|logs) ;;
  *)
    echo "usage: $0 <garage|rustfs> <up|down|ps|logs>" >&2
    exit 64
    ;;
esac

compose_file="$(provider_compose_file "$provider")"
service_name="$(provider_service_name "$provider")"

load_garage_env() {
  env_path="$("$script_dir/garage-credentials.sh" ensure)"
  set -a
  . "$env_path"
  set +a
}

garage_buckets() {
  printf '%s\n' \
    dasobjectstore-bench-large \
    dasobjectstore-bench-small \
    dasobjectstore-bench-concurrent \
    dasobjectstore-bench-crash-restart \
    dasobjectstore-bench-interrupted \
    dasobjectstore-bench-metadata-recovery \
    dasobjectstore-bench-disk-full \
    dasobjectstore-bench-disk-removal \
    dasobjectstore-bench-destage
}

wait_for_garage() {
  attempts=30
  while [ "$attempts" -gt 0 ]; do
    if docker_compose "$compose_file" exec -T "$service_name" /garage status >/dev/null 2>&1; then
      return
    fi
    attempts=$((attempts - 1))
    sleep 1
  done

  echo "Garage did not become ready for benchmark provisioning" >&2
  exit 69
}

provision_garage() {
  wait_for_garage
  garage_buckets | while read -r bucket; do
    docker_compose "$compose_file" exec -T "$service_name" /garage bucket create "$bucket" >/dev/null 2>&1 || true
    docker_compose "$compose_file" exec -T "$service_name" /garage bucket allow \
      --read \
      --write \
      --owner \
      "$bucket" \
      --key "$GARAGE_DEFAULT_ACCESS_KEY" >/dev/null
  done
}

if [ "$provider" = "garage" ]; then
  load_garage_env
fi

if [ "$dry_run" = "1" ]; then
  echo "provider=$provider"
  echo "action=$action"
  echo "compose_file=$compose_file"
  echo "service_name=$service_name"
  if [ "$provider" = "garage" ]; then
    echo "garage_env_path=$env_path"
    echo "garage_default_access_key=$GARAGE_DEFAULT_ACCESS_KEY"
    echo "garage_default_bucket=$GARAGE_DEFAULT_BUCKET"
  fi
  exit 0
fi

case "$action" in
  up)
    docker_compose "$compose_file" up -d "$service_name"
    if [ "$provider" = "garage" ]; then
      provision_garage
    fi
    ;;
  down)
    docker_compose "$compose_file" down
    ;;
  ps)
    docker_compose "$compose_file" ps
    ;;
  logs)
    docker_compose "$compose_file" logs "$service_name"
    ;;
esac
