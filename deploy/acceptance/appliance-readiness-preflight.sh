#!/usr/bin/env bash
# Read-only standalone-appliance readiness preflight. It intentionally creates
# no ObjectStore, token, credential, TLS asset, session, or service state.
set -u -o pipefail

ROOT="/"

usage() {
    cat >&2 <<'EOF'
usage: appliance-readiness-preflight.sh

Run as root on a Linux DASObjectStore standalone appliance. The preflight only
reads package, configuration, service, authority, socket, and storage state.
It exits non-zero unless the current Monas/Synoptikon Pistis appliance profile
is ready.
EOF
}

if [ "$#" -gt 0 ]; then
    if [ "$#" -eq 2 ] && [ "$1" = "--root" ] \
        && [ "${DASOBJECTSTORE_PREFLIGHT_TEST_ROOT:-}" = "yes" ] \
        && [ "${2#/}" != "$2" ]; then
        ROOT="${2%/}"
        [ -n "$ROOT" ] || ROOT="/"
    else
        usage
        exit 2
    fi
fi

root_path() {
    if [ "$ROOT" = "/" ]; then
        printf '%s\n' "$1"
    else
        printf '%s%s\n' "$ROOT" "$1"
    fi
}

failures=0

pass() {
    printf 'PASS %s\n' "$1"
}

fail() {
    printf 'FAIL %s: %s\n' "$1" "$2" >&2
    failures=$((failures + 1))
}

require_command() {
    local command_name="$1"
    if command -v "$command_name" >/dev/null 2>&1; then
        pass "command_${command_name}"
        return 0
    fi
    fail "command_${command_name}" "required command is unavailable"
    return 1
}

require_regular_file() {
    local check="$1"
    local path="$2"
    if [ -L "$path" ]; then
        fail "$check" "path must not be a symlink"
    elif [ ! -f "$path" ]; then
        fail "$check" "regular file is absent"
    else
        pass "$check"
    fi
}

require_directory() {
    local check="$1"
    local path="$2"
    if [ -L "$path" ]; then
        fail "$check" "path must not be a symlink"
    elif [ ! -d "$path" ]; then
        fail "$check" "directory is absent"
    else
        pass "$check"
    fi
}

require_owner_mode() {
    local check="$1"
    local path="$2"
    local expected="$3"
    local observed
    if ! observed="$(stat -c '%U:%G:%a' "$path" 2>/dev/null)"; then
        fail "$check" "ownership or mode cannot be read"
    elif [ "$observed" != "$expected" ]; then
        fail "$check" "expected $expected"
    else
        pass "$check"
    fi
}

require_not_world_readable() {
    local check="$1"
    local path="$2"
    local mode
    if ! mode="$(stat -c '%a' "$path" 2>/dev/null)"; then
        fail "$check" "mode cannot be read"
    elif [ $((8#$mode & 0007)) -ne 0 ]; then
        fail "$check" "file is readable, writable, or executable by other users"
    else
        pass "$check"
    fi
}

check_package() {
    if command -v dpkg-query >/dev/null 2>&1; then
        local state
        state="$(dpkg-query -W -f='${db:Status-Abbrev} ${Version}' dasobjectstore 2>/dev/null || true)"
        if [[ "$state" == ii\ * ]]; then
            pass "package_dasobjectstore"
        else
            fail "package_dasobjectstore" "Debian package is not installed"
        fi
    elif command -v rpm >/dev/null 2>&1 && rpm -q dasobjectstore >/dev/null 2>&1; then
        pass "package_dasobjectstore"
    else
        fail "package_dasobjectstore" "neither an installed Debian nor RPM package was found"
    fi
}

check_binary() {
    local name="$1"
    local path
    path="$(root_path "/usr/bin/$name")"
    if [ -L "$path" ] || [ ! -x "$path" ]; then
        fail "binary_${name}" "executable is absent or symlinked"
    else
        pass "binary_${name}"
    fi
}

check_authority_config() {
    local config="$1"
    if python3 - "$config" <<'PY'
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
try:
    value = json.loads(path.read_text())
except (OSError, json.JSONDecodeError):
    raise SystemExit(1)

authentication = value.get("authentication")
if not isinstance(authentication, dict):
    raise SystemExit(1)
if authentication.get("authority") not in {"monas", "synoptikon"}:
    raise SystemExit(1)
if not isinstance(authentication.get("session_ttl_seconds"), int):
    raise SystemExit(1)
if authentication["session_ttl_seconds"] <= 0:
    raise SystemExit(1)
PY
    then
        pass "authentication_authority"
    else
        fail "authentication_authority" "Monas/Synoptikon Pistis authority configuration is absent or invalid"
    fi
}

check_packaged_tls_paths() {
    local config="$1"
    if python3 - "$config" <<'PY'
import json
import pathlib
import sys

try:
    value = json.loads(pathlib.Path(sys.argv[1]).read_text())
except (OSError, json.JSONDecodeError):
    raise SystemExit(1)

tls = value.get("tls")
if not isinstance(tls, dict):
    raise SystemExit(1)
if tls.get("certificate_path") != "/opt/dasobjectstore/tls/server.crt":
    raise SystemExit(1)
if tls.get("private_key_path") != "/opt/dasobjectstore/tls/server.key":
    raise SystemExit(1)
PY
    then
        pass "web_config_tls_paths"
    else
        fail "web_config_tls_paths" "TLS paths must match the documented standalone package paths"
    fi
}

check_store_registry() {
    local registry="$1"
    if python3 - "$registry" <<'PY'
import json
import pathlib
import sys

try:
    definitions = json.loads(pathlib.Path(sys.argv[1]).read_text())
except (OSError, json.JSONDecodeError):
    raise SystemExit(1)
if not isinstance(definitions, list) or not definitions:
    raise SystemExit(1)
if not all(
    isinstance(definition, dict)
    and isinstance(definition.get("store_id"), str)
    and definition["store_id"].strip()
    for definition in definitions
):
    raise SystemExit(1)
PY
    then
        pass "configured_store_registry"
    else
        fail "configured_store_registry" "daemon-owned registry is absent, malformed, or has no configured ObjectStore"
    fi
}

if [ "$(id -u)" -eq 0 ]; then
    pass "operator_root"
else
    fail "operator_root" "run through sudo; the preflight must read protected appliance state"
fi

for command_name in python3 stat getent systemctl; do
    require_command "$command_name" || true
done

check_package

for binary in dasobjectstore dasobjectstored dasobjectstore-server; do
    check_binary "$binary"
done

for group in dasobjectstore dasobjectstore-admin; do
    if getent group "$group" >/dev/null 2>&1; then
        pass "group_${group}"
    else
        fail "group_${group}" "required package group is absent"
    fi
done
if id dasobjectstore >/dev/null 2>&1; then
    pass "service_identity"
else
    fail "service_identity" "dasobjectstore service identity is absent"
fi

daemon_config="$(root_path /etc/dasobjectstore/daemon.json)"
web_config="$(root_path /opt/dasobjectstore/config.json)"
certificate="$(root_path /opt/dasobjectstore/tls/server.crt)"
private_key="$(root_path /opt/dasobjectstore/tls/server.key)"
store_registry="$(root_path /var/lib/dasobjectstore/stores.json)"
managed_root="$(root_path /srv/dasobjectstore)"
ssd_root="$managed_root/ssd"
hdd_root="$managed_root/hdd"
socket="$(root_path /run/dasobjectstore/dasobjectstored.sock)"

require_regular_file "daemon_config" "$daemon_config"
require_owner_mode "daemon_config_permissions" "$daemon_config" "root:dasobjectstore:640"
require_regular_file "web_config" "$web_config"
require_owner_mode "web_config_permissions" "$web_config" "root:dasobjectstore:640"
check_authority_config "$web_config"
check_packaged_tls_paths "$web_config"
if "$(root_path /usr/bin/dasobjectstored)" --config "$daemon_config" --check-config >/dev/null 2>&1; then
    pass "daemon_config_validation"
else
    fail "daemon_config_validation" "daemon rejected the installed configuration"
fi
if "$(root_path /usr/bin/dasobjectstore-server)" --config "$web_config" --check-config --json >/dev/null 2>&1; then
    pass "web_config_validation"
else
    fail "web_config_validation" "Web server rejected the installed configuration"
fi
require_regular_file "tls_certificate" "$certificate"
require_regular_file "tls_private_key" "$private_key"
require_not_world_readable "tls_private_key_permissions" "$private_key"

require_regular_file "store_registry" "$store_registry"
require_owner_mode "store_registry_permissions" "$store_registry" "dasobjectstore:dasobjectstore:640"
check_store_registry "$store_registry"
require_directory "managed_root" "$managed_root"
require_owner_mode "managed_root_permissions" "$managed_root" "dasobjectstore:dasobjectstore:750"
require_directory "ssd_root" "$ssd_root"
require_owner_mode "ssd_root_permissions" "$ssd_root" "dasobjectstore:dasobjectstore:750"
require_directory "hdd_root" "$hdd_root"
require_owner_mode "hdd_root_permissions" "$hdd_root" "root:root:755"
if [ -S "$socket" ] && [ ! -L "$socket" ]; then
    pass "daemon_socket"
else
    fail "daemon_socket" "daemon Unix socket is absent or unsafe"
fi

for service in dasobjectstored.service dasobjectstore-server.service; do
    if systemctl is-active --quiet "$service"; then
        pass "service_${service}"
    else
        fail "service_${service}" "service is not active"
    fi
done

if [ "$failures" -ne 0 ]; then
    printf 'DASObjectStore appliance readiness: NOT READY (%s failed checks)\n' "$failures" >&2
    exit 1
fi

printf 'DASObjectStore appliance readiness: READY (read-only preflight only)\n'
