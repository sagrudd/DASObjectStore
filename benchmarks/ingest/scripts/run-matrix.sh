#!/usr/bin/env sh
set -eu

script_dir="$(dirname "$0")"

. "$script_dir/matrix.sh"

for scenario in $scenarios; do
  "$script_dir/run.sh" "$scenario"
done
