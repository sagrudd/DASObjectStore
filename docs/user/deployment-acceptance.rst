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
bounded capability issuance, daemon-owned logical-capacity reservation,
forged-capability rejection, verify-before-commit ordering, quota settlement
only after catalogue publication, exact-replay idempotency, and retry after
catalogue failure. A capability without its persisted capacity reservation
fails closed rather than publishing an uncharged object version.
The capacity handoff is restart-safe: the capability registry records a
settlement intent before quota mutation and records completion afterward. An
exact retry inspects that reservation, completing it when still present or
recognizing the bounded post-commit crash window when it is already absent.
The evidence labels provider execution as ``surrogate_only``: it does not
replace the later live Garage ``head-object`` and shared-SQLite appliance run.

Physical appliance acceptance
-----------------------------

Before a non-production appliance exercise, run the credential-free,
read-only package preflight as a local administrator:

.. code-block:: console

   sudo deploy/acceptance/appliance-readiness-preflight.sh

The preflight creates no ObjectStore, object, credential, token, TLS asset,
browser session, or service state. It requires the installed Debian or RPM
package, packaged binaries, current service identity and groups, active daemon
and Web services, valid daemon/Web configuration, the local-user PAM authority,
TLS certificate/key presence at the documented package paths with a
non-world-readable private key, the daemon socket, the managed SSD/HDD layout,
and a daemon-owned ObjectStore registry containing only non-blank identifiers.
It exits non-zero when any authority or storage prerequisite is absent,
malformed, inactive, or unsafe.

This is an appliance readiness gate, not proof of a successful authenticated
user journey, object ingest, provider operation, or physical acceptance. It is
intentionally limited to the standalone ``local_user`` package profile.

Use the EPIC C harness for maintenance, device-mapping, staging-accounting, and
control-plane evidence. Its default inspection is read-only:

.. code-block:: console

   deploy/acceptance/epic-c-appliance.sh inspect

The load mode refuses a dirty checkout, a non-Linux host, another active file
ingest, missing authenticated dashboard state, an undeclared quiescent window,
or synthetic data that would make ``CODEX`` reach 1 TiB. It generates random
data only below ``$HOME/.dasobjectstore-codex-validation`` and requires the
exact confirmation:

.. code-block:: console

   DASOBJECTSTORE_ACCEPTANCE_QUIESCENT=yes \
   DASOBJECTSTORE_ACCEPTANCE_CONFIRM='RUN EPIC C CODEX LOAD' \
   DASOBJECTSTORE_ACCEPTANCE_COOKIE_FILE="$HOME/.config/dasobjectstore/acceptance.cookies" \
   deploy/acceptance/epic-c-appliance.sh load

The resulting mode-``0600`` evidence is commit/package/service-bound and
records HTTPS p50/p95/p99 latency, accept queues, CPU/IO/memory PSI,
per-device queue state, mounted-device telemetry mapping, priority
cancellation latency, and post-load staging recovery. It never restarts a
service or removes managed ObjectStore payload. Validate a retained report
with:

.. code-block:: console

   deploy/acceptance/epic-c-appliance.sh validate /path/to/evidence.json

Surrogate or older-commit evidence must never be relabelled as physical
acceptance.
