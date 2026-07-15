Storage Profile and Host-Mode Matrix
=====================================

This matrix describes implementation readiness, not whether a particular
machine currently has a mounted or healthy store. ``Preview`` means the
contract and local tests are in place but provisioning or production acceptance
is incomplete. ``Blocked`` requires the unavailable DASServer/Garage appliance
or deployment credentials.

.. list-table::
   :header-rows: 1

   * - Profile
     - ``per_user``
     - ``system``
     - ``integrated``
   * - ``folder``
     - Preview: bounded macOS backend, XDG path contract, validated launchd plan, local tests
     - Preview: bounded backend and idempotent package root provisioning; explicit adoption/reconciliation remains pending
     - Preview: backend contract exists; Mnemosyne/Synoptikon provisioning adapter pending
   * - ``drive``
     - Preview: injected SSD identity validator and guarded backend
     - Preview: dedicated-drive contract; host probing and service hooks pending
     - Preview: product adapter and runtime inventory pending
   * - ``appliance``
     - Unsupported: appliance authority is not per-user
     - Preview: Linux package/systemd/reboot may use a disposable VM surrogate; physical device acceptance requires DAS access
     - Blocked: Garage/provider soak, credentials, physical telemetry, replacement, and performance require DAS access

A disposable Linux VM may provide interim package, systemd/cgroup, reboot, and
synthetic loop-device evidence while the DASServer is unavailable. It is not a
substitute for physical enclosure identity, SMART/NVMe, real multi-HDD/Garage
durability, replacement, or performance acceptance. Those gates require a
quiescent DASServer validation window.

Upgrade and migration policy
----------------------------

Existing appliance metadata remains readable and is never silently rewritten as
a folder or drive manifest. Manifest schema v1 is strict; unknown fields and
future versions fail closed. Creating or adopting a folder/drive store requires
an explicit profile decision, stable backend identity, finite capacity, and
operator-authorized catalogue registration.

Promotion uses the resumable migration checkpoint state machine. Source
placements remain retained through destination verification and are retired only
after explicit confirmation. A failed or interrupted checkpoint preserves source
retention. Actual copy workers, archive packaging, package installation, and
appliance acceptance remain open campaign gates. The daemon now owns a
versioned Unix-socket portable catalogue export/import handoff for bounded
folder profiles: export carries validated IDs, versions, hashes, provenance,
protection, and logical placements without paths or credentials; import
verifies every destination payload before committing catalogue rows and always
reports source retention. The provider-neutral S3 adapter boundary is
approved, with Garage retained as the local compatibility provider behind the
daemon authority.
Import requests carry an explicit replay-safe transaction id and private
profile namespace. Successful daemon imports record that verified handoff in
the schema-versioned shared-SQLite adapter; backend paths and credentials do
not cross the socket or Web boundary. Cross-file rollback and physical
appliance placement reconciliation remain deployment-gated. The daemon also
keeps an atomic private handoff journal with prepared, profile-committed, and
fully-committed states so interrupted imports can be reconciled after restart.
The daemon replay operation re-verifies prepared or profile-committed entries;
already committed entries are safe no-ops.
Reconciliation listing and transfer execution are isolated behind a
provider-neutral range/resume/cancellation adapter; Garage is the current
compatibility implementation. Typed list/download envelopes carry prefix,
destination, range/resume, and cancellation state, so other providers can
implement the same recovery contract without inheriting Garage command shape;
manifest-plan execution, checkpoints, partial-range validation, progress, and
cancellation are shared across adapters.
The shared-SQLite metadata seam is schema-versioned (v0.4) and records profile
namespace, transaction id, source retention, and object versions atomically
with idempotent retries and conflict rejection. It remains isolated from
legacy appliance placement rows until a daemon-owned physical handoff is
available.
Authenticated standalone Web clients may use the matching GET export and POST
import routes; these routes carry only the versioned catalogue document and
never expose backend paths or credentials.

The matrix must be revised when daemon-owned provisioning, product adapters, or
real-world acceptance changes a row; it must not be used to infer hardware
health or availability.

The local fixture matrix is executable on macOS without Docker or DAS access.
It covers bounded-folder creation, source-preserving adoption, checkpoint
reload/recovery, quota rejection, and symlink drift using only a uniquely named
child beneath ``$HOME/.dasobjectstore-codex-validation``. Package-created and
container-mounted fixtures remain separate deployment gates.
