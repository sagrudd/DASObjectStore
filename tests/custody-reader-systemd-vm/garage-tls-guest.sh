#!/bin/bash
# Source-only until exact frozen runner/artifact review. All keys stay in guest.
set -euo pipefail
phase=bootstrap
trap 'status=$?; printf "VM_GARAGE_JOIN_EXIT phase=%s status=%s\n" "$phase" "$status"; if test "$status" != 0; then poweroff -f; fi' EXIT
test "$#" = 0
test "$(id -u)" = 0
/bin/bash /mnt/cidata/garage-guest.sh --retain-for-tls
test "$(systemctl show -p ActiveState --value das-vm-garage-test.service)" = inactive
test -f /var/lib/das-garage-fixture/retained.json
phase=continuation_provision
install -m 644 /dev/null /run/das-systemd-vm-fixture/garage-joined
systemctl start das-vm-garage.service
ready=no
for unused in $(seq 1 30); do
    if runuser -u das-vm-garage -- /usr/bin/timeout --kill-after=2s 2s \
        /opt/das-vm-garage -c /var/lib/das-garage-fixture/garage.toml status \
        > /run/das-systemd-vm-fixture/continuation-status.private 2>&1 \
        && grep -q 'v2.3.0' /run/das-systemd-vm-fixture/continuation-status.private \
        && ! grep -q 'NO ROLE ASSIGNED' /run/das-systemd-vm-fixture/continuation-status.private; then
        ready=yes; break
    fi
    sleep .2
done
test "$ready" = yes
install -o 2002 -g 2002 -m 600 /dev/null /var/lib/das-garage-fixture/continuation-result.private
cat > /etc/systemd/system/das-vm-garage-continuation.service <<'UNIT'
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
ExecStart=/opt/das-vm-adapter --exact runtime::service::tests::custody_garage_vm_tests::actual::provision_garage_continuation_vm --ignored --nocapture
StandardOutput=append:/var/lib/das-garage-fixture/continuation-result.private
StandardError=append:/var/lib/das-garage-fixture/continuation-result.private
UNIT
systemctl daemon-reload
systemctl start das-vm-garage-continuation.service
for unused in $(seq 1 900); do
    if test "$(systemctl show -p SubState --value das-vm-garage-continuation.service)" = exited; then break; fi
    if systemctl is-failed --quiet das-vm-garage-continuation.service; then exit 1; fi
    sleep .2
done
test "$(systemctl show -p ActiveState --value das-vm-garage-continuation.service)" = active
test "$(systemctl show -p SubState --value das-vm-garage-continuation.service)" = exited
test "$(systemctl show -p ExecMainStatus --value das-vm-garage-continuation.service)" = 0
test "$(systemctl show -p Result --value das-vm-garage-continuation.service)" = success
test "$(systemctl show -p ExecMainPID --value das-vm-garage-continuation.service)" -gt 0
invocation=$(systemctl show -p InvocationID --value das-vm-garage-continuation.service)
[[ "$invocation" =~ ^[0-9a-f]{32}$ ]]
grep -qx VM_GARAGE_CONTINUATION_NEW_READ_KEY_OLD_GRANTS_REVOKED /var/lib/das-garage-fixture/continuation-result.private
systemctl stop das-vm-garage-continuation.service
test "$(systemctl show -p ActiveState --value das-vm-garage-continuation.service)" = inactive
phase=protected_tls
/bin/bash /mnt/cidata/tls-guest.sh --garage-existing
phase=complete
printf 'VM_GARAGE_JOIN_COMPLETE\n'
poweroff
