Managed Service Boundary
========================

DASObjectStore is a client/server storage appliance. Normal users should submit
jobs through ``dasobjectstore`` or the Web/API surface; they should not write
directly into managed DAS mountpoints.

Linux packages define a managed daemon named ``dasobjectstored``. The package
assets create a dedicated service identity:

.. code-block:: text

   user:  dasobjectstore
   group: dasobjectstore

The daemon owns runtime state, persistent state, logs, and managed storage
mutation. Local users are authorized by store writer/admin policy, such as
membership in a store's writer group, rather than by direct filesystem write
permission to HDD members.

Packaged Paths
--------------

The Linux package assets reserve these paths:

.. code-block:: text

   /etc/dasobjectstore/daemon.json
   /run/dasobjectstore/dasobjectstored.sock
   /var/lib/dasobjectstore
   /var/log/dasobjectstore
   /srv/dasobjectstore

The Unix-domain socket is the local client transport. The daemon will use peer
credentials on Linux to identify the submitting local actor before accepting
storage-mutating jobs.

The packaged daemon also accepts an ``ingest_resource_policy`` object in its
JSON configuration. Its worker counts and memory budget become the daemon's
transactional CPU, memory, socket-worker, and I/O-worker admission budget for
local file ingest. Older configuration files may omit this object and receive
the safe built-in policy; operators should use ``--check-config`` before
deploying a changed policy.
The same budget is used when Garage reconciliation hands staged provider data
back to the local ingest pipeline; it does not bypass daemon admission.

Packaged systemd deployments place the Web control plane in
``dasobjectstore-control.slice`` and storage work in
``dasobjectstore-storage.slice``. Both domains enable CPU, memory, and I/O
accounting. The control slice receives higher CPU/I/O weight and a 256 MiB
``MemoryLow`` reservation; the storage slice has a 75% ``MemoryHigh`` boundary.
These are host-level protection defaults around the daemon's transactional
admission policy, not substitutes for per-device telemetry or capacity rules.
Inspect the effective values with ``systemctl show`` on a cgroup-v2 Linux host.

The packaged daemon also owns appliance telemetry collection. By default,
``/etc/dasobjectstore/daemon.json`` enables telemetry with a 30 second cadence
and writes the current JSON state under:

.. code-block:: text

   /var/lib/dasobjectstore/telemetry/appliance-telemetry.v1.json

The telemetry directory is daemon-owned state; operators and Web/API readers
should treat the JSON file as read-only and use supported interfaces as they are
added.
For a decision tree covering warm-up, missing-device reasons, marker/device
mapping, and safe evidence collection, see :doc:`telemetry-troubleshooting`.
Authenticated daemon API callers can request appliance telemetry through the
``appliance_telemetry`` command. The response contains current CPU, memory,
capacity, session, and per-disk IO summaries, bounded time-series windows for
Home-dashboard charts, available-window metadata, and missing-data intervals.
Chart series are downsampled by requested window: raw cadence for 1 hour,
one-minute buckets for 1 day, ten-minute buckets for 10 days, and hourly
buckets for 3 months. Percentages are exposed as basis-point integers so API
consumers do not need to handle floating-point drift.
The daemon bounds that JSON history by retaining raw cadence samples for the
last hour, one-minute buckets through one day, ten-minute buckets through ten
days, and hourly buckets through 92 days.
When managed HDD roots contain ``.dasobjectstore/device.env`` markers with
``role=hdd:<disk-id>``, the daemon records their capacity in the same telemetry
sample. Marker fields such as ``label``, ``device``, ``filesystem``, and
``enclosure_id`` are preserved when present so operator surfaces can group disk
capacity by enclosure as the hardware registry matures. Marker-provided
``bay_label`` values are also preserved in capacity and disk IO telemetry so
current deployments can correlate known bays before the authoritative physical
bay registry exists.
The telemetry schema also records ``disk_io`` entries for per-disk throughput
and operation-rate data. On Linux, the daemon retains the previous
``/proc/diskstats`` sample internally and calculates rates over the configured
telemetry cadence. The first sample after daemon startup or counter reset
reports missing IO rates explicitly instead of guessing from capacity or ingest
state.
Session telemetry is derived from the standalone Web auth registry and the
remote easyconnect paired-session registry when those files exist. The daemon
counts unrevoked, unexpired Web and remote-agent sessions, distinct logged-in
users, and administrator/operator sessions when the host group file is readable
for local authority classification.

Packaged installations restrict the socket directory and socket file to the
``dasobjectstore`` group. A local user must be in this transport group before
the CLI can connect to ``dasobjectstored``:

.. code-block:: console

   sudo usermod -aG dasobjectstore "$USER"

Start a new login session after changing group membership, then verify it with
``id -nG``. Store writer groups such as ``mnemosyne`` are still checked
separately by store policy after the client has connected to the daemon.

Permission Model
----------------

Managed DAS roots should be owned by the daemon service identity. Ingest users
should be members of the daemon transport group and the relevant store writer
group, for example ``mnemosyne``. The writer group authorizes daemon job
submission for that store. It should not be used to grant broad write access to
individual HDD filesystems.

Store creation boundary
-----------------------

When the packaged daemon is available, the normal
``dasobjectstore store create`` command submits a typed
``create_object_store`` request. Every writable store requires an explicit
writer group. The
daemon validates the store policy, selects its system-managed registry, and
records the accepted creation job. The CLI may still mirror the resulting
definition to a validated portable SSD registry and apply platform ACLs; it
does not write the host registry in this path.

An explicit hidden ``--registry-path`` is reserved for local tests and
migration tooling. Unassigned definitions are permitted only as explicitly
read-only import or migration state and cannot accept ingress until an
administrator assigns a writer group. Normal creation without a writer group
must fail closed rather than mutate the host registry from the CLI.

Daemon-owned store drain
-------------------------

``dasobjectstore store drain`` is a daemon operation. The client sends the
store identifier, dry-run flag, policy allowance, and confirmation marker over
the Unix socket; ``dasobjectstored`` discovers managed HDD roots, performs the
metadata and payload removal, and returns the complete report. The client no
longer accepts local SQLite or HDD-root overrides for this command, so a normal
CLI process cannot redirect a destructive operation around daemon policy.

The same boundary applies to ``dasobjectstore ingest drain-queue``. The CLI
sends the store, reason, dry-run, allowance, and confirmation fields to the
daemon; the daemon selects its live metadata path, authorizes the administrator,
updates queue state, and returns the cancellation report.

Normal ``dasobjectstore disk retire`` requests follow the same boundary: the
daemon authenticates the administrator, selects the live metadata database,
records its current timestamp, and returns the state-transition report.
The force-retirement variant applies its policy allowance and exact confirmation
inside the daemon before the risk-gated state transition.

``dasobjectstore disk lockdown-das`` follows the same boundary. The daemon
discovers the managed SSD/HDD roots, plans optional service-account creation,
checks administrator authorization and the exact confirmation marker, executes
the ownership and mode changes, and records the completed job. The CLI remains
responsible only for argument parsing and report rendering.

The Debian package configuration checks the managed root at
``/srv/dasobjectstore``. If that path already exists and is owned by an ordinary
user or group, package configuration stops and asks the operator to repair the
ownership through the formal disk lockdown workflow before continuing.

Package upgrade stops the Web and daemon processes before replacing binaries;
post-install starts them with the retained configuration. Final DEB/RPM removal
stops and disables the installed units but deliberately retains configuration,
catalogue, credentials, telemetry, and managed storage roots. Package removal
is never authorization to delete stored data. Use formal DASObjectStore
management operations for retirement or deletion.

Provider transfer and reconciliation operations require a working AWS CLI v2
``aws`` command. DEB/RPM metadata recommends the distribution package when one
exists, but does not make it a hard dependency because supported ARM64
distributions do not all publish that package. Install official AWS CLI v2 on
such hosts before enabling S3 provider workflows; the daemon fails provider
commands explicitly when it is absent.

The hidden ``--local-direct`` ingest mode is a developer/test fallback while the
daemon implementation is being completed. It is not the normal production
storage path.

Provider-stream uploads follow the same boundary. The client sends a bounded,
framed stream to ``dasobjectstored``; the daemon authorizes the store writer,
admits logical and backend capacity, verifies the declared size and SHA-256
while streaming, then performs staged ``fsync``/rename and commits the
catalogue before returning the path-free acknowledgement. Clients and Web
workers must not write request bodies directly into managed profile roots. The
authenticated standalone HTTP PUT route now feeds this stream through a
bounded backpressure channel and closes it on body cancellation. HTTP GET/range
now uses the same path-free daemon stream, waits for stream-open acceptance
before returning a response, and relays verified frames through a bounded
channel. Multipart listener adapters remain a separate deployment seam and
must preserve these daemon-owned staging, cancellation, backpressure, and
catalogue rules.

Source Path Reads
-----------------

Daemon-side ingest accepts user-provided source paths. In packaged Linux
deployments the systemd unit sets ``ProtectHome=read-only`` so
``dasobjectstored`` can read source trees under home directories while still
preventing writes through the service sandbox. This is an interim packaging
policy for local daemon ingest; storage mutation remains daemon-owned and
limited to the managed runtime, state, log, and ``/srv/dasobjectstore`` paths.

Debian and RPM packages also enable
``dasobjectstore-source-access.path``. Its root-owned helper watches the
standard udisks mount roots (``/run/media`` and ``/media``) and grants the
daemon only execute/traverse access to newly created per-user mount roots. It
does not grant source write access and does not recursively change files on an
external volume. Filesystems without POSIX ACL support must still be mounted
with service-readable ``uid``, ``gid``, and ``mode`` options; the ingest CLI
reports that condition explicitly.

When ``udisks2`` is installed, the package also regenerates
``/etc/udev/rules.d/99-dasobjectstore-external-mounts.rules`` with the numeric
``dasobjectstore`` group ID. FAT, exFAT, and NTFS mounts then receive a
read-only group view (``dmask=0037,fmask=0137``) at mount time. Existing mounts
must be unmounted and mounted again before the policy takes effect; fstab
mounts remain administrator-owned and are not overridden.

The service sandbox does not override normal Unix permissions. The source tree
must be readable and searchable by the ``dasobjectstore`` service user, or by a
group/ACL that grants that service identity access. Prefer granting read-only
access to the specific ingest directory instead of broad write permissions to a
home directory or managed DAS root.
