#!/bin/bash
# Public tool payload composition only: no RPM install, scriptlets, or VM boot.
set -euo pipefail
export LC_ALL=C
umask 022
trap 'exit 124' TERM
( sleep 900; kill -TERM "$$" ) &
qemu_watchdog_pid=$!
trap 'kill "$qemu_watchdog_pid" 2>/dev/null || true' EXIT
readonly source=/opt/das-qemu-fedora44
readonly prefix=/opt/das-qemu-tools
readonly evidence=/opt/das-qemu-payload-evidence
test ! -e "$prefix" && test ! -e "$evidence"
test ! -e /dev/kvm
test "$(awk '/CapEff:/{print $2}' /proc/self/status)" = 0000000000000000
test "$(awk '/NoNewPrivs:/{print $2}' /proc/self/status)" = 1
test "$(sha256sum "$source/transaction/transaction.json" | awk '{print $1}')" = c55ec4fe0d8d643cf8546be91a0dadb7fc466317df524f49d080d6d66aff0bf9
mkdir "$prefix" "$evidence" "$evidence/archives"
test "$(sha256sum "$source/evidence/sha256.txt" | awk '{print $1}')" = d514c95e9123508a92ecdc96edf0e08ee0452962a7a76c58c13189c05f301f5d
sha256sum --check "$source/evidence/sha256.txt" > "$evidence/source-hashes.txt"
rpm -qa --qf '%{NAME} %{EPOCHNUM}:%{VERSION}-%{RELEASE} %{ARCH}\n' | sort > "$evidence/base-before.txt"
test "$(sha256sum "$evidence/base-before.txt" | awk '{print $1}')" = f96d1eee3844fd3468a88bb5b55cb428359a89c7aea5ec053df8d4a026665db9
count=0
for file in "$source"/transaction/packages/*.rpm; do
    count=$((count+1))
    name=${file##*/}
    grep -Fq "\"package_path\":\".\\/packages\\/$name\"" "$source/transaction/transaction.json"
    test "$(sha256sum "$file" | awk '{print $1}')" = \
         "$(sha256sum "$source/rpms/$name" | awk '{print $1}')"
    rpmkeys --define '_keyring rpmdb' --define '_pkgverify_level signature' \
      --dbpath "$source/keydb" --checksig --verbose "$file" >> "$evidence/signatures.txt"
    rpm -qp --qf '%{NAME} %{EPOCHNUM}:%{VERSION}-%{RELEASE} %{ARCH}\n' "$file" >> "$evidence/payload-nevra.txt"
    # Every link is relative and remains lexically within the isolated prefix.
    rpm -qp --qf '[%{FILENAMES}\t%{FILELINKTOS}\n]' "$file" > "$evidence/archives/$name.links"
    awk -F '\t' '
      $2!="" {
        if ($2 ~ /^\//) exit 1;
        n=split($1,p,"/"); depth=n-2;
        n=split($2,p,"/");
        for(i=1;i<=n;i++) { if(p[i]=="..") depth--; else if(p[i]!="." && p[i]!="") depth++; if(depth<0) exit 1; }
      }' "$evidence/archives/$name.links"
    rpm2archive "$file" > "$evidence/archives/$name.tar"
    tar -tf "$evidence/archives/$name.tar" > "$evidence/archives/$name.paths"
    awk '/^\// || /(^|\/)\.\.(\/|$)/ { bad=1 } END { exit bad }' "$evidence/archives/$name.paths"
    tar --no-same-owner --no-same-permissions -xf "$evidence/archives/$name.tar" -C "$prefix"
done
test "$count" = 34
# Setuid/setgid payload modes are unnecessary for these unprivileged tools.
test -z "$(find "$prefix" -type f -perm /6000 -print -quit)"
export LD_LIBRARY_PATH="$prefix/usr/lib64"
for tool in qemu-system-aarch64 qemu-img genisoimage createrepo_c; do
    ldd "$prefix/usr/bin/$tool" > "$evidence/$tool.ldd"
    if grep -q 'not found' "$evidence/$tool.ldd"; then exit 1; fi
    "$prefix/usr/bin/$tool" --version > "$evidence/$tool.version" 2>&1
    sha256sum "$prefix/usr/bin/$tool" >> "$evidence/tools.sha256"
done
test -f "$prefix/usr/share/edk2/aarch64/QEMU_EFI-silent-pflash.raw"
test -f "$prefix/usr/share/edk2/aarch64/vars-template-pflash.raw"
test -d "$prefix/usr/share/qemu"
sha256sum "$prefix/usr/share/edk2/aarch64/QEMU_EFI-silent-pflash.raw" \
  "$prefix/usr/share/edk2/aarch64/vars-template-pflash.raw" > "$evidence/firmware.sha256"
unset LD_LIBRARY_PATH
rpm -qa --qf '%{NAME} %{EPOCHNUM}:%{VERSION}-%{RELEASE} %{ARCH}\n' | sort > "$evidence/base-after.txt"
test "$(sha256sum "$evidence/base-before.txt" | awk '{print $1}')" = \
     "$(sha256sum "$evidence/base-after.txt" | awk '{print $1}')"
echo PUBLIC_QEMU_PAYLOAD_TOOLS_VERIFIED_NO_BOOT
