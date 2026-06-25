#!/usr/bin/env sh
set -eu

output_root="${DASOBJECTSTORE_BENCH_OUTPUT_DIR:-benchmarks/output/object-services}"
env_path="$output_root/garage/garage.env"

random_hex() {
  bytes="$1"
  if command -v openssl >/dev/null 2>&1; then
    openssl rand -hex "$bytes"
  else
    od -An -N "$bytes" -tx1 /dev/urandom | tr -d ' \n'
  fi
}

write_env_file() {
  mkdir -p "$(dirname "$env_path")"
  access_key="${GARAGE_DEFAULT_ACCESS_KEY:-GK$(random_hex 16)}"
  secret_key="${GARAGE_DEFAULT_SECRET_KEY:-$(random_hex 32)}"
  bucket="${GARAGE_DEFAULT_BUCKET:-dasobjectstore-bench-large}"

  umask 077
  {
    printf 'GARAGE_DEFAULT_ACCESS_KEY=%s\n' "$access_key"
    printf 'GARAGE_DEFAULT_SECRET_KEY=%s\n' "$secret_key"
    printf 'GARAGE_DEFAULT_BUCKET=%s\n' "$bucket"
  } > "$env_path"
}

case "${1:-ensure}" in
  ensure)
    if [ ! -s "$env_path" ]; then
      write_env_file
    fi
    printf '%s\n' "$env_path"
    ;;
  path)
    printf '%s\n' "$env_path"
    ;;
  *)
    echo "usage: $0 [ensure|path]" >&2
    exit 64
    ;;
esac
