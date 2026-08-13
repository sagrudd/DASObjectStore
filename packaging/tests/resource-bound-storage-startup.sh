#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
unit_root="$repo_root/packaging/linux/systemd"
helper="$repo_root/packaging/linux/usr/libexec/dasobjectstore/verify-managed-storage-mounts"
manifest="$repo_root/packaging/linux/etc/dasobjectstore/managed-storage.v1.json"
garage_renderer="$repo_root/crates/dasobjectstore-object-service/src/garage.rs"

require_text() {
  local path="$1"
  local expected="$2"
  grep -Fq -- "$expected" "$path" || {
    printf 'resource-bound startup asset %s must contain: %s\n' "$path" "$expected" >&2
    exit 1
  }
}

reject_text() {
  local path="$1"
  local forbidden="$2"
  if grep -Fq -- "$forbidden" "$path"; then
    printf 'resource-bound startup asset %s must not contain: %s\n' "$path" "$forbidden" >&2
    exit 1
  fi
}

require_pattern() {
  local path="$1"
  local expected="$2"
  grep -Eq -- "$expected" "$path" || {
    printf 'resource-bound startup asset %s must match: %s\n' "$path" "$expected" >&2
    exit 1
  }
}

for path in \
  "$unit_root/dasobjectstore-storage-ready.service" \
  "$unit_root/dasobjectstored.service" \
  "$unit_root/dasobjectstore-garage.service" \
  "$helper" \
  "$manifest"; do
  [[ -f "$path" ]] || {
    printf 'missing resource-bound startup asset: %s\n' "$path" >&2
    exit 1
  }
done

require_text "$unit_root/dasobjectstore-storage-ready.service" 'Type=simple'
require_text "$unit_root/dasobjectstore-storage-ready.service" \
  'ExecStartPre=/usr/libexec/dasobjectstore/verify-managed-storage-mounts --manifest /etc/dasobjectstore/managed-storage.v1.json'
require_text "$unit_root/dasobjectstore-storage-ready.service" \
  'ExecStart=/usr/libexec/dasobjectstore/verify-managed-storage-mounts --manifest /etc/dasobjectstore/managed-storage.v1.json --watch --interval-seconds 2'
require_text "$unit_root/dasobjectstore-storage-ready.service" \
  'Before=dasobjectstored.service dasobjectstore-garage.service'

for writer in dasobjectstored.service dasobjectstore-garage.service; do
  require_text "$unit_root/$writer" 'BindsTo=dasobjectstore-storage-ready.service'
  require_pattern "$unit_root/$writer" '^After=.*dasobjectstore-storage-ready\.service'
done

# The gate must inspect mount identity rather than accepting an existing
# directory, which could silently redirect writes to the system filesystem.
require_text "$helper" 'findmnt'
require_text "$helper" '--mountpoint'
require_text "$helper" 'blkid'
require_text "$helper" '.dasobjectstore'
require_text "$helper" 'device.env'
require_text "$helper" 'live.sqlite'
require_text "$helper" '/'

# Containers are subordinate to systemd's verified storage lifecycle. Docker
# must not resurrect Garage independently after a disk or mount disappears.
require_text "$garage_renderer" 'restart: \"no\"'
reject_text "$garage_renderer" 'push_str("    restart: unless-stopped'

fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT
ssd="$fixture/ssd"
hdd="$fixture/hdd"
fake_bin="$fixture/bin"
mkdir -p "$ssd/.dasobjectstore" "$hdd/.dasobjectstore" "$fake_bin"
printf 'role=ssd\ndevice=ssd-identity\nfilesystem=ext4\n' >"$ssd/.dasobjectstore/device.env"
printf 'metadata\n' >"$ssd/.dasobjectstore/live.sqlite"
printf 'role=hdd:hdd-identity\ndevice=hdd-identity\nfilesystem=ext4\n' >"$hdd/.dasobjectstore/device.env"

cat >"$fixture/manifest.json" <<EOF
{
  "schema_version": 1,
  "ssd": {"path": "$ssd", "uuid": "ssd-uuid", "label": "DAS_SSD", "device": "ssd-identity", "filesystem": "ext4", "role": "ssd"},
  "hdds": [
    {"path": "$hdd", "uuid": "hdd-uuid", "label": "DAS_HDD", "device": "hdd-identity", "filesystem": "ext4", "role": "hdd:hdd-identity"}
  ]
}
EOF

cat >"$fake_bin/findmnt" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
[[ ! -e "$TEST_STATE/missing-mount" ]] || exit 1
path="$2"
field="${!#}"
case "$field" in
  TARGET) printf '%s\n' "$path" ;;
  SOURCE) [[ "$path" == "$TEST_SSD" ]] && printf '/dev/ssd\n' || printf '/dev/hdd\n' ;;
  FSTYPE) printf 'ext4\n' ;;
  OPTIONS) printf 'rw,relatime\n' ;;
  *) exit 64 ;;
esac
EOF
cat >"$fake_bin/blkid" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
field="$4"
source="$5"
case "$field:$source" in
  UUID:/dev/ssd) printf 'ssd-uuid\n' ;;
  UUID:/dev/hdd) printf 'hdd-uuid\n' ;;
  LABEL:/dev/ssd) printf 'DAS_SSD\n' ;;
  LABEL:/dev/hdd) printf 'DAS_HDD\n' ;;
  *) exit 64 ;;
esac
EOF
chmod 0755 "$fake_bin/findmnt" "$fake_bin/blkid"

TEST_STATE="$fixture" TEST_SSD="$ssd" PATH="$fake_bin:$PATH" \
  python3 "$helper" --manifest "$fixture/manifest.json" >/dev/null

TEST_STATE="$fixture" TEST_SSD="$ssd" PATH="$fake_bin:$PATH" \
  python3 "$helper" --manifest "$fixture/manifest.json" --watch \
    --interval-seconds 0.05 >/dev/null 2>&1 &
watch_pid=$!
sleep 0.15
touch "$fixture/missing-mount"
if wait "$watch_pid"; then
  printf 'resource-bound startup monitor survived a lost mount\n' >&2
  exit 1
fi
rm "$fixture/missing-mount"

touch "$fixture/missing-mount"
if TEST_STATE="$fixture" TEST_SSD="$ssd" PATH="$fake_bin:$PATH" \
  python3 "$helper" --manifest "$fixture/manifest.json" >/dev/null 2>&1; then
  printf 'resource-bound startup accepted a missing mount\n' >&2
  exit 1
fi
rm "$fixture/missing-mount"

python3 - "$fixture/manifest.json" "$fixture/root-fallback.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as source:
    manifest = json.load(source)
manifest["ssd"]["path"] = "/"
with open(sys.argv[2], "w", encoding="utf-8") as destination:
    json.dump(manifest, destination)
PY
if TEST_STATE="$fixture" TEST_SSD="$ssd" PATH="$fake_bin:$PATH" \
  python3 "$helper" --manifest "$fixture/root-fallback.json" >/dev/null 2>&1; then
  printf 'resource-bound startup accepted the system root as managed SSD\n' >&2
  exit 1
fi

printf 'resource-bound storage startup package guard passed\n'
