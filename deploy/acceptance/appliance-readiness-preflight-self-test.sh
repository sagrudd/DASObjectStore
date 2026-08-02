#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PREFLIGHT="$SCRIPT_DIR/appliance-readiness-preflight.sh"
TMP="$(mktemp -d /tmp/das-preflight.XXXXXX)"
trap 'rm -rf "$TMP"' EXIT

ROOT="$TMP/root"
BIN="$TMP/bin"
mkdir -p \
    "$BIN" \
    "$ROOT/usr/bin" \
    "$ROOT/etc/dasobjectstore" \
    "$ROOT/etc/pam.d" \
    "$ROOT/opt/dasobjectstore/tls" \
    "$ROOT/var/lib/dasobjectstore" \
    "$ROOT/srv/dasobjectstore/ssd" \
    "$ROOT/srv/dasobjectstore/hdd" \
    "$ROOT/run/dasobjectstore"

for binary in dasobjectstore dasobjectstored dasobjectstore-server; do
    cat >"$ROOT/usr/bin/$binary" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
    chmod 0755 "$ROOT/usr/bin/$binary"
done

cat >"$ROOT/etc/dasobjectstore/daemon.json" <<'EOF'
{"socket_path":"/run/dasobjectstore/dasobjectstored.sock"}
EOF
cat >"$ROOT/opt/dasobjectstore/config.json" <<'EOF'
{
  "authentication": {"authority":"local_user","session_ttl_seconds":3600},
  "tls": {
    "certificate_path":"/opt/dasobjectstore/tls/server.crt",
    "private_key_path":"/opt/dasobjectstore/tls/server.key"
  }
}
EOF
printf 'auth required pam_unix.so\n' >"$ROOT/etc/pam.d/dasobjectstore"
printf 'certificate fixture\n' >"$ROOT/opt/dasobjectstore/tls/server.crt"
printf 'private-key fixture\n' >"$ROOT/opt/dasobjectstore/tls/server.key"
printf '[{"store_id":"CODEX"}]\n' >"$ROOT/var/lib/dasobjectstore/stores.json"

python3 - "$ROOT/run/dasobjectstore/dasobjectstored.sock" <<'PY'
import socket
import sys

listener = socket.socket(socket.AF_UNIX)
listener.bind(sys.argv[1])
listener.close()
PY

cat >"$BIN/id" <<'EOF'
#!/usr/bin/env bash
if [ "$1" = "-u" ]; then
    printf '0\n'
fi
exit 0
EOF
cat >"$BIN/getent" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
cat >"$BIN/systemctl" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
cat >"$BIN/dpkg-query" <<'EOF'
#!/usr/bin/env bash
printf 'ii 0.1.2\n'
EOF
cat >"$BIN/stat" <<'EOF'
#!/usr/bin/env bash
format="$2"
path="$3"
if [ "$format" = "%a" ]; then
    printf '640\n'
elif [[ "$path" == */srv/dasobjectstore/hdd ]]; then
    printf 'root:root:755\n'
elif [[ "$path" == */etc/dasobjectstore/* || "$path" == */opt/dasobjectstore/config.json ]]; then
    printf 'root:dasobjectstore:640\n'
elif [[ "$path" == */var/lib/dasobjectstore/stores.json ]]; then
    printf 'dasobjectstore:dasobjectstore:640\n'
else
    printf 'dasobjectstore:dasobjectstore:750\n'
fi
EOF
chmod 0755 "$BIN"/*

run_preflight() {
    PATH="$BIN:$PATH" DASOBJECTSTORE_PREFLIGHT_TEST_ROOT=yes \
        "$PREFLIGHT" --root "$ROOT"
}

run_preflight

python3 - "$ROOT/opt/dasobjectstore/config.json" <<'PY'
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
value = json.loads(path.read_text())
value["authentication"]["authority"] = "unconfigured"
path.write_text(json.dumps(value))
PY
if run_preflight >"$TMP/authority.out" 2>&1; then
    printf 'error: preflight accepted missing authenticated authority\n' >&2
    exit 1
fi
grep -Fq 'FAIL authentication_authority' "$TMP/authority.out"

python3 - "$ROOT/opt/dasobjectstore/config.json" <<'PY'
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
value = json.loads(path.read_text())
value["authentication"]["authority"] = "local_user"
value["tls"]["certificate_path"] = "/unapproved/tls/server.crt"
path.write_text(json.dumps(value))
PY
if run_preflight >"$TMP/tls-path.out" 2>&1; then
    printf 'error: preflight accepted an unapproved TLS certificate path\n' >&2
    exit 1
fi
grep -Fq 'FAIL web_config_tls_paths' "$TMP/tls-path.out"

python3 - "$ROOT/opt/dasobjectstore/config.json" <<'PY'
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
value = json.loads(path.read_text())
value["tls"]["certificate_path"] = "/opt/dasobjectstore/tls/server.crt"
path.write_text(json.dumps(value))
PY
printf '[]\n' >"$ROOT/var/lib/dasobjectstore/stores.json"
if run_preflight >"$TMP/storage.out" 2>&1; then
    printf 'error: preflight accepted empty storage authority\n' >&2
    exit 1
fi
grep -Fq 'FAIL configured_store_registry' "$TMP/storage.out"

printf '[{"store_id":"   "}]\n' >"$ROOT/var/lib/dasobjectstore/stores.json"
if run_preflight >"$TMP/blank-store-id.out" 2>&1; then
    printf 'error: preflight accepted a blank ObjectStore identifier\n' >&2
    exit 1
fi
grep -Fq 'FAIL configured_store_registry' "$TMP/blank-store-id.out"

printf 'appliance readiness preflight self-test passed\n'
