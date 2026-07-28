#!/usr/bin/env bash
set -euo pipefail

# Destructive, operator-gated appliance acceptance for daemon-owned multipart
# completion. The payload is generated beneath the CODEX validation root and
# the same deterministic part file is reused to produce a multi-GiB object
# without consuming multi-GiB local scratch space.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
VALIDATION_ROOT="${DASOBJECTSTORE_CODEX_VALIDATION_ROOT:-${HOME:?HOME is required}/.dasobjectstore-codex-validation}"
APPROVED_ROOT="${HOME:?HOME is required}/.dasobjectstore-codex-validation"
CONFIRMATION="${DASOBJECTSTORE_MULTIPART_RECOVERY_CONFIRM:-}"
EXPECTED_CONFIRMATION="RUN CODEX MULTIPART RECOVERY ACCEPTANCE"

usage() {
    cat <<'EOF'
Usage: direct-s3-multipart-recovery.sh

Required environment:
  DASOBJECTSTORE_MULTIPART_RECOVERY_CONFIRM='RUN CODEX MULTIPART RECOVERY ACCEPTANCE'
  DASOBJECTSTORE_S3_ENDPOINT=http://127.0.0.1:3900
  DASOBJECTSTORE_S3_PROFILE=<temporary AWS profile>
  DASOBJECTSTORE_ACCEPTANCE_COOKIE_FILE=<authenticated cookie jar>

Optional environment:
  DASOBJECTSTORE_S3_BUCKET=dos-codex
  DASOBJECTSTORE_S3_KEY=codex/acceptance/multipart-recovery.bin
  DASOBJECTSTORE_MULTIPART_PART_MIB=64
  DASOBJECTSTORE_MULTIPART_PART_COUNT=96
  DASOBJECTSTORE_DAEMON_SERVICE=dasobjectstored
  DASOBJECTSTORE_GATEWAY_SERVICE=dasobjectstore-server
  DASOBJECTSTORE_ACCEPTANCE_BASE_URL=https://127.0.0.1:8448/products/dasobjectstore
  DASOBJECTSTORE_ACCEPTANCE_CA_FILE=/opt/dasobjectstore/tls/server.crt

The default object is 6 GiB. The harness:
  * uploads every part exactly once;
  * interrupts the first CompleteMultipartUpload client;
  * restarts the gateway and daemon while completion remains durable;
  * retries completion without retransmitting any part;
  * verifies size and SHA-256 by downloading the committed object;
  * verifies prefix, pagination, key marker, and upload-id marker behavior;
  * aborts only the test-owned listing canaries.

Run only in a declared appliance acceptance window. This harness restarts both
DASObjectStore services and transfers a multi-GiB generated object.
EOF
}

fail() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

[ "${1:-}" != "--help" ] || {
    usage
    exit 0
}
[ "$(uname -s)" = "Linux" ] || fail "multipart recovery acceptance requires Linux"
case "$VALIDATION_ROOT" in
    "$APPROVED_ROOT"|"$APPROVED_ROOT"/*) ;;
    *) fail "validation root must remain beneath $APPROVED_ROOT" ;;
esac
[ "$CONFIRMATION" = "$EXPECTED_CONFIRMATION" ] ||
    fail "set DASOBJECTSTORE_MULTIPART_RECOVERY_CONFIRM to the exact documented phrase"
[ -n "${DASOBJECTSTORE_S3_ENDPOINT:-}" ] || fail "DASOBJECTSTORE_S3_ENDPOINT is required"
[ -n "${DASOBJECTSTORE_S3_PROFILE:-}" ] || fail "DASOBJECTSTORE_S3_PROFILE is required"
[ -r "${DASOBJECTSTORE_ACCEPTANCE_COOKIE_FILE:-}" ] ||
    fail "DASOBJECTSTORE_ACCEPTANCE_COOKIE_FILE must be readable"
[ -z "$(git -C "$REPO_DIR" status --porcelain)" ] ||
    fail "acceptance requires a clean committed revision"

for command in aws curl python3 sha256sum timeout sudo systemctl truncate; do
    command -v "$command" >/dev/null || fail "required command is unavailable: $command"
done

BUCKET="${DASOBJECTSTORE_S3_BUCKET:-dos-codex}"
PART_MIB="${DASOBJECTSTORE_MULTIPART_PART_MIB:-64}"
PART_COUNT="${DASOBJECTSTORE_MULTIPART_PART_COUNT:-96}"
DAEMON_SERVICE="${DASOBJECTSTORE_DAEMON_SERVICE:-dasobjectstored}"
GATEWAY_SERVICE="${DASOBJECTSTORE_GATEWAY_SERVICE:-dasobjectstore-server}"
API_BASE="${DASOBJECTSTORE_ACCEPTANCE_BASE_URL:-https://127.0.0.1:8448/products/dasobjectstore}"
CA_FILE="${DASOBJECTSTORE_ACCEPTANCE_CA_FILE:-/opt/dasobjectstore/tls/server.crt}"
[ -r "$CA_FILE" ] || fail "acceptance CA file is not readable: $CA_FILE"

case "$BUCKET" in
    *codex*|*CODEX*) ;;
    *) fail "automated multipart acceptance is restricted to a CODEX bucket" ;;
esac
case "$PART_MIB:$PART_COUNT" in
    *[!0-9:]*|:*|*:) fail "part size and count must be positive integers" ;;
esac
[ "$PART_MIB" -gt 0 ] && [ "$PART_COUNT" -gt 0 ] ||
    fail "part size and count must be positive integers"

PART_BYTES=$((PART_MIB * 1024 * 1024))
TOTAL_BYTES=$((PART_BYTES * PART_COUNT))
[ "$PART_BYTES" -ge $((5 * 1024 * 1024)) ] || fail "S3 parts must be at least 5 MiB"
[ "$TOTAL_BYTES" -gt $((5 * 1024 * 1024 * 1024)) ] ||
    fail "acceptance object must exceed 5 GiB"
[ "$TOTAL_BYTES" -lt $((1024 * 1024 * 1024 * 1024)) ] ||
    fail "acceptance object must remain below 1 TiB"

COMMIT="$(git -C "$REPO_DIR" rev-parse HEAD)"
RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)-${COMMIT:0:12}"
KEY="${DASOBJECTSTORE_S3_KEY:-codex/acceptance/multipart-recovery-$RUN_ID.bin}"
case "$KEY" in
    codex/acceptance/*) ;;
    *) fail "test key must remain under codex/acceptance/" ;;
esac
RUN_ROOT="$VALIDATION_ROOT/multipart-recovery/$RUN_ID"
EVIDENCE_DIR="$VALIDATION_ROOT/deployment-evidence"
PART_FILE="$RUN_ROOT/generated-zero-part.bin"
PARTS_JSON="$RUN_ROOT/completed-parts.json"
DOWNLOAD="$RUN_ROOT/verified-download.bin"
mkdir -p "$RUN_ROOT" "$EVIDENCE_DIR"
chmod 700 "$RUN_ROOT" "$EVIDENCE_DIR"
truncate -s "$PART_BYTES" "$PART_FILE"

aws_s3() {
    aws --profile "$DASOBJECTSTORE_S3_PROFILE" \
        --endpoint-url "$DASOBJECTSTORE_S3_ENDPOINT" \
        s3api "$@"
}

cleanup_canaries=()
main_object_committed=false
cleanup() {
    local item upload_id key
    for item in "${cleanup_canaries[@]}"; do
        key="${item%%|*}"
        upload_id="${item#*|}"
        aws_s3 abort-multipart-upload --bucket "$BUCKET" --key "$key" \
            --upload-id "$upload_id" >/dev/null 2>&1 || true
    done
    if [ "$main_object_committed" = true ]; then
        aws_s3 delete-object --bucket "$BUCKET" --key "$KEY" >/dev/null 2>&1 || true
    fi
}
trap cleanup EXIT

UPLOAD_ID="$(aws_s3 create-multipart-upload \
    --bucket "$BUCKET" --key "$KEY" \
    --query UploadId --output text)"
[ -n "$UPLOAD_ID" ] && [ "$UPLOAD_ID" != "None" ] || fail "multipart initiation returned no upload ID"

printf '{"Parts":[' >"$PARTS_JSON"
for ((part = 1; part <= PART_COUNT; part++)); do
    etag="$(aws_s3 upload-part --bucket "$BUCKET" --key "$KEY" \
        --upload-id "$UPLOAD_ID" --part-number "$part" --body "$PART_FILE" \
        --query ETag --output text)"
    [ "$part" -eq 1 ] || printf ',' >>"$PARTS_JSON"
    python3 - "$etag" "$part" >>"$PARTS_JSON" <<'PY'
import json, sys
print(
    json.dumps(
        {"ETag": sys.argv[1], "PartNumber": int(sys.argv[2])},
        separators=(",", ":"),
    ),
    end="",
)
PY
done
printf ']}\n' >>"$PARTS_JSON"

# A short-lived client is deliberately killed. The daemon-owned completion
# must continue, and the durable upload/job must remain queryable.
set +e
timeout 1 aws --profile "$DASOBJECTSTORE_S3_PROFILE" \
    --endpoint-url "$DASOBJECTSTORE_S3_ENDPOINT" s3api complete-multipart-upload \
    --bucket "$BUCKET" --key "$KEY" --upload-id "$UPLOAD_ID" \
    --multipart-upload "file://$PARTS_JSON" >"$RUN_ROOT/first-completion.json" 2>"$RUN_ROOT/first-completion.stderr"
FIRST_COMPLETE_RC=$?
set -e

# The daemon-owned status must remain visible after the client disappears.
STATUS_BEFORE_RESTART="$RUN_ROOT/status-before-restart.json"
for attempt in $(seq 1 30); do
    if curl --fail --silent --show-error --cacert "$CA_FILE" \
        --cookie "$DASOBJECTSTORE_ACCEPTANCE_COOKIE_FILE" --get \
        --data-urlencode "key=$KEY" \
        "$API_BASE/api/v1/profile-s3/stores/CODEX/multipart/$UPLOAD_ID/status" \
        >"$STATUS_BEFORE_RESTART"; then
        break
    fi
    sleep 1
done
python3 - "$STATUS_BEFORE_RESTART" "$UPLOAD_ID" <<'PY'
import json, sys
status = json.load(open(sys.argv[1], encoding="utf-8"))
assert status["reservation_id"] == sys.argv[2], status
assert status["status"]["job_id"].startswith("mpc-"), status
assert status["status"]["state"] != "committed" or status.get("receipt"), status
PY

sudo systemctl restart "$GATEWAY_SERVICE"
sudo systemctl is-active --quiet "$GATEWAY_SERVICE" || fail "gateway did not recover"
sudo systemctl restart "$DAEMON_SERVICE"
sudo systemctl is-active --quiet "$DAEMON_SERVICE" || fail "daemon did not recover"

# Reissuing the identical completion is the sole recovery action. No upload-part
# command occurs after the interruption.
COMPLETE_RESPONSE="$RUN_ROOT/retried-completion.json"
for attempt in $(seq 1 120); do
    if aws_s3 complete-multipart-upload --bucket "$BUCKET" --key "$KEY" \
        --upload-id "$UPLOAD_ID" --multipart-upload "file://$PARTS_JSON" \
        >"$COMPLETE_RESPONSE" 2>"$RUN_ROOT/retry-$attempt.stderr"; then
        break
    fi
    sleep 1
done
[ -s "$COMPLETE_RESPONSE" ] || fail "completion retry did not converge"
main_object_committed=true

STATUS_COMMITTED="$RUN_ROOT/status-committed.json"
curl --fail --silent --show-error --cacert "$CA_FILE" \
    --cookie "$DASOBJECTSTORE_ACCEPTANCE_COOKIE_FILE" --get \
    --data-urlencode "key=$KEY" \
    "$API_BASE/api/v1/profile-s3/stores/CODEX/multipart/$UPLOAD_ID/status" \
    >"$STATUS_COMMITTED"
python3 - "$STATUS_COMMITTED" "$TOTAL_BYTES" <<'PY'
import json, sys
status = json.load(open(sys.argv[1], encoding="utf-8"))
assert status["status"]["state"] == "committed", status
assert status["status"]["phase"] == "complete", status
assert status["receipt"]["size_bytes"] == int(sys.argv[2]), status
assert status["receipt"]["checksum"].startswith("sha256:"), status
PY

HEAD_JSON="$RUN_ROOT/head.json"
aws_s3 head-object --bucket "$BUCKET" --key "$KEY" >"$HEAD_JSON"
python3 - "$HEAD_JSON" "$TOTAL_BYTES" <<'PY'
import json, sys
head = json.load(open(sys.argv[1], encoding="utf-8"))
assert head["ContentLength"] == int(sys.argv[2]), head
PY

aws_s3 get-object --bucket "$BUCKET" --key "$KEY" "$DOWNLOAD" >/dev/null
ACTUAL_SHA256="$(sha256sum "$DOWNLOAD" | awk '{print $1}')"
EXPECTED_SHA256="$(python3 - "$TOTAL_BYTES" <<'PY'
import hashlib, sys
remaining = int(sys.argv[1])
block = bytes(8 * 1024 * 1024)
digest = hashlib.sha256()
while remaining:
    chunk = block[:min(len(block), remaining)]
    digest.update(chunk)
    remaining -= len(chunk)
print(digest.hexdigest())
PY
)"
[ "$ACTUAL_SHA256" = "$EXPECTED_SHA256" ] || fail "committed object SHA-256 mismatch"

aws_s3 list-objects-v2 --bucket "$BUCKET" --prefix "$KEY" >"$RUN_ROOT/object-list.json"
aws_s3 list-multipart-uploads --bucket "$BUCKET" --prefix "$KEY" >"$RUN_ROOT/upload-list.json"
python3 - "$RUN_ROOT/object-list.json" "$RUN_ROOT/upload-list.json" "$KEY" <<'PY'
import json, sys
objects = json.load(open(sys.argv[1], encoding="utf-8")).get("Contents", [])
uploads = json.load(open(sys.argv[2], encoding="utf-8")).get("Uploads", [])
key = sys.argv[3]
assert [item["Key"] for item in objects] == [key], objects
assert all(item["Key"] != key for item in uploads), uploads
PY

# Create three zero-payload listing canaries: two matching the prefix and one
# deliberately outside it. Pagination must return only the matching pair.
CANARY_PREFIX="codex/acceptance/list-$RUN_ID/"
for suffix in a b; do
    canary_key="${CANARY_PREFIX}${suffix}"
    canary_id="$(aws_s3 create-multipart-upload --bucket "$BUCKET" --key "$canary_key" \
        --query UploadId --output text)"
    cleanup_canaries+=("$canary_key|$canary_id")
done
outside_key="codex/acceptance/outside-$RUN_ID"
outside_id="$(aws_s3 create-multipart-upload --bucket "$BUCKET" --key "$outside_key" \
    --query UploadId --output text)"
cleanup_canaries+=("$outside_key|$outside_id")

PAGE1="$RUN_ROOT/list-page-1.json"
PAGE2="$RUN_ROOT/list-page-2.json"
aws_s3 list-multipart-uploads --bucket "$BUCKET" --prefix "$CANARY_PREFIX" \
    --max-uploads 1 >"$PAGE1"
read -r NEXT_KEY NEXT_UPLOAD < <(python3 - "$PAGE1" "$CANARY_PREFIX" <<'PY'
import json, sys
page = json.load(open(sys.argv[1], encoding="utf-8"))
assert page.get("Prefix") == sys.argv[2], page
assert len(page.get("Uploads", [])) == 1, page
assert page.get("IsTruncated") is True, page
print(page["NextKeyMarker"], page["NextUploadIdMarker"])
PY
)
aws_s3 list-multipart-uploads --bucket "$BUCKET" --prefix "$CANARY_PREFIX" \
    --key-marker "$NEXT_KEY" --upload-id-marker "$NEXT_UPLOAD" \
    --max-uploads 1 >"$PAGE2"
python3 - "$PAGE1" "$PAGE2" "$CANARY_PREFIX" "$outside_key" <<'PY'
import json, sys
one = json.load(open(sys.argv[1], encoding="utf-8"))
two = json.load(open(sys.argv[2], encoding="utf-8"))
prefix, outside = sys.argv[3:]
assert two.get("Prefix") == prefix, two
uploads = one.get("Uploads", []) + two.get("Uploads", [])
keys = [item["Key"] for item in uploads]
assert len(keys) == 2 and len(set(keys)) == 2, keys
assert all(key.startswith(prefix) for key in keys), keys
assert outside not in keys, keys
PY

REPORT="$EVIDENCE_DIR/direct-s3-multipart-recovery-$COMMIT.txt"
{
    printf 'source_commit=%s\n' "$COMMIT"
    printf 'bucket=%s\n' "$BUCKET"
    printf 'key=%s\n' "$KEY"
    printf 'upload_id=%s\n' "$UPLOAD_ID"
    printf 'logical_bytes=%s\n' "$TOTAL_BYTES"
    printf 'part_count=%s\n' "$PART_COUNT"
    printf 'uploaded_parts_after_disconnect=0\n'
    printf 'first_completion_exit=%s\n' "$FIRST_COMPLETE_RC"
    printf 'gateway_restart_recovery=passed\n'
    printf 'daemon_restart_recovery=passed\n'
    printf 'idempotent_completion_retry=passed\n'
    printf 'durable_status_and_receipt=passed\n'
    printf 'head_exact_size=passed\n'
    printf 'download_sha256=%s\n' "$ACTUAL_SHA256"
    printf 'duplicate_or_retransmitted_parts=none\n'
    printf 'prefix_listing_pagination=passed\n'
    printf 'direct_s3_multipart_recovery=passed\n'
} >"$REPORT"
chmod 600 "$REPORT"
printf 'Direct-S3 multipart recovery acceptance passed.\nReport: %s\n' "$REPORT"
