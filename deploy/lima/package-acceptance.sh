#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
workspace_root="$(cd "$repo_root/.." && pwd)"
validation_root="${HOME}/.dasobjectstore-codex-validation/lima"
keep_guest="${DASOBJECTSTORE_LIMA_KEEP:-0}"
cpus="${DASOBJECTSTORE_LIMA_CPUS:-4}"
memory="${DASOBJECTSTORE_LIMA_MEMORY_GIB:-6}"
disk="${DASOBJECTSTORE_LIMA_DISK_GIB:-20}"

usage() {
  cat <<'USAGE'
Usage: package-acceptance.sh <ubuntu|alma|all|delete>

Runs native ARM64 package acceptance in disposable Lima guests. Successful
guests are deleted by default after evidence is copied beneath
$HOME/.dasobjectstore-codex-validation/lima. Set DASOBJECTSTORE_LIMA_KEEP=1 to
retain them. The delete command removes only this harness's two named guests.
USAGE
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || {
    printf 'error: required command is unavailable: %s\n' "$1" >&2
    exit 1
  }
}

instance_name() {
  printf 'dasobjectstore-%s-arm64\n' "$1"
}

template_name() {
  case "$1" in
    ubuntu) printf 'ubuntu-24.04\n' ;;
    alma) printf 'almalinux-9\n' ;;
    *) return 1 ;;
  esac
}

prepare_archives() {
  mkdir -p "$validation_root/artifacts" "$validation_root/evidence"
  git -C "$repo_root" archive --format=tar.gz \
    --output="$validation_root/artifacts/DASObjectStore.tar.gz" HEAD
  git -C "$workspace_root/prosopikon" archive --format=tar.gz \
    --output="$validation_root/artifacts/prosopikon.tar.gz" HEAD
  tar -C "$repo_root/crates/dasobjectstore-gui-web/dist" -czf \
    "$validation_root/artifacts/web-dist.tar.gz" .
  cp "$repo_root/deploy/lima/guest-package-acceptance.sh" \
    "$validation_root/artifacts/guest-package-acceptance.sh"
}

wait_after_reboot() {
  local instance="$1"
  local attempt
  for attempt in $(seq 1 90); do
    if limactl shell "$instance" true >/dev/null 2>&1; then
      return 0
    fi
    sleep 2
  done
  printf 'error: Lima guest did not return after reboot: %s\n' "$instance" >&2
  return 1
}

delete_guest() {
  local instance="$1"
  limactl stop "$instance" >/dev/null 2>&1 || true
  limactl delete --force "$instance" >/dev/null 2>&1 || true
}

run_one() {
  local distro="$1"
  local instance template evidence
  instance="$(instance_name "$distro")"
  template="$(template_name "$distro")"
  evidence="$validation_root/evidence/${distro}-arm64.txt"

  delete_guest "$instance"
  limactl start --yes --name="$instance" --arch=aarch64 --vm-type=vz \
    --cpus="$cpus" --memory="$memory" --disk="$disk" --containerd=none \
    --mount-none "template:$template"
  limactl shell "$instance" mkdir -p /var/tmp/dasobjectstore-acceptance
  limactl copy "$validation_root/artifacts/DASObjectStore.tar.gz" \
    "$validation_root/artifacts/prosopikon.tar.gz" \
    "$validation_root/artifacts/web-dist.tar.gz" \
    "$validation_root/artifacts/guest-package-acceptance.sh" \
    "$instance:/var/tmp/dasobjectstore-acceptance/"
  limactl shell "$instance" sudo bash \
    /var/tmp/dasobjectstore-acceptance/guest-package-acceptance.sh initial "$distro"
  limactl shell "$instance" sudo systemctl reboot >/dev/null 2>&1 || true
  wait_after_reboot "$instance"
  limactl shell "$instance" sudo bash \
    /var/tmp/dasobjectstore-acceptance/guest-package-acceptance.sh post-reboot "$distro"
  limactl copy "$instance:/var/tmp/dasobjectstore-acceptance/evidence.txt" "$evidence"
  printf 'Lima acceptance passed: %s\nEvidence: %s\n' "$instance" "$evidence"
  if [[ "$keep_guest" != "1" ]]; then
    delete_guest "$instance"
  fi
}

require_command limactl
require_command git
require_command tar

case "${1:-}" in
  ubuntu|alma)
    prepare_archives
    run_one "$1"
    ;;
  all)
    prepare_archives
    run_one ubuntu
    run_one alma
    ;;
  delete)
    delete_guest "$(instance_name ubuntu)"
    delete_guest "$(instance_name alma)"
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac
