#!/usr/bin/env bash
set -euo pipefail

if [[ "${DASOBJECTSTORE_RUN_PRIVILEGED_WORKSPACE_ACCEPTANCE:-}" != "1" ]]; then
  echo "SKIP: set DASOBJECTSTORE_RUN_PRIVILEGED_WORKSPACE_ACCEPTANCE=1 explicitly" >&2
  exit 77
fi
if [[ "$(uname -s)" != "Linux" || "${EUID}" -ne 0 ]]; then
  echo "FAIL: synthetic workspace acceptance requires root on Linux" >&2
  exit 1
fi

validation_root="${DASOBJECTSTORE_VALIDATION_ROOT:-/home/stephen/.dasobjectstore-codex-validation}"
case "${validation_root}" in
  /home/*/.dasobjectstore-codex-validation) ;;
  *)
    echo "FAIL: validation root must be a dedicated per-user .dasobjectstore-codex-validation directory" >&2
    exit 1
    ;;
esac

workspace_id="codex-workspace-e2e-$$"
fixture="${validation_root}/workspace-e2e/${workspace_id}"
aggregate_root="${fixture}/aggregates"
socket_path="/run/dasobjectstore/${workspace_id}.sock"
config_path="${fixture}/workspace-host.json"
broker_pid=""
loop_devices=()
mounts=()

cleanup() {
  set +e
  if [[ -n "${broker_pid}" ]]; then
    kill "${broker_pid}" 2>/dev/null
    wait "${broker_pid}" 2>/dev/null
  fi
  if findmnt -rn -T "${aggregate_root}/${workspace_id}" >/dev/null 2>&1; then
    umount "${aggregate_root}/${workspace_id}" 2>/dev/null
  fi
  rm -f "/etc/exports.d/dasobjectstore-workspace-${workspace_id}-synthetic-client.exports"
  exportfs -ra 2>/dev/null
  for mountpoint in "${mounts[@]}"; do
    umount "${mountpoint}" 2>/dev/null
  done
  for loop_device in "${loop_devices[@]}"; do
    losetup -d "${loop_device}" 2>/dev/null
  done
  rm -rf "${fixture}"
  rm -f "${socket_path}"
}
trap cleanup EXIT

mkdir -p "${fixture}" "${aggregate_root}"
chmod 0700 "${fixture}"
for index in 1 2; do
  image="${fixture}/disk-${index}.img"
  mountpoint="${fixture}/disk-${index}"
  truncate -s 2G "${image}"
  mkfs.ext4 -q -F -O quota,project "${image}"
  loop_device="$(losetup --find --show "${image}")"
  loop_devices+=("${loop_device}")
  mkdir "${mountpoint}"
  mount -o prjquota "${loop_device}" "${mountpoint}"
  mounts+=("${mountpoint}")
done

cat >"${config_path}" <<EOF
{
  "schema_version": 1,
  "aggregate_root": "${aggregate_root}",
  "nfs_clients": {
    "synthetic-client": {"address_or_cidr": "192.168.1.48"}
  },
  "disks": {
    "loop-a": {"root": "${fixture}/disk-1", "workspace_directory": ".workspaces"},
    "loop-b": {"root": "${fixture}/disk-2", "workspace_directory": ".workspaces"}
  }
}
EOF
chown root:root "${config_path}"
chmod 0600 "${config_path}"

export DASOBJECTSTORE_ACCEPTANCE_CONFIG="${config_path}"
export DASOBJECTSTORE_ACCEPTANCE_SOCKET="${socket_path}"
export DASOBJECTSTORE_ACCEPTANCE_WORKSPACE="${workspace_id}"
export DASOBJECTSTORE_ACCEPTANCE_AGGREGATE="${aggregate_root}/${workspace_id}"

python3 - <<'PY'
import json
import os
import socket
import subprocess
import time

config = os.environ["DASOBJECTSTORE_ACCEPTANCE_CONFIG"]
socket_path = os.environ["DASOBJECTSTORE_ACCEPTANCE_SOCKET"]
workspace = os.environ["DASOBJECTSTORE_ACCEPTANCE_WORKSPACE"]
aggregate_path = os.environ["DASOBJECTSTORE_ACCEPTANCE_AGGREGATE"]
broker = "/usr/libexec/dasobjectstore/dasobjectstore-workspace-host"
branches = [
    {"disk_id": "loop-a", "branch_id": f"{workspace}-a", "project_id": 41001, "quota_bytes": 536870912},
    {"disk_id": "loop-b", "branch_id": f"{workspace}-b", "project_id": 41002, "quota_bytes": 536870912},
]
aggregate = {"mount_identity": workspace, "branches": branches, "minimum_free_bytes": 67108864}

def start_broker():
    try:
        os.unlink(socket_path)
    except FileNotFoundError:
        pass
    listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    listener.bind(socket_path)
    listener.listen(16)
    os.chmod(socket_path, 0o600)
    def prepare():
        os.dup2(listener.fileno(), 3)
    environment = os.environ.copy()
    process = subprocess.Popen(
        ["/bin/sh", "-c", 'export LISTEN_PID=$$ LISTEN_FDS=1; exec "$1" "$2"', "acceptance", broker, config],
        pass_fds=(listener.fileno(),),
        preexec_fn=prepare,
        env=environment,
    )
    listener.close()
    for _ in range(100):
        if process.poll() is not None:
            raise RuntimeError("workspace broker exited during startup")
        if os.path.exists(socket_path):
            return process
        time.sleep(0.02)
    raise RuntimeError("workspace broker socket did not become ready")

sequence = 0
def request(kind, **payload):
    global sequence
    sequence += 1
    body = {
        "protocol_version": 7,
        "request_id": f"acceptance-{sequence}",
        "workspace_id": workspace,
        "operation": {"kind": kind, **payload},
    }
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
        client.connect(socket_path)
        client.sendall(json.dumps(body).encode() + b"\n")
        response = b""
        while not response.endswith(b"\n"):
            chunk = client.recv(65536)
            if not chunk:
                break
            response += chunk
    parsed = json.loads(response)
    if not parsed.get("ok"):
        raise RuntimeError(f"{kind}: {parsed.get('error_message')}")
    return parsed

process = start_broker()
try:
    provisioned = request("provision", branches=branches)
    assert all(item["state"] == "ready" and item["quota_enforced"] for item in provisioned["branches"])
    mounted = request("mount_aggregate", aggregate=aggregate)
    assert mounted["aggregate"]["state"] == "ready"
    attached = request(
        "attach_nfs",
        export={"mount_identity": workspace, "client_id": "synthetic-client", "access_mode": "read_write"},
    )
    assert attached["export"]["state"] == "ready" and attached["export"]["root_squash"]

    output = os.path.join(aggregate_path, "results")
    os.mkdir(output)
    with open(os.path.join(output, "synthetic.txt"), "wb") as handle:
        handle.write(b"synthetic-workspace-acceptance\n" * 2048)
    checkpoint = request(
        "checkpoint_inventory",
        checkpoint={"relative_prefix": "results", "max_files": 8, "max_logical_bytes": 1048576},
    )
    assert checkpoint["checkpoint"]["members"][0]["relative_path"] == "synthetic.txt"

    os.symlink("/etc/passwd", os.path.join(output, "unsafe-link"))
    try:
        request(
            "checkpoint_inventory",
            checkpoint={"relative_prefix": "results", "max_files": 8, "max_logical_bytes": 1048576},
        )
        raise RuntimeError("symlink safety check unexpectedly succeeded")
    except RuntimeError as error:
        if "symlink" not in str(error):
            raise
    os.unlink(os.path.join(output, "unsafe-link"))

    process.terminate()
    process.wait(timeout=10)
    process = start_broker()
    inspected = request("inspect_aggregate", aggregate=aggregate)
    assert inspected["aggregate"]["state"] == "ready"
    inspected_nfs = request(
        "inspect_nfs",
        export={"mount_identity": workspace, "client_id": "synthetic-client", "access_mode": "read_write"},
    )
    assert inspected_nfs["export"]["state"] == "ready"

    request(
        "detach_nfs",
        export={"mount_identity": workspace, "client_id": "synthetic-client", "access_mode": "read_write"},
    )
    os.unlink(os.path.join(output, "synthetic.txt"))
    os.rmdir(output)
    request("unmount_aggregate", aggregate=aggregate)
    cleaned = request("cleanup", branches=branches)
    assert all(item["state"] == "absent" for item in cleaned["branches"])
finally:
    process.terminate()
    process.wait(timeout=10)

print(json.dumps({
    "schema_version": "dasobjectstore.workspace_synthetic_acceptance.v1",
    "workspace_id": workspace,
    "project_quota": "verified",
    "mergerfs": "verified",
    "nfs_root_squash": "verified",
    "checkpoint": "verified",
    "restart_recovery": "verified",
    "symlink_fail_closed": "verified",
    "cleanup": "verified",
}, sort_keys=True))
PY
