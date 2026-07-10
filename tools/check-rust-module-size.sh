#!/usr/bin/env bash
# Guard the production portion of Rust modules from uncontrolled growth.
#
# Unit tests commonly live in the same file as the implementation. Count only
# the lines before the `#[cfg(test)] mod tests` module so conditional test-only
# helpers elsewhere in production code do not hide a regression.
set -euo pipefail

readonly max_lines=1000
readonly exceptions_file="tools/rust-module-size-exceptions.txt"

if [[ ! -f "$exceptions_file" ]]; then
    echo "missing module-size exceptions file: $exceptions_file" >&2
    exit 2
fi

is_exception() {
    local path="$1"
    grep -Fqx -- "$path" "$exceptions_file"
}

violations=0
while IFS= read -r path; do
    # Extracted test modules contain no production implementation.
    if [[ "$path" == */tests.rs ]]; then
        continue
    fi

    production_lines=$(awk '
        previous ~ /^#\[cfg\(test\)\]$/ && $0 ~ /^mod tests/ { exit }
        { count += 1; previous = $0 }
        END { print count + 0 }
    ' "$path")

    if (( production_lines <= max_lines )); then
        continue
    fi

    if is_exception "$path"; then
        printf 'baseline exception: %s (%s production lines)\n' "$path" "$production_lines"
        continue
    fi

    printf 'production module exceeds %s lines: %s (%s lines)\n' \
        "$max_lines" "$path" "$production_lines" >&2
    violations=1
done < <(git ls-files 'crates/**/src/**/*.rs' 'crates/**/src/*.rs')

if (( violations )); then
    echo "Split the module by responsibility or add a reviewed, temporary baseline exception." >&2
    exit 1
fi
