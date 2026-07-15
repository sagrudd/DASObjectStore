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
   DASOBJECTSTORE_LOCAL_ROOT="$HOME/.dasobjectstore-codex-validation" \
     DASOBJECTSTORE_LOCAL_PROFILE=alleleanchor-mvp \
     deploy/local-docker/local.sh up
   DASOBJECTSTORE_LOCAL_ROOT="$HOME/.dasobjectstore-codex-validation" \
     DASOBJECTSTORE_LOCAL_PROFILE=alleleanchor-mvp \
     deploy/local-docker/local.sh smoke
   deploy/acceptance/verify-release-readiness.sh

The verifier rejects missing, failed, or stale-commit evidence. A successful
report proves the transactional per-user macOS service lifecycle, native ARM64
Ubuntu and AlmaLinux package lifecycle, and root-scoped Garage S3 compatibility
for one exact commit. It does not imply physical DAS readiness.

Hardware acceptance after returning home
----------------------------------------

Wait for the operator to confirm a quiescent DASServer window before using the
documented SSH identity or touching the appliance. Then run x86_64 package
parity plus generated-data physical-drive identity, SMART/NVMe, replacement,
full-disk/corruption, multi-HDD/Garage durability, control-plane saturation,
and performance/soak matrices. Those results must be recorded separately; a
surrogate report must never be relabelled as hardware acceptance.
