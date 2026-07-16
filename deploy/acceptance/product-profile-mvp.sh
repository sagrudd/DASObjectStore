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

COMMIT="$(git -C "$REPO_DIR" rev-parse HEAD)"
EVIDENCE_DIR="$VALIDATION_ROOT/deployment-evidence"
REPORT="$EVIDENCE_DIR/product-profile-mvp-$COMMIT.txt"

if [ -n "$(git -C "$REPO_DIR" status --porcelain)" ]; then
    printf 'error: product-profile acceptance requires a clean committed revision\n' >&2
    exit 1
fi

DASOBJECTSTORE_CODEX_VALIDATION_ROOT="$VALIDATION_ROOT" \
    cargo test --manifest-path "$REPO_DIR/Cargo.toml" \
    -p dasobjectstore-mnemosyne --test product_profile_mvp_acceptance

/bin/mkdir -p "$EVIDENCE_DIR"
/bin/chmod 700 "$EVIDENCE_DIR"
{
    printf 'source_commit=%s\n' "$COMMIT"
    printf 'profile_provision=passed\n'
    printf 'idempotent_reprovision=passed\n'
    printf 'generated_object_count=64\n'
    printf 'generated_object_bytes=4096\n'
    printf 'put_list_get_range_verify_delete=passed\n'
    printf 'quota_rejection=passed\n'
    printf 'restart_recovery=passed\n'
    printf 'customer_or_project_data_used=no\n'
    printf 'product_profile_mvp=passed\n'
} > "$REPORT"
/bin/chmod 600 "$REPORT"
printf 'Product-profile MVP acceptance passed.\n'
printf 'Report: %s\n' "$REPORT"
