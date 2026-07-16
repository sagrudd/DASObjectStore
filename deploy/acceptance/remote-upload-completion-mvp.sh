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
    printf 'error: remote-upload completion acceptance requires a clean committed revision\n' >&2
    exit 1
fi

COMMIT="$(git -C "$REPO_DIR" rev-parse HEAD)"
EVIDENCE_DIR="$VALIDATION_ROOT/deployment-evidence"
REPORT="$EVIDENCE_DIR/remote-upload-completion-mvp-$COMMIT.txt"

DASOBJECTSTORE_CODEX_VALIDATION_ROOT="$VALIDATION_ROOT" \
    cargo test --manifest-path "$REPO_DIR/Cargo.toml" \
    -p dasobjectstore-daemon --test remote_upload_completion_mvp_acceptance

/bin/mkdir -p "$EVIDENCE_DIR"
/bin/chmod 700 "$EVIDENCE_DIR"
{
    printf 'source_commit=%s\n' "$COMMIT"
    printf 'paired_session_scope=passed\n'
    printf 'application_identity_scope=passed\n'
    printf 'one_time_capability_issue=passed\n'
    printf 'forged_capability_rejection=passed\n'
    printf 'provider_verification_before_commit=passed\n'
    printf 'idempotent_exact_replay=passed\n'
    printf 'catalogue_failure_retry=passed\n'
    printf 'credential_response_redaction=passed\n'
    printf 'garage_appliance_execution=surrogate_only\n'
    printf 'remote_upload_completion_mvp=passed\n'
} > "$REPORT"
/bin/chmod 600 "$REPORT"
printf 'Remote-upload completion MVP acceptance passed.\n'
printf 'Report: %s\n' "$REPORT"
