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

require_supported_provider() {
  provider="$1"
  workload="$2"

  case "$provider" in
    garage|rustfs) ;;
    *)
      echo "unsupported provider for $workload workload: $provider" >&2
      exit 64
      ;;
  esac
}

require_command() {
  command_name="$1"
  message="$2"

  if ! command -v "$command_name" >/dev/null 2>&1; then
    echo "$message" >&2
    exit 69
  fi
}

require_s3_cli() {
  if command -v aws >/dev/null 2>&1; then
    return
  fi
  if command -v docker >/dev/null 2>&1; then
    return
  fi

  echo "AWS CLI or Docker is required for S3 benchmark workloads" >&2
  exit 69
}

load_garage_credentials() {
  garage_env_path="$(benchmarks/object-services/scripts/garage-credentials.sh ensure)"
  set -a
  . "$garage_env_path"
  set +a
}

require_compose_command() {
  message="$1"

  if command -v docker >/dev/null 2>&1 && docker compose version >/dev/null 2>&1; then
    return
  fi
  if command -v docker-compose >/dev/null 2>&1; then
    return
  fi

  echo "$message" >&2
  exit 69
}

docker_compose() {
  compose_file="$1"
  shift

  if command -v docker >/dev/null 2>&1 && docker compose version >/dev/null 2>&1; then
    docker compose -f "$compose_file" "$@"
    return
  fi
  if command -v docker-compose >/dev/null 2>&1; then
    docker-compose -f "$compose_file" "$@"
    return
  fi

  echo "Docker Compose is required for this benchmark workload" >&2
  exit 69
}

safe_rm_rf_benchmark_path() {
  path="$1"

  case "$path" in
    ''|/|.)
      echo "refusing to remove unsafe benchmark path: $path" >&2
      exit 70
      ;;
    benchmarks/output/object-services/*|*/object-services/*)
      rm -rf "$path"
      ;;
    *)
      echo "refusing to remove path outside object-services benchmark output: $path" >&2
      exit 70
      ;;
  esac
}

configure_provider_s3() {
  provider="$1"
  workload="$2"

  require_supported_provider "$provider" "$workload"

  case "$provider" in
    garage)
      load_garage_credentials
      endpoint="${DASOBJECTSTORE_S3_ENDPOINT:-http://127.0.0.1:3900}"
      region="${AWS_DEFAULT_REGION:-garage}"
      export AWS_ACCESS_KEY_ID="${AWS_ACCESS_KEY_ID:-$GARAGE_DEFAULT_ACCESS_KEY}"
      export AWS_SECRET_ACCESS_KEY="${AWS_SECRET_ACCESS_KEY:-$GARAGE_DEFAULT_SECRET_KEY}"
      ;;
    rustfs)
      endpoint="${DASOBJECTSTORE_S3_ENDPOINT:-http://127.0.0.1:9000}"
      region="${AWS_DEFAULT_REGION:-us-east-1}"
      export AWS_ACCESS_KEY_ID="${AWS_ACCESS_KEY_ID:-rustfsadmin}"
      export AWS_SECRET_ACCESS_KEY="${AWS_SECRET_ACCESS_KEY:-rustfsadmin}"
      ;;
  esac

  export AWS_DEFAULT_REGION="$region"
}

provider_compose_file() {
  case "$1" in
    garage) echo "benchmarks/object-services/providers/garage/compose.yml" ;;
    rustfs) echo "benchmarks/object-services/providers/rustfs/compose.yml" ;;
  esac
}

provider_service_name() {
  case "$1" in
    garage) echo "garage" ;;
    rustfs) echo "rustfs" ;;
  esac
}

provider_data_path() {
  output_root="$1"
  provider="$2"

  case "$provider" in
    garage) echo "$output_root/garage/data" ;;
    rustfs) echo "$output_root/rustfs/data" ;;
  esac
}

file_size() {
  wc -c < "$1" | tr -d ' '
}

ensure_sparse_file() {
  path="$1"
  bytes="$2"

  if [ ! -f "$path" ] || [ "$(file_size "$path")" != "$bytes" ]; then
    rm -f "$path"
    dd if=/dev/zero of="$path" bs=1 count=0 seek="$bytes" 2>/dev/null
  fi
}

ensure_allocated_file() {
  path="$1"
  bytes="$2"

  if [ ! -f "$path" ] || [ "$(file_size "$path")" != "$bytes" ]; then
    rm -f "$path"
    dd if=/dev/zero of="$path" bs=1048576 count="$((bytes / 1048576))" 2>/dev/null
    remainder=$((bytes % 1048576))
    if [ "$remainder" -gt 0 ]; then
      dd if=/dev/zero bs=1 count="$remainder" 2>/dev/null >> "$path"
    fi
  fi
}

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
  if command -v aws >/dev/null 2>&1; then
    aws --endpoint-url "$endpoint" s3api "$@"
    return
  fi

  docker run --rm \
    --add-host host.docker.internal:host-gateway \
    -v "$PWD:/work" \
    -v /tmp:/tmp \
    -w /work \
    -e AWS_ACCESS_KEY_ID="${AWS_ACCESS_KEY_ID:-}" \
    -e AWS_SECRET_ACCESS_KEY="${AWS_SECRET_ACCESS_KEY:-}" \
    -e AWS_DEFAULT_REGION="${AWS_DEFAULT_REGION:-us-east-1}" \
    "${DASOBJECTSTORE_AWS_CLI_IMAGE:-amazon/aws-cli:2}" \
    --endpoint-url "$(container_s3_endpoint)" s3api "$@"
}

container_s3_endpoint() {
  case "$endpoint" in
    http://127.0.0.1:*) printf 'http://host.docker.internal:%s\n' "${endpoint##*:}" ;;
    http://localhost:*) printf 'http://host.docker.internal:%s\n' "${endpoint##*:}" ;;
    *) printf '%s\n' "$endpoint" ;;
  esac
}
