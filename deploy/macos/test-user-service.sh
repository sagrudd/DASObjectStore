#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ORIGINAL_HOME="${HOME:?HOME is required}"
VALIDATION_ROOT="${DASOBJECTSTORE_CODEX_VALIDATION_ROOT:-$ORIGINAL_HOME/.dasobjectstore-codex-validation}"
case "$VALIDATION_ROOT" in
    "$ORIGINAL_HOME/.dasobjectstore-codex-validation"|"$ORIGINAL_HOME/.dasobjectstore-codex-validation"/*) ;;
    *) printf 'error: validation root must remain beneath %s/.dasobjectstore-codex-validation\n' "$ORIGINAL_HOME" >&2; exit 1 ;;
esac

ROOT="$VALIDATION_ROOT/launchd-user-service-$$"
cleanup() {
    local status="$?"
    /bin/rm -rf "$ROOT"
    exit "$status"
}
trap cleanup EXIT
/bin/mkdir -p "$ROOT/home/Library" "$ROOT/bin" "$ROOT/config"

FAKE_DAS="$ROOT/bin/dasobjectstore"
FAKE_DAEMON="$ROOT/bin/dasobjectstored"
FAKE_LAUNCHCTL="$ROOT/bin/launchctl"
FAKE_PLUTIL="$ROOT/bin/plutil"
LAUNCH_LOG="$ROOT/launchctl.log"
LOADED="$ROOT/loaded"
CONFIG="$ROOT/config/daemon.json"

printf '#!/usr/bin/env bash\nexit 0\n' > "$FAKE_DAEMON"
printf '{}\n' > "$CONFIG"
cat > "$FAKE_DAS" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
label=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        --label) label="$2"; shift 2 ;;
        *) shift ;;
    esac
done
[ -n "$label" ]
cat <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict><key>Label</key><string>$label</string></dict></plist>
PLIST
EOF
cat > "$FAKE_LAUNCHCTL" <<EOF
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "\$*" >> "$LAUNCH_LOG"
case "\$1" in
    print) [ -f "$LOADED" ] ;;
    bootstrap)
        if [ -f "$ROOT/reject-bootstrap-once" ]; then
            /bin/rm "$ROOT/reject-bootstrap-once"
            exit 1
        fi
        /usr/bin/touch "$LOADED"
        ;;
    kickstart) [ -f "$LOADED" ] ;;
    bootout) /bin/rm -f "$LOADED" ;;
    *) exit 64 ;;
esac
EOF
cat > "$FAKE_PLUTIL" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
[ "$1" = "-lint" ]
/usr/bin/grep -q '<plist' "$2"
EOF
/bin/chmod 700 "$FAKE_DAS" "$FAKE_DAEMON" "$FAKE_LAUNCHCTL" "$FAKE_PLUTIL"

export HOME="$ROOT/home"
export DASOBJECTSTORE_BIN="$FAKE_DAS"
export DASOBJECTSTORE_LAUNCHCTL="$FAKE_LAUNCHCTL"
export DASOBJECTSTORE_PLUTIL="$FAKE_PLUTIL"

LABEL="org.dasobjectstore.test.$$"
COMMON=(--home "$HOME" --label "$LABEL")
INSTALL=("${COMMON[@]}" --executable "$FAKE_DAEMON" --config "$CONFIG")
PLIST="$HOME/Library/LaunchAgents/$LABEL.plist"
STATE="$HOME/.local/state/dasobjectstore"

"$SCRIPT_DIR/user-service.sh" print "${INSTALL[@]}" | /usr/bin/grep -q "<string>$LABEL</string>"
[ ! -e "$PLIST" ]
[ ! -e "$STATE" ]

"$SCRIPT_DIR/user-service.sh" install "${INSTALL[@]}"
[ -f "$PLIST" ]
[ -f "$LOADED" ]
[ "$(/usr/bin/stat -f '%Lp' "$PLIST")" = "600" ]
[ "$(/usr/bin/stat -f '%Lp' "$STATE")" = "700" ]
"$SCRIPT_DIR/user-service.sh" status "${COMMON[@]}" >/dev/null

printf 'persistent sentinel\n' > "$STATE/sentinel"
OLD_SUM="$(/sbin/md5 -q "$PLIST")"
/usr/bin/touch "$ROOT/reject-bootstrap-once"
if "$SCRIPT_DIR/user-service.sh" install "${INSTALL[@]}" >/dev/null 2>&1; then
    printf 'error: rejected bootstrap unexpectedly succeeded\n' >&2
    exit 1
fi
[ "$(/sbin/md5 -q "$PLIST")" = "$OLD_SUM" ]
[ -f "$LOADED" ]

"$SCRIPT_DIR/user-service.sh" install "${INSTALL[@]}" >/dev/null
"$SCRIPT_DIR/user-service.sh" uninstall "${COMMON[@]}"
[ ! -e "$PLIST" ]
[ ! -e "$LOADED" ]
[ -f "$STATE/sentinel" ]
if "$SCRIPT_DIR/user-service.sh" status "${COMMON[@]}" >/dev/null 2>&1; then
    printf 'error: uninstalled service reported loaded\n' >&2
    exit 1
else
    [ "$?" -eq 3 ]
fi

/usr/bin/grep -q "bootstrap gui/$(/usr/bin/id -u) $PLIST" "$LAUNCH_LOG"
/usr/bin/grep -q "kickstart -k gui/$(/usr/bin/id -u)/$LABEL" "$LAUNCH_LOG"
EVIDENCE_DIR="$VALIDATION_ROOT/deployment-evidence"
COMMIT="$(git -C "$SCRIPT_DIR/../.." rev-parse HEAD 2>/dev/null || printf 'unavailable')"
/bin/mkdir -p "$EVIDENCE_DIR"
/bin/chmod 700 "$EVIDENCE_DIR"
{
    printf 'source_commit=%s\n' "$COMMIT"
    printf 'platform=macos\n'
    printf 'architecture=%s\n' "$(/usr/bin/uname -m)"
    printf 'render=passed\ninstall=passed\nstatus=passed\n'
    printf 'rollback=passed\nreinstall=passed\nuninstall=passed\n'
    printf 'persistent_state_retained=yes\n'
} > "$EVIDENCE_DIR/macos-launchd-$COMMIT.txt"
/bin/chmod 600 "$EVIDENCE_DIR/macos-launchd-$COMMIT.txt"
printf 'macOS per-user launchd deployment acceptance passed\n'
