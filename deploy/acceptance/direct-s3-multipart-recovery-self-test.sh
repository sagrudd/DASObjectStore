#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
HARNESS="$SCRIPT_DIR/direct-s3-multipart-recovery.sh"

test -x "$HARNESS" || {
    printf 'error: harness is not executable\n' >&2
    exit 1
}
"$HARNESS" --help | grep -Fq 'interrupts the first CompleteMultipartUpload client'

set +e
output="$(
    HOME="$(mktemp -d)" \
    DASOBJECTSTORE_MULTIPART_RECOVERY_CONFIRM=wrong \
    "$HARNESS" 2>&1
)"
status=$?
set -e
test "$status" -ne 0
case "$(uname -s)" in
    Linux) printf '%s\n' "$output" | grep -Fq 'exact documented phrase' ;;
    *) printf '%s\n' "$output" | grep -Fq 'requires Linux' ;;
esac

grep -Fq 'RUN CODEX MULTIPART RECOVERY ACCEPTANCE' "$HARNESS"
grep -Fq 'uploaded_parts_after_disconnect=0' "$HARNESS"
grep -Fq 'durable_status_and_receipt=passed' "$HARNESS"
grep -Fq 'direct S3 acceptance requires an HTTPS endpoint' "$HARNESS"
grep -Fq 'AWS_CA_BUNDLE="$CA_FILE"' "$HARNESS"
grep -Fq -- '--key-marker "$NEXT_KEY"' "$HARNESS"
grep -Fq -- '--upload-id-marker "$NEXT_UPLOAD"' "$HARNESS"
grep -Fq 'sudo systemctl restart "$GATEWAY_SERVICE"' "$HARNESS"
grep -Fq 'sudo systemctl restart "$DAEMON_SERVICE"' "$HARNESS"
printf 'Direct-S3 multipart recovery harness self-test passed.\n'
