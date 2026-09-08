#!/bin/bash
# Reviewed public-only extraction and solver plan. NEVER executes a transaction.
set -euo pipefail
export LC_ALL=C
umask 022
trap 'exit 124' TERM
( sleep 900; kill -TERM "$$" ) &
qemu_watchdog_pid=$!
trap 'kill "$qemu_watchdog_pid" 2>/dev/null || true' EXIT
readonly out=/opt/das-qemu-fedora44
test "$(uname -m)" = aarch64
test "$(rpm -E '%fedora')" = 44
test ! -e "$out/repo-tools"
sha256sum --check "$out/evidence/sha256.txt"
mkdir "$out/repo-tools" "$out/archives"
rpm -qa --qf '%{NAME} %{EPOCHNUM}:%{VERSION}-%{RELEASE} %{ARCH}\n' | sort > "$out/evidence/installed-before.txt"
for package in createrepo_c createrepo_c-libs drpm; do
    case "$package" in
      drpm) file="$out/rpms/drpm-0.5.3-2.fc44.aarch64.rpm" ;;
      *) file="$out/rpms/$package-1.2.1-5.fc44.aarch64.rpm" ;;
    esac
    rpmkeys --define '_keyring rpmdb' --define '_pkgverify_level signature' \
      --dbpath "$out/keydb" --checksig --verbose "$file"
    cp "$file" "$out/archives/"
    archive="$out/archives/${file##*/}.tar"
    rpm2archive "$out/archives/${file##*/}" > "$archive"
    tar -tf "$archive" > "$out/evidence/$package-archive-paths.txt"
    awk '/^\// || /(^|\/)\.\.(\/|$)/ { bad=1 } END { exit bad }' "$out/evidence/$package-archive-paths.txt"
    tar --no-same-owner -xf "$archive" -C "$out/repo-tools"
done
# Only these three package payloads are overlaid, not older libc/loader libraries.
LD_LIBRARY_PATH="$out/repo-tools/usr/lib64" ldd "$out/repo-tools/usr/bin/createrepo_c" > "$out/evidence/createrepo-ldd.txt"
if grep -q 'not found' "$out/evidence/createrepo-ldd.txt"; then exit 1; fi
LD_LIBRARY_PATH="$out/repo-tools/usr/lib64" "$out/repo-tools/usr/bin/createrepo_c" "$out/rpms"
dnf5 --assumeyes --disable-repo='*' --repofrompath=fixture,file://"$out/rpms" \
 --setopt=fixture.gpgcheck=1 --setopt=fixture.gpgkey=file://"$out/fedora44.asc" \
 --setopt=install_weak_deps=False install --no-allow-downgrade \
 --store="$out/transaction" \
 qemu-system-aarch64-core qemu-img edk2-aarch64 genisoimage createrepo_c
rpm -qa --qf '%{NAME} %{EPOCHNUM}:%{VERSION}-%{RELEASE} %{ARCH}\n' | sort > "$out/evidence/installed-after-plan.txt"
test "$(sha256sum "$out/evidence/installed-before.txt" | awk '{print $1}')" = \
     "$(sha256sum "$out/evidence/installed-after-plan.txt" | awk '{print $1}')"
echo PUBLIC_QEMU_TRANSACTION_STORED_NOT_EXECUTED
