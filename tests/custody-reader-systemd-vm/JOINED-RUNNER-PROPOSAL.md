# Held joined runner integration

No execution is authorized by this proposal. Baseline full-loader PASS and
independent runner review remain prerequisites. The existing modern loader
runner and its pinned adapter stay unchanged while that baseline runs.

The smallest subsequent change is one fixture-only `joined-kvm-modern` mode in
the existing `run.sh`, reusing its verified modern QEMU/firmware, exact public
Fedora44 backing disk, fresh overlay/vars/seed, no NIC, and existing independently
bounded KVM container. It is not a product endpoint/CLI or lifecycle change.

Before boot, this mode would:

1. Check a newly frozen stripped adapter hash whose exact source contains the
   joined entries and their live-listener retry observer. Preserve the previous
   loader artifact's distinct attribution; never call a successor test qualified
   by the older artifact's run.
2. Copy the exact public71-RPM AWS closure to owned seed staging. Check the fixed
   closure manifest and payload hashes there. Generate its local repository
   metadata using the signed `createrepo_c` from the already reviewed modern
   public tool prefix; no guest package installation or credential input occurs
   in this preparation step. Missing tool/library support denies.
3. Add only the committed `tls-guest.sh` (as guest entry), installer/transaction
   guard, responder, exact adapter and public closure to the read-only seed.
   Hash the seed and firmware as the existing runner does. No TLS/Ed25519/backend
   key is created until inside the disposable guest; none is a seed input.
4. Keep all container/VM resource and independent wall-clock limits. On timeout
   stop only the owned guest/container; never export booted disks or commit a
   container containing generated credential state.

The guest's actual offline installer first proves unchanged prior package set,
exact new Install-only transaction, signed payloads, RPM transaction test and
unchanged systemd binaries. It then starts the reviewed real loader/TLS client
matrix. Readiness requires both the loaded reader listener and a mode-specific
backend marker written only after successful HTTPServer bind/listen; service
Type=exec is not used as port-readiness evidence.

The reader keeps its real TCP listener alive until the verifier has completed
both original-byte and freshly re-signed same-ID replay attempts. Any second
connection fails the case, independently of GET count or an error returned by
the client. The actual journal must still contain exactly the original attempt,
with unchanged status/marker/result; failed exchanges must never be passed.
No second request is dispatched. The per-case GET count, immutable publication
and ledger hashes, exact binary/package identities and non-secret attempt
journals are the evidence to select; private key directories and credential
bytes must not enter logs or exported evidence.

These are proposed runner steps, not executed results. Even a complete joined
PASS would qualify the real AWS/FIFO/TLS protocol composition with a synthetic
loopback S3 responder, not actual Garage retention, backend TLS/authentication,
physical off-NUC independence or release admission.
