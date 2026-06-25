#!/usr/bin/env sh
set -eu

value_or_unknown() {
  command_name="$1"
  shift

  if command -v "$command_name" >/dev/null 2>&1; then
    "$@" 2>/dev/null || printf 'unknown\n'
  else
    printf 'unknown\n'
  fi
}

first_line() {
  sed -n '1p'
}

cpu_name() {
  if command -v sysctl >/dev/null 2>&1; then
    sysctl -n machdep.cpu.brand_string 2>/dev/null && return
  fi
  if command -v lscpu >/dev/null 2>&1; then
    lscpu 2>/dev/null | awk -F: '/Model name/ { sub(/^[ \t]+/, "", $2); print $2; exit }' && return
  fi
  if [ -r /proc/cpuinfo ]; then
    awk -F: '/model name/ { sub(/^[ \t]+/, "", $2); print $2; exit }' /proc/cpuinfo && return
  fi
  printf 'unknown\n'
}

memory_bytes() {
  if command -v sysctl >/dev/null 2>&1; then
    sysctl -n hw.memsize 2>/dev/null && return
  fi
  if [ -r /proc/meminfo ]; then
    awk '/MemTotal/ { print $2 * 1024; exit }' /proc/meminfo && return
  fi
  printf 'unknown\n'
}

host_os="$(uname -s 2>/dev/null || printf 'unknown')"
kernel="$(uname -r 2>/dev/null || printf 'unknown')"
machine="$(uname -m 2>/dev/null || printf 'unknown')"
cpu="$(cpu_name | first_line)"
memory_bytes="$(memory_bytes | first_line)"
docker_version="$(value_or_unknown docker docker --version | first_line)"
compose_version="$(value_or_unknown docker docker compose version | first_line)"
aws_version="$(value_or_unknown aws aws --version | first_line)"
aws_backend="local aws"
if [ "$aws_version" = "unknown" ]; then
  if command -v docker >/dev/null 2>&1; then
    aws_backend="container: ${DASOBJECTSTORE_AWS_CLI_IMAGE:-amazon/aws-cli:2}"
  else
    aws_backend="unavailable"
  fi
fi
hash_tool="unknown"

if command -v sha256sum >/dev/null 2>&1; then
  hash_tool="$(sha256sum --version 2>/dev/null | first_line)"
elif command -v shasum >/dev/null 2>&1; then
  hash_tool="shasum"
fi

cat <<EOF
# Object Service Benchmark Environment

| Field | Value |
| --- | --- |
| Host OS | \`$host_os\` |
| Kernel | \`$kernel\` |
| Machine | \`$machine\` |
| CPU | \`$cpu\` |
| Memory bytes | \`$memory_bytes\` |
| Docker version | \`$docker_version\` |
| Docker Compose version | \`$compose_version\` |
| AWS CLI version | \`$aws_version\` |
| AWS CLI backend | \`$aws_backend\` |
| Hash tool | \`$hash_tool\` |
| Benchmark output root | \`${DASOBJECTSTORE_BENCH_OUTPUT_DIR:-benchmarks/output/object-services}\` |
| SSD ingest path | \`${DASOBJECTSTORE_SSD_INGEST_PATH:-/tmp/dasobjectstore-bench/ssd}\` |
| HDD destage root | \`${DASOBJECTSTORE_HDD_ROOT_PATH:-/tmp/dasobjectstore-bench/hdd}\` |
EOF
