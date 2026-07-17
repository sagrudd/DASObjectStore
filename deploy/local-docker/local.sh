#!/usr/bin/env bash
set -euo pipefail

# Local macOS profile: DASObjectStore daemon in Docker, Garage as the
# daemon-owned S3 provider, and all persistent state on an attached volume.

umask 077

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
BUILD_CONTEXT="${DASOBJECTSTORE_BUILD_CONTEXT:-$(cd "$REPO_DIR/.." && pwd)}"
ROOT="${DASOBJECTSTORE_LOCAL_ROOT:-/Volumes/Seagate/DASObjectStore}"
VALIDATION_ROOT="${HOME:-/Users/stephen}/.dasobjectstore-codex-validation"
PINAKOTHEKE_ROOT="${HOME:-/Users/stephen}/.x-img/dasobjectstore"
PROFILE="${DASOBJECTSTORE_LOCAL_PROFILE:-alleleanchor-mvp}"
STORE_ID="${DASOBJECTSTORE_LOCAL_STORE_ID:-alleleanchor_mvp}"
STORE_BUCKET="${DASOBJECTSTORE_LOCAL_STORE_BUCKET:-alleleanchor-mvp}"
STORE_PREFIX="${DASOBJECTSTORE_LOCAL_STORE_PREFIX:-mvp}"
CONSUMER_NAME="${DASOBJECTSTORE_LOCAL_CONSUMER:-alleleanchor}"
ROOT_KEY="$(printf '%s' "$ROOT" | cksum | awk '{print $1}')"
PROJECT="${DASOBJECTSTORE_LOCAL_PROJECT:-dasobjectstore-local-${ROOT_KEY}}"
GARAGE_PROJECT="${DASOBJECTSTORE_LOCAL_GARAGE_PROJECT:-dasobjectstore-${ROOT_KEY}}"
API_PORT="${DASOBJECTSTORE_LOCAL_API_PORT:-3900}"
CAPACITY_LIMIT_BYTES="${DASOBJECTSTORE_LOCAL_CAPACITY_LIMIT_BYTES:-1099511627776}"
RPC_PORT=""
WEB_PORT=""
ADMIN_PORT=""
GARAGE_IMAGE="${DASOBJECTSTORE_GARAGE_IMAGE:-dxflrs/garage:v2.3.0}"
DAEMON_IMAGE="${DASOBJECTSTORE_LOCAL_IMAGE:-dasobjectstore-local:dev}"
SOURCE_COMMIT="$(git -C "$REPO_DIR" rev-parse HEAD 2>/dev/null || printf 'unavailable')"

PROFILE_ROOT="$ROOT/$PROFILE"
PRIVATE_ROOT="${DASOBJECTSTORE_LOCAL_PRIVATE_ROOT:-${HOME:-/Users/stephen}/.config/dasobjectstore}/${PROFILE}-${ROOT_KEY}"
ROOT_BINDING="$PRIVATE_ROOT/storage-root"
CONFIG_DIR="$PRIVATE_ROOT/config"
STATE_DIR="$PROFILE_ROOT/state"
RUNTIME_DIR="$PROFILE_ROOT/runtime"
LOG_DIR="$PROFILE_ROOT/logs"
META_DIR="$PROFILE_ROOT/garage-meta"
DATA_DIR="$PROFILE_ROOT/garage-data"
CREDENTIALS_DIR="$PRIVATE_ROOT/credentials"
OBJECT_SERVICE_DIR="$PRIVATE_ROOT/object-service"
REGISTRY_PATH="$STATE_DIR/stores.json"
DEVICE_MARKER="$PROFILE_ROOT/.dasobjectstore/device.env"
GARAGE_PROJECT_DIR="$STATE_DIR/garage"
GARAGE_COMPOSE="$CONFIG_DIR/garage.compose.yml"
GARAGE_CONFIG="$CONFIG_DIR/garage.toml"
DAEMON_CONFIG="$CONFIG_DIR/daemon.json"
STORE_MANIFEST="$CONFIG_DIR/store-manifest.json"
STACK_COMPOSE="$PROFILE_ROOT/compose.yml"
GARAGE_SECRETS="$CREDENTIALS_DIR/garage.env"
CONSUMER_CONFIG="$CREDENTIALS_DIR/${CONSUMER_NAME}-store.toml"
GARAGE_CREDENTIAL_REGISTRY="$OBJECT_SERVICE_DIR/garage-credentials.json"

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
  smoke        Run generated-data S3 put/head/list/get/checksum/delete acceptance.
  completion-smoke
               Run remote client -> daemon -> Garage -> catalogue/quota acceptance.
  status       Show daemon and nested Garage Compose status without secrets.
  down         Stop Garage through the daemon, then stop the daemon container.
  config       Print the generated scoped consumer adapter config path.
  describe     Print secret-free endpoint/ObjectStore identity as JSON.
  paths        Print non-secret root/project paths without creating them.

Configuration is supplied through environment variables. The defaults target
/Volumes/Seagate/DASObjectStore and the local alleleanchor-mvp profile.
EOF
}

require_volume_root() {
    case "/$ROOT/" in
        */../*|*/./*) die "DASOBJECTSTORE_LOCAL_ROOT must not contain '.' or '..' path components (got $ROOT)" ;;
    esac
    case "$ROOT" in
        /Volumes/*) ;;
        "$VALIDATION_ROOT"|"$VALIDATION_ROOT"/*|"$PINAKOTHEKE_ROOT") ;;
        *) die "DASOBJECTSTORE_LOCAL_ROOT must be under /Volumes or $VALIDATION_ROOT, or be the exact Pinakotheke managed root $PINAKOTHEKE_ROOT (got $ROOT)" ;;
    esac
    case "$ROOT" in
      "$VALIDATION_ROOT"|"$VALIDATION_ROOT"/*)
        mkdir -p "$VALIDATION_ROOT"
        local authority_marker="$VALIDATION_ROOT/.codex-validation-root"
        if [ ! -e "$authority_marker" ]; then
            printf 'DASObjectStore Codex generated-data validation root\n' > "$authority_marker"
            chmod 600 "$authority_marker"
        fi
        mkdir -p "$ROOT"
        local resolved_validation_root resolved_root
        resolved_validation_root="$(cd "$VALIDATION_ROOT" && pwd -P)"
        resolved_root="$(cd "$ROOT" && pwd -P)"
        case "$resolved_root" in
            "$resolved_validation_root"|"$resolved_validation_root"/*) ;;
            *) die "validation root resolves outside $VALIDATION_ROOT: $ROOT" ;;
        esac
        local marker="$ROOT/.codex-validation-root"
        if [ ! -e "$marker" ]; then
            if find "$ROOT" -mindepth 1 -maxdepth 1 -print -quit | grep -q .; then
                die "validation root is not empty; use a dedicated generated-data directory: $ROOT"
            fi
            printf 'DASObjectStore Codex generated-data validation root\n' > "$marker"
            chmod 600 "$marker"
        fi
        local used_kib
        used_kib="$(du -sk "$VALIDATION_ROOT" | awk '{print $1}')"
        [ "${used_kib:-0}" -le 1073741824 ] || \
            die "validation root exceeds the 1 TiB generated-data safety limit: $VALIDATION_ROOT"
        return
        ;;
    esac
    if [ "$ROOT" = "$PINAKOTHEKE_ROOT" ]; then
        mkdir -p "$ROOT"
        local marker="$ROOT/.pinakotheke-dasobjectstore-root"
        if [ ! -e "$marker" ]; then
            if find "$ROOT" -mindepth 1 -maxdepth 1 -print -quit | grep -q .; then
                die "Pinakotheke DASObjectStore root is not empty and has no authority marker: $ROOT"
            fi
            printf 'DASObjectStore-managed Pinakotheke local profile\n' > "$marker"
            chmod 600 "$marker"
        fi
        return
    fi
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
    [[ "$API_PORT" =~ ^[0-9]+$ ]] && [ "$API_PORT" -ge 1 ] && [ "$API_PORT" -le 65532 ] || \
        die "DASOBJECTSTORE_LOCAL_API_PORT must be between 1 and 65532 (got $API_PORT)"
    RPC_PORT="$((API_PORT + 1))"
    WEB_PORT="$((API_PORT + 2))"
    ADMIN_PORT="$((API_PORT + 3))"
}

validate_capacity_limit() {
    [[ "$CAPACITY_LIMIT_BYTES" =~ ^[0-9]+$ ]] && [ "$CAPACITY_LIMIT_BYTES" -ge 1048576 ] || \
        die "DASOBJECTSTORE_LOCAL_CAPACITY_LIMIT_BYTES must be at least 1048576"
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
        "$OBJECT_SERVICE_DIR" "$(dirname "$REGISTRY_PATH")" "$(dirname "$DEVICE_MARKER")"
    chmod 700 "$PROFILE_ROOT" "$PRIVATE_ROOT" "$CONFIG_DIR" "$STATE_DIR" \
        "$RUNTIME_DIR" "$LOG_DIR" "$META_DIR" "$DATA_DIR" "$CREDENTIALS_DIR" \
        "$OBJECT_SERVICE_DIR"
    if [ -f "$ROOT_BINDING" ]; then
        [ "$(cat "$ROOT_BINDING")" = "$ROOT" ] || \
            die "private config is bound to a different storage root: $PRIVATE_ROOT"
    else
        printf '%s\n' "$ROOT" > "$ROOT_BINDING"
        chmod 600 "$ROOT_BINDING"
    fi
}

print_paths() {
    printf 'storage_root=%s\n' "$ROOT"
    printf 'profile_root=%s\n' "$PROFILE_ROOT"
    printf 'private_root=%s\n' "$PRIVATE_ROOT"
    printf 'stack_project=%s\n' "$PROJECT"
    printf 'garage_project=%s\n' "$GARAGE_PROJECT"
}

describe_profile() {
    [ -f "$CONSUMER_CONFIG" ] || die "profile is not provisioned: run '$0 up'"
    printf '{"schema_version":"dasobjectstore.local_profile_description.v1",'
    printf '"endpoint_id":"local-docker-%s",' "$ROOT_KEY"
    printf '"object_store_id":"%s",' "$STORE_ID"
    printf '"profile_id":"%s",' "$PROFILE"
    printf '"status":"ready",'
    printf '"api_url":"http://127.0.0.1:%s",' "$API_PORT"
    printf '"credential_ref":"dasobjectstore.local-profile:%s:%s"}\n' "$PROFILE" "$ROOT_KEY"
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

rpc_bind_addr = "[::]:$RPC_PORT"
rpc_public_addr = "127.0.0.1:$RPC_PORT"
rpc_secret = "$(env_value GARAGE_RPC_SECRET)"

[s3_api]
s3_region = "garage"
api_bind_addr = "[::]:$API_PORT"

[s3_web]
bind_addr = "[::]:$WEB_PORT"
root_domain = ".web.garage.localhost"
index = "index.html"

[admin]
api_bind_addr = "[::]:$ADMIN_PORT"
admin_token = "$(env_value GARAGE_ADMIN_TOKEN)"
metrics_token = "$(env_value GARAGE_METRICS_TOKEN)"
EOF
    chmod 600 "$GARAGE_CONFIG"
}

render_daemon_config() {
    cat > "$DAEMON_CONFIG" <<EOF
{
  "service_user": "dasobjectstore",
  "service_group": "dasobjectstore",
  "config_path": "/etc/dasobjectstore/daemon.json",
  "runtime_dir": "/run/dasobjectstore",
  "socket_path": "/run/dasobjectstore/dasobjectstored.sock",
  "state_dir": "/var/lib/dasobjectstore",
  "log_dir": "/var/log/dasobjectstore",
  "product_root": "/opt/dasobjectstore",
  "object_service": {
    "compose_project": "$GARAGE_PROJECT"
  },
  "telemetry": {
    "enabled": true,
    "cadence_seconds": 30
  }
}
EOF
    chmod 600 "$DAEMON_CONFIG"
}

render_store_manifest() {
    cat > "$STORE_MANIFEST" <<EOF
{
  "schema_version": 1,
  "store_id": "$STORE_ID",
  "deployment_profile": "folder",
  "host_mode": "per_user",
  "protection": "local_only",
  "backend": {
    "kind": "folder",
    "root_identity": "local-docker:$ROOT_KEY:$PROFILE"
  }
}
EOF
    chmod 600 "$STORE_MANIFEST"
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
      args:
        DASOBJECTSTORE_SOURCE_COMMIT: "$SOURCE_COMMIT"
    init: true
    restart: unless-stopped
    # The image entrypoint is already dasobjectstored; pass only its arguments.
    command: ["--config", "/etc/dasobjectstore/daemon.json"]
    environment:
      DASOBJECTSTORE_STORE_REGISTRY_PATH: /var/lib/dasobjectstore/stores.json
      DASOBJECTSTORE_SUBOBJECT_REGISTRY_PATH: /var/lib/dasobjectstore/subobjects.json
      DASOBJECTSTORE_LIVE_SQLITE_PATH: /var/lib/dasobjectstore/live.sqlite
      AWS_DEFAULT_REGION: garage
    volumes:
      - "$CONFIG_DIR:/etc/dasobjectstore:ro"
      - "$STATE_DIR:/var/lib/dasobjectstore"
      - "$OBJECT_SERVICE_DIR:/var/lib/dasobjectstore/object-service"
      - "$LOG_DIR:/var/log/dasobjectstore"
      - "$ROOT:/Volumes/Seagate/DASObjectStore"
      - "/var/run/docker.sock:/var/run/docker.sock"
    tmpfs:
      - /run/dasobjectstore
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
    validate_profile_name "DASOBJECTSTORE_LOCAL_STORE_ID" "$STORE_ID"
    validate_profile_name "DASOBJECTSTORE_LOCAL_CONSUMER" "$CONSUMER_NAME"
    validate_profile_name "DASOBJECTSTORE_LOCAL_PROJECT" "$PROJECT"
    validate_profile_name "DASOBJECTSTORE_LOCAL_GARAGE_PROJECT" "$GARAGE_PROJECT"
    validate_port
    validate_capacity_limit
    ensure_profile_dirs
    require_build_context
    ensure_local_ssd_marker
    local bin
    bin="$(das_bin)"
    render_garage_config
    render_daemon_config
    render_store_manifest
    "$bin" store create "$STORE_ID" \
        --class generated_data \
        --copies 1 \
        --capacity-limit-bytes "$CAPACITY_LIMIT_BYTES" \
        --backend-reserve-bytes 0 \
        --bucket "$STORE_BUCKET" \
        --ssd-root "$PROFILE_ROOT" \
        --registry-path "$REGISTRY_PATH" \
        --json >/dev/null
    "$bin" service render-compose \
        --stores-file "$REGISTRY_PATH" \
        --project-name "$GARAGE_PROJECT" \
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

connect_daemon_to_garage_network() {
    local daemon_container garage_network
    daemon_container="$(docker_compose ps -q dasobjectstored)"
    garage_network="${GARAGE_PROJECT}_default"
    [ -n "$daemon_container" ] || die "daemon container is not running"
    if ! docker network inspect --format '{{range .Containers}}{{.Name}}{{"\n"}}{{end}}' \
        "$garage_network" | grep -Fxq "$(docker inspect --format '{{.Name}}' "$daemon_container" | sed 's#^/##')"; then
        docker network connect "$garage_network" "$daemon_container"
    fi
}

disconnect_daemon_from_garage_network() {
    local daemon_container daemon_name garage_network
    daemon_container="$(docker_compose ps -q dasobjectstored)"
    garage_network="${GARAGE_PROJECT}_default"
    [ -n "$daemon_container" ] || return 0
    docker network inspect "$garage_network" >/dev/null 2>&1 || return 0
    daemon_name="$(docker inspect --format '{{.Name}}' "$daemon_container" | sed 's#^/##')"
    if docker network inspect --format '{{range .Containers}}{{.Name}}{{"\n"}}{{end}}' \
        "$garage_network" | grep -Fxq "$daemon_name"; then
        docker network disconnect "$garage_network" "$daemon_container"
    fi
}

restore_host_bind_ownership() {
    local host_uid host_gid
    host_uid="$(id -u)"
    host_gid="$(id -g)"
    docker_compose exec -T --user 0 dasobjectstored \
        chown -R "$host_uid:$host_gid" \
        /var/lib/dasobjectstore \
        /var/log/dasobjectstore \
        "/Volumes/Seagate/DASObjectStore/$PROFILE"
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
    docker_compose exec -T dasobjectstored \
        dasobjectstore store profile-binding \
        --manifest /etc/dasobjectstore/store-manifest.json \
        --backend-root "/Volumes/Seagate/DASObjectStore/$PROFILE" \
        --capacity-limit-bytes "$CAPACITY_LIMIT_BYTES" \
        --operation provision >/dev/null
    start_garage >/dev/null
    connect_daemon_to_garage_network
    wait_for_garage
    docker_compose exec -T dasobjectstored \
        dasobjectstore service provision --provider garage >/dev/null
    # The local profile uses bind mounts so host-side adapters and subsequent
    # validation runs must be able to traverse daemon-created state. The
    # container remains root and can continue updating these generated paths.
    restore_host_bind_ownership
    require_command python3
    python3 "$SCRIPT_DIR/export-alleleanchor-config.py" \
        --registry "$GARAGE_CREDENTIAL_REGISTRY" \
        --store-id "$STORE_ID" \
        --endpoint "http://127.0.0.1:${API_PORT}" \
        --prefix "$STORE_PREFIX" \
        --output "$CONSUMER_CONFIG"
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
    printf 'Consumer config: %s\n' "$CONSUMER_CONFIG"
}

down() {
    require_docker
    [ -f "$STACK_COMPOSE" ] || exit 0
    if docker_compose exec -T dasobjectstored test -S /run/dasobjectstore/dasobjectstored.sock >/dev/null 2>&1; then
        disconnect_daemon_from_garage_network
        docker_compose exec -T dasobjectstored \
            dasobjectstore service down \
            --compose-file /etc/dasobjectstore/garage.compose.yml \
            --project-directory /var/lib/dasobjectstore/garage >/dev/null || true
    fi
    docker_compose down
}

smoke() {
    require_volume_root
    require_command aws
    require_command python3
    [ -f "$STACK_COMPOSE" ] || die "profile is not rendered: run '$0 up'"
    [ -f "$CONSUMER_CONFIG" ] || die "adapter config is unavailable: run '$0 provision'"
    wait_for_daemon
    wait_for_garage

    local endpoint bucket credential_path access_key secret_key
    local container_id image_revision
    container_id="$(docker_compose ps -q dasobjectstored)"
    [ -n "$container_id" ] || die "daemon container is not running"
    image_revision="$(docker inspect --format '{{ index .Config.Labels "org.opencontainers.image.revision" }}' "$container_id")"
    [ "$image_revision" = "$SOURCE_COMMIT" ] || die \
        "daemon image revision $image_revision does not match source $SOURCE_COMMIT; run '$0 up'"
    endpoint="$(awk -F'"' '/^endpoint = / { print $2; exit }' "$CONSUMER_CONFIG")"
    bucket="$(awk -F'"' '/^bucket = / { print $2; exit }' "$CONSUMER_CONFIG")"
    credential_path="$(awk -F'"' '/^path = / { print $2; exit }' "$CONSUMER_CONFIG")"
    [ -n "$endpoint" ] || die "adapter config has no endpoint"
    [ -n "$bucket" ] || die "adapter config has no bucket"
    [ -f "$credential_path" ] || die "adapter credential file is unavailable"
    access_key="$(awk -F'"' '/^access_key = / { print $2; exit }' "$credential_path")"
    secret_key="$(awk -F'"' '/^secret_key = / { print $2; exit }' "$credential_path")"
    [ -n "$access_key" ] || die "adapter credential has no access key"
    [ -n "$secret_key" ] || die "adapter credential has no secret key"

    local smoke_root source downloaded key checksum downloaded_checksum
    local head_size head_checksum listed evidence_dir evidence commit timestamp
    smoke_root="$VALIDATION_ROOT/local-docker-smoke-$$"
    source="$smoke_root/source.bin"
    downloaded="$smoke_root/downloaded.bin"
    key="codex-smoke/$(date -u +%Y%m%dT%H%M%SZ)-$$.bin"
    mkdir -p "$smoke_root"
    chmod 700 "$smoke_root"
    dd if=/dev/urandom of="$source" bs=65536 count=1 2>/dev/null
    checksum="$(shasum -a 256 "$source" | awk '{print $1}')"

    export AWS_ACCESS_KEY_ID="$access_key"
    export AWS_SECRET_ACCESS_KEY="$secret_key"
    export AWS_DEFAULT_REGION=garage
    local uploaded=0
    cleanup_smoke() {
        if [ "$uploaded" -eq 1 ]; then
            aws --endpoint-url "$endpoint" s3api delete-object \
                --bucket "$bucket" --key "$key" >/dev/null 2>&1 || true
        fi
        rm -rf "$smoke_root"
    }
    trap cleanup_smoke EXIT

    aws --endpoint-url "$endpoint" s3api put-object \
        --bucket "$bucket" --key "$key" --body "$source" \
        --metadata "sha256=$checksum" >/dev/null
    uploaded=1
    head_size="$(aws --endpoint-url "$endpoint" s3api head-object \
        --bucket "$bucket" --key "$key" --query ContentLength --output text)"
    head_checksum="$(aws --endpoint-url "$endpoint" s3api head-object \
        --bucket "$bucket" --key "$key" --query Metadata.sha256 --output text)"
    [ "$head_size" = "65536" ] || die "S3 HEAD size mismatch: $head_size"
    [ "$head_checksum" = "$checksum" ] || die "S3 HEAD checksum metadata mismatch"
    listed="$(aws --endpoint-url "$endpoint" s3api list-objects-v2 \
        --bucket "$bucket" --prefix "$key" --query 'Contents[0].Key' --output text)"
    [ "$listed" = "$key" ] || die "S3 LIST did not return the generated object"
    aws --endpoint-url "$endpoint" s3api get-object \
        --bucket "$bucket" --key "$key" "$downloaded" >/dev/null
    downloaded_checksum="$(shasum -a 256 "$downloaded" | awk '{print $1}')"
    [ "$downloaded_checksum" = "$checksum" ] || die "S3 GET checksum mismatch"
    aws --endpoint-url "$endpoint" s3api delete-object \
        --bucket "$bucket" --key "$key" >/dev/null
    uploaded=0
    if aws --endpoint-url "$endpoint" s3api head-object \
        --bucket "$bucket" --key "$key" >/dev/null 2>&1; then
        die "S3 DELETE left the generated object addressable"
    fi

    evidence_dir="$VALIDATION_ROOT/deployment-evidence"
    mkdir -p "$evidence_dir"
    chmod 700 "$evidence_dir"
    commit="$image_revision"
    timestamp="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    evidence="$evidence_dir/local-docker-s3-$commit.txt"
    {
        printf 'source_commit=%s\n' "$commit"
        printf 'timestamp_utc=%s\n' "$timestamp"
        printf 'profile=%s\n' "$PROFILE"
        printf 'provider=garage\n'
        printf 'generated_bytes=65536\n'
        printf 'put=passed\nhead=passed\nlist=passed\nget=passed\n'
        printf 'checksum=passed\ndelete=passed\n'
    } > "$evidence"
    chmod 600 "$evidence"
    cleanup_smoke
    trap - EXIT
    printf 'Local Docker/Garage S3 acceptance passed.\nEvidence: %s\n' "$evidence"
}

completion_smoke() {
    require_volume_root
    require_docker
    require_command aws
    require_command python3
    [ -f "$STACK_COMPOSE" ] || die "profile is not rendered: run '$0 up'"
    [ -f "$CONSUMER_CONFIG" ] || die "adapter config is unavailable: run '$0 provision'"
    wait_for_daemon
    wait_for_garage

    local container_id image_revision credential_path access_key secret_key bucket
    container_id="$(docker_compose ps -q dasobjectstored)"
    [ -n "$container_id" ] || die "daemon container is not running"
    image_revision="$(docker inspect --format '{{ index .Config.Labels "org.opencontainers.image.revision" }}' "$container_id")"
    [ "$image_revision" = "$SOURCE_COMMIT" ] || die \
        "daemon image revision $image_revision does not match source $SOURCE_COMMIT; run '$0 up'"
    credential_path="$(awk -F'"' '/^path = / { print $2; exit }' "$CONSUMER_CONFIG")"
    bucket="$(awk -F'"' '/^bucket = / { print $2; exit }' "$CONSUMER_CONFIG")"
    [ -f "$credential_path" ] || die "adapter credential file is unavailable"
    access_key="$(awk -F'"' '/^access_key = / { print $2; exit }' "$credential_path")"
    secret_key="$(awk -F'"' '/^secret_key = / { print $2; exit }' "$credential_path")"
    [ -n "$access_key" ] || die "adapter credential has no access key"
    [ -n "$secret_key" ] || die "adapter credential has no secret key"
    [ -n "$bucket" ] || die "adapter config has no bucket"

    local run_id smoke_root source key checksum config_host config_container source_container
    local output evidence_dir evidence object_version used_before
    run_id="$(date -u +%Y%m%dT%H%M%SZ)-$$"
    smoke_root="$PROFILE_ROOT/completion-smoke-$run_id"
    source="$smoke_root/source.bin"
    key="codex-completion/$run_id.bin"
    config_host="$CONFIG_DIR/remote-completion-$run_id.json"
    config_container="/etc/dasobjectstore/remote-completion-$run_id.json"
    source_container="/Volumes/Seagate/DASObjectStore/$PROFILE/completion-smoke-$run_id/source.bin"
    mkdir -p "$smoke_root"
    chmod 700 "$smoke_root"
    dd if=/dev/urandom of="$source" bs=4096 count=1 2>/dev/null
    checksum="$(shasum -a 256 "$source" | awk '{print $1}')"
    object_version="$(printf '%s' "${checksum:0:16}" | python3 -c 'import sys; print(max(1, int(sys.stdin.read(), 16) & (2**63 - 1)))')"
    used_before="$(LEDGER_PATH="$STATE_DIR/capacity-ledgers/$STORE_ID.json" python3 - <<'PY'
import json
import os
from pathlib import Path

path = Path(os.environ["LEDGER_PATH"])
print(json.loads(path.read_text(encoding="utf-8")).get("used_bytes", 0) if path.exists() else 0)
PY
)"

    REMOTE_CONFIG_PATH="$config_host" ACCESS_KEY="$access_key" SECRET_KEY="$secret_key" \
        STORE_ID="$STORE_ID" BUCKET="$bucket" API_PORT="$API_PORT" python3 - <<'PY'
import json
import os
from pathlib import Path

path = Path(os.environ["REMOTE_CONFIG_PATH"])
document = {
    "endpoint_url": f"http://garage:{os.environ['API_PORT']}",
    "region": "garage",
    "profile": "dasobjectstore",
    "auth_authority": "aws-profile",
    "default_appliance_id": "local-docker",
    "paired_appliances": [{
        "appliance_id": "local-docker",
        "display_name": "Local Docker generated-data acceptance",
        "appliance_base_url": "http://127.0.0.1",
        "discovery_url": "http://127.0.0.1",
        "auth_authority": "aws-profile",
        "paired_actor": "codex-generated-data",
        "default_object_store": os.environ["STORE_ID"],
        "session": {
            "session_id": "CODEXLOCALDOCKERSESSION",
            "issued_at": "2026-01-01T00:00:00Z",
            "expires_at": "2099-01-01T00:00:00Z",
            "credentials": {
                "access_key_id": os.environ["ACCESS_KEY"],
                "secret_access_key": os.environ["SECRET_KEY"],
            },
        },
        "object_stores": [{
            "object_store": os.environ["STORE_ID"],
            "bucket": os.environ["BUCKET"],
            "can_read": True,
            "can_write": True,
            "object_type": "generated_data",
        }],
    }],
}
path.write_text(json.dumps(document), encoding="utf-8")
path.chmod(0o600)
PY

    cleanup_completion_smoke() {
        rm -rf "$smoke_root"
        rm -f "$config_host"
    }
    trap cleanup_completion_smoke EXIT
    output="$(docker_compose exec -T dasobjectstored \
        dasobjectstore-remote --config "$config_container" upload "$STORE_ID" \
        --source "$source_container" --key "$key" \
        --submit-to-daemon --daemon-socket /run/dasobjectstore/dasobjectstored.sock)"
    printf '%s\n' "$output" | grep -q 'state=Complete' || die \
        "remote upload did not reach daemon-owned terminal completion"

    AWS_ACCESS_KEY_ID="$access_key" AWS_SECRET_ACCESS_KEY="$secret_key" AWS_DEFAULT_REGION=garage \
        aws --endpoint-url "http://127.0.0.1:$API_PORT" s3api head-object \
        --bucket "$bucket" --key "$key" --query ContentLength --output text | grep -qx 4096 || \
        die "Garage HEAD did not verify the completed generated object"

    LIVE_SQLITE="$STATE_DIR/live.sqlite" STORE_ID="$STORE_ID" OBJECT_KEY="$key" \
        OBJECT_VERSION="$object_version" EXPECTED_CHECKSUM="sha256:$checksum" python3 - <<'PY'
import json
import os
import sqlite3

connection = sqlite3.connect(os.environ["LIVE_SQLITE"])
rows = connection.execute(
    "SELECT object_id, object_version, object_json FROM profile_catalogue_objects WHERE store_id = ?",
    (os.environ["STORE_ID"],),
).fetchall()
matching = [row for row in rows if row[0] == os.environ["OBJECT_KEY"]]
if len(matching) != 1:
    raise SystemExit("shared catalogue does not contain exactly one completed object")
object_id, version, raw = matching[0]
if version != int(os.environ["OBJECT_VERSION"]):
    raise SystemExit("shared catalogue object version does not match completion identity")
record = json.loads(raw)
checksum = record.get("checksum", {})
expected = os.environ["EXPECTED_CHECKSUM"].removeprefix("sha256:")
if (record.get("size_bytes") != 4096 or checksum.get("algorithm") != "sha256"
        or checksum.get("value") != expected):
    raise SystemExit("shared catalogue size/checksum does not match provider verification")
PY

    LEDGER_PATH="$STATE_DIR/capacity-ledgers/$STORE_ID.json" USED_BEFORE="$used_before" \
        python3 - <<'PY'
import json
import os
from pathlib import Path

ledger = json.loads(Path(os.environ["LEDGER_PATH"]).read_text(encoding="utf-8"))
if ledger.get("used_bytes") != int(os.environ["USED_BEFORE"]) + 4096:
    raise SystemExit("logical quota was not charged exactly once for the completed object")
if ledger.get("reservations"):
    raise SystemExit("completed object left an unsettled capacity reservation")
PY

    evidence_dir="$VALIDATION_ROOT/deployment-evidence"
    evidence="$evidence_dir/local-docker-remote-completion-$image_revision.txt"
    mkdir -p "$evidence_dir"
    chmod 700 "$evidence_dir"
    {
        printf 'source_commit=%s\n' "$image_revision"
        printf 'profile=%s\nprovider=garage\ngenerated_bytes=4096\n' "$PROFILE"
        printf 'remote_client_submission=passed\ndaemon_terminal_completion=passed\n'
        printf 'provider_head_verification=passed\nshared_catalogue_commit=passed\n'
        printf 'logical_quota_settlement=passed\nreservation_release=passed\n'
    } > "$evidence"
    chmod 600 "$evidence"
    cleanup_completion_smoke
    trap - EXIT
    printf 'Local Docker remote-upload completion acceptance passed.\nEvidence: %s\n' "$evidence"
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
    smoke)
        smoke
        ;;
    completion-smoke)
        completion_smoke
        ;;
    status)
        status
        ;;
    down)
        down
        ;;
    config)
        printf '%s\n' "$CONSUMER_CONFIG"
        ;;
    describe)
        describe_profile
        ;;
    paths)
        print_paths
        ;;
    help|--help|-h)
        usage
        ;;
    *)
        usage >&2
        exit 2
        ;;
esac
