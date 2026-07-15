# Lima Linux package acceptance

This harness is the approved macOS surrogate for native ARM64 package evidence
while the DASServer is unavailable. It runs Ubuntu 24.04 and AlmaLinux 9 guests
sequentially, builds the native DEB/RPM, and validates install, same-version
upgrade/reinstall hooks, reboot recovery, cgroup-v2 resource properties, final
uninstall, and persistent-state retention.

AWS CLI is a recommended rather than hard OS-package dependency because Ubuntu
24.04 ARM64 does not publish an ``awscli`` package. The guest harness installs
official AWS CLI v2 ARM64 and verifies it before package installation.

```bash
./deploy/lima/package-acceptance.sh ubuntu
./deploy/lima/package-acceptance.sh alma
# or both sequentially
./deploy/lima/package-acceptance.sh all
```

Successful guests are deleted after their evidence is copied to
`$HOME/.dasobjectstore-codex-validation/lima/evidence`. Set
`DASOBJECTSTORE_LIMA_KEEP=1` to retain a guest for inspection. Failed guests
are retained automatically. `package-acceptance.sh delete` removes only the two
harness-owned guest names.

The source packages are built from committed `HEAD`; unrelated uncommitted
worktree edits never enter the guest. A previously verified WebAssembly bundle
is supplied explicitly through `DASOBJECTSTORE_PREBUILT_WEB_DIST`, avoiding a
second architecture-independent Web build in each Linux guest. Package content
still runs the production authentication guard.

The two-day self-signed listener certificate is generated inside each
disposable guest after package installation. It is local surrogate material,
never enters a package or Git, and does not relax the production listener.

This evidence does not close physical enclosure, SMART/NVMe, replacement,
multi-HDD/Garage durability, performance, or x86_64 acceptance. Those remain
for the real DASServer.
