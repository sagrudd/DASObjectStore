#!/bin/bash
# Executed only by the reviewed offline NoCloud seed inside the full VM.
set -euo pipefail
phase=start
trap 'status=$?; printf "VM_EXIT phase=%s status=%s\n" "$phase" "$status"; if test "$status" != 0; then poweroff -f; fi' EXIT
trap 'printf "VM_HUP phase=%s\n" "$phase"; exit 129' HUP
test "$(cat /proc/1/comm)" = systemd
systemctl stop serial-getty@ttyAMA0.service
phase=version
version_output=$(systemctl --version)
version=${version_output%%$'\n'*}
case "$version" in 'systemd 259'*) ;; *) echo VM_UNSUPPORTED_SYSTEMD; exit 1;; esac
printf 'VM_VERSION %s\n' "$version"
test ! -e /run/das-systemd-vm-fixture
test ! -e /var/lib/das-vm
phase=accounts
echo VM_PHASE_accounts
groupadd --gid 2000 das-vm
useradd --uid 2000 --gid 2000 --no-create-home --shell /usr/sbin/nologin das-vm
phase=public_files
echo VM_PHASE_public_files
install -d -m 755 /run/das-systemd-vm-fixture
install -d -m 700 /var/lib/das-vm
install -d -o 2000 -g 2000 -m 700 /run/das-vm-results
install -m 755 /mnt/cidata/adapter /opt/das-vm-adapter
touch /run/das-systemd-vm-fixture/permit
chmod 644 /run/das-systemd-vm-fixture/permit
# The plaintext and host-mode key are newly generated inside this guest only.
phase=synthetic_credential
echo VM_PHASE_synthetic_credential
head -c 32 /dev/urandom > /var/lib/das-vm/plain
chmod 600 /var/lib/das-vm/plain
if ! systemd-creds encrypt --with-key=host --name=reader /var/lib/das-vm/plain /var/lib/das-vm/reader.enc >/dev/null 2>&1; then
    echo VM_CREDENTIAL_ENCRYPTION_DENIED
    exit 1
fi
phase=unit
echo VM_PHASE_unit
sha256sum /opt/das-vm-adapter /var/lib/das-vm/reader.enc
cat > /etc/systemd/system/das-vm-reader.service <<'UNIT'
[Unit]
Description=Disposable actual Rust systemd adapter fixture
[Service]
Type=exec
User=2000
Group=2000
NoNewPrivileges=yes
CapabilityBoundingSet=
PrivateMounts=yes
LoadCredentialEncrypted=reader:/var/lib/das-vm/reader.enc
ExecStart=/opt/das-vm-adapter --exact runtime::custody_reader::systemd::tests::actual_systemd_vm_adapter_boundary --ignored --nocapture
StandardOutput=journal
StandardError=journal
UNIT
make_binding() {
    python3 - "$1" <<'PY'
import hashlib,json,pathlib,sys
mode=sys.argv[1]; p=pathlib.Path('/run/das-systemd-vm-fixture')
x=json.loads(pathlib.Path('/mnt/cidata/binding.json').read_text())
x.update(uid=2000,service_identity='das-vm-reader.service',credential_name='reader',
 executable_sha256=hashlib.sha256(pathlib.Path('/opt/das-vm-adapter').read_bytes()).hexdigest(),
 encrypted_source_sha256=hashlib.sha256(pathlib.Path('/var/lib/das-vm/reader.enc').read_bytes()).hexdigest())
if mode=='wrong-name': x['credential_name']='other'
if mode=='wrong-executable': x['executable_sha256']='1'*64
(p/'binding.json').write_text(json.dumps(x,sort_keys=True,separators=(',',':')))
(p/'mode').write_text(mode)
PY
}
wait_result() {
    for unused in $(seq 1 450); do
        if test -f /run/das-vm-results/passed && \
           test "$(systemctl show -p ActiveState --value das-vm-reader.service)" = inactive; then
            test "$(systemctl show -p ExecMainStatus --value das-vm-reader.service)" = 0
            test "$(systemctl show -p Result --value das-vm-reader.service)" = success
            return 0
        fi
        if systemctl is-failed --quiet das-vm-reader.service; then
            journalctl -u das-vm-reader.service -o cat --no-pager | grep -E '^VM_PROBE_(STAGE|ERROR|METADATA) ' || true
            return 1
        fi
        sleep .2
    done
    return 1
}
for mode in positive wrong-name wrong-executable plaintext reload positive; do
    phase="adapter_$mode"
    printf 'VM_PHASE_%s\n' "$phase"
    systemctl stop das-vm-reader.service
    rm -f /run/das-vm-results/passed /run/das-vm-results/ready /run/das-systemd-vm-fixture/continue
    make_binding "$mode"
    if test "$mode" = plaintext; then
        sed -i 's/LoadCredentialEncrypted=reader:/LoadCredential=reader:/' /etc/systemd/system/das-vm-reader.service
    else
        sed -i 's/LoadCredential=reader:/LoadCredentialEncrypted=reader:/' /etc/systemd/system/das-vm-reader.service
    fi
    systemctl daemon-reload
    systemctl start das-vm-reader.service
    if test "$mode" = reload; then
        for unused in $(seq 1 450); do
            test ! -f /run/das-vm-results/ready || break
            sleep .2
        done
        test -f /run/das-vm-results/ready
        systemctl daemon-reload
        touch /run/das-systemd-vm-fixture/continue
    fi
    wait_result
    printf 'VM_ADAPTER_PASS %s\n' "$mode"
done
echo VM_ADAPTER_ALL_PASS
phase=complete
poweroff
