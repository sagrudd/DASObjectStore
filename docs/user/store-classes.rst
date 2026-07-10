Object Store Classes
====================

Stores define how DASObjectStore ingests, places, protects, and repairs data.
The store class gives a safe default policy, and individual stores may override
selected settings where the CLI supports it.

How to Inspect Defaults
-----------------------

Use ``store defaults`` to see the JSON policy for a class:

.. code-block:: console

   dasobjectstore store defaults --class generated_data

Repeat the command for any supported class:

.. code-block:: console

   dasobjectstore store defaults --class reproducible_cache
   dasobjectstore store defaults --class critical_metadata
   dasobjectstore store defaults --class export_bundle
   dasobjectstore store defaults --class ingest_staging

Class Summary
-------------

.. list-table::
   :header-rows: 1
   :widths: 22 36 12 30

   * - Class
     - Intended use
     - Copies
     - Notes
   * - ``reproducible_cache``
     - Public or reproducible data with a download or rebuild cost.
     - 1
     - May be marked redownload-required if a disk fails and no spare copy can
       be made. Use for reference datasets that can be reacquired.
   * - ``generated_data``
     - Pipeline outputs, derived analysis results, and user-generated data.
     - 2
     - Protected class. Defaults to SSD-first ingest and acknowledgement after
       HDD placement.
   * - ``critical_metadata``
     - Manifests, indexes, provenance, credential references, and control data.
     - 3
     - Protected class with the strongest default redundancy and write rejection
       under unsafe capacity pressure.
   * - ``export_bundle``
     - Packaged datasets intended for external transfer.
     - 2
     - Protected class. Defaults to read-only file export behavior.
   * - ``ingest_staging``
     - Temporary SSD-backed ingest state.
     - 1
     - Mutable internal staging class. Not intended as a user data store.

Important Defaults
------------------

Remote S3/API, Web, and USB-mounted sources are SSD-first. Server-local CLI
imports may use ``dasobjectstore ingest direct-import`` to land directly onto
managed HDDs only when the store policy permits it; otherwise the daemon stages
the import through SSD first.

Protected classes are ``generated_data``, ``critical_metadata``, and
``export_bundle``. Protected classes cannot use direct-to-HDD ingest, immediate
delete retention, mutable policy, or redownload-required repair behavior.

``reproducible_cache`` is intentionally less protective. It is suitable for
massive public datasets where local loss is acceptable if the source and hash
are known.

Copy counts are currently limited to 1, 2, or 3.
