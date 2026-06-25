#!/usr/bin/env sh
set -eu

provider="${1:?provider required}"
object_bytes="${DASOBJECTSTORE_LARGE_OBJECT_BYTES:-1073741824}"
bucket="${DASOBJECTSTORE_BENCH_BUCKET:-dasobjectstore-bench-large}"
object_count="${DASOBJECTSTORE_LARGE_OBJECT_COUNT:-1}"
object_prefix="${DASOBJECTSTORE_BENCH_OBJECT_PREFIX:-large-object}"

exec "$(dirname "$0")/s3-roundtrip.sh" "$provider" large-object "$object_bytes" "$object_count" "$bucket" "$object_prefix"
