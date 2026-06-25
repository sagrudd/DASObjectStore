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
      endpoint="${DASOBJECTSTORE_S3_ENDPOINT:-http://127.0.0.1:3900}"
      region="${AWS_DEFAULT_REGION:-garage}"
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
  aws --endpoint-url "$endpoint" s3api "$@"
}
