#!/usr/bin/env bash
set -euo pipefail

if [[ "${DASOBJECTSTORE_RUN_PRIVILEGED_WORKSPACE_ACCEPTANCE:-}" != "1" ]]; then
  echo "SKIP: set DASOBJECTSTORE_RUN_PRIVILEGED_WORKSPACE_ACCEPTANCE=1 explicitly" >&2
  exit 77
fi
if [[ "$(uname -s)" != "Linux" ]]; then
  echo "FAIL: privileged workspace acceptance requires Linux" >&2
  exit 1
fi
if [[ "${EUID}" -ne 0 ]]; then
  echo "FAIL: privileged workspace acceptance requires root inspection authority" >&2
  exit 1
fi

workspace_id="${DASOBJECTSTORE_ACCEPTANCE_WORKSPACE_ID:-}"
if [[ ! "${workspace_id}" =~ ^codex-[a-z0-9][a-z0-9._-]{0,63}$ ]]; then
  echo "FAIL: DASOBJECTSTORE_ACCEPTANCE_WORKSPACE_ID must name a synthetic codex-* workspace" >&2
  exit 1
fi

dasobjectstore workspace inspect "${workspace_id}" --json >/dev/null
dasobjectstore workspace cleanup-plan "${workspace_id}" --json >/dev/null

mount_identity="dasobjectstore-workspace-${workspace_id}"
if ! findmnt --json --types fuse.mergerfs,fuse \
  | grep -Fq "\"source\":\"${mount_identity}\""; then
  echo "FAIL: no live mergerfs mount with the expected workspace identity" >&2
  exit 1
fi
if ! pgrep -a mergerfs | grep -Fq "fsname=${mount_identity}"; then
  echo "FAIL: mergerfs process identity does not match the workspace" >&2
  exit 1
fi
if ! exportfs -v | grep -F "${workspace_id}" | grep -Eq 'root_squash|all_squash'; then
  echo "FAIL: no host-restricted root-squashed NFS export evidence for the workspace" >&2
  exit 1
fi

echo "PASS: ${workspace_id} has typed daemon visibility, mergerfs identity, and squashed NFS evidence"
