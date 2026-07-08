Ingesting Files from a Mounted Disk
===================================

Use ``dasobjectstore ingest files`` to load a directory tree from an external
disk into a system-managed object store or SubObject endpoint. Do not copy files
directly onto DAS member disks.

The CLI is the client surface. In normal operation it submits an ingest job to
the managed ``dasobjectstored`` service, streams or references source data as
required by the local transport, and renders daemon progress events. The daemon
reads the store policy, discovers managed HDD members, selects copy placements,
stages each file through the DAS SSD, writes the requested verified HDD copies,
and reports byte-level progress while the copy is running.

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

The packaged daemon reads the path supplied with ``--source``. On Linux, the CLI
prepares source ACLs before submitting the job so the ``dasobjectstore`` service
can traverse private home directories and read the selected import tree without
requiring ``sudo`` or broad home-directory mode changes. The daemon does not
need write access to the source path.

DASObjectStore discovers prepared HDD members under the managed mount root and
chooses placements for each object. Operators must not choose individual disks
for normal file ingest.

What Happens
------------

When ``dasobjectstore ingest files`` is run, the CLI performs client-side
argument parsing and submits an ingest job request containing:

* the target object store or SubObject endpoint;
* the mounted source directory;
* the logical object type assigned to the imported files;
* an optional copy-count override;
* the existing-object conflict policy;
* whether the request is a dry run.

The daemon is responsible for authorization, policy lookup, SSD staging,
placement selection, HDD fan-out, verification, metadata mutation, and progress
events. The operator sees job submission details first, followed by byte-level
progress as daemon event streaming is available for the active job.

File ingest uses a bounded split SSD pipeline by default. The source reader
writes staged payload bytes to SSD and then moves on to the next file when
queue pressure allows. A bounded side worker syncs the staged SSD payload and
calculates the SHA-256 checksum; only after that succeeds is the file eligible
for HDD settlement. In the TUI, ``ssd-stage`` means source bytes are landing on
SSD, ``ssd-flush`` means the staged payload is being synced, and
``checksum-manifest-capture`` means the staged payload is being hashed before
HDD placement.

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

``--strict`` is the default and the safest commercial behavior. It reuses the
existing object only when the incoming file checksum matches the stored object
checksum. The local DAS metadata path records SHA-256 content hashes for this
comparison. If a checksum is unavailable or differs, the daemon must ingest the
incoming payload as a new stored version rather than silently overwrite the
existing payload.

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

The three policy flags are mutually exclusive. For normal operator use, prefer
the default strict policy unless the source dataset is known to be immutable and
the import is being repeated for operational recovery.

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
  and landing the file on the mandatory SSD.
* ``hdd-copy:<disk-id>:<copy-number>`` means DASObjectStore is settling and
  verifying one HDD copy from the SSD payload. The disk ID is reported for
  auditability; it is selected by DASObjectStore.

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

The operations TUI provides the console workflow contract for planning,
confirmation, launch, monitoring, reconnect, and completion review. It uses the
same daemon job model as the CLI and Web UI, with visibility into file counts,
scaled data volume, SSD staging, HDD fan-out, verification, resource policy,
worker queues, pressure, bottlenecks, and throughput trends.
