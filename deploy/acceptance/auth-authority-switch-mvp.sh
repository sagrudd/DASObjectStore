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
    printf 'error: auth-authority switch acceptance requires a clean committed revision\n' >&2
    exit 1
fi

COMMIT="$(git -C "$REPO_DIR" rev-parse HEAD)"
EVIDENCE_DIR="$VALIDATION_ROOT/deployment-evidence"
REPORT="$EVIDENCE_DIR/auth-authority-switch-mvp-$COMMIT.txt"

DASOBJECTSTORE_CODEX_VALIDATION_ROOT="$VALIDATION_ROOT" \
    cargo test --manifest-path "$REPO_DIR/Cargo.toml" \
    -p dasobjectstore-cli --test auth_authority_switch_mvp_acceptance

/bin/mkdir -p "$EVIDENCE_DIR"
/bin/chmod 700 "$EVIDENCE_DIR"
{
    printf 'source_commit=%s\n' "$COMMIT"
    printf 'dry_run_non_mutating=passed\n'
    printf 'registry_migration=passed\n'
    printf 'monas_composed_session=passed\n'
    printf 'monas_revocation=passed\n'
    printf 'intrinsic_source_retained=passed\n'
    printf 'rollback_authentication=passed\n'
    printf 'browser_bearer_exported=no\n'
    printf 'auth_authority_switch_mvp=passed_surrogate\n'
} > "$REPORT"
/bin/chmod 600 "$REPORT"
printf 'Authentication-authority switch MVP surrogate acceptance passed.\n'
printf 'Report: %s\n' "$REPORT"
