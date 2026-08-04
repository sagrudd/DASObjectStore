#!/usr/bin/env bash
# Jenkins qualification helper for ADR-0001 and its merged transitive TLS 1.2
# amendment. It is a source/metadata inventory only: it does not build,
# package, install, deploy, or activate DAS.
set -euo pipefail

output=${RUSTLS_PROVIDER_OWNERSHIP_OUTPUT:-/tmp/dasobjectstore-rustls-provider-classes.tsv}

command -v jq >/dev/null
metadata=$(cargo metadata --locked --no-deps --format-version 1)

printf 'package\ttarget\tkind\tclass\tsource\n' > "$output"
printf '%s\n' "$metadata" | jq -r '
  .packages[] as $package | $package.targets[] |
  [
    $package.name,
    .name,
    (.kind | join(",")),
    (if (.kind | index("test")) then "isolated-test-process"
     elif (.kind | index("example")) then "example-process"
     elif .src_path | endswith("crates/dasobjectstore-cli/src/main.rs") then "native-provider-owner"
     elif .src_path | endswith("crates/dasobjectstore-cli/src/server_main.rs") then "native-provider-owner"
     elif .src_path | endswith("crates/dasobjectstore-cli/src/local_auth_helper_main.rs") then "native-provider-owner"
     elif .src_path | endswith("crates/dasobjectstore-cli/src/auth_migrate_main.rs") then "native-provider-owner"
     elif .src_path | endswith("crates/dasobjectstore-daemon/src/main.rs") then "native-provider-owner"
     elif .src_path | endswith("crates/dasobjectstore-remote/src/main.rs") then "native-provider-owner"
     elif .src_path | endswith("crates/dasobjectstore-gui-web/src/main.rs") then "wasm-browser-rustls-absent"
     elif .src_path | endswith("crates/dasobjectstore-workspace-host/src/main.rs") then "native-rustls-absent"
     else "provider-neutral-library"
     end),
    .src_path
  ] | @tsv
' | sort >> "$output"

# The workspace deliberately has no Cargo examples. A new one must receive an
# explicit provider class before it can enter a qualification graph.
if printf '%s\n' "$metadata" | jq -e '[.packages[].targets[] | select(.kind | index("example"))] | length == 0' >/dev/null; then
  :
else
  echo "unclassified Cargo example target" >&2
  exit 1
fi

require_first_statement() {
  local source=$1 installer=$2
  perl -0ne "exit 0 if /fn main\\s*\\([^)]*\\)\\s*(?:->[^\\{]+)?\\{\\s*if let Err\\(error\\) = ${installer}\\(\\)/s; exit 1" "$source"
}

require_first_statement crates/dasobjectstore-cli/src/main.rs 'tls_provider::install'
require_first_statement crates/dasobjectstore-cli/src/server_main.rs 'tls_provider::install'
require_first_statement crates/dasobjectstore-cli/src/local_auth_helper_main.rs 'tls_provider::install'
require_first_statement crates/dasobjectstore-cli/src/auth_migrate_main.rs 'tls_provider::install'
require_first_statement crates/dasobjectstore-daemon/src/main.rs 'install_tls_crypto_provider'
require_first_statement crates/dasobjectstore-remote/src/main.rs 'install_tls_crypto_provider'

# Provider installation is executable-owned. The only library-source calls are
# disposable unit fixtures, after their enclosing cfg(test) module marker.
for source in \
  crates/dasobjectstore-gui-api/src/auth_routes.rs \
  crates/dasobjectstore-gui-api/src/mtls_listener.rs \
  crates/dasobjectstore-gui-api/src/s3_endpoint_probe.rs \
  crates/dasobjectstore-remote/src/trust.rs; do
  awk '
    /#\[cfg\(test\)\]/{ in_test = 1 }
    /install_default\(\)/ && !in_test { exit 1 }
    END { if (!in_test) exit 1 }
  ' "$source"
done

actual_installers=$(rg -l 'install_default\(\)' crates -g '*.rs' | sort)
expected_installers=$(printf '%s\n' \
  crates/dasobjectstore-cli/src/tls_provider.rs \
  crates/dasobjectstore-daemon/src/main.rs \
  crates/dasobjectstore-gui-api/src/auth_routes.rs \
  crates/dasobjectstore-gui-api/src/mtls_listener.rs \
  crates/dasobjectstore-gui-api/src/s3_endpoint_probe.rs \
  crates/dasobjectstore-remote/src/main.rs \
  crates/dasobjectstore-remote/src/trust.rs | sort)
test "$actual_installers" = "$expected_installers"
! rg -n 'replace_default\(' crates -g '*.rs'

# Browser, root helper and reference downstream fixture do not have a native
# Rustls dependency. If that changes, their target class must be reconsidered.
for manifest in \
  crates/dasobjectstore-gui-web/Cargo.toml \
  crates/dasobjectstore-workspace-host/Cargo.toml \
  crates/dasobjectstore-reference/fixtures/downstream-consumer/Cargo.toml.template; do
  ! grep -Eq '^rustls([[:space:]]|=)' "$manifest"
done

# Every workspace library's production entry module is prohibited from global
# provider installation or replacement. Test modules are checked separately.
for source in crates/*/src/lib.rs; do
  ! rg -n 'install_default\(|replace_default\(' "$source"
done

echo "Rustls provider ownership inventory: $output"
