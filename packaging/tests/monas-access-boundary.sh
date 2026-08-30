#!/usr/bin/env bash
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
helper="$repo_root/packaging/linux/usr/libexec/dasobjectstore/manage-monas-access-boundary"
work="$(mktemp -d /tmp/das-monas-boundary.XXXXXX)"
trap 'rm -rf "$work"' EXIT
root="$work/root"; state="$work/state"; bin="$work/bin"
mkdir -p "$root/var/lib/dasobjectstore/auth" "$root/run/dasobjectstore" \
  "$root/srv/dasobjectstore" "$root/opt/dasobjectstore/tls" "$state" "$bin"
printf '[]\n' >"$root/var/lib/dasobjectstore/stores.json"
printf '{"schema_version":"dasobjectstore.appliance_identity.v1","appliance_id":"das-appliance-test"}\n' > \
  "$root/var/lib/dasobjectstore/appliance-identity.json"

cat >"$bin/getent" <<'EOF'
#!/bin/sh
[ "$1:$2" = group:mnemosyne-pistis-das ] && [ ! -e "$TEST_STATE/missing-group" ]
EOF
cat >"$bin/stat" <<'EOF'
#!/bin/sh
format=$2; path=$3; owner=dasobjectstore; group=mnemosyne-pistis-das; mode=750
case "$path" in */stores.json|*/appliance-identity.json) mode=640;; */dasobjectstored.sock) mode=660;; */auth|*/srv/dasobjectstore|*/tls) group=dasobjectstore;; esac
[ ! -e "$TEST_STATE/wrong-group" ] || group=root
[ ! -e "$TEST_STATE/identity-private-group" ] || { case "$path" in */appliance-identity.json) group=dasobjectstore;; esac; }
[ ! -e "$TEST_STATE/identity-legacy-mode" ] || { case "$path" in */appliance-identity.json) mode=650;; esac; }
[ ! -e "$TEST_STATE/private-shared" ] || { case "$path" in */auth|*/srv/dasobjectstore|*/tls) group=mnemosyne-pistis-das;; esac; }
links=1; [ ! -e "$TEST_STATE/identity-hardlink" ] || links=2
case "$format" in %U) echo "$owner";; %G) echo "$group";; %a) echo "$mode";; %h) echo "$links";; *) exit 64;; esac
EOF
cat >"$bin/ss" <<'EOF'
#!/bin/sh
[ ! -e "$TEST_STATE/live" ] || printf 'u_str LISTEN 0 4096 %s 1 * 0\n' "$TEST_SOCKET"
EOF
cat >"$bin/chgrp" <<'EOF'
#!/bin/sh
set -eu
[ "$1" = mnemosyne-pistis-das ] && { [ "$2" = "$TEST_SOCKET" ] || [ "$2" = "$TEST_IDENTITY" ]; }
[ "$2" != "$TEST_IDENTITY" ] || rm -f "$TEST_STATE/identity-private-group"
EOF
cat >"$bin/chmod" <<'EOF'
#!/bin/sh
set -eu
[ "$1" = 0640 ] && [ "$2" = "$TEST_IDENTITY" ]
rm -f "$TEST_STATE/identity-legacy-mode"
EOF
cat >"$bin/unlink" <<'EOF'
#!/bin/sh
[ "$1" = "$TEST_SOCKET" ] && /bin/unlink "$1"
EOF
chmod 0755 "$bin"/*
run() { TEST_STATE="$state" TEST_SOCKET="$root/run/dasobjectstore/dasobjectstored.sock" TEST_IDENTITY="$root/var/lib/dasobjectstore/appliance-identity.json" PATH="$bin:$PATH" "$helper" --test-root "$root" "$1"; }
deny() { run "$1" >/dev/null 2>&1 && exit 1 || true; }

run pre-start
run publish-identity
touch "$state/identity-private-group" "$state/identity-legacy-mode"
run publish-identity
[ ! -e "$state/identity-private-group" ] && [ ! -e "$state/identity-legacy-mode" ]
for marker in missing-group wrong-group private-shared; do touch "$state/$marker"; deny pre-start; rm "$state/$marker"; done
mv "$root/var/lib/dasobjectstore/stores.json" "$work/stores.json"; deny pre-start; mv "$work/stores.json" "$root/var/lib/dasobjectstore/stores.json"
mv "$root/var/lib/dasobjectstore/appliance-identity.json" "$work/appliance-identity.json"; run pre-start; mv "$work/appliance-identity.json" "$root/var/lib/dasobjectstore/appliance-identity.json"
touch "$state/identity-hardlink"; deny publish-identity; rm "$state/identity-hardlink"
printf '{}\n' >"$root/var/lib/dasobjectstore/pistis-grants.json"; deny pre-start; rm "$root/var/lib/dasobjectstore/pistis-grants.json"
python3 - "$root/run/dasobjectstore/dasobjectstored.sock" <<'PY'
import socket, sys
s=socket.socket(socket.AF_UNIX); s.bind(sys.argv[1]); s.close()
PY
deny pre-start
deny publish-socket
touch "$state/live"; run publish-socket
rm "$state/live"; run retire-socket
[[ ! -e "$root/run/dasobjectstore/dasobjectstored.sock" ]]
echo 'DASObjectStore Monas package access boundary: pass'
