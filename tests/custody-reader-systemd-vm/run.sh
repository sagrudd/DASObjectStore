#!/bin/bash
# Container entry point: public inputs only until the isolated guest boots.
set -euo pipefail
test "$(id -u)" = 1000
test -e /.dockerenv
test "$(awk '/CapEff:/{print $2}' /proc/self/status)" = 0000000000000000
test "$(awk '/NoNewPrivs:/{print $2}' /proc/self/status)" = 1
test ! -e /dev/kvm
mkdir /tmp/vm /tmp/seed
cp /adapter /tmp/seed/adapter
strip --strip-debug /tmp/seed/adapter
ldd /tmp/seed/adapter
sha256sum /tmp/seed/adapter
/tmp/seed/adapter runtime::custody_reader::systemd --skip actual_systemd_vm_adapter_boundary
cp /custody-reader-systemd-vm/guest.sh /tmp/seed/guest.sh
cp /binding.jcs.json /tmp/seed/binding.json
printf '%s\n' 'instance-id: das-systemd-vm-56096818' 'local-hostname: das-systemd-vm' > /tmp/seed/meta-data
printf '%s\n' '#cloud-config' 'runcmd:' '  - [ mkdir, -p, /mnt/cidata ]' \
 '  - [ mount, -o, ro, /dev/disk/by-label/cidata, /mnt/cidata ]' \
 '  - [ bash, /mnt/cidata/guest.sh ]' > /tmp/seed/user-data
genisoimage -quiet -output /tmp/vm/seed.iso -volid cidata -joliet -rock /tmp/seed
cp /usr/share/AAVMF/AAVMF_VARS.fd /tmp/vm/AAVMF_VARS.fd
qemu-img create -q -f qcow2 -F qcow2 -b /opt/custody-systemd-vm/public/Fedora-Cloud-Base-Generic-44-1.7.aarch64.qcow2 /tmp/vm/guest.qcow2
sha256sum /tmp/vm/seed.iso /usr/share/AAVMF/AAVMF_CODE.fd /tmp/vm/AAVMF_VARS.fd
exec qemu-system-aarch64 -no-reboot \
 -boot order=c,menu=off,strict=on \
 -accel tcg,thread=multi -machine virt-7.2 -cpu max -smp 2 -m 2048 \
 -nodefaults -nic none -display none -monitor none -serial stdio \
 -drive if=pflash,format=raw,readonly=on,file=/usr/share/AAVMF/AAVMF_CODE.fd \
 -drive if=pflash,format=raw,file=/tmp/vm/AAVMF_VARS.fd \
 -drive if=none,id=os,format=qcow2,file=/tmp/vm/guest.qcow2 -device virtio-blk-pci,drive=os,bootindex=1 \
 -drive if=none,id=seed,format=raw,readonly=on,file=/tmp/vm/seed.iso -device virtio-blk-pci,drive=seed
