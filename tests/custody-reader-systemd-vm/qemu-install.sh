#!/bin/bash
# Root-reviewed exact stored transaction, public unbooted container only.
set -euo pipefail
export LC_ALL=C
trap 'exit 124' TERM
( sleep 900; kill -TERM "$$" ) &
qemu_watchdog_pid=$!
trap 'kill "$qemu_watchdog_pid" 2>/dev/null || true' EXIT
readonly out=/opt/das-qemu-fedora44
test ! -e /dev/kvm
test "$(awk '/CapEff:/{print $2}' /proc/self/status)" = 0000000000000000
test "$(awk '/NoNewPrivs:/{print $2}' /proc/self/status)" = 1
test "$(sha256sum "$out/transaction/transaction.json" | awk '{print $1}')" = c55ec4fe0d8d643cf8546be91a0dadb7fc466317df524f49d080d6d66aff0bf9
sha256sum --check "$out/evidence/sha256.txt" > "$out/evidence/preinstall-hashes.txt"
for file in "$out"/transaction/packages/*.rpm; do
    test "$(sha256sum "$file" | awk '{print $1}')" = \
         "$(sha256sum "$out/rpms/${file##*/}" | awk '{print $1}')"
    rpmkeys --define '_keyring rpmdb' --define '_pkgverify_level signature' \
      --dbpath "$out/keydb" --checksig --verbose "$file" >> "$out/evidence/preinstall-signatures.txt"
done
rpm -qa --qf '%{NAME} %{EPOCHNUM}:%{VERSION}-%{RELEASE} %{ARCH}\n' | sort > "$out/evidence/install-start.txt"
test "$(sha256sum "$out/evidence/install-start.txt" | awk '{print $1}')" = f96d1eee3844fd3468a88bb5b55cb428359a89c7aea5ec053df8d4a026665db9
# No ignore/skip switches, solver network or substitution. This is the reviewed
# 34-new-install transaction, not a fresh resolve from changing repositories.
dnf5 --assumeyes --disable-repo='*' replay "$out/transaction" > "$out/evidence/install-log.txt" 2>&1
if grep -Ei 'scriptlet.*(fail|error)|error.*scriptlet|failed.*scriptlet' "$out/evidence/install-log.txt"; then exit 1; fi
rpm -qa --qf '%{NAME} %{EPOCHNUM}:%{VERSION}-%{RELEASE} %{ARCH}\n' | sort > "$out/evidence/install-end.txt"
while IFS= read -r installed; do
    grep -Fxq "$installed" "$out/evidence/install-end.txt"
done < "$out/evidence/install-start.txt"
test "$(( $(wc -l < "$out/evidence/install-end.txt") - $(wc -l < "$out/evidence/install-start.txt") ))" = 34
qemu-system-aarch64 --version > "$out/evidence/qemu-version.txt"
qemu-img --version > "$out/evidence/qemu-img-version.txt"
genisoimage --version > "$out/evidence/genisoimage-version.txt" 2>&1
createrepo_c --version > "$out/evidence/createrepo-version.txt"
rpm -ql edk2-aarch64 > "$out/evidence/firmware-paths.txt"
sha256sum /usr/bin/qemu-system-aarch64 /usr/bin/qemu-img /usr/bin/genisoimage \
  > "$out/evidence/tool-sha256.txt"
echo PUBLIC_QEMU_INSTALL_COMPLETE_NO_BOOT
