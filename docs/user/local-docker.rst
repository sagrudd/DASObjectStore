Local Docker profile
====================

The repository includes a canonical macOS development profile at
``deploy/local-docker``. It runs ``dasobjectstored`` in a container, keeps the
Linux package configuration paths under ``/etc/dasobjectstore`` inside that
container, and starts Garage through the daemon's Compose control plane.

Persistent state is placed on an attached volume such as
``/Volumes/Seagate/DASObjectStore``. Docker Desktop must be granted file-sharing
access to that volume before starting the profile.

When an attached volume is unavailable, macOS or Linux contract tests may use
only the dedicated generated-data root
``$HOME/.dasobjectstore-codex-validation`` or a child beneath it. The helper
rejects arbitrary home folders and path traversal, creates authority markers,
and enforces a 1 TiB ceiling across the complete validation root. This mode is
a bounded folder substitute; it is not evidence that the host has a dedicated
SSD.

The profile is intentionally single-node and single-volume. It is suitable for
S3-compatible adapter and contract validation, not for appliance throughput,
SMART, repair, or redundancy claims.

Prerequisites
-------------

* Docker Desktop with the Compose plugin;
* ``/Volumes/Seagate`` added under Docker Desktop **Settings > Resources >
  File Sharing** (the bind-mount preflight fails early with an actionable
  message if this is missing);
* the DASObjectStore checkout and sibling ``prosopikon`` checkout in one build
  context;
* a host ``dasobjectstore`` CLI, built with ``cargo build --locked
  -p dasobjectstore-cli``.

Start and stop
--------------

From the DASObjectStore checkout:

.. code-block:: console

   $ ./deploy/local-docker/local.sh up
   $ ./deploy/local-docker/local.sh status
   $ ./deploy/local-docker/local.sh config
   $ ./deploy/local-docker/local.sh completion-smoke
   $ ./deploy/local-docker/local.sh down

``up`` renders the daemon and nested Garage Compose files, builds the daemon
image, starts the daemon, provisions the ``alleleanchor_mvp`` store (bucket
``alleleanchor-mvp``) and scoped key, and writes a mode-0600 AlleleAnchor
config plus credential file under the volume profile. Secret values are never
printed. Provisioning also idempotently registers the local folder manifest and
canonical container-visible backend with the daemon so capacity admission and
remote completion use the same authority. The default logical capacity is 1
TiB and can be lowered with ``DASOBJECTSTORE_LOCAL_CAPACITY_LIMIT_BYTES``.
The daemon joins its daemon-owned Garage network after provisioning and pins
the Garage signing region, so container-side remote completion verifies through
the ``garage`` service without Docker Desktop port hairpinning.
The generated store definition and folder binding receive the same finite
capacity policy, making restart reconciliation deterministic.
The authoritative live object catalogue is persisted at
``/var/lib/dasobjectstore/live.sqlite`` in the mounted daemon state rather than
using the appliance-only ``/srv`` default.

On Linux, the root-running daemon normalizes ownership of its generated bind
mounts back to the invoking host user after provisioning. This allows the
host-side scoped-config exporter and later safety checks to read only the
validation profile without requiring ``sudo``. ``down`` disconnects the daemon
from the nested Garage network before Garage removes that network, then stops
the daemon stack; it does not prune unrelated Docker resources.

``completion-smoke`` creates one 4096-byte generated object beneath the
managed profile root and exercises the version-matched remote client over
the daemon Unix socket. Success requires terminal daemon completion, Garage
HEAD verification, an exact shared-SQLite catalogue row, one logical quota
charge, and no remaining reservation. Its temporary paired-session file stays
under the private configuration root, is mode 0600, is never printed, and is
removed with the generated source after the run. The completed provider object
is retained because deleting it independently would make the authoritative
catalogue row stale; use a dedicated generated-data profile for this test.

Configuration paths
-------------------

``DASOBJECTSTORE_LOCAL_API_PORT`` selects the S3 API port and reserves the
next three consecutive ports for Garage RPC, Web, and administration. The
profile validates the complete range and configures Garage and Compose with
the same values, allowing an isolated validation profile to coexist with an
already-running local authority.

The generated daemon container uses these stable paths:

* ``/etc/dasobjectstore/daemon.json``;
* ``/etc/dasobjectstore/garage.compose.yml``;
* ``/etc/dasobjectstore/garage.toml``;
* ``/var/lib/dasobjectstore`` for state and managed credentials;
* ``/var/lib/dasobjectstore/stores.json`` for the writable daemon-owned store
  registry;
* ``/run/dasobjectstore/dasobjectstored.sock`` for the local client boundary.

The host volume is mounted at the same path inside the daemon container so
nested Compose volume sources remain valid to Docker Desktop. The profile's
initial generated store definition is only a bootstrap seed; subsequent daemon
mutations use the writable state registry, not the read-only ``/etc`` mount.
This is why the profile does not attempt to write to the macOS host ``/etc``
directory.

Validation boundary
-------------------

Pinakotheke may use the exact managed root
``$HOME/.x-img/dasobjectstore``. The helper creates an authority marker only in
an empty directory and keeps credentials under
``$HOME/.config/dasobjectstore``. Configure ``pinakotheke-local`` as the
profile, ``pinakotheke_local`` as the store ID, ``pinakotheke-local`` as the
bucket, ``media`` as the prefix, and ``pinakotheke`` as the consumer through
the corresponding ``DASOBJECTSTORE_LOCAL_*`` environment variables. Arbitrary
home-directory roots remain rejected. After provisioning, ``local.sh describe``
returns the stable secret-free endpoint/ObjectStore identity and opaque
credential reference for the consumer.

The local profile can close a local S3-compatible adapter validation gate for
AlleleAnchor and other clients. It does not replace Linux appliance soak,
multi-disk redundancy, SMART, repair, or throughput acceptance. Treat USB
disconnects, sleep, Docker Desktop restarts, and APFS/VM behavior as explicit
development failure modes. The daemon container receives the Docker socket so
it can own the nested Garage lifecycle; that authority is acceptable for this
local development profile only. AlleleAnchor's local ``FileStore`` and
containerised workflow stages remain consumer-side substitutes: they consume
exported scoped S3 configuration and immutable object/manifests, never private
DAS host paths or storage lifecycle state.

The local daemon image also contains the version-matched
``dasobjectstore-remote`` client and a digest-pinned AWS CLI. This is the
supported foundation for consumers that must submit authoritative completion
from inside Docker Desktop's Linux VM: macOS cannot connect through a
container-created Unix socket merely because its bind-mounted pathname is
visible on the host. A consumer wrapper must still constrain source-path
translation to the managed profile root and provide scoped credentials; the
image does not expose the daemon socket or credentials to the browser.
