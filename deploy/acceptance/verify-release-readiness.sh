#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
VALIDATION_ROOT="${DASOBJECTSTORE_CODEX_VALIDATION_ROOT:-${HOME:?HOME is required}/.dasobjectstore-codex-validation}"
COMMIT="$(git -C "$REPO_DIR" rev-parse HEAD)"
EVIDENCE_DIR="$VALIDATION_ROOT/deployment-evidence"
LIMA_DIR="$VALIDATION_ROOT/lima/evidence"

case "$VALIDATION_ROOT" in
    "$HOME/.dasobjectstore-codex-validation"|"$HOME/.dasobjectstore-codex-validation"/*) ;;
    *) printf 'error: validation root must remain beneath %s/.dasobjectstore-codex-validation\n' "$HOME" >&2; exit 1 ;;
esac

require_field() {
    local file="$1"
    local field="$2"
    local expected="$3"
    [ -f "$file" ] || {
        printf 'error: required deployment evidence is missing: %s\n' "$file" >&2
        exit 1
    }
    /usr/bin/grep -Fxq "$field=$expected" "$file" || {
        printf 'error: %s does not record %s=%s\n' "$file" "$field" "$expected" >&2
        exit 1
    }
}

MACOS="$EVIDENCE_DIR/macos-launchd-$COMMIT.txt"
DOCKER="$EVIDENCE_DIR/local-docker-s3-$COMMIT.txt"
PRODUCT="$EVIDENCE_DIR/product-profile-mvp-$COMMIT.txt"
APPLICATION_AUTH="$EVIDENCE_DIR/application-auth-mvp-$COMMIT.txt"
UBUNTU="$LIMA_DIR/ubuntu-arm64.txt"
ALMA="$LIMA_DIR/alma-arm64.txt"

for field in render install status rollback reinstall uninstall; do
    require_field "$MACOS" "$field" passed
done
require_field "$MACOS" source_commit "$COMMIT"
require_field "$MACOS" persistent_state_retained yes

for field in put head list get checksum delete; do
    require_field "$DOCKER" "$field" passed
done
require_field "$DOCKER" source_commit "$COMMIT"
require_field "$DOCKER" generated_bytes 65536

for field in profile_provision idempotent_reprovision put_list_get_range_verify_delete quota_rejection restart_recovery product_profile_mvp; do
    require_field "$PRODUCT" "$field" passed
done
require_field "$PRODUCT" source_commit "$COMMIT"
require_field "$PRODUCT" generated_object_count 64
require_field "$PRODUCT" generated_object_bytes 4096
require_field "$PRODUCT" customer_or_project_data_used no

for field in administrator_registration ed25519_proof_exchange overlapping_key_rotation key_revocation identity_revocation mtls_request_revalidation redacted_audit application_auth_mvp; do
    require_field "$APPLICATION_AUTH" "$field" passed
done
require_field "$APPLICATION_AUTH" source_commit "$COMMIT"
require_field "$APPLICATION_AUTH" private_key_persisted no

for evidence in "$UBUNTU" "$ALMA"; do
    for field in install upgrade reboot uninstall; do
        require_field "$evidence" "$field" passed
    done
    require_field "$evidence" source_commit "$COMMIT"
    require_field "$evidence" architecture aarch64
    require_field "$evidence" persistent_state_retained yes
done
require_field "$UBUNTU" distribution ubuntu
require_field "$ALMA" distribution alma

/bin/mkdir -p "$EVIDENCE_DIR"
/bin/chmod 700 "$EVIDENCE_DIR"
REPORT="$EVIDENCE_DIR/local-release-readiness-$COMMIT.txt"
{
    printf 'source_commit=%s\n' "$COMMIT"
    printf 'macos_per_user=passed\n'
    printf 'ubuntu_arm64_package=passed\n'
    printf 'almalinux_arm64_package=passed\n'
    printf 'local_docker_garage_s3=passed\n'
    printf 'product_profile_mvp=passed\n'
    printf 'application_auth_mvp=passed\n'
    printf 'local_deployment_readiness=passed\n'
    printf 'physical_das_acceptance=blocked_unavailable_host\n'
    printf 'x86_64_package_parity=blocked_unavailable_host\n'
} > "$REPORT"
/bin/chmod 600 "$REPORT"
printf 'Local deployment release-readiness evidence is complete.\n'
printf 'External hardware gates remain blocked and are not reported as passed.\n'
printf 'Report: %s\n' "$REPORT"
