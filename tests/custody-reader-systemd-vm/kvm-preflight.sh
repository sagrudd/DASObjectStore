#!/bin/bash
# Public no-boot device/access probe. No ioctl, keys or host mutation.
set -euo pipefail
test "$(id -u)" = 1000
test "$(id -g)" = 1000
test "$(id -G)" = '1000 994'
test "$(awk '/CapEff:/{print $2}' /proc/self/status)" = 0000000000000000
test "$(awk '/NoNewPrivs:/{print $2}' /proc/self/status)" = 1
test -c /dev/kvm
test "$(stat -c '%t:%T' /dev/kvm)" = a:e8
test -r /dev/kvm && test -w /dev/kvm
id
stat -c '%F %u:%g %a %t:%T' /dev/kvm
echo KVM_PUBLIC_PREFLIGHT_PASS
