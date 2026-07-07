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
     --source /mnt/external/zymo_fecal_2025.05

The user running ingest must be authorized by the store's writer group. For the
example above, membership in ``mnemosyne`` is required. Ingest does not require
``sudo`` because the daemon, not the user's shell process, owns managed storage
mutation.

DASObjectStore discovers prepared HDD members under the managed mount root and
chooses placements for each object. Operators must not choose individual disks
for normal file ingest.

What Happens
------------

When ``dasobjectstore ingest files`` is run, the CLI performs client-side
argument parsing and submits an ingest job request containing:

* the target object store or SubObject endpoint;
* the mounted source directory;
* an optional copy-count override;
* whether the request is a dry run.

The daemon is responsible for authorization, policy lookup, SSD staging,
placement selection, HDD fan-out, verification, metadata mutation, and progress
events. The operator sees job submission details first, followed by byte-level
progress as daemon event streaming is available for the active job.

The daemon socket path in packaged Linux deployments is:

.. code-block:: text

   /run/dasobjectstore/dasobjectstored.sock

Group Requirements
------------------

Store creation and writer-group assignment are administrator actions:

.. code-block:: console

   sudo groupadd mnemosyne
   sudo dasobjectstore store create zymo_fecal_2025.05 \
     --class reproducible_cache \
     --writer-group mnemosyne

Add ingest users to the writer group so the daemon can authorize their ingest
jobs:

.. code-block:: console

   sudo usermod -aG mnemosyne "$USER"

The user must start a new login session before the new group is visible to
normal processes. Check membership with:

.. code-block:: console

   id

The output must include ``mnemosyne`` before non-root ingest job submission will
be allowed. If the group already exists, skip ``groupadd``.

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

Interrupted imports should be inspected through daemon job status and store
state before retrying.

The operations TUI provides the console workflow contract for planning,
confirmation, launch, monitoring, reconnect, and completion review. It uses the
same daemon job model as the CLI and Web UI, with visibility into file counts,
scaled data volume, SSD staging, HDD fan-out, verification, resource policy,
worker queues, pressure, bottlenecks, and throughput trends.
