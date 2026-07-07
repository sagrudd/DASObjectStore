Getting Started
===============

DASObjectStore is managed through the ``dasobjectstore`` CLI. A bare command or
bare command group prints contextual help:

.. code-block:: console

   dasobjectstore
   dasobjectstore store
   dasobjectstore ingest

Use ``--help`` on any command when you need the full option list:

.. code-block:: console

   dasobjectstore disk prepare-das --help
   dasobjectstore store create --help

Current Operating Assumptions
-----------------------------

The MVP assumes:

* Linux is the full-operation target for disk preparation and service operation.
* macOS is supported for development, documentation, inspection, and selected
  read/export workflows.
* A DAS pool has one mandatory SSD ingest device and one or more HDD capacity
  members.
* Normal writes are SSD-first. Direct-to-HDD import is reserved for
  reproducible public datasets with known hashes.
* Store metadata is mirrored to the DAS SSD when a known SSD root is present so
  the DAS can be moved between hosts.

Useful Discovery Commands
-------------------------

Inspect attached disks and enclosure hints:

.. code-block:: console

   dasobjectstore probe --pretty
   dasobjectstore probe --json

Check health and connection warnings:

.. code-block:: console

   dasobjectstore health --summary
   dasobjectstore health --connections

List store commands:

.. code-block:: console

   dasobjectstore store

List object store classes by asking for defaults. The class names are:
``reproducible_cache``, ``generated_data``, ``critical_metadata``,
``export_bundle``, and ``ingest_staging``.

