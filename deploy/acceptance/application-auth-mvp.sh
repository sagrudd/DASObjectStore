#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
VALIDATION_ROOT="${DASOBJECTSTORE_CODEX_VALIDATION_ROOT:-${HOME:?HOME is required}/.dasobjectstore-codex-validation}"
APPROVED_ROOT="${HOME:?HOME is required}/.dasobjectstore-codex-validation"

case "$VALIDATION_ROOT" in
    "$APPROVED_ROOT"|"$APPROVED_ROOT"/*) ;;
    *) printf 'error: validation root must remain beneath %s\n' "$APPROVED_ROOT" >&2; exit 1 ;;
esac

if [ -n "$(git -C "$REPO_DIR" status --porcelain)" ]; then
    printf 'error: application-auth acceptance requires a clean committed revision\n' >&2
    exit 1
fi

COMMIT="$(git -C "$REPO_DIR" rev-parse HEAD)"
EVIDENCE_DIR="$VALIDATION_ROOT/deployment-evidence"
REPORT="$EVIDENCE_DIR/application-auth-mvp-$COMMIT.txt"

DASOBJECTSTORE_CODEX_VALIDATION_ROOT="$VALIDATION_ROOT" \
    cargo test --manifest-path "$REPO_DIR/Cargo.toml" \
    -p dasobjectstore-daemon --test application_auth_mvp_acceptance

/bin/mkdir -p "$EVIDENCE_DIR"
/bin/chmod 700 "$EVIDENCE_DIR"
{
    printf 'source_commit=%s\n' "$COMMIT"
    printf 'administrator_registration=passed\n'
    printf 'ed25519_proof_exchange=passed\n'
    printf 'overlapping_key_rotation=passed\n'
    printf 'key_revocation=passed\n'
    printf 'identity_revocation=passed\n'
    printf 'mtls_request_revalidation=passed\n'
    printf 'redacted_audit=passed\n'
    printf 'private_key_persisted=no\n'
    printf 'application_auth_mvp=passed\n'
} > "$REPORT"
/bin/chmod 600 "$REPORT"
printf 'Application-auth MVP acceptance passed.\n'
printf 'Report: %s\n' "$REPORT"
