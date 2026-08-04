#!/usr/bin/env bash
# Jenkins qualification helper for ADR-0001 and its merged transitive TLS 1.2
# amendment; it performs no deployment.
# It verifies the locked dependency graph only. Runtime TLS and subprocess
# ownership evidence remain separate required qualification corpus entries.
set -euo pipefail

output=${RUSTLS_PROVIDER_GRAPH_OUTPUT:-/tmp/dasobjectstore-rustls-provider-feature-tree.txt}
tree=$(cargo tree --locked --workspace --target all --edges normal,build,dev -e features)
printf '%s\n' "$tree" > "$output"

for forbidden in 'rustls feature "ring"' 'rustls feature "custom-provider"' 'rustls feature "fips"' 'rustls feature "logging"'; do
  if grep -Fq "$forbidden" "$output"; then
    echo "forbidden Rustls feature: $forbidden" >&2
    exit 1
  fi
done

grep -Fq 'rustls feature "aws-lc-rs"' "$output"
test "$(grep -E '[[:space:]│├└]rustls v[0-9.]+' "$output" | sed -E 's/.*rustls v([0-9.]+).*/\1/' | sort -u | wc -l | tr -d ' ')" -eq 1

# The ADR-0001 amendment permits only Reqwest's known provider-neutral
# transitive TLS 1.2 path.
if grep -Fq 'rustls feature "tls12"' "$output"; then
  grep -Fq 'reqwest feature "rustls-tls-webpki-roots-no-provider"' "$output"
  grep -Fq 'hyper-rustls feature "tls12"' "$output"
fi
