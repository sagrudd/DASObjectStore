Ingesting Files from a Mounted Disk
===================================

Use ``dasobjectstore ingest files`` to load a directory tree from an external
disk into a system-managed object store or SubObject endpoint. Do not copy files
directly onto DAS member disks.

The CLI is the client surface. In normal operation it submits an ingest job to
the managed ``dasobjectstored`` service, streams or references source data as
required by the local transport, and renders daemon progress events. The daemon
reads the store policy, discovers managed HDD members, selects copy placements,
chooses the landing path allowed for the ingest origin, writes the requested
verified HDD copies, and reports byte-level progress while the copy is running.
Remote and Web upload origins are always SSD-first. Local-server ingest may
write directly to HDD only when the target store policy explicitly uses
``DirectToHdd``; otherwise it stages each file through the DAS SSD first.

This command is not a local filesystem copy into a DAS member disk. The normal
path sends a daemon request over the packaged local daemon socket. The daemon is
the only component that should mutate managed SSD/HDD roots.

Example
-------

For a store created as:

.. code-block:: console

   sudo dasobjectstore store create zymo_fecal_2025.05 \
     --class reproducible_cache \
     --writer-group mnemosyne

import files from a mounted external disk:

.. code-block:: console

   dasobjectstore ingest files zymo_fecal_2025.05 \
     --source /mnt/external/zymo_fecal_2025.05 \
     --object-type fastq

Add ``--tui`` to render the embedded terminal upload view while the daemon
upload runs. This is not a separate command or product surface; it is a
graphical nicety for this long-running CLI action.

.. code-block:: console

   dasobjectstore ingest files zymo_fecal_2025.05 \
     --source /mnt/external/zymo_fecal_2025.05 \
     --object-type fastq \
     --tui

The user running ingest must be authorized by the store's writer group. For the
example above, membership in ``mnemosyne`` is required. Ingest does not require
``sudo`` because the daemon, not the user's shell process, owns managed storage
mutation.

Inspect and request the local landing policy explicitly before using the
server-local direct-import workflow:

.. code-block:: console

   dasobjectstore store ingest-policy zymo_fecal_2025.05 --json

Enabling direct HDD landing is an administrator action and requires the exact
confirmation marker. It changes only the store's ingest mode; it does not grant
permission to write managed disks or bypass source verification:

.. code-block:: console

   sudo dasobjectstore store ingest-policy zymo_fecal_2025.05 \
     --ingest-mode direct-to-hdd \
     --confirm "confirm direct hdd ingest"

For data already on a server-local NVMe/SATA path, request the explicit route
and keep the daemon progress view visible:

.. code-block:: console

   dasobjectstore ingest direct-import zymo_fecal_2025.05 \
     --source /home/stephen/zymo_fecal_2025.05 \
     --copies 1 \
     --hdd-workers 4 \
     --tui \
     --lazy

The first progress frame is a daemon preflight explanation. It reports the
source topology, mount point, filesystem, backing-device source, major:minor
identifier, classified origin, store policy, selected landing mode, and routing
reason. ``direct_to_hdd_when_policy_allows`` means the verified local source is
being copied directly to distinct HDD targets; ``ssd_first`` means the daemon
will stage through SSD. A USB/removable, NFS/SMB/FUSE, virtual, or unknown source
always remains SSD-first even when the store policy permits direct HDD landing.
If mount or device details are unavailable, the daemon reports them as
``unknown`` and keeps the fail-closed SSD-first route.

The packaged daemon reads the path supplied with ``--source``. On Linux, the CLI
prepares source ACLs before submitting the job so the ``dasobjectstore`` service
can traverse private home directories and read the selected import tree without
requiring ``sudo`` or broad home-directory mode changes. The daemon does not
need write access to the source path. The read-only ACL is applied recursively
to the explicitly selected tree even when its root directory is already
traversable, because individual payload files may still have private modes.

If a removable-media parent is root-owned, the CLI retries the read-only ACL
grant with ``sudo -n`` when the invoking user is already authorized for
passwordless sudo. If that retry fails with ``Operation not permitted``, the
mount likely does not support POSIX ACLs; remount it with service-readable
``uid``, ``gid``, and ``mode`` options (or pre-grant ``dasobjectstore``
read/traverse access) before retrying. The CLI never grants daemon write access
to the source. Packaged Debian/RPM installs also run a root-owned watcher for
``/run/media`` and ``/media`` that prepares per-user mount-root traversal
automatically, so the manual ACL commands are only a recovery path for an
already-running installation or a non-standard mount root.
When ``udisks2`` is available, the package also installs a udev mount policy so
FAT, exFAT, and NTFS volumes are mounted with a read-only
``dasobjectstore`` group view; unmount and remount an already attached volume
after package installation.

DASObjectStore discovers prepared HDD members under the managed mount root and
chooses placements for each object. Operators must not choose individual disks
for normal file ingest.

During HDD settlement the daemon permits at most one active writer per managed
HDD member. The effective HDD settlement concurrency is therefore capped by the
number of discovered HDD roots, even when an authoritative performance policy
requests more workers. When more than one HDD writer is active, new settlement
work reserves idle disks only and selects among them by projected fractional
free space, then absolute free space. This keeps usage distributed across the
array without creating a fixed preference for a particular disk.

What Happens
------------

When ``dasobjectstore ingest files`` is run, the CLI performs client-side
argument parsing and submits an ingest job request containing:

* the target object store or SubObject endpoint;
* the mounted source directory;
* the logical object type assigned to the imported files;
* an optional copy-count override;
* an optional HDD settlement worker override;
* a local-server origin hint, which the daemon verifies against the source
  mount and device topology before using it for landing-mode selection;
* the existing-object conflict policy;
* whether the request is a dry run.

The daemon is responsible for authorization, policy lookup, landing-mode
selection, SSD staging when required, placement selection, HDD fan-out,
verification, metadata mutation, and progress events. During normal CLI
operation the operator sees daemon-emitted byte-level progress lines as the job
runs, followed by the final daemon job submission summary.

Ingress-origin rules are deliberately simple:

* Normal ``dasobjectstore ingest files`` submits a local-server hint. The
  daemon verifies the source mount and backing device; a verified local NVMe or
  SATA source may use direct HDD ingest only when the target store policy
  explicitly permits it. USB/removable, NFS/SMB/FUSE, virtual, and unknown
  sources always stage through SSD first.
* ``local_server_direct_import`` identifies the explicit direct-import
  workflow for data already on the DAS server. It uses the same daemon
  verification and policy gate, so it also falls back to SSD staging when the
  source cannot be verified as server-local.
* ``remote_s3`` is used for paired ``dasobjectstore-remote`` uploads and raw
  S3-compatible remote upload plans. These uploads always stage through the
  selected ObjectStore SSD before daemon-owned HDD settlement.
* ``web_upload`` is used for browser-mediated upload workflows. Web-origin
  bytes also always stage through the selected ObjectStore SSD before HDD
  settlement.

Clients do not override the ingress origin to force a disk placement. The
daemon derives the landing mode from the authenticated submission path and the
target store policy.

File ingest uses a bounded split SSD pipeline by default. The source reader
writes staged payload bytes to SSD and then moves on to the next file when
queue pressure allows. A bounded side worker syncs the staged SSD payload and
calculates the SHA-256 checksum; only after that succeeds is the file eligible
for HDD settlement. In the TUI, ``ssd-stage`` means source bytes are landing on
SSD, ``ssd-flush`` means the staged payload is being synced, and
``checksum-manifest-capture`` means the staged payload is being hashed before
HDD placement.

When a verified DAS-server source is used and the store policy permits
``DirectToHdd``—including through ``dasobjectstore ingest direct-import``—the
daemon uses ``direct_to_hdd_when_policy_allows`` landing mode. In that mode it
reads the local source only while copying, calculates its checksum in flight,
and writes directly to daemon-selected HDD targets without creating an SSD
payload. The job stages through SSD when the policy does not permit direct
ingest or source verification fails. This policy is for reproducible or
externally recoverable datasets; protected generated or critical stores remain
SSD-first.

By default, the daemon derives HDD settlement fan-out from complete distinct
HDD target sets: ``min(managed_hdd_count / copies, 4)``, with at least one
worker when a valid target exists. A one-HDD test or degraded enclosure uses
one worker; a four-HDD, single-copy ingest can use four concurrent writers; and
redundant-copy jobs are bounded by the number of complete distinct disk sets.
This keeps source-to-SSD staging moving without concurrent writes to the same
disk. The daemon also rejects managed HDD inventories that present the same
physical disk more than once, because redundant copies must land on distinct
disks. Operators may override this for a run:

.. code-block:: console

   dasobjectstore ingest files zymo_fecal_2025.05 \
     --source /home/stephen/zymo_fecal_2025.05/ \
     --hdd-workers 5 \
     --tui

The daemon validates ``--hdd-workers`` against the detected managed HDD count
for the ObjectStore enclosure.

Object Types
------------

Object type is workflow-facing metadata. It lets DASObjectStore make a folder of
POD5 files, a FASTQ delivery, a BAM/CRAM alignment set, or an ENA/SRA public
dataset discoverable without asking downstream orchestration to infer intent
from paths alone.

If no type is supplied, file ingress uses ``naive``. Use ``--object-type`` when
the dataset is known:

.. code-block:: console

   dasobjectstore ingest files nanopore_run_42 \
     --source /mnt/sequencer/run_42/pod5 \
     --object-type pod5

.. code-block:: console

   dasobjectstore ingest files rnaseq_fastq \
     --source /mnt/delivery/fastq \
     --object-type fastq

Discrete-file ingress uses the same vocabulary:

.. code-block:: console

   dasobjectstore object put cohort/sample.bam \
     --source ./sample.bam \
     --object-type bam \
     --ssd-root /srv/dasobjectstore/ssd \
     --disk-root disk-a=/srv/dasobjectstore/hdd/disk-a

Supported object types are ``naive``, ``bam``, ``cram``, ``sam``, ``pod5``,
``fastq``, ``fasta``, ``reference_genome``, ``ena_sra``, ``vcf``, ``bcf``,
``bed``, ``gff_gtf``, ``count_matrix``, ``gene_expression_matrix``,
``genome_assembly``, ``transcriptome_assembly``, ``alignment_index``,
``nanopore_run``, ``illumina_run``, ``single_cell_fastq``, and ``ann_data``.

The daemon socket path in packaged Linux deployments is:

.. code-block:: text

   /run/dasobjectstore/dasobjectstored.sock

If the CLI reports ``Permission denied`` while connecting to this socket, the
current login session is not in the daemon transport group. Ask an administrator
to add the user to ``dasobjectstore`` and then start a new login session:

.. code-block:: console

   sudo usermod -aG dasobjectstore "$USER"

Check the active session with ``id -nG`` before retrying ingest.

Existing Objects
----------------

Object IDs are derived from the endpoint prefix and the source-relative file
path. If a later import contains a file that maps to an object ID already known
to the store, DASObjectStore uses an explicit conflict policy.

Normal ingest does not perform a pre-copy duplicate check: it calculates the
checksum while bytes are copied and treats the payload as a new stored version.
Use ``--strict`` only when an operator explicitly needs preflight
deduplication. It reads the incoming source before copying and reuses an
existing object only when the incoming checksum matches the stored checksum.
If a checksum is unavailable or differs, the daemon ingests the incoming
payload as a new stored version rather than silently overwriting existing
content.

``--lazy`` is a faster operator-selected policy for trusted repeat imports. It
reuses the existing object when the object ID and size match. If the size
differs, the incoming payload is ingested as a new stored version.

``--force`` always ingests the incoming payload. Existing content is preserved;
the new payload is treated as a new stored version even when the size or
checksum matches.

Examples:

.. code-block:: console

   dasobjectstore ingest files zymo_fecal_2025.05 \
     --source /mnt/external/zymo_fecal_2025.05 \
     --strict

.. code-block:: console

   dasobjectstore ingest files zymo_fecal_2025.05 \
     --source /mnt/external/zymo_fecal_2025.05 \
     --lazy

.. code-block:: console

   dasobjectstore ingest files zymo_fecal_2025.05 \
     --source /mnt/external/zymo_fecal_2025.05 \
     --force

The three policy flags are mutually exclusive. Normal operator use leaves them
unset so checksum calculation stays in the copy path. Use ``--strict`` for a
deliberate, preflight duplicate check; use ``--lazy`` only for trusted repeat
imports where a size match is sufficient. The dry-run receipt reports the
selected policy; do not proceed when an explicitly requested policy is not
shown verbatim.

Group Requirements
------------------

Store creation and writer-group assignment are administrator actions:

.. code-block:: console

   sudo groupadd mnemosyne
   sudo dasobjectstore store create zymo_fecal_2025.05 \
     --class reproducible_cache \
     --writer-group mnemosyne

Add ingest users to the daemon transport group and the store writer group so the
CLI can connect and the daemon can authorize their ingest jobs:

.. code-block:: console

   sudo usermod -aG dasobjectstore "$USER"
   sudo usermod -aG mnemosyne "$USER"

The user must start a new login session before the new group is visible to
normal processes. Check membership with:

.. code-block:: console

   id

The output must include both ``dasobjectstore`` and ``mnemosyne`` before
non-root ingest job submission will be allowed. If the group already exists,
skip ``groupadd``.

On Linux, ``dasobjectstored`` reads the UID and primary GID attached to the
local Unix-socket connection, resolves the username and supplementary groups
from the host account database, and checks the target ObjectStore writer group
before any managed DAS root is written. Membership in ``dasobjectstore`` only
allows a client to reach the daemon socket; it does not grant write authority
for every store.

The copy count defaults to the store policy. Use ``--copies`` only when the
override is intentional:

.. code-block:: console

   dasobjectstore ingest files zymo_fecal_2025.05 \
     --source /mnt/external/zymo_fecal_2025.05 \
     --copies 1

Use ``--dry-run`` to inspect the planned file set without importing:

.. code-block:: console

   dasobjectstore ingest files zymo_fecal_2025.05 \
     --source /mnt/external/zymo_fecal_2025.05 \
     --dry-run

Progress Output
---------------

The progress output is intentionally similar to an ``rsync --info=progress2``
operator view. Daemon progress events report cumulative work bytes, percent
complete when the total is known, transfer rate, file counts, remaining files,
current stage, and SSD pressure.

Example line:

.. code-block:: text

      104857600  42%    82.4 MiB/s files=3/12 remaining=8 stage=ssd-ingest ssd=AcceptingWrites

Stages:

* ``ssd-ingest`` means DASObjectStore is reading from the mounted source disk
  and landing the file on SSD because the request is SSD-first.
* ``source-read`` means the daemon is hashing a local-server source file before
  direct-to-HDD placement when the target store policy allows that path.
* ``hdd-copy:<disk-id>:<copy-number>`` means DASObjectStore is settling and
  verifying one HDD copy from the staged SSD payload or verified local source
  hash. The disk ID is reported for auditability; it is selected by
  DASObjectStore.

SSD Stress
----------

Progress lines include the latest daemon SSD-pressure sample when available:

.. code-block:: text

   ssd=AcceptingWrites

Pressure states come from the SSD capacity policy:

* ``AcceptingWrites``: normal ingest can continue.
* ``High``: SSD pressure is elevated; destage work should be prioritized.
* ``Critical``: SSD space is stretched; normal ingest may need to pause until
  settlement frees space.

Source Layout and Object IDs
----------------------------

The command imports regular files beneath ``--source``. Object IDs are derived
from the resolved endpoint prefix and relative path. For a direct object store,
the endpoint prefix is the store ID. For example:

.. code-block:: text

   zymo_fecal_2025.05/nested/sample.fastq.gz

For a nested SubObject, the endpoint prefix includes the root store and
SubObject path. For example, importing into ``Vervet`` beneath
``ENA/Xenognostikon`` produces:

.. code-block:: text

   ENA/Xenognostikon/Vervet/nested/sample.fastq.gz

Symlinks and non-regular files are not imported by this path.

Operational Notes
-----------------

The normal CLI path builds a daemon ingest request and reports daemon job
progress. The hidden ``--local-direct`` mode exists only for developer tests and
should not be used as the production ingest path.

Packaged Linux services allow read-only daemon access to home directories so
user-provided source paths can be ingested without falling back to direct local
mutation. This is a service sandbox choice, not an authorization bypass: the
daemon still evaluates the ingest request, and the CLI grants the service
identity read/traverse ACLs for the selected source tree before submission.

Interrupted imports should be inspected through daemon job status and store
state before retrying.

Inspect and Drain the Ingest Queue
----------------------------------

Inspect queued or active ingest jobs for a store:

.. code-block:: console

   dasobjectstore ingest queue generated-data

Use JSON when automating:

.. code-block:: console

   dasobjectstore ingest queue generated-data --json

If an import was started by mistake, preview queue cancellation first:

.. code-block:: console

   sudo dasobjectstore ingest drain-queue generated-data --dry-run

To cancel active queued jobs for that store:

.. code-block:: console

   sudo dasobjectstore ingest drain-queue generated-data \
     --allow-ingest-queue-drain \
     --confirm "confirm ingest queue drain"

Queue drain marks active ingest jobs as ``Cancelled`` and records a failure
message; it does not delete queue rows. This preserves an audit trail while
stopping accidental work from continuing through the settlement path.

Emergency ingest control
-------------------------

During an I/O or control-plane incident, a local administrator can pause new
source reads without interrupting the object currently being checksummed or
finalized. Staged data remains durable and the daemon resumes from the next
source object after the incident is cleared:

.. code-block:: console

   sudo dasobjectstore ingest control --action pause \
     --reason "protect Web availability" \
     --confirm "confirm ingest control"

Use ``throttle`` to keep ingest moving with a bounded delay between source
objects, or ``resume`` to return to normal admission. The operation is
daemon-owned and authenticated; it does not kill jobs, remove staged files, or
require a service restart. Use ``--dry-run`` to preview the resulting state.
The control is process-local and returns to ``running`` after a daemon restart;
it currently gates daemon file-ingest source reads (including direct-HDD
imports), while provider-specific S3 workers retain their own admission gate.
The Web/TUI surfaces should be treated as degraded until the state returns to
``running``.

The authenticated Web administrator route mirrors the CLI contract at
``POST /api/v1/workspaces/admin/ingest-control``. It uses the daemon's reserved
control bridge and returns a typed ``paused``, ``throttled``, or ``running``
state; it never mutates storage directly.

For captured operator evidence, add ``--tui`` to the CLI command. This renders
a compact, line-oriented acknowledgement containing the resulting admission
state, whether it changed, the applied/preview mode, and the operator reason.
It is a one-shot view: it does not accept keyboard commands or poll daemon
state. ``--tui`` and ``--json`` are mutually exclusive so machine-readable
output cannot be silently replaced by presentation text.

Operator triage for ingest pressure
-----------------------------------

Use the following sequence when ingest pressure threatens Web or daemon
responsiveness:

1. Inspect ``dasobjectstore ingest status`` and the TUI progress snapshot. Look
   for ``High``/``Critical`` SSD pressure, growing HDD settlement queues,
   stalled verification, or a control-plane degraded warning.
2. Apply ``ingest control --action throttle`` first when forward progress is
   still desirable. Apply ``pause`` when Web/API latency or queue growth is
   worsening. Both actions stop only new source-object admission; an object
   already being checksummed, fsynced, or atomically renamed is allowed to
   finish.
3. Confirm liveness and authenticated dashboard access, then monitor queue
   depth and pressure while staged work drains. Use ``resume`` only after the
   limiting condition is stable.
4. Capture the CLI/TUI state, timestamps, pressure, queue depths, and any typed
   degraded response for escalation. Do not kill the daemon, delete staging
   files, or restart services as the default recovery action.

Provider-specific S3 workers keep their own admission gate; the emergency
control documented here governs daemon file-ingest source reads. Appliance
throughput, PSI, and full-disk acceptance still require the separate quiescent
DASServer soak campaign.

The operations TUI provides the console workflow contract for planning,
confirmation, launch, monitoring, reconnect, and completion review. It uses the
same daemon job model as the CLI and Web UI, with visibility into file counts,
scaled data volume, SSD staging when used, HDD fan-out, verification, resource
policy, worker queues, pressure, bottlenecks, and throughput trends.
