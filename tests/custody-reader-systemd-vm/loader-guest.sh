#!/bin/bash
# Full loader only: real systemd delivery, synthetic public storage setup, no GET.
set -euo pipefail
phase=start
trap 'status=$?; printf "VM_LOADER_EXIT phase=%s status=%s\n" "$phase" "$status"; if test "$status" != 0; then journalctl -u das-vm-loader.service -o cat --no-pager | grep -E "^VM_LOADER_(STAGE|ERROR) " || true; poweroff -f; fi' EXIT
test "$(cat /proc/1/comm)" = systemd
systemctl stop serial-getty@ttyAMA0.service
version_output=$(systemctl --version)
version=${version_output%%$'\n'*}
case "$version" in 'systemd 259'*) ;; *) exit 1;; esac
printf 'VM_LOADER_VERSION %s\n' "$version"
test ! -e /run/das-systemd-vm-fixture
groupadd --gid 2000 das-vm
useradd --uid 2000 --gid 2000 --no-create-home --shell /usr/sbin/nologin das-vm
install -d -m 755 /run/das-systemd-vm-fixture
install -d -o 2000 -g 2000 -m 700 /run/das-vm-results /run/das-vm-scratch
install -m 755 /mnt/cidata/adapter /opt/das-vm-adapter
# Genuine public helper bytes, not a runnable AWS qualification in this guest.
test -f /mnt/cidata/aws
install -m 755 /mnt/cidata/aws /opt/das-vm-aws
sha256sum /opt/das-vm-adapter /opt/das-vm-aws
touch /run/das-systemd-vm-fixture/permit
chmod 644 /run/das-systemd-vm-fixture/permit
first_pid=
first_invocation=
first_publication=
for mode in positive restart malformed wrong-key stale-current positive; do
    phase="prepare_$mode"
    printf 'VM_LOADER_PHASE %s\n' "$phase"
    if test -f /etc/systemd/system/das-vm-loader.service; then
        systemctl stop das-vm-loader.service
    fi
    printf '%s' "$mode" > /run/das-systemd-vm-fixture/mode
    rm -f /run/das-vm-results/passed
    # No secret-bearing stdout/stderr leaves this root setup operation.
    install -m 600 /dev/null /run/das-systemd-vm-fixture/prepare.log
    if test "$mode" != restart && ! HOME=/root /opt/das-vm-adapter --exact runtime::custody_reader::manager_tests::loader_vm_tests::prepare_actual_loader_vm --ignored --nocapture >/run/das-systemd-vm-fixture/prepare.log 2>&1; then
        grep -E '^VM_LOADER_PREP_(STAGE|LOCATION|RENAME_ERROR) ' /run/das-systemd-vm-fixture/prepare.log || true
        echo VM_LOADER_PREPARATION_DENIED
        exit 1
    fi
    encrypted=$(python3 -c 'import json; print(json.load(open("/run/das-systemd-vm-fixture/loader.json"))["root"]+"/reader.enc")')
    case "$encrypted" in /var/lib/.das-manager-review-*/reader.enc) ;; *) exit 1;; esac
    publication=$(sha256sum "$encrypted" /run/das-systemd-vm-fixture/loader.json "${encrypted%/reader.enc}/ledger.sqlite3" "${encrypted%/reader.enc}"/records/*.jcs)
    if test "$mode" = restart; then
        test "$publication" = "$first_publication"
    fi
    cat > /etc/systemd/system/das-vm-loader.service <<UNIT
[Unit]
Description=Disposable actual complete credential loader fixture
[Service]
Type=exec
User=2000
Group=2000
NoNewPrivileges=yes
CapabilityBoundingSet=
PrivateMounts=yes
LoadCredentialEncrypted=reader:$encrypted
ExecStart=/opt/das-vm-adapter --exact runtime::custody_reader::manager_tests::loader_vm_tests::actual_loader_vm_boundary --ignored --nocapture
StandardOutput=journal
StandardError=journal
UNIT
    systemctl daemon-reload
    phase="load_$mode"
    systemctl start das-vm-loader.service
    phase="process_identity_$mode"
    # v259 dbus-service.c BUS_EXEC_STATUS_VTABLE("ExecMain", ...), with
    # execute.c::exec_status_exit retaining pid after a fast expected denial.
    # MainPID may already be zero for a successfully completed negative case.
    pid=$(systemctl show -p ExecMainPID --value das-vm-loader.service)
    invocation=$(systemctl show -p InvocationID --value das-vm-loader.service)
    if [[ "$pid" =~ ^[0-9]+$ ]]; then printf 'VM_LOADER_OBSERVED_PID %s\n' "$pid"; fi
    if test -n "$invocation"; then echo VM_LOADER_INVOCATION_PRESENT; else echo VM_LOADER_INVOCATION_ABSENT; fi
    phase="nonzero_pid_$mode"
    test "$pid" -gt 0
    phase="invocation_shape_$mode"
    # A completed fast rejection may clear InvocationID. Identity continuity
    # is asserted for every positive/restart, including the final positive.
    if test "$mode" = positive || test "$mode" = restart; then
        [[ "$invocation" =~ ^[0-9a-f]{32}$ ]]
    fi
    if test -z "$first_pid"; then
        first_pid=$pid
        first_invocation=$invocation
        first_publication=$publication
    elif test "$mode" = restart; then
        test "$pid" != "$first_pid"
        test "$invocation" != "$first_invocation"
        echo VM_LOADER_RESTART_DISTINCT_PID_AND_INVOCATION_UNCHANGED_PUBLICATION
    fi
    success=no
    phase="terminal_result_$mode"
    for unused in $(seq 1 750); do
        if test -f /run/das-vm-results/passed && test "$(systemctl show -p ActiveState --value das-vm-loader.service)" = inactive; then
            test "$(systemctl show -p ExecMainStatus --value das-vm-loader.service)" = 0
            test "$(systemctl show -p Result --value das-vm-loader.service)" = success
            success=yes
            break
        fi
        if systemctl is-failed --quiet das-vm-loader.service; then
            journalctl -u das-vm-loader.service -o cat --no-pager | grep -E '^VM_(PROBE|LOADER)_(STAGE|ERROR) ' || true
            exit 1
        fi
        sleep .2
    done
    test "$success" = yes
    phase="unchanged_publication_$mode"
    after=$(sha256sum "$encrypted" /run/das-systemd-vm-fixture/loader.json "${encrypted%/reader.enc}/ledger.sqlite3" "${encrypted%/reader.enc}"/records/*.jcs)
    test "$after" = "$publication"
    printf 'VM_LOADER_PASS %s\n' "$mode"
done
echo VM_LOADER_ALL_PASS
phase=complete
poweroff
