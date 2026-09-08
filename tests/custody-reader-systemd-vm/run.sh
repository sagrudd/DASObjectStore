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
if test "${1:-adapter}" = loader-kvm-modern; then
    prefix=/opt/das-qemu-tools
    tool_env=(env LD_LIBRARY_PATH="$prefix/usr/lib64")
    qemu="$prefix/usr/bin/qemu-system-aarch64"
    qemu_img="$prefix/usr/bin/qemu-img"
    iso="$prefix/usr/bin/genisoimage"
    firmware="$prefix/usr/share/edk2/aarch64/QEMU_EFI-silent-pflash.raw"
    variables="$prefix/usr/share/edk2/aarch64/vars-template-pflash.raw"
    machine=virt-10.2
    data_path=(-L "$prefix/usr/share/qemu")
    test "$(sha256sum /adapter | awk '{print $1}')" = 29ea1504285ee4f6708f5cd5b11bc4d61a47b544229201d43a7237f2fe0b9fe2
fi
if test "${1:-adapter}" = loader-kvm || test "${1:-adapter}" = loader-kvm-modern; then
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
if test "${1:-adapter}" != loader-kvm-modern; then strip --strip-debug /tmp/seed/adapter; fi
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
