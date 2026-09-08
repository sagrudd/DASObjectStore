#!/bin/bash
# Container entry point: public inputs only until the isolated guest boots.
set -euo pipefail
test "$(id -u)" = 1000
test -e /.dockerenv
test "$(awk '/CapEff:/{print $2}' /proc/self/status)" = 0000000000000000
test "$(awk '/NoNewPrivs:/{print $2}' /proc/self/status)" = 1
acceleration=(-accel tcg,thread=multi -cpu max)
tool_env=()
qemu=qemu-system-aarch64
qemu_img=qemu-img
iso=genisoimage
firmware=/usr/share/AAVMF/AAVMF_CODE.fd
variables=/usr/share/AAVMF/AAVMF_VARS.fd
machine=virt-7.2
data_path=()
if test "${1:-adapter}" = loader-kvm-modern || test "${1:-adapter}" = joined-kvm-modern || test "${1:-adapter}" = garage-kvm-modern || test "${1:-adapter}" = garage-tls-kvm-modern; then
    prefix=/opt/das-qemu-tools
    tool_env=(env LD_LIBRARY_PATH="$prefix/usr/lib64")
    qemu="$prefix/usr/bin/qemu-system-aarch64"
    qemu_img="$prefix/usr/bin/qemu-img"
    iso="$prefix/usr/bin/genisoimage"
    firmware="$prefix/usr/share/edk2/aarch64/QEMU_EFI-silent-pflash.raw"
    variables="$prefix/usr/share/edk2/aarch64/vars-template-pflash.raw"
    machine=virt-10.2
    data_path=(-L "$prefix/usr/share/qemu")
    if test "${1:-adapter}" = loader-kvm-modern; then
        test "$(sha256sum /adapter | awk '{print $1}')" = 29ea1504285ee4f6708f5cd5b11bc4d61a47b544229201d43a7237f2fe0b9fe2
    elif test "${1:-adapter}" = joined-kvm-modern; then
        # Distinct7843e1a7 joined-driver artifact; never attributed to baseline29ea.
        test "$(sha256sum /adapter | awk '{print $1}')" = e4ae758ff55bfff831e70b6b429752c398fd5ca1e9397c24d4d0f1a22622ca2f
    elif test "${1:-adapter}" = garage-tls-kvm-modern; then
        # Distinct5d49f949 diagnostic source; no production/grant correction.
        test "$(sha256sum /adapter | awk '{print $1}')" = 3ece032e476c5e79985394074fa94d420e432d5e26210158444eac82afc4d281
    else
        # Actual Garage role-correct source1f05aced, reviewed borrowed composition.
        test "$(sha256sum /adapter | awk '{print $1}')" = 8abb7395214d5c0aa9421eca61657e6a7781e97ef00be42064c468953d90ce96
    fi
fi
if test "${1:-adapter}" = loader-kvm || test "${1:-adapter}" = loader-kvm-modern || test "${1:-adapter}" = joined-kvm-modern || test "${1:-adapter}" = garage-kvm-modern || test "${1:-adapter}" = garage-tls-kvm-modern; then
    test "$(id -g)" = 1000
    case " $(id -G) " in *' 994 '*) ;; *) exit 1;; esac
    test -c /dev/kvm
    test "$(stat -c '%t:%T' /dev/kvm)" = a:e8
    test -r /dev/kvm && test -w /dev/kvm
    acceleration=(-accel kvm -cpu host)
else
    test ! -e /dev/kvm
fi
mkdir /tmp/vm /tmp/seed
cp /adapter /tmp/seed/adapter
if test "${1:-adapter}" != loader-kvm-modern && test "${1:-adapter}" != joined-kvm-modern && test "${1:-adapter}" != garage-kvm-modern && test "${1:-adapter}" != garage-tls-kvm-modern; then strip --strip-debug /tmp/seed/adapter; fi
ldd /tmp/seed/adapter
sha256sum /tmp/seed/adapter
/tmp/seed/adapter runtime::custody_reader::systemd --skip actual_systemd_vm_adapter_boundary
case "${1:-adapter}" in
 adapter) cp /custody-reader-systemd-vm/guest.sh /tmp/seed/guest.sh ;;
 loader|loader-kvm|loader-kvm-modern)
   test -f /aws
   cp /aws /tmp/seed/aws
   sha256sum /tmp/seed/aws
   cp /custody-reader-systemd-vm/loader-guest.sh /tmp/seed/guest.sh ;;
 joined-kvm-modern|garage-kvm-modern|garage-tls-kvm-modern)
   # Public-only preparation. Never copy the prep key database/GnuPG directory.
   aws_source=/opt/das-aws-fedora44
   test "$(sha256sum "$aws_source/evidence/sha256.txt" | awk '{print $1}')" = a4e9bc0fb3de4422e3ac814d46459f71a5677dc3516df59363cc8f2554ea52bf
   sha256sum --check "$aws_source/evidence/sha256.txt"
   mkdir /tmp/seed/aws-rpms /tmp/seed/aws-rpms/rpms /tmp/seed/aws-rpms/evidence
   aws_payloads=("$aws_source"/rpms/*.rpm)
   test "${#aws_payloads[@]}" = 71
   for payload in "${aws_payloads[@]}"; do
       test -f "$payload" && test ! -L "$payload"
       cp "$payload" /tmp/seed/aws-rpms/rpms/
   done
   cp "$aws_source/evidence/sha256.txt" /tmp/seed/aws-rpms/evidence/
   cp "$aws_source/fedora44.asc" /tmp/seed/aws-rpms/
   # Copied payloads are checked against the exact same manifest, with only the
   # fixed staging prefix changed for this verification command.
   sed 's|/opt/das-aws-fedora44/rpms/|/tmp/seed/aws-rpms/rpms/|' "$aws_source/evidence/sha256.txt" | sha256sum --check -
   "${tool_env[@]}" "$prefix/usr/bin/createrepo_c" /tmp/seed/aws-rpms/rpms
   test -f /tmp/seed/aws-rpms/rpms/repodata/repomd.xml
   if test "${1:-adapter}" = garage-kvm-modern || test "${1:-adapter}" = garage-tls-kvm-modern; then
       test -f /garage && test ! -L /garage
       test "$(sha256sum /garage | awk '{print $1}')" = 8ced2ad3040262571de08aa600959aa51f97576d55da7946fcde6f66140705e2
       cp /garage /tmp/seed/garage
       test "$(sha256sum /tmp/seed/garage | awk '{print $1}')" = 8ced2ad3040262571de08aa600959aa51f97576d55da7946fcde6f66140705e2
       if test "${1:-adapter}" = garage-tls-kvm-modern; then
           cp /custody-reader-systemd-vm/garage-tls-guest.sh /tmp/seed/guest.sh
           cp /custody-reader-systemd-vm/garage-guest.sh /tmp/seed/garage-guest.sh
           cp /custody-reader-systemd-vm/tls-guest.sh /tmp/seed/tls-guest.sh
           /tmp/seed/adapter continuation_fixture_requires_only_selected_read_grant --test-threads=1
           /tmp/seed/adapter continuation_selects_garage_region --test-threads=1
           /tmp/seed/adapter prepare_diagnostics_never_emit_panic_payload --test-threads=1
       else
           cp /custody-reader-systemd-vm/garage-guest.sh /tmp/seed/guest.sh
       fi
       /tmp/seed/adapter actual_garage_dispatch --test-threads=1
       /tmp/seed/adapter runtime::custody_garage::retention --test-threads=1
       /tmp/seed/adapter finite_batch --test-threads=1
   else
       cp /custody-reader-systemd-vm/tls-guest.sh /tmp/seed/guest.sh
   fi
   for file in aws-guest-install.sh aws-transaction-guard.py s3-responder.py; do
       cp "/custody-reader-systemd-vm/$file" /tmp/seed/
   done
   /tmp/seed/adapter joined_retry_observer --test-threads=1
   ;;
 *) exit 1 ;;
esac
cp /binding.jcs.json /tmp/seed/binding.json
printf '%s\n' 'instance-id: das-systemd-vm-56096818' 'local-hostname: das-systemd-vm' > /tmp/seed/meta-data
printf '%s\n' '#cloud-config' 'runcmd:' '  - [ mkdir, -p, /mnt/cidata ]' \
 '  - [ mount, -o, ro, /dev/disk/by-label/cidata, /mnt/cidata ]' \
 '  - [ bash, /mnt/cidata/guest.sh ]' > /tmp/seed/user-data
"${tool_env[@]}" "$iso" -quiet -output /tmp/vm/seed.iso -volid cidata -joliet -rock /tmp/seed
cp "$variables" /tmp/vm/AAVMF_VARS.fd
"${tool_env[@]}" "$qemu_img" create -q -f qcow2 -F qcow2 -b /opt/custody-systemd-vm/public/Fedora-Cloud-Base-Generic-44-1.7.aarch64.qcow2 /tmp/vm/guest.qcow2
sha256sum /tmp/vm/seed.iso "$firmware" /tmp/vm/AAVMF_VARS.fd
exec "${tool_env[@]}" "$qemu" -no-reboot "${data_path[@]}" \
 -boot order=c,menu=off,strict=on \
 "${acceleration[@]}" -machine "$machine" -smp 2 -m 2048 \
 -nodefaults -nic none -display none -monitor none -serial stdio \
 -drive if=pflash,format=raw,readonly=on,file="$firmware" \
 -drive if=pflash,format=raw,file=/tmp/vm/AAVMF_VARS.fd \
 -drive if=none,id=os,format=qcow2,file=/tmp/vm/guest.qcow2 -device virtio-blk-pci,drive=os,bootindex=1 \
 -drive if=none,id=seed,format=raw,readonly=on,file=/tmp/vm/seed.iso -device virtio-blk-pci,drive=seed
