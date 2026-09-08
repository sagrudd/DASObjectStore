#!/bin/bash
# HELD source fixture. Root must review frozen artifact/runner before execution.
# Actual Garage/overlay admission and retention; no Compose or live authority.
set -euo pipefail
phase=start
finish() {
    local status=$?
    printf 'VM_GARAGE_EXIT phase=%s status=%s\n' "$phase" "$status"
    if test "$status" != 0; then
        # Only source-location markers; never dump private command diagnostics.
        if test "$phase" = admission_retention && test -f /var/lib/das-garage-fixture/result.private; then
            grep -E '^VM_GARAGE_LOCATION [A-Za-z0-9_./-]+:[0-9]+$' /var/lib/das-garage-fixture/result.private | head -2 || true
            grep -E '^VM_GARAGE_COMMAND operation=(garage|head-object|put-object|get-object) category=(access_denied|not_found|timeout|provider_failure)$' /var/lib/das-garage-fixture/result.private | tail -4 || true
            grep -E '^VM_GARAGE_BATCH phase=(Prevalidation|WriterHandoff|ReaderHandoff|AdapterConstruction|ObjectRetention) index=(none|[0-9]+) completed=[0-9]+$' /var/lib/das-garage-fixture/result.private | head -1 || true
        fi
        poweroff -f
    fi
}
trap finish EXIT
test "$(id -u)" = 0
test "$(cat /proc/1/comm)" = systemd
test ! -e /run/das-systemd-vm-fixture
test ! -e /var/lib/das-garage-fixture
systemctl stop serial-getty@ttyAMA0.service
install -d -m 755 /run/das-systemd-vm-fixture
install -m 644 /dev/null /run/das-systemd-vm-fixture/permit
phase=offline_aws_install
/bin/bash /mnt/cidata/aws-guest-install.sh
test "$(sha256sum /mnt/cidata/garage | awk '{print $1}')" = 8ced2ad3040262571de08aa600959aa51f97576d55da7946fcde6f66140705e2
install -m 755 /mnt/cidata/garage /opt/das-vm-garage
install -m 755 /mnt/cidata/adapter /opt/das-vm-adapter
groupadd --gid 2002 das-vm-garage
useradd --uid 2002 --gid 2002 --no-create-home --shell /usr/sbin/nologin das-vm-garage
install -d -o 2002 -g 2002 -m 700 /var/lib/das-garage-fixture
umask 077
phase=private_config
# Public template plus guest-only random RPC secret, never printed or exported.
python3 - <<'PY'
import os,secrets
path='/var/lib/das-garage-fixture/garage.toml'
fd=os.open(path,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o600)
data='''metadata_dir = "/var/lib/das-garage-fixture/meta"
data_dir = "/var/lib/das-garage-fixture/data"
db_engine = "sqlite"
replication_factor = 1
rpc_bind_addr = "127.0.0.1:3902"
rpc_public_addr = "127.0.0.1:3902"
rpc_secret = "%s"
[s3_api]
s3_region = "garage"
api_bind_addr = "127.0.0.1:3901"
''' % secrets.token_hex(32)
with os.fdopen(fd,'w') as f:
    f.write(data); f.flush(); os.fsync(f.fileno()); os.fchown(f.fileno(),2002,2002)
PY
cat > /etc/systemd/system/das-vm-garage.service <<'UNIT'
[Service]
Type=exec
User=2002
Group=2002
NoNewPrivileges=yes
CapabilityBoundingSet=
PrivateTmp=yes
PrivateMounts=yes
ProtectSystem=strict
ProtectHome=yes
ReadWritePaths=/var/lib/das-garage-fixture
RestrictAddressFamilies=AF_INET AF_UNIX
IPAddressDeny=any
IPAddressAllow=localhost
UMask=0077
MemoryMax=512M
TasksMax=64
RuntimeMaxSec=300
TimeoutStopSec=10
Restart=no
Environment=RUST_LOG=error
ExecStart=/opt/das-vm-garage -c /var/lib/das-garage-fixture/garage.toml server --single-node
StandardOutput=null
StandardError=null
UNIT
systemctl daemon-reload
phase=garage_readiness
systemctl start das-vm-garage.service
# No default bucket/key. Single-node flag configures layout only (upstream2.3).
ready=false
for unused in $(seq 1 30); do
    systemctl is-active --quiet das-vm-garage.service
    if runuser -u das-vm-garage -- /usr/bin/timeout --kill-after=2s 2s \
        /opt/das-vm-garage -c /var/lib/das-garage-fixture/garage.toml status \
        > /run/das-systemd-vm-fixture/garage-status.private 2>&1; then
        if grep -q 'v2.3.0' /run/das-systemd-vm-fixture/garage-status.private \
            && ! grep -q 'NO ROLE ASSIGNED' /run/das-systemd-vm-fixture/garage-status.private; then
            ready=true; break
        fi
    fi
    sleep .2
done
test "$ready" = true
phase=admission_retention
install -o 2002 -g 2002 -m 600 /dev/null /var/lib/das-garage-fixture/result.private
cat > /etc/systemd/system/das-vm-garage-test.service <<'UNIT'
[Service]
Type=exec
RemainAfterExit=yes
User=2002
Group=2002
NoNewPrivileges=yes
CapabilityBoundingSet=
PrivateTmp=yes
PrivateMounts=yes
ProtectSystem=strict
ProtectHome=yes
ReadWritePaths=/var/lib/das-garage-fixture
RestrictAddressFamilies=AF_INET AF_UNIX
IPAddressDeny=any
IPAddressAllow=localhost
UMask=0077
MemoryMax=768M
TasksMax=64
RuntimeMaxSec=180
TimeoutStopSec=10
Restart=no
ExecStart=/opt/das-vm-adapter --exact runtime::service::tests::custody_garage_vm_tests::actual::actual_garage_admission_and_finite_retention_vm --ignored --nocapture
StandardOutput=append:/var/lib/das-garage-fixture/result.private
StandardError=append:/var/lib/das-garage-fixture/result.private
UNIT
systemctl daemon-reload
systemctl start das-vm-garage-test.service
for unused in $(seq 1 900); do
    if test "$(systemctl show -p SubState --value das-vm-garage-test.service)" = exited; then break; fi
    if systemctl is-failed --quiet das-vm-garage-test.service; then exit 1; fi
    sleep .2
done
test "$(systemctl show -p ActiveState --value das-vm-garage-test.service)" = active
test "$(systemctl show -p SubState --value das-vm-garage-test.service)" = exited
test "$(systemctl show -p ExecMainStatus --value das-vm-garage-test.service)" = 0
test "$(systemctl show -p Result --value das-vm-garage-test.service)" = success
test "$(systemctl show -p ExecMainPID --value das-vm-garage-test.service)" -gt 0
invocation=$(systemctl show -p InvocationID --value das-vm-garage-test.service)
[[ "$invocation" =~ ^[0-9a-f]{32}$ ]]
grep -qx 'VM_GARAGE_ADMISSION_BATCH_PASS objects=2 reconstructed_handoff=denied' /var/lib/das-garage-fixture/result.private
systemctl stop das-vm-garage-test.service das-vm-garage.service
test "$(systemctl show -p ActiveState --value das-vm-garage-test.service)" = inactive
test "$(systemctl show -p ActiveState --value das-vm-garage.service)" = inactive
printf 'VM_GARAGE_ADMISSION_BATCH_ALL_PASS_NOT_COMPOSE_OR_WORM\n'
phase=complete
poweroff
