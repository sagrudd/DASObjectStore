#!/usr/bin/env sh
set -eu

script_dir="$(dirname "$0")"
template_path="$script_dir/../reports/report-template.md"
benchmark_date="${DASOBJECTSTORE_BENCHMARK_DATE:-$(date +%F)}"

while IFS= read -r line; do
  case "$line" in
    'Benchmark date: `YYYY-MM-DD`')
      printf 'Benchmark date: `%s`\n' "$benchmark_date"
      ;;
    *)
      printf '%s\n' "$line"
      ;;
  esac
done < "$template_path"

cat <<'EOF'

## Environment Snapshot

EOF

"$script_dir/environment-snapshot.sh"

cat <<'EOF'

## Raw Input Inventory

EOF

"$script_dir/report-input-index.sh"
