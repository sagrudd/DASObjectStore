#!/usr/bin/env bash
set -euo pipefail

# Production RPM/DEB payloads may contain the public application-auth contract
# (including the enum that lets the daemon reject development credentials), but
# they must never ship development self-signing material or an enablement
# switch. This guard intentionally inspects package payload text and paths,
# not compiled binaries, so the production rejection contract remains present.

payload_root="${1:?usage: validate-package-auth-content.sh PAYLOAD_ROOT}"
if [[ ! -d "$payload_root" ]]; then
  printf 'package auth guard requires a payload directory: %s\n' "$payload_root" >&2
  exit 1
fi

forbidden_path_pattern='(^|/)(self[-_]sign|development[-_]self|dev[-_]issuer|validation[-_]private[-_]key)'
forbidden_text_pattern='DASOBJECTSTORE_(ENABLE|ALLOW|DEVELOPMENT)_SELF_SIGNING|development[_-]self[_-]signing[_-](enabled|issuer|key)|validation[_-]private[_-]key|self[_-]signing[_-](key|issuer)'

while IFS= read -r -d '' path; do
  relative="${path#"$payload_root"/}"
  relative_lower="$(printf '%s' "$relative" | tr '[:upper:]' '[:lower:]')"
  if [[ "$relative_lower" =~ $forbidden_path_pattern ]]; then
    printf 'development self-signing asset is forbidden in package payload: %s\n' "$relative" >&2
    exit 1
  fi

  case "$path" in
    *.conf|*.json|*.md|*.path|*.postinst|*.service|*.sh|*.spec|*.toml|*.yaml|*.yml|*/control)
      if grep -Eiq -- "$forbidden_text_pattern" "$path"; then
        printf 'development self-signing marker is forbidden in package payload: %s\n' "$relative" >&2
        exit 1
      fi
      ;;
  esac
done < <(find "$payload_root" -type f -print0)

printf 'package auth guard passed: no development self-signing payload or enablement marker\n'
