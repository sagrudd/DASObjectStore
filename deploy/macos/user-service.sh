#!/usr/bin/env bash
set -euo pipefail

# Install the render-only daemon plan as a per-user macOS launchd service.
# Persistent state and configuration are deliberately retained on uninstall.

umask 077

DEFAULT_LABEL="org.dasobjectstore.dasobjectstored"
COMMAND="${1:-}"
[ -n "$COMMAND" ] && shift || true

LABEL="$DEFAULT_LABEL"
HOME_DIR="${HOME:-}"
STATE_HOME="${XDG_STATE_HOME:-}"
RUNTIME_HOME="${XDG_RUNTIME_DIR:-}"
EXECUTABLE=""
CONFIG=""
DAS_BIN="${DASOBJECTSTORE_BIN:-dasobjectstore}"
LAUNCHCTL="${DASOBJECTSTORE_LAUNCHCTL:-/bin/launchctl}"
PLUTIL="${DASOBJECTSTORE_PLUTIL:-/usr/bin/plutil}"

die() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

usage() {
    cat <<'EOF'
Usage: user-service.sh <install|status|uninstall|print> [options]

Options:
  --executable PATH  Absolute dasobjectstored path (install/print).
  --config PATH      Absolute daemon configuration path (install/print).
  --label LABEL      launchd label (default: org.dasobjectstore.dasobjectstored).
  --home PATH        User home (default: HOME).
  --state-home PATH  XDG state home (default: XDG_STATE_HOME or HOME/.local/state).
  --runtime-home PATH
                     XDG runtime directory when one is available.
  --dasobjectstore PATH
                     dasobjectstore CLI used to render the validated plan.

Install and uninstall affect only the invoking user's gui/<uid> launchd domain.
Uninstall never removes daemon configuration or persistent state.
EOF
}

absolute_path() {
    case "$2" in
        /*) ;;
        *) die "$1 must be absolute: $2" ;;
    esac
}

owner_uid() {
    /usr/bin/stat -f '%u' "$1"
}

require_owned_directory() {
    local path="$1"
    [ -d "$path" ] || die "directory is unavailable: $path"
    [ ! -L "$path" ] || die "refusing symlinked directory: $path"
    [ "$(owner_uid "$path")" = "$UID_NUMBER" ] || \
        die "directory is not owned by uid $UID_NUMBER: $path"
}

ensure_owned_directory() {
    local path="$1"
    local mode="$2"
    if [ -e "$path" ] || [ -L "$path" ]; then
        require_owned_directory "$path"
    else
        /bin/mkdir -p "$path"
        require_owned_directory "$path"
        /bin/chmod "$mode" "$path"
    fi
}

require_regular_file() {
    local description="$1"
    local path="$2"
    [ -f "$path" ] || die "$description is not a regular file: $path"
}

service_loaded() {
    "$LAUNCHCTL" print "$SERVICE_TARGET" >/dev/null 2>&1
}

render_plan() {
    local destination="$1"
    local lint="${2:-yes}"
    local args=(
        store user-service-plan
        --executable "$EXECUTABLE"
        --config "$CONFIG"
        --home "$HOME_DIR"
        --state-home "$STATE_HOME"
        --label "$LABEL"
    )
    if [ -n "$RUNTIME_HOME" ]; then
        args+=(--runtime-home "$RUNTIME_HOME")
    fi
    "$DAS_BIN" "${args[@]}" > "$destination"
    if [ "$lint" = "yes" ]; then
        "$PLUTIL" -lint "$destination" >/dev/null
    fi
}

rollback_install() {
    "$LAUNCHCTL" bootout "$SERVICE_TARGET" >/dev/null 2>&1 || true
    /bin/rm -f "$PLIST_PATH"
    if [ -n "$BACKUP_PATH" ] && [ -f "$BACKUP_PATH" ]; then
        /bin/mv "$BACKUP_PATH" "$PLIST_PATH"
        if [ "$WAS_LOADED" = "yes" ]; then
            "$LAUNCHCTL" bootstrap "$SERVICE_DOMAIN" "$PLIST_PATH" >/dev/null 2>&1 || true
        fi
    fi
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --executable) [ "$#" -ge 2 ] || die "--executable requires a value"; EXECUTABLE="$2"; shift 2 ;;
        --config) [ "$#" -ge 2 ] || die "--config requires a value"; CONFIG="$2"; shift 2 ;;
        --label) [ "$#" -ge 2 ] || die "--label requires a value"; LABEL="$2"; shift 2 ;;
        --home) [ "$#" -ge 2 ] || die "--home requires a value"; HOME_DIR="$2"; shift 2 ;;
        --state-home) [ "$#" -ge 2 ] || die "--state-home requires a value"; STATE_HOME="$2"; shift 2 ;;
        --runtime-home) [ "$#" -ge 2 ] || die "--runtime-home requires a value"; RUNTIME_HOME="$2"; shift 2 ;;
        --dasobjectstore) [ "$#" -ge 2 ] || die "--dasobjectstore requires a value"; DAS_BIN="$2"; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *) die "unknown option: $1" ;;
    esac
done

case "$COMMAND" in
    install|status|uninstall|print) ;;
    ""|-h|--help) usage; exit 0 ;;
    *) usage >&2; die "unknown command: $COMMAND" ;;
esac

[ "$(/usr/bin/uname -s)" = "Darwin" ] || die "per-user launchd installation requires macOS"
UID_NUMBER="$(/usr/bin/id -u)"
[ "$UID_NUMBER" -ne 0 ] || die "per-user launchd installation must not run as root or through sudo"
[ -n "$HOME_DIR" ] || die "HOME or --home is required"
absolute_path "home" "$HOME_DIR"
require_owned_directory "$HOME_DIR"

[[ "$LABEL" =~ ^[A-Za-z0-9._-]+$ ]] || die "invalid launchd label: $LABEL"
LAUNCH_AGENTS="$HOME_DIR/Library/LaunchAgents"
PLIST_PATH="$LAUNCH_AGENTS/$LABEL.plist"
SERVICE_DOMAIN="gui/$UID_NUMBER"
SERVICE_TARGET="$SERVICE_DOMAIN/$LABEL"

if [ -z "$STATE_HOME" ]; then
    STATE_HOME="$HOME_DIR/.local/state"
fi
absolute_path "state home" "$STATE_HOME"
STATE_DIR="$STATE_HOME/dasobjectstore"

case "$COMMAND" in
    print)
        [ -n "$EXECUTABLE" ] || die "print requires --executable"
        [ -n "$CONFIG" ] || die "print requires --config"
        absolute_path "daemon executable" "$EXECUTABLE"
        absolute_path "daemon config" "$CONFIG"
        # The CLI has already validated the definition; linting is reserved for
        # the on-disk installation candidate because /dev/stdout is not a file.
        render_plan /dev/stdout no
        ;;
    status)
        if service_loaded; then
            printf 'loaded: %s\nplist: %s\nstate: %s\n' "$SERVICE_TARGET" "$PLIST_PATH" "$STATE_DIR"
        else
            printf 'not loaded: %s\nplist: %s\nstate: %s\n' "$SERVICE_TARGET" "$PLIST_PATH" "$STATE_DIR"
            exit 3
        fi
        ;;
    uninstall)
        if service_loaded; then
            "$LAUNCHCTL" bootout "$SERVICE_TARGET"
        fi
        if [ -e "$PLIST_PATH" ] || [ -L "$PLIST_PATH" ]; then
            [ ! -L "$PLIST_PATH" ] || die "refusing symlinked service plist: $PLIST_PATH"
            [ "$(owner_uid "$PLIST_PATH")" = "$UID_NUMBER" ] || \
                die "service plist is not owned by uid $UID_NUMBER: $PLIST_PATH"
            /bin/rm "$PLIST_PATH"
        fi
        printf 'uninstalled %s; retained state at %s\n' "$LABEL" "$STATE_DIR"
        ;;
    install)
        [ -n "$EXECUTABLE" ] || die "install requires --executable"
        [ -n "$CONFIG" ] || die "install requires --config"
        absolute_path "daemon executable" "$EXECUTABLE"
        absolute_path "daemon config" "$CONFIG"
        require_regular_file "daemon executable" "$EXECUTABLE"
        [ -x "$EXECUTABLE" ] || die "daemon executable is not executable: $EXECUTABLE"
        require_regular_file "daemon config" "$CONFIG"
        ensure_owned_directory "$HOME_DIR/Library" 700
        ensure_owned_directory "$LAUNCH_AGENTS" 700
        ensure_owned_directory "$STATE_HOME" 700
        ensure_owned_directory "$STATE_DIR" 700
        ensure_owned_directory "$STATE_DIR/logs" 700
        /bin/chmod 700 "$STATE_DIR" "$STATE_DIR/logs"

        TEMP_PATH="$(/usr/bin/mktemp "$LAUNCH_AGENTS/.$LABEL.plist.XXXXXX")"
        BACKUP_PATH=""
        WAS_LOADED="no"
        trap '/bin/rm -f "$TEMP_PATH" "$BACKUP_PATH"' EXIT
        render_plan "$TEMP_PATH"
        /bin/chmod 600 "$TEMP_PATH"

        if [ -e "$PLIST_PATH" ] || [ -L "$PLIST_PATH" ]; then
            [ ! -L "$PLIST_PATH" ] || die "refusing symlinked service plist: $PLIST_PATH"
            [ "$(owner_uid "$PLIST_PATH")" = "$UID_NUMBER" ] || \
                die "service plist is not owned by uid $UID_NUMBER: $PLIST_PATH"
            BACKUP_PATH="$(/usr/bin/mktemp "$LAUNCH_AGENTS/.$LABEL.backup.XXXXXX")"
            /bin/cp -p "$PLIST_PATH" "$BACKUP_PATH"
        fi
        if service_loaded; then
            WAS_LOADED="yes"
            "$LAUNCHCTL" bootout "$SERVICE_TARGET"
        fi
        /bin/mv "$TEMP_PATH" "$PLIST_PATH"
        TEMP_PATH=""
        if ! "$LAUNCHCTL" bootstrap "$SERVICE_DOMAIN" "$PLIST_PATH" || \
           ! "$LAUNCHCTL" kickstart -k "$SERVICE_TARGET"; then
            rollback_install
            die "launchd rejected the service; the previous installation was restored"
        fi
        /bin/rm -f "$BACKUP_PATH"
        BACKUP_PATH=""
        trap - EXIT
        printf 'installed and started %s\nplist: %s\nstate: %s\n' \
            "$SERVICE_TARGET" "$PLIST_PATH" "$STATE_DIR"
        ;;
esac
