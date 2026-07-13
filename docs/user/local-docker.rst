Local Docker profile
====================

The repository includes a canonical macOS development profile at
``deploy/local-docker``. It runs ``dasobjectstored`` in a container, keeps the
Linux package configuration paths under ``/etc/dasobjectstore`` inside that
container, and starts Garage through the daemon's Compose control plane.

Persistent state is placed on an attached volume such as
``/Volumes/Seagate/DASObjectStore``. Docker Desktop must be granted file-sharing
access to that volume before starting the profile.

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
   $ ./deploy/local-docker/local.sh down

``up`` renders the daemon and nested Garage Compose files, builds the daemon
image, starts the daemon, provisions the ``alleleanchor_mvp`` store (bucket
``alleleanchor-mvp``) and scoped key, and writes a mode-0600 AlleleAnchor
config plus credential file under the volume profile. Secret values are never
printed.

Configuration paths
-------------------

The generated daemon container uses these stable paths:

* ``/etc/dasobjectstore/daemon.json``;
* ``/etc/dasobjectstore/garage.compose.yml``;
* ``/etc/dasobjectstore/garage.toml``;
* ``/var/lib/dasobjectstore`` for state and managed credentials;
* ``/run/dasobjectstore/dasobjectstored.sock`` for the local client boundary.

The host volume is mounted at the same path inside the daemon container so
nested Compose volume sources remain valid to Docker Desktop. This is why the
profile does not attempt to write to the macOS host ``/etc`` directory.

Validation boundary
-------------------

The local profile can close a local S3-compatible adapter validation gate for
AlleleAnchor and other clients. It does not replace Linux appliance soak,
multi-disk redundancy, SMART, repair, or throughput acceptance. Treat USB
disconnects, sleep, Docker Desktop restarts, and APFS/VM behavior as explicit
development failure modes.
