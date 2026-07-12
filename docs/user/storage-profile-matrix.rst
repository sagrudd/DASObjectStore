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
     - Preview: bounded macOS backend, XDG path contract, local tests
     - Preview: bounded backend and packaged path contract; service/package hooks pending
     - Preview: backend contract exists; Mnemosyne/Synoptikon provisioning adapter pending
   * - ``drive``
     - Preview: injected SSD identity validator and guarded backend
     - Preview: dedicated-drive contract; host probing and service hooks pending
     - Preview: product adapter and runtime inventory pending
   * - ``appliance``
     - Unsupported: appliance authority is not per-user
     - Blocked: Linux package/device/reboot and appliance acceptance require DAS access
     - Blocked: Garage/provider soak, credentials, and production telemetry require DAS access

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
retention. Actual copy workers, portable export/import, package installation,
S3 gateway choice, and appliance acceptance remain open campaign gates.

The matrix must be revised when daemon-owned provisioning, product adapters, or
real-world acceptance changes a row; it must not be used to infer hardware
health or availability.
