#!/bin/bash
# HELD until baseline full-loader PASS and independent joined-runner review.
# Fresh disposable guest only. Default uses the protocol responder; the explicit
# Garage successor uses retained actual backend state. Neither proves off-NUC provenance.
set -euo pipefail
garage_existing=no
if test "${1:-}" = --garage-existing && test "$#" = 1; then
    garage_existing=yes
else
    test "$#" = 0
fi
phase=start
trap 'status=$?; printf "VM_JOINED_EXIT phase=%s status=%s\n" "$phase" "$status"; if test "$status" != 0; then poweroff -f; fi' EXIT
test "$(cat /proc/1/comm)" = systemd
test "$(id -u)" = 0
if test "$garage_existing" = no; then
    test ! -e /run/das-systemd-vm-fixture
else
    test -f /run/das-systemd-vm-fixture/garage-joined
    test -f /var/lib/das-garage-fixture/continuation.private
fi
systemctl stop serial-getty@ttyAMA0.service
for uid in 2000 2001; do
    groupadd --gid "$uid" "das-vm-$uid"
    useradd --uid "$uid" --gid "$uid" --no-create-home --shell /usr/sbin/nologin "das-vm-$uid"
done
install -d -m 755 /run/das-systemd-vm-fixture /run/das-vm-tls-public
install -d -o 2000 -g 2000 -m 700 /run/das-vm-reader-material /run/das-vm-results /run/das-vm-scratch
install -d -o 2001 -g 2001 -m 700 /run/das-vm-verifier-material
install -d -o 2001 -g 2001 -m 755 /run/das-vm-verifier-results
touch /run/das-systemd-vm-fixture/permit
chmod 644 /run/das-systemd-vm-fixture/permit
install -m 755 /mnt/cidata/adapter /opt/das-vm-adapter
phase=offline_aws_install
if test "$garage_existing" = no; then /bin/bash /mnt/cidata/aws-guest-install.sh; fi
sha256sum /opt/das-vm-adapter /usr/bin/aws
readonly driver=runtime::custody_reader::manager_tests::tls_vm_tests

wait_success() {
    local unit=$1 marker=$2
    for unused in $(seq 1 900); do
        if test -f "$marker" && test "$(systemctl show -p ActiveState --value "$unit")" = active \
            && test "$(systemctl show -p SubState --value "$unit")" = exited; then
            test "$(systemctl show -p ExecMainStatus --value "$unit")" = 0
            test "$(systemctl show -p Result --value "$unit")" = success
            test "$(systemctl show -p ExecMainPID --value "$unit")" -gt 0
            local invocation
            invocation=$(systemctl show -p InvocationID --value "$unit")
            [[ "$invocation" =~ ^[0-9a-f]{32}$ ]]
            return
        fi
        if systemctl is-failed --quiet "$unit"; then return 1; fi
        sleep .2
    done
    return 1
}

modes=(positive binding corrupt disconnect)
if test "$garage_existing" = yes; then modes=(positive); fi
for mode in "${modes[@]}"; do
    phase="prepare_$mode"
    printf '%s' "$mode" > /run/das-systemd-vm-fixture/mode
    chmod 644 /run/das-systemd-vm-fixture/mode
    printf '0' > /run/das-systemd-vm-fixture/get-count
    chmod 644 /run/das-systemd-vm-fixture/get-count
    rm -f /run/das-systemd-vm-fixture/protocol-ready
    # Exact run-owned files only; prior immutable publications/ledgers remain.
    for path in /run/das-vm-tls-public /run/das-vm-reader-material /run/das-vm-verifier-material /run/das-vm-results /run/das-vm-verifier-results; do
        find "$path" -mindepth 1 -maxdepth 1 -type f -delete
        test "$(find "$path" -mindepth 1 -maxdepth 1 | wc -l)" = 0
    done
    install -m 600 /dev/null /run/das-systemd-vm-fixture/tls-prepare.log
    /opt/das-vm-adapter --exact "$driver::prepare_joined_tls_vm" --ignored \
        > /run/das-systemd-vm-fixture/tls-prepare.log 2>&1
    encrypted=$(python3 -c 'import json; print(json.load(open("/run/das-systemd-vm-fixture/loader.json"))["root"]+"/reader.enc")')
    case "$encrypted" in /var/lib/.das-manager-review-*/reader.enc) ;; *) exit 1;; esac
    publication=$(sha256sum "$encrypted" "${encrypted%/reader.enc}/ledger.sqlite3" "${encrypted%/reader.enc}"/records/*.jcs)
    cat > /etc/systemd/system/das-vm-protocol.service <<UNIT
[Service]
Type=exec
NoNewPrivileges=yes
CapabilityBoundingSet=
RuntimeMaxSec=240
ExecStart=/usr/bin/python3 /mnt/cidata/s3-responder.py
StandardOutput=null
StandardError=null
UNIT
    cat > /etc/systemd/system/das-vm-tls-reader.service <<UNIT
[Service]
Type=exec
User=2000
RemainAfterExit=yes
Group=2000
NoNewPrivileges=yes
CapabilityBoundingSet=
PrivateMounts=yes
RuntimeMaxSec=240
LoadCredentialEncrypted=reader:$encrypted
ExecStart=/opt/das-vm-adapter --exact $driver::serve_joined_tls_vm --ignored
StandardOutput=null
StandardError=null
UNIT
    cat > /etc/systemd/system/das-vm-tls-verifier.service <<UNIT
[Service]
Type=exec
User=2001
RemainAfterExit=yes
Group=2001
NoNewPrivileges=yes
CapabilityBoundingSet=
PrivateMounts=yes
RuntimeMaxSec=180
ExecStart=/opt/das-vm-adapter --exact $driver::verify_joined_tls_vm --ignored
StandardOutput=null
StandardError=null
UNIT
    # All unit files loaded before the actual reader start; never reload beneath it.
    systemctl daemon-reload
    lifecycles=(initial)
    if test "$garage_existing" = yes; then lifecycles=(initial restart binding); fi
    previous_reader_pid=0
    for lifecycle in "${lifecycles[@]}"; do
    if test "$lifecycle" != initial; then
        rm /run/das-vm-results/ready /run/das-vm-results/server-passed \
            /run/das-vm-verifier-material/passed /run/das-vm-verifier-results/replay-checked
        if test "$lifecycle" = restart; then
            systemctl stop das-vm-garage.service
            test "$(systemctl show -p ActiveState --value das-vm-garage.service)" = inactive
            systemctl start das-vm-garage.service
            ready=no
            for unused in $(seq 1 30); do
                if runuser -u das-vm-garage -- /usr/bin/timeout --kill-after=2s 2s \
                    /opt/das-vm-garage -c /var/lib/das-garage-fixture/garage.toml status \
                    > /run/das-systemd-vm-fixture/restart-status.private 2>&1 \
                    && grep -q 'v2.3.0' /run/das-systemd-vm-fixture/restart-status.private \
                    && ! grep -q 'NO ROLE ASSIGNED' /run/das-systemd-vm-fixture/restart-status.private; then
                    ready=yes; break
                fi
                sleep .2
            done
            test "$ready" = yes
        fi
        mode=positive
        if test "$lifecycle" = binding; then mode=binding; fi
        printf '%s' "$mode" > /run/das-systemd-vm-fixture/mode
    fi
    if test "$garage_existing" = no; then systemctl start das-vm-protocol.service; fi
    systemctl start das-vm-tls-reader.service
    ready=no
    for unused in $(seq 1 750); do
        if test -f /run/das-vm-results/ready && { test "$garage_existing" = yes || \
            { test -f /run/das-systemd-vm-fixture/protocol-ready \
              && test "$(cat /run/das-systemd-vm-fixture/protocol-ready)" = "$mode"; }; }; then
            ready=yes
            break
        fi
        if systemctl is-failed --quiet das-vm-tls-reader.service; then exit 1; fi
        if test "$garage_existing" = no && systemctl is-failed --quiet das-vm-protocol.service; then exit 1; fi
        sleep .2
    done
    test "$ready" = yes
    phase="exchange_$mode"
    systemctl start das-vm-tls-verifier.service
    wait_success das-vm-tls-verifier.service /run/das-vm-verifier-material/passed
    wait_success das-vm-tls-reader.service /run/das-vm-results/server-passed
    reader_pid=$(systemctl show -p ExecMainPID --value das-vm-tls-reader.service)
    test "$reader_pid" != "$previous_reader_pid"
    previous_reader_pid=$reader_pid
    # Preserve completed-unit metadata until all terminal checks above, then
    # explicitly end the fixture lifecycle. No production restart policy change.
    systemctl stop das-vm-tls-reader.service das-vm-tls-verifier.service
    test "$(systemctl show -p ActiveState --value das-vm-tls-reader.service)" = inactive
    test "$(systemctl show -p ActiveState --value das-vm-tls-verifier.service)" = inactive
    if test "$garage_existing" = no; then systemctl stop das-vm-protocol.service; fi
    expected=1
    if test "$mode" = binding; then expected=0; fi
    if test "$garage_existing" = no; then
        test "$(cat /run/das-systemd-vm-fixture/get-count)" = "$expected"
    fi
    after=$(sha256sum "$encrypted" "${encrypted%/reader.enc}/ledger.sqlite3" "${encrypted%/reader.enc}"/records/*.jcs)
    test "$after" = "$publication"
    # Retain each non-secret attempt journal inside this guest before the next
    # case creates fresh identities. Never export credential/key directories.
    journal_name="journal-$mode.sqlite3"
    if test "$garage_existing" = yes; then journal_name="journal-$mode-$lifecycle.sqlite3"; fi
    install -m 600 /run/das-vm-verifier-material/journal.sqlite3 "/run/das-systemd-vm-fixture/$journal_name"
    if test "$garage_existing" = no; then
        printf 'VM_JOINED_PASS mode=%s actual_GETs=%s journal_replay=denied\n' "$mode" "$expected"
    else
        printf 'VM_GARAGE_TLS_PASS lifecycle=%s mode=%s journal_replay=denied formal_attestation=incomplete\n' "$lifecycle" "$mode"
    fi
    done
done
if test "$garage_existing" = no; then
    echo VM_JOINED_PROTOCOL_ALL_PASS_NOT_GARAGE
else
    systemctl stop das-vm-garage.service
    test "$(systemctl show -p ActiveState --value das-vm-garage.service)" = inactive
    echo VM_GARAGE_TLS_ALL_PASS_NOT_FORMAL_CUSTODY
fi
phase=complete
poweroff
