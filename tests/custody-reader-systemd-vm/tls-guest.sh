#!/bin/bash
# HELD until baseline full-loader PASS and independent joined-runner review.
# Only a fresh disposable guest; no real backend or off-NUC provenance claim.
set -euo pipefail
phase=start
trap 'status=$?; printf "VM_JOINED_EXIT phase=%s status=%s\n" "$phase" "$status"; if test "$status" != 0; then poweroff -f; fi' EXIT
test "$(cat /proc/1/comm)" = systemd
test "$(id -u)" = 0
test ! -e /run/das-systemd-vm-fixture
systemctl stop serial-getty@ttyAMA0.service
for uid in 2000 2001; do
    groupadd --gid "$uid" "das-vm-$uid"
    useradd --uid "$uid" --gid "$uid" --no-create-home --shell /usr/sbin/nologin "das-vm-$uid"
done
install -d -m 755 /run/das-systemd-vm-fixture /run/das-vm-tls-public
install -d -o 2000 -g 2000 -m 700 /run/das-vm-reader-material /run/das-vm-results /run/das-vm-scratch
install -d -o 2001 -g 2001 -m 700 /run/das-vm-verifier-material
touch /run/das-systemd-vm-fixture/permit
chmod 644 /run/das-systemd-vm-fixture/permit
install -m 755 /mnt/cidata/adapter /opt/das-vm-adapter
phase=offline_aws_install
/bin/bash /mnt/cidata/aws-guest-install.sh
sha256sum /opt/das-vm-adapter /usr/bin/aws
readonly driver=runtime::custody_reader::manager_tests::tls_vm_tests

wait_success() {
    local unit=$1 marker=$2
    for unused in $(seq 1 900); do
        if test -f "$marker" && test "$(systemctl show -p ActiveState --value "$unit")" = inactive; then
            test "$(systemctl show -p ExecMainStatus --value "$unit")" = 0
            test "$(systemctl show -p Result --value "$unit")" = success
            return
        fi
        if systemctl is-failed --quiet "$unit"; then return 1; fi
        sleep .2
    done
    return 1
}

for mode in positive binding corrupt disconnect; do
    phase="prepare_$mode"
    printf '%s' "$mode" > /run/das-systemd-vm-fixture/mode
    chmod 644 /run/das-systemd-vm-fixture/mode
    printf '0' > /run/das-systemd-vm-fixture/get-count
    chmod 644 /run/das-systemd-vm-fixture/get-count
    # Exact run-owned files only; prior immutable publications/ledgers remain.
    for path in /run/das-vm-tls-public /run/das-vm-reader-material /run/das-vm-verifier-material /run/das-vm-results; do
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
    systemctl start das-vm-protocol.service das-vm-tls-reader.service
    ready=no
    for unused in $(seq 1 750); do
        if test -f /run/das-vm-results/ready; then ready=yes; break; fi
        if systemctl is-failed --quiet das-vm-tls-reader.service; then exit 1; fi
        sleep .2
    done
    test "$ready" = yes
    phase="exchange_$mode"
    systemctl start das-vm-tls-verifier.service
    wait_success das-vm-tls-verifier.service /run/das-vm-verifier-material/passed
    wait_success das-vm-tls-reader.service /run/das-vm-results/server-passed
    systemctl stop das-vm-protocol.service
    expected=1
    if test "$mode" = binding; then expected=0; fi
    test "$(cat /run/das-systemd-vm-fixture/get-count)" = "$expected"
    after=$(sha256sum "$encrypted" "${encrypted%/reader.enc}/ledger.sqlite3" "${encrypted%/reader.enc}"/records/*.jcs)
    test "$after" = "$publication"
    # Retain each non-secret attempt journal inside this guest before the next
    # case creates fresh identities. Never export credential/key directories.
    install -m 600 /run/das-vm-verifier-material/journal.sqlite3 "/run/das-systemd-vm-fixture/journal-$mode.sqlite3"
    printf 'VM_JOINED_PASS mode=%s actual_GETs=%s journal_replay=denied\n' "$mode" "$expected"
done
echo VM_JOINED_PROTOCOL_ALL_PASS_NOT_GARAGE
phase=complete
poweroff
