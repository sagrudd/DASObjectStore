Ingesting Files from a Mounted Disk
===================================

Use ``dasobjectstore ingest files`` to load a directory tree from an external
disk into a system-managed object store or SubObject endpoint. Do not copy files
directly onto DAS member disks.

The command reads the store policy, discovers managed HDD members, selects copy
placements, stages each file through the DAS SSD, writes the requested verified
HDD copies, and reports byte-level progress while the copy is running.

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

The user running ingest must be a member of the store's writer group. For the
example above, membership in ``mnemosyne`` is required. Ingest does not require
``sudo``.

DASObjectStore discovers prepared HDD members under the managed mount root and
chooses placements for each object. Operators must not choose individual disks
for normal file ingest.

Group Requirements
------------------

Store creation and writer-group assignment are administrator actions:

.. code-block:: console

   sudo groupadd mnemosyne
   sudo dasobjectstore store create zymo_fecal_2025.05 \
     --class reproducible_cache \
     --writer-group mnemosyne

Add ingest users to the writer group:

.. code-block:: console

   sudo usermod -aG mnemosyne "$USER"

The user must start a new login session before the new group is visible to
normal processes. Check membership with:

.. code-block:: console

   id

The output must include ``mnemosyne`` before non-root ingest will be allowed.
If the group already exists, skip ``groupadd``.

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
operator view. It reports cumulative work bytes, percent complete, transfer
rate, file counts, remaining files, current stage, stage-local bytes, and SSD
stress.

Example line:

.. code-block:: text

       104857600  42.13%    82.4 MiB/s files=3/12 remaining=8 stage=ssd-ingest stage_bytes=104857600 ssd=pressure=AcceptingWrites used=31%

Stages:

* ``ssd-ingest`` means DASObjectStore is reading from the mounted source disk
  and landing the file on the mandatory SSD.
* ``hdd-copy:<disk-id>:<copy-number>`` means DASObjectStore is settling and
  verifying one HDD copy from the SSD payload. The disk ID is reported for
  auditability; it is selected by DASObjectStore.

SSD Stress
----------

Each file starts with an SSD stress line, and progress lines include the latest
sample when available:

.. code-block:: text

   SSD stress before file: pressure=AcceptingWrites used=31%

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

Current Limits
--------------

This command performs synchronous ingest and settlement. It gives clear operator
progress for large local imports, but it is not yet a resumable job scheduler.
If an import is interrupted, inspect the output and rerun after checking the
store state.
