#!/bin/bash
# Guest-only, offline, new-packages-only installation. Not host deployment.
set -euo pipefail
export LC_ALL=C
umask 077
test "$(id -u)" = 0
test "$(cat /proc/1/comm)" = systemd
test "$(uname -m)" = aarch64
test "$(rpm -E '%fedora')" = 44
test -f /run/das-systemd-vm-fixture/permit
# The runner must have omitted virtual NICs; loopback alone is intentional.
test "$(find /sys/class/net -mindepth 1 -maxdepth 1 -printf '%f\n')" = lo
readonly out=/opt/das-aws-fedora44
readonly evidence=/run/das-systemd-vm-fixture/aws-install
test ! -e "$out"
test ! -e "$evidence"
mkdir "$evidence"
# Exact public closure, including locally generated repository metadata, copied
# from the read-only seed. Metadata is solver input, not signature authority.
cp -a /mnt/cidata/aws-rpms "$out"
test "$(sha256sum "$out/evidence/sha256.txt" | cut -d' ' -f1)" = a4e9bc0fb3de4422e3ac814d46459f71a5677dc3516df59363cc8f2554ea52bf
sha256sum --check "$out/evidence/sha256.txt" > "$evidence/payload-check.txt"
test -f "$out/rpms/repodata/repomd.xml"
mkdir "$evidence/gnupg"
fingerprint=$(gpg --batch --homedir "$evidence/gnupg" --with-colons --show-keys "$out/fedora44.asc" | awk -F: '$1=="fpr" {print $10; exit}')
test "$fingerprint" = 36F612DCF27F7D1A48A835E4DBFCF71C6D9F90A6
test "$(gpg --batch --homedir "$evidence/gnupg" --with-colons --show-keys "$out/fedora44.asc" | awk -F: '$1=="pub" {n++} END {print n}')" = 1
mkdir "$evidence/keydb"
rpmkeys --define '_keyring rpmdb' --dbpath "$evidence/keydb" --import "$out/fedora44.asc"
for payload in "$out"/rpms/*.rpm; do
    rpmkeys --define '_keyring rpmdb' --define '_pkgverify_level signature' \
        --dbpath "$evidence/keydb" --checksig --verbose "$payload"
done > "$evidence/signatures.txt"
snapshot() {
    rpm -qa --qf '%{NAME}\t%{EPOCHNUM}\t%{VERSION}\t%{RELEASE}\t%{ARCH}\n' | sort
}
snapshot > "$evidence/before.tsv"
sha256sum /usr/lib/systemd/systemd /usr/bin/systemctl /usr/bin/systemd-creds > "$evidence/systemd-before.sha256"
dnf5 --assumeyes --disable-repo='*' --repofrompath=das-vm-aws,file://"$out/rpms" \
    --setopt=das-vm-aws.gpgcheck=1 --setopt=das-vm-aws.gpgkey=file://"$out/fedora44.asc" \
    --setopt=install_weak_deps=False install --no-allow-downgrade \
    --store="$evidence/transaction" awscli2 > "$evidence/plan.log" 2>&1
snapshot > "$evidence/after-plan.tsv"
cmp "$evidence/before.tsv" "$evidence/after-plan.tsv"
python3 /mnt/cidata/aws-transaction-guard.py "$evidence/transaction" "$out" "$evidence/before.tsv" "$evidence"
mapfile -t selected < "$evidence/selected-rpms.txt"
test "${#selected[@]}" -gt 0
test "${#selected[@]}" -le 71
# RPM's actual dependency/conflict transaction test; never --nodeps/replace.
rpm --test -ivh "${selected[@]}" > "$evidence/rpm-test.log" 2>&1
snapshot > "$evidence/after-test.tsv"
cmp "$evidence/before.tsv" "$evidence/after-test.tsv"
# Stored replay preserves the reviewed operations; no ignore/skip relaxation.
dnf5 --assumeyes --disable-repo='*' replay "$evidence/transaction" > "$evidence/replay.log" 2>&1
snapshot > "$evidence/after.tsv"
cmp "$evidence/expected-installed.tsv" "$evidence/after.tsv"
sha256sum --check "$evidence/systemd-before.sha256"
test "$(rpm -q --qf '%{VERSION}-%{RELEASE}' awscli2)" = 2.33.0-1.fc44
sha256sum /usr/bin/aws | cut -d' ' -f1 > /run/das-systemd-vm-fixture/aws-executable.sha256
chmod 644 /run/das-systemd-vm-fixture/aws-executable.sha256
echo VM_AWS_SIGNED_INSTALL_NEW_ONLY_PASS
