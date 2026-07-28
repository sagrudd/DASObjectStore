#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

cat >"$TMP/valid.json" <<'EOF'
{
  "schema_version": "dasobjectstore.epic-c-appliance-evidence.v1",
  "mode": "inspect",
  "safety": {
    "store_id": "CODEX",
    "generated_bytes": 0,
    "customer_or_project_data_used": false
  }
}
EOF
python3 "$SCRIPT_DIR/epic-c-appliance.py" validate "$TMP/valid.json"

python3 - "$TMP/valid.json" "$TMP/invalid.json" <<'PY'
import json, sys
record = json.load(open(sys.argv[1]))
record["safety"]["store_id"] = "project-data"
json.dump(record, open(sys.argv[2], "w"))
PY
if python3 "$SCRIPT_DIR/epic-c-appliance.py" validate "$TMP/invalid.json"; then
    printf 'error: unsafe store identity was accepted\n' >&2
    exit 1
fi

if DASOBJECTSTORE_CODEX_VALIDATION_ROOT="$TMP/not-approved" \
    "$SCRIPT_DIR/epic-c-appliance.sh" inspect >/dev/null 2>&1; then
    printf 'error: validation root outside the approved boundary was accepted\n' >&2
    exit 1
fi

printf 'EPIC C appliance harness safety self-test passed\n'
