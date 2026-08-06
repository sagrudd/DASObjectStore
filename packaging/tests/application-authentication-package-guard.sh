#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
guard="$repo_root/packaging/validate-package-auth-content.sh"
fixture_root="${CODEX_VALIDATION_ROOT:-${HOME:-/tmp}/.dasobjectstore-codex-validation}/package-auth-guard-$$"
trap 'rm -rf "$fixture_root"' EXIT

mkdir -p "$fixture_root/etc/dasobjectstore"
printf '%s\n' '{"bind_address":"127.0.0.1"}' >"$fixture_root/etc/dasobjectstore/daemon.json"
bash "$guard" "$fixture_root"

printf '%s\n' '{"development_self_signing_enabled":true}' \
  >"$fixture_root/etc/dasobjectstore/daemon.json"
if bash "$guard" "$fixture_root" >/dev/null 2>&1; then
  printf 'package auth guard failed to reject a development self-signing switch\n' >&2
  exit 1
fi

rm -f "$fixture_root/etc/dasobjectstore/daemon.json"
touch "$fixture_root/etc/dasobjectstore/development-self-signing.key"
if bash "$guard" "$fixture_root" >/dev/null 2>&1; then
  printf 'package auth guard failed to reject a development self-signing key\n' >&2
  exit 1
fi

rm -rf "$fixture_root/etc"
mkdir -p "$fixture_root/etc/pam.d"
touch "$fixture_root/etc/pam.d/dasobjectstore"
if bash "$guard" "$fixture_root" >/dev/null 2>&1; then
  printf 'package auth guard failed to reject a PAM payload\n' >&2
  exit 1
fi

rm -rf "$fixture_root/etc"
mkdir -p "$fixture_root/DEBIAN"
printf '%s\n' 'systemctl restart dasobjectstored.service' >"$fixture_root/DEBIAN/postinst"
if bash "$guard" "$fixture_root" >/dev/null 2>&1; then
  printf 'package auth guard failed to reject automatic service lifecycle\n' >&2
  exit 1
fi

printf 'package auth guard regression tests passed\n'
