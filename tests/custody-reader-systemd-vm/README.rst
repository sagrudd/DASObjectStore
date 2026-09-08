Real-systemd boundary fixture: public preparation only
====================================================

This directory does not install DAS or confer activation authority.  The
preparation script downloads authenticated public inputs and extracts tool
payloads without running package maintainer scripts.  It never boots a VM.
The real-systemd positive remains unqualified until a separately reviewed run.

Preparation is restricted to the reviewed ARM64 base image
``sha256:81597dc683e32508706300031ae0391e332f122c4d54fa92661aa674a8dda0f4``.
The outer controller must inspect that exact image for absent declared volumes,
use ``--pull=never``, and create only its own disposable container with no mounts,
devices, privilege, or host namespace sharing.  Preparation permits public
network downloads only.  Resource limits are four CPUs, 6 GiB RAM, 256 processes,
all capabilities dropped, no-new-privileges, and a 900-second total timeout.
The downloaded payload budget is 4 GiB and the guest virtual disk limit 24 GiB.

APT authenticates the retained Debian 20260907T000000Z snapshot indexes and
package payloads.  The Fedora checksum signature must verify against exactly
``36F612DCF27F7D1A48A835E4DBFCF71C6D9F90A6`` before downloading and hashing the
selected image.  No unsigned checksum or mutable-image fallback is accepted.
Actual package versions come from the retained DEBs, not unchanged dpkg status.
The immutable preparation image retains indexes, signatures, public keyring,
packages, and the pristine guest image below ``/opt/custody-systemd-vm``.

The first bounded preparation attempt stopped before useful downloads because
APT attempted UID/group changes with all capabilities dropped.  The correction
keeps APT under the container's existing UID 0, disables inherited APT hooks,
and retains capability-free isolation; it does not add setuid capabilities.
The failed owned container was removed.  GNU tar preserves the base's merged-usr
directory symlinks during extraction and checks them after each package.

Verified preparation receipt
----------------------------

The corrected public-only attempt completed with exit zero in 114.66 seconds
on 2026-09-08.  Its exact script SHA256 is
``37b88897ea474b3ad28b98c9df327f8cad689b0da8634ee9620084cf5553cd2a``.
The resulting clean, unbooted image is
``sha256:b3fa5d9f3fcfe6b64d73fb5ff37c0d12a972600145430d553bb54df525423865``
(``das-systemd-vm-public:fedora44-56096818``).  It has no declared volumes and
defaults to immediate exit, not boot.  Both stopped preparation containers have
been removed; the public image and its authenticated inputs remain recoverable.

The selected guest image verified as
``55c60a3b80d3616a08705afd0459e75fe9f03c54aba7a46e4002a41a72fa0d5b``.
Its qcow2 physical size is 528158720 bytes and virtual size 5368709120 bytes,
with no backing image, corruption or dirty flag.  Actual QEMU reports 7.2.22
(Debian ``1:7.2+dfsg-7+deb12u18+b3``).  The 47 downloaded tool packages total
25.2 MB.  ``evidence/`` retains actual package/index/public-input hashes,
signature-verifier output, tool versions and image metadata.  The raw public
authentication chain remains in the clean image.  Preparation output was
summarized here; its stream digest was
``884ecf13d2f7e498c60dfc357742718aefa190508507f3108ebd5e6b841cc0ba``.
No real-systemd or DAS runtime qualification follows from this receipt.

Proposed boot boundary (not executed)
------------------------------------

The clean preparation image may subsequently be reviewed for a fresh container
with networking disabled, no mounts/devices, all capabilities dropped and
no-new-privileges.  QEMU runs as an unprivileged container user, using TCG only.
No KVM, host cgroup binding, shared directory, monitor socket, or published port
is permitted.  A per-run qcow2 overlay, copied UEFI variables, and read-only
NoCloud seed ISO remain inside that container.  The seed contains public fixture
code only.  No secure-boot claim is made; distribution example snakeoil key
payloads are not fixture identities and must not be used for credential trust.

The proposed QEMU argument vector, pending root review, is::

  /usr/bin/qemu-system-aarch64
    -accel tcg,thread=multi -machine virt-7.2 -cpu max -smp 2 -m 2048
    -nodefaults -nic none -display none -monitor none -serial stdio
    -drive if=pflash,format=raw,readonly=on,file=/usr/share/AAVMF/AAVMF_CODE.fd
    -drive if=pflash,format=raw,file=/run/vm/AAVMF_VARS.fd
    -drive if=none,id=os,format=qcow2,file=/run/vm/guest.qcow2
    -device virtio-blk-pci,drive=os
    -drive if=none,id=seed,format=raw,readonly=on,file=/run/vm/seed.iso
    -device virtio-blk-pci,drive=seed

The boot/test phase also has a 900-second total bound.  It must first measure
the actual guest systemd version and then exercise the selected v259 boundary,
not infer it from a current distribution package listing.  Any synthetic
systemd host credential key is generated only inside the disposable guest after
boot approval.  Retain coarse results and public hashes only.  Never export or
commit a booted container or guest disk after credential generation; remove its
overlay, variables, seed, keys and owned container after retaining evidence.

Primary public input references
-------------------------------

* https://fedoraproject.org/cloud/download/
* https://fedoraproject.org/security/
* https://snapshot.debian.org/archive/debian/20260907T000000Z/
* https://snapshot.debian.org/archive/debian-security/20260907T000000Z/
* https://www.qemu.org/docs/master/system/arm/virt.html
* https://docs.cloud-init.io/en/latest/reference/datasources/nocloud.html
