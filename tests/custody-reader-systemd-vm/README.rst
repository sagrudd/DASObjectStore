Isolated real-systemd boundary fixture
=====================================

This directory does not install DAS or confer activation authority.  The
preparation script downloads authenticated public inputs and extracts tool
payloads without running package maintainer scripts.  It never boots a VM.
The separate boot scripts exercise the actual Rust adapter inside a disposable
guest. Preparation alone is not a real-systemd positive; actual run outcomes
and limitations are retained separately below ``evidence/``.

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

Reviewed isolated boot boundary
------------------------------

The clean preparation image was separately reviewed for a fresh container
with networking disabled, no mounts/devices, all capabilities dropped and
no-new-privileges.  QEMU runs as an unprivileged container user, using TCG only.
No KVM, host cgroup binding, shared directory, monitor socket, or published port
is permitted.  A per-run qcow2 overlay, copied UEFI variables, and read-only
NoCloud seed ISO remain inside that container.  The seed contains public fixture
code only.  No secure-boot claim is made; distribution example snakeoil key
payloads are not fixture identities and must not be used for credential trust.

The reviewed QEMU argument vector, implemented in ``run.sh``, is::

  /usr/bin/qemu-system-aarch64
    -no-reboot -boot order=c,menu=off,strict=on
    -accel tcg,thread=multi -machine virt-7.2 -cpu max -smp 2 -m 2048
    -nodefaults -nic none -display none -monitor none -serial stdio
    -drive if=pflash,format=raw,readonly=on,file=/usr/share/AAVMF/AAVMF_CODE.fd
    -drive if=pflash,format=raw,file=/tmp/vm/AAVMF_VARS.fd
    -drive if=none,id=os,format=qcow2,file=/tmp/vm/guest.qcow2
    -device virtio-blk-pci,drive=os,bootindex=1
    -drive if=none,id=seed,format=raw,readonly=on,file=/tmp/vm/seed.iso
    -device virtio-blk-pci,drive=seed

The boot/test phase also has a 900-second total bound.  It must first measure
the actual guest systemd version and then exercise the selected v259 boundary,
not infer it from a current distribution package listing.  Any synthetic
systemd host credential key is generated only inside the disposable guest after
boot approval.  Retain coarse results and public hashes only.  Never export or
commit a booted container or guest disk after credential generation; remove its
overlay, variables, seed, keys and owned container after retaining evidence.

The driver tests actual encrypted credential acquisition metadata, the closed
systemd property checks, executable identity, wrong-name/wrong-executable and
plaintext rejection, and restart-required behavior after a real daemon reload.
It checks the dedicated file-permission helper without reading credential
contents. It does not exercise the complete zeroizing loader, manager, backend,
HTTP adapter, installation or S4 path. Each case requires both its marker and
actual successful unit exit; a powered-off QEMU process is not a test pass.

Actual v259 ACL metadata exposed the old mode-only guard's false denial; the
closed ACL correction and raw public measurements are retained in
``evidence/acl-measurement.txt``. The corrected adapter reached executable
validation but exhausted its 15-second caller deadline under TCG, recorded in
``evidence/acl-corrected-deadline.txt``. The subsequent fixture uses a reviewed
60-second caller allowance within the existing 1–300-second API bounds and
90-second result polling; production defaults and guards are unchanged. The
900-second outer whole-run bound remains in force. The large debug-test
executable is stripped of debug sections, not replaced with a mock adapter.

The corrected real-systemd run passed all six cases on 2026-09-08, with actual
systemd 259.5-1.fc44 and successful unit exits, in 437.77 seconds. Its exact
artifact/source/seed hashes and scope are retained in
``evidence/adapter-all-pass.txt``. The booted guest was removed without state
export. This is the adapter boundary qualification described above, not a
whole-current-tree loader or deployment qualification.

Qualification sequencing lesson: check the actual selected init system's
credential representation before full packaging. Synthetic property/permission
fixtures cannot establish its real ACL construction. Measure and bound
emulator-only test budgets separately from production limits; preserve the
failed runs as evidence rather than rewriting them as successful qualification.

Proposed KVM-only loader successor (not yet executed)
---------------------------------------------------

The original TCG results remain immutable. A separately reviewed successor may
use the existing DGX KVM device, without installing/loading a host module or
changing host users, groups, ACLs, services or packages. Read-only inspection on
2026-09-08 found ``/dev/kvm`` character device10:232, owner0:994, mode0660,
with an existing named UID1000 read/write ACL. This is a fixture resource fact,
not target installation authority. Only this device is proposed; no other
device, host filesystem mount, network or privilege is permitted.

The exact proposed outer invocation is::

  docker create --pull=never --name das-systemd-loader-kvm
    --network none --cap-drop ALL --security-opt no-new-privileges
    --cpus 4 --memory 6g --pids-limit 256 --user 1000:1000
    --group-add 994
    --device /dev/kvm:/dev/kvm:rw
    --entrypoint /usr/bin/timeout
    sha256:b3fa5d9f3fcfe6b64d73fb5ff37c0d12a972600145430d553bb54df525423865
    --signal=TERM --kill-after=10 900
    /bin/bash /custody-reader-systemd-vm/run.sh loader-kvm

Before starting it, inspect absent mounts, networknone, no privilege, ALL
capabilities dropped, NNP and exactly the one selected read/write device.
The selected command explicitly adds group994 only inside the disposable
container, because Docker may not preserve the host's named UID1000 ACL when
constructing its device node. This does not alter host group membership or
grant access to other host files/devices. A public, no-boot preflight must
inspect UID/GID, supplemental group, device identity/mode and actual read/write
access using these same flags. The non-root entry point also requires the
actual character-device identity and read/write access. Denial stops before
boot; there is no root-user, extra-group or accelerator fallback. QEMU differs
only in ``-accel kvm -cpu host`` instead of TCG/max; the
same fresh disks, seed, bootindex, two vCPUs,2GiB guest RAM and900s outer bound
remain. There is no TCG fallback if KVM cannot initialize. Never commit the
booted container. Record actual accelerator and source hashes separately;
neither a KVM test nor a TCG test proves a production latency guarantee.

Modern public tool payload composition
--------------------------------------

The retained old-tool KVM failure occurred in firmware before Linux, not in
the reader. The reviewed modern-tool successor uses the signed Fedora44
QEMU10.2.2 and edk2 20260213 payloads. ``qemu-prepare.sh`` authenticates the
public closure; ``qemu-plan.sh`` stores a repository-solved, install-new-only
selection. The attempted offline RPM replay failed and is retained as failure
evidence, not successful installation.

``qemu-payloads.sh`` instead composes only the exact34 selected payloads under
fresh ``/opt/das-qemu-tools``, without RPM installation, scriptlets or base
library replacement. It verifies selected hashes/signatures, confines relative
links, rejects unresolved runtime dependencies, and records tool/firmware hashes
and unchanged base package metadata. This is a tool-payload composition claim,
not native RPM installation qualification. No guest boots during preparation.

The runner must use explicit prefix binaries and
``LD_LIBRARY_PATH=/opt/das-qemu-tools/usr/lib64``. QEMU's data directory is
``-L /opt/das-qemu-tools/usr/share/qemu``; the pflash code and fresh variables
template are the exact files under ``usr/share/edk2/aarch64`` in that prefix.
The public test binary is pre-stripped in the verified build container with
before/after hashes and tool provenance, and is not modified by this runner.
The resulting clean, unbooted tool image and fixed runner command require
review before another independently recorded loader test. The prior TCG
adapter success and all failed loader attempts retain their original scopes.

Primary public input references
-------------------------------

* https://fedoraproject.org/cloud/download/
* https://fedoraproject.org/security/
* https://snapshot.debian.org/archive/debian/20260907T000000Z/
* https://snapshot.debian.org/archive/debian-security/20260907T000000Z/
* https://www.qemu.org/docs/master/system/arm/virt.html
* https://docs.cloud-init.io/en/latest/reference/datasources/nocloud.html
