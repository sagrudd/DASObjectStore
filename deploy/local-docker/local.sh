#!/usr/bin/env bash
set -euo pipefail

# Local macOS profile: DASObjectStore daemon in Docker, Garage as the
# daemon-owned S3 provider, and all persistent state on an attached volume.

umask 077

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
BUILD_CONTEXT="${DASOBJECTSTORE_BUILD_CONTEXT:-$(cd "$REPO_DIR/.." && pwd)}"
ROOT="${DASOBJECTSTORE_LOCAL_ROOT:-/Volumes/Seagate/DASObjectStore}"
PROFILE="${DASOBJECTSTORE_LOCAL_PROFILE:-alleleanchor-mvp}"
PROJECT="${DASOBJECTSTORE_LOCAL_PROJECT:-dasobjectstore-local}"
API_PORT="${DASOBJECTSTORE_LOCAL_API_PORT:-3900}"
GARAGE_IMAGE="${DASOBJECTSTORE_GARAGE_IMAGE:-dxflrs/garage:v2.3.0}"
DAEMON_IMAGE="${DASOBJECTSTORE_LOCAL_IMAGE:-dasobjectstore-local:dev}"

PROFILE_ROOT="$ROOT/$PROFILE"
CONFIG_DIR="$PROFILE_ROOT/config"
STATE_DIR="$PROFILE_ROOT/state"
RUNTIME_DIR="$PROFILE_ROOT/runtime"
LOG_DIR="$PROFILE_ROOT/logs"
META_DIR="$PROFILE_ROOT/garage-meta"
DATA_DIR="$PROFILE_ROOT/garage-data"
CREDENTIALS_DIR="$PROFILE_ROOT/credentials"
REGISTRY_PATH="$CONFIG_DIR/stores.json"
DEVICE_MARKER="$PROFILE_ROOT/.dasobjectstore/device.env"
GARAGE_PROJECT_DIR="$STATE_DIR/garage"
GARAGE_COMPOSE="$CONFIG_DIR/garage.compose.yml"
GARAGE_CONFIG="$CONFIG_DIR/garage.toml"
DAEMON_CONFIG="$CONFIG_DIR/daemon.json"
STACK_COMPOSE="$PROFILE_ROOT/compose.yml"
GARAGE_SECRETS="$CREDENTIALS_DIR/garage.env"
ALLELEANCHOR_CONFIG="$CREDENTIALS_DIR/alleleanchor-store.toml"
GARAGE_CREDENTIAL_REGISTRY="$STATE_DIR/object-service/garage-credentials.json"

die() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

usage() {
    cat <<'EOF'
Usage: local.sh <command>

Commands:
  render       Create the Seagate-backed daemon and Garage Compose files.
  build        Build the daemon image from the DASObjectStore workspace.
  up           Render, build, start the daemon, start Garage, and provision stores.
  provision    Re-run daemon-owned Garage bucket/key provisioning and export config.
  status       Show daemon and nested Garage Compose status without secrets.
  down         Stop Garage through the daemon, then stop the daemon container.
  config       Print the generated AlleleAnchor adapter config path.

Configuration is supplied through environment variables. The defaults target
/Volumes/Seagate/DASObjectStore and the local alleleanchor-mvp profile.
EOF
}

require_volume_root() {
    case "$ROOT" in
        /Volumes/*) ;;
        *) die "DASOBJECTSTORE_LOCAL_ROOT must be under /Volumes (got $ROOT)" ;;
    esac
    [ -d "/Volumes" ] || die "/Volumes is unavailable"
    [ -d "$ROOT" ] || mkdir -p "$ROOT"
    [ -w "$ROOT" ] || die "DAS root is not writable: $ROOT"
}

validate_profile_name() {
    local label="$1"
    local value="$2"
    [[ "$value" =~ ^[a-z0-9][a-z0-9_-]*$ ]] || \
        die "$label must contain only lowercase letters, digits, hyphens, or underscores (got $value)"
}

validate_port() {
    [[ "$API_PORT" =~ ^[0-9]+$ ]] && [ "$API_PORT" -ge 1 ] && [ "$API_PORT" -le 65535 ] || \
        die "DASOBJECTSTORE_LOCAL_API_PORT must be between 1 and 65535 (got $API_PORT)"
}

require_build_context() {
    [ -f "$BUILD_CONTEXT/DASObjectStore/Cargo.toml" ] || \
        die "build context is missing DASObjectStore/Cargo.toml: $BUILD_CONTEXT"
    [ -f "$BUILD_CONTEXT/prosopikon/Cargo.toml" ] || \
        die "build context is missing prosopikon/Cargo.toml: $BUILD_CONTEXT"
}

require_command() {
    command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

das_bin() {
    if [ -n "${DASOBJECTSTORE_BIN:-}" ]; then
        [ -x "$DASOBJECTSTORE_BIN" ] || die "DASOBJECTSTORE_BIN is not executable"
        printf '%s\n' "$DASOBJECTSTORE_BIN"
        return
    fi
    if command -v dasobjectstore >/dev/null 2>&1; then
        command -v dasobjectstore
        return
    fi
    for candidate in \
        "$REPO_DIR/target/debug/dasobjectstore" \
        "$REPO_DIR/target/release/dasobjectstore"; do
        if [ -x "$candidate" ]; then
            printf '%s\n' "$candidate"
            return
        fi
    done
    die "dasobjectstore binary not found; build it or set DASOBJECTSTORE_BIN"
}

ensure_profile_dirs() {
    require_volume_root
    mkdir -p "$CONFIG_DIR" "$STATE_DIR" "$RUNTIME_DIR" "$LOG_DIR" \
        "$META_DIR" "$DATA_DIR" "$CREDENTIALS_DIR" "$GARAGE_PROJECT_DIR" \
        "$(dirname "$REGISTRY_PATH")"
    chmod 700 "$PROFILE_ROOT" "$CONFIG_DIR" "$STATE_DIR" "$RUNTIME_DIR" \
        "$LOG_DIR" "$META_DIR" "$DATA_DIR" "$CREDENTIALS_DIR"
}

ensure_local_ssd_marker() {
    cat > "$DEVICE_MARKER" <<EOF
role=ssd
mount=$PROFILE_ROOT
transport=local-docker
EOF
    chmod 600 "$DEVICE_MARKER"
}

env_value() {
    local name="$1"
    [ -f "$GARAGE_SECRETS" ] || return 1
    awk -F= -v name="$name" '$1 == name { print substr($0, index($0, "=") + 1); exit }' \
        "$GARAGE_SECRETS"
}

ensure_garage_secrets() {
    require_command openssl
    if [ ! -s "$GARAGE_SECRETS" ]; then
        cat > "$GARAGE_SECRETS" <<EOF
GARAGE_RPC_SECRET=$(openssl rand -hex 32)
GARAGE_ADMIN_TOKEN=$(openssl rand -hex 32)
GARAGE_METRICS_TOKEN=$(openssl rand -hex 32)
EOF
        chmod 600 "$GARAGE_SECRETS"
    fi
    for name in GARAGE_RPC_SECRET GARAGE_ADMIN_TOKEN GARAGE_METRICS_TOKEN; do
        [ -n "$(env_value "$name" || true)" ] || die "missing $name in $GARAGE_SECRETS"
    done
}

render_garage_config() {
    ensure_garage_secrets
    cat > "$GARAGE_CONFIG" <<EOF
metadata_dir = "/var/lib/garage/meta"
data_dir = "/var/lib/garage/data"
db_engine = "sqlite"
replication_factor = 1
compression_level = 0
block_size = "10M"

rpc_bind_addr = "[::]:3901"
rpc_public_addr = "127.0.0.1:3901"
rpc_secret = "$(env_value GARAGE_RPC_SECRET)"

[s3_api]
s3_region = "garage"
api_bind_addr = "[::]:3900"

[s3_web]
bind_addr = "[::]:3902"
root_domain = ".web.garage.localhost"
index = "index.html"

[admin]
api_bind_addr = "[::]:3903"
admin_token = "$(env_value GARAGE_ADMIN_TOKEN)"
metrics_token = "$(env_value GARAGE_METRICS_TOKEN)"
EOF
    chmod 600 "$GARAGE_CONFIG"
}

render_daemon_config() {
    cat > "$DAEMON_CONFIG" <<'EOF'
{
  "service_user": "dasobjectstore",
  "service_group": "dasobjectstore",
  "config_path": "/etc/dasobjectstore/daemon.json",
  "runtime_dir": "/run/dasobjectstore",
  "socket_path": "/run/dasobjectstore/dasobjectstored.sock",
  "state_dir": "/var/lib/dasobjectstore",
  "log_dir": "/var/log/dasobjectstore",
  "product_root": "/opt/dasobjectstore",
  "telemetry": {
    "enabled": true,
    "cadence_seconds": 30
  }
}
EOF
    chmod 600 "$DAEMON_CONFIG"
}

render_stack_compose() {
    cat > "$STACK_COMPOSE" <<EOF
name: $PROJECT
services:
  dasobjectstored:
    image: $DAEMON_IMAGE
    build:
      context: "$BUILD_CONTEXT"
      dockerfile: "DASObjectStore/deploy/local-docker/Dockerfile"
    init: true
    restart: unless-stopped
    command: ["dasobjectstored", "--config", "/etc/dasobjectstore/daemon.json"]
    volumes:
      - "$CONFIG_DIR:/etc/dasobjectstore:ro"
      - "$STATE_DIR:/var/lib/dasobjectstore"
      - "$RUNTIME_DIR:/run/dasobjectstore"
      - "$LOG_DIR:/var/log/dasobjectstore"
      - "$ROOT:/Volumes/Seagate/DASObjectStore"
      - "/var/run/docker.sock:/var/run/docker.sock"
    healthcheck:
      test: ["CMD-SHELL", "test -S /run/dasobjectstore/dasobjectstored.sock"]
      interval: 5s
      timeout: 3s
      retries: 20
      start_period: 10s
EOF
    chmod 600 "$STACK_COMPOSE"
}

render_profile() {
    validate_profile_name "DASOBJECTSTORE_LOCAL_PROFILE" "$PROFILE"
    validate_profile_name "DASOBJECTSTORE_LOCAL_PROJECT" "$PROJECT"
    validate_port
    ensure_profile_dirs
    require_build_context
    ensure_local_ssd_marker
    local bin
    bin="$(das_bin)"
    render_garage_config
    render_daemon_config
    "$bin" store create alleleanchor_mvp \
        --class generated_data \
        --copies 1 \
        --bucket alleleanchor-mvp \
        --ssd-root "$PROFILE_ROOT" \
        --registry-path "$REGISTRY_PATH" \
        --json >/dev/null
    "$bin" service render-compose \
        --stores-file "$REGISTRY_PATH" \
        --project-name "$PROJECT" \
        --ssd-metadata-path "$META_DIR" \
        --hdd-data-path "$DATA_DIR" \
        --provider garage \
        --service-name garage \
        --image "$GARAGE_IMAGE" \
        --bind-address 127.0.0.1 \
        --api-port "$API_PORT" \
        --config-path "$GARAGE_CONFIG" > "$GARAGE_COMPOSE"
    render_stack_compose
    printf 'Rendered profile: %s\n' "$PROFILE_ROOT"
    printf 'Stack Compose: %s\n' "$STACK_COMPOSE"
    printf 'Garage Compose: %s\n' "$GARAGE_COMPOSE"
}

docker_compose() {
    docker compose -f "$STACK_COMPOSE" "$@"
}

validate_compose() {
    docker_compose config --quiet
}

require_docker() {
    require_command docker
    docker compose version >/dev/null 2>&1 || die "Docker Compose is unavailable"
}

validate_docker_volume_mount() {
    docker run --rm \
        --entrypoint /bin/true \
        --mount "type=bind,src=$ROOT,dst=/mnt,readonly" \
        "$DAEMON_IMAGE" 2>/tmp/dasobjectstore-local-mount-error || {
        printf 'error: Docker Desktop cannot bind-mount %s. Add that path under ' "$ROOT" >&2
        printf 'Settings > Resources > File Sharing, then retry.\n' >&2
        sed -n '1,3p' /tmp/dasobjectstore-local-mount-error >&2 || true
        return 1
    }
}

wait_for_daemon() {
    local attempt
    for attempt in $(seq 1 60); do
        if docker_compose exec -T dasobjectstored test -S /run/dasobjectstore/dasobjectstored.sock >/dev/null 2>&1; then
            return
        fi
        sleep 1
    done
    docker_compose logs --tail 80 dasobjectstored >&2 || true
    die "dasobjectstored did not create its socket"
}

start_garage() {
    docker_compose exec -T dasobjectstored \
        dasobjectstore service up \
        --compose-file /etc/dasobjectstore/garage.compose.yml \
        --project-directory /var/lib/dasobjectstore/garage
}

wait_for_garage() {
    local attempt code
    for attempt in $(seq 1 60); do
        code="$(curl --max-time 2 --silent --output /dev/null --write-out '%{http_code}' \
            "http://127.0.0.1:${API_PORT}/" || true)"
        if [ "$code" != "000" ] && [ -n "$code" ]; then
            return
        fi
        sleep 1
    done
    docker_compose exec -T dasobjectstored \
        dasobjectstore service status \
        --compose-file /etc/dasobjectstore/garage.compose.yml \
        --project-directory /var/lib/dasobjectstore/garage \
        --json >&2 || true
    die "Garage did not open port $API_PORT"
}

provision() {
    require_docker
    [ -f "$STACK_COMPOSE" ] || render_profile
    wait_for_daemon
    start_garage >/dev/null
    wait_for_garage
    docker_compose exec -T dasobjectstored \
        dasobjectstore service provision --provider garage >/dev/null
    require_command python3
    python3 "$SCRIPT_DIR/export-alleleanchor-config.py" \
        --registry "$GARAGE_CREDENTIAL_REGISTRY" \
        --store-id alleleanchor_mvp \
        --endpoint "http://127.0.0.1:${API_PORT}" \
        --prefix mvp \
        --output "$ALLELEANCHOR_CONFIG"
}

up() {
    render_profile
    require_docker
    validate_compose
    docker_compose build dasobjectstored
    validate_docker_volume_mount
    docker_compose up -d dasobjectstored
    provision
    printf 'Local DASObjectStore profile is ready.\n'
}

status() {
    require_docker
    [ -f "$STACK_COMPOSE" ] || die "profile is not rendered: run '$0 render'"
    docker_compose ps
    printf 'Garage Compose: %s\n' "$GARAGE_COMPOSE"
    printf 'AlleleAnchor config: %s\n' "$ALLELEANCHOR_CONFIG"
}

down() {
    require_docker
    [ -f "$STACK_COMPOSE" ] || exit 0
    if docker_compose exec -T dasobjectstored test -S /run/dasobjectstore/dasobjectstored.sock >/dev/null 2>&1; then
        docker_compose exec -T dasobjectstored \
            dasobjectstore service down \
            --compose-file /etc/dasobjectstore/garage.compose.yml \
            --project-directory /var/lib/dasobjectstore/garage >/dev/null || true
    fi
    docker_compose down
}

command="${1:-}"
case "$command" in
    render)
        render_profile
        ;;
    build)
        render_profile
        require_docker
        validate_compose
        docker_compose build dasobjectstored
        validate_docker_volume_mount
        ;;
    up)
        up
        ;;
    provision)
        provision
        ;;
    status)
        status
        ;;
    down)
        down
        ;;
    config)
        printf '%s\n' "$ALLELEANCHOR_CONFIG"
        ;;
    help|--help|-h)
        usage
        ;;
    *)
        usage >&2
        exit 2
        ;;
esac
