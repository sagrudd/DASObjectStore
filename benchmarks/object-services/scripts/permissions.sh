normalize_benchmark_path_permissions() {
  path="$1"
  uid="${DASOBJECTSTORE_BENCH_UID:-$(id -u)}"
  gid="${DASOBJECTSTORE_BENCH_GID:-$(id -g)}"
  timeout_seconds="${DASOBJECTSTORE_BENCH_PERMISSION_FIX_TIMEOUT_SECONDS:-120}"
  fix_image="${DASOBJECTSTORE_BENCH_PERMISSION_FIX_IMAGE:-alpine:3.20}"

  if [ ! -e "$path" ]; then
    return
  fi

  case "$uid:$gid" in
    ''|*[!0-9:]*|*::*)
      echo "invalid benchmark permission normalization setting" >&2
      exit 64
      ;;
  esac
  case "$timeout_seconds" in
    ''|*[!0-9]*|0)
      echo "invalid benchmark permission normalization timeout" >&2
      exit 64
      ;;
  esac

  if ! command -v docker >/dev/null 2>&1; then
    echo "Docker is required to normalize benchmark provider output permissions" >&2
    exit 69
  fi

  parent_dir="$(cd "$(dirname "$path")" && pwd)"
  basename_path="$(basename "$path")"

  if command -v timeout >/dev/null 2>&1; then
    timeout "$timeout_seconds" docker run --rm \
      -v "$parent_dir/$basename_path:/fix_path" \
      "$fix_image" \
      sh -c "chown -R $uid:$gid /fix_path && chmod -R u+rwX /fix_path" \
      >/dev/null
    return
  fi

  docker run --rm \
    -v "$parent_dir/$basename_path:/fix_path" \
    "$fix_image" \
    sh -c "chown -R $uid:$gid /fix_path && chmod -R u+rwX /fix_path" \
    >/dev/null
}
