#!/usr/bin/env sh
set -eu

provider="${1:?provider required}"
object_bytes="${DASOBJECTSTORE_SMALL_OBJECT_BYTES:-65536}"
object_count="${DASOBJECTSTORE_SMALL_OBJECT_COUNT:-1000}"
bucket="${DASOBJECTSTORE_BENCH_BUCKET:-dasobjectstore-bench-small}"
object_prefix="${DASOBJECTSTORE_BENCH_OBJECT_PREFIX:-small-object}"

exec "$(dirname "$0")/s3-roundtrip.sh" "$provider" small-object "$object_bytes" "$object_count" "$bucket" "$object_prefix"
