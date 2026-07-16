Deployment Acceptance
=====================

DASObjectStore keeps local compatibility evidence separate from physical DAS
acceptance. All automated payloads belong beneath
``$HOME/.dasobjectstore-codex-validation`` and must remain below 1 TiB. Never
use user, customer, or project data.

Local release-candidate sequence
--------------------------------

Run each harness from the same committed revision:

.. code-block:: console

   deploy/macos/test-user-service.sh
   deploy/lima/package-acceptance.sh all
   deploy/acceptance/product-profile-mvp.sh
   deploy/acceptance/application-auth-mvp.sh
   deploy/acceptance/auth-authority-switch-mvp.sh
   deploy/acceptance/remote-upload-completion-mvp.sh
   DASOBJECTSTORE_LOCAL_ROOT="$HOME/.dasobjectstore-codex-validation" \
     DASOBJECTSTORE_LOCAL_PROFILE=alleleanchor-mvp \
     deploy/local-docker/local.sh up
   DASOBJECTSTORE_LOCAL_ROOT="$HOME/.dasobjectstore-codex-validation" \
     DASOBJECTSTORE_LOCAL_PROFILE=alleleanchor-mvp \
     deploy/local-docker/local.sh smoke
   deploy/acceptance/verify-release-readiness.sh

The verifier rejects missing, failed, or stale-commit evidence. A successful
report proves the transactional per-user macOS service lifecycle, native ARM64
Ubuntu and AlmaLinux package lifecycle, root-scoped Garage S3 compatibility,
and the bounded product-profile MVP workflow for one exact commit. The product
workflow provisions and idempotently reprovisions a Synoptikon-owned folder
profile, writes 64 generated 4 KiB objects, exercises list/get/range/verify/
delete, rejects an over-quota write, and reopens durable catalogue/accounting
state. It cleans its fixture and never uses user, customer, or project data.
The application-auth workflow uses generated public/private key material only
in process to prove administrator identity registration, Ed25519 proof
exchange, overlapping rotation, key and principal revocation, per-request mTLS
revocation enforcement, and redacted audit persistence. Private keys are never
written to the evidence or daemon registries. It does not imply production CA
or physical DAS readiness.

The authentication-authority switch workflow seeds generated intrinsic state,
runs the packaged migration executable, authenticates the preserved session
through the real Monas composer, proves Monas-side revocation, and then proves
the retained intrinsic source still authenticates for rollback. Its evidence
is explicitly ``surrogate``: it validates the software transition and
non-exporting cookie boundary, not package service switching on a deployment
host.

The remote-upload completion workflow uses the real daemon request handler and
durable session, identity, capability, credential, and replay registries with
an injected provider/catalogue authority. It proves scope intersection,
bounded capability issuance, forged-capability rejection, verify-before-
commit ordering, exact-replay idempotency, and retry after catalogue failure.
The evidence labels provider execution as ``surrogate_only``: it does not
replace the later live Garage ``head-object`` and shared-SQLite appliance run.

Hardware acceptance after returning home
----------------------------------------

Wait for the operator to confirm a quiescent DASServer window before using the
documented SSH identity or touching the appliance. Then run x86_64 package
parity plus generated-data physical-drive identity, SMART/NVMe, replacement,
full-disk/corruption, multi-HDD/Garage durability, control-plane saturation,
and performance/soak matrices. Those results must be recorded separately; a
surrogate report must never be relabelled as hardware acceptance.
