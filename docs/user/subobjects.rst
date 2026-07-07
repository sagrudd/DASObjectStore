SubObjects
==========

Use SubObjects when a store needs named, searchable, nested endpoints without
creating a separate object-store policy boundary for every dataset.

An object store remains the policy and bucket boundary. A SubObject inherits the
root store policy, copy count, and service configuration, but has its own unique
endpoint name and object prefix. This lets large public or reproducible datasets
be organised by study, programme, or derived dataset while still being ingested
with a short endpoint name.

Example Layout
--------------

For ENA-derived datasets, create one object store and then nest named
SubObjects:

.. code-block:: text

   ObjectStore: ENA
     SubObject: Xenognostikon
       SubObject: Vervet

Create the root object store first:

.. code-block:: console

   sudo dasobjectstore store create ENA \
     --class reproducible_cache \
     --writer-group mnemosyne

Create the top-level SubObject:

.. code-block:: console

   sudo dasobjectstore subobject create Xenognostikon --store ENA

Create a nested SubObject:

.. code-block:: console

   sudo dasobjectstore subobject create Vervet --parent Xenognostikon

The resulting object prefix is:

.. code-block:: text

   ENA/Xenognostikon/Vervet

When a known DAS SSD is present, SubObject metadata is also mirrored into the
portable SSD registry at ``.dasobjectstore/subobjects.json``. This keeps nested
endpoint names available when the DAS is moved to another host and adopted
there.

Ingesting into a SubObject
--------------------------

SubObjects are formal ingest endpoints. Use the SubObject name where a store ID
would otherwise be used:

.. code-block:: console

   dasobjectstore ingest files Vervet \
     --source /mnt/external/ena/xenognostikon/vervet

The source directory structure is preserved in logical object IDs beneath the
SubObject prefix. For example, this source file:

.. code-block:: text

   /mnt/external/ena/xenognostikon/vervet/run-1/sample.fastq.gz

is imported as:

.. code-block:: text

   ENA/Xenognostikon/Vervet/run-1/sample.fastq.gz

The HDD payload is still written into DASObjectStore-managed content-addressed
storage. Do not expect the member disks to contain a browsable copy of the input
folder tree.

Listing and Searching
---------------------

List all known SubObjects:

.. code-block:: console

   dasobjectstore subobject list

Search by endpoint name or object prefix:

.. code-block:: console

   dasobjectstore subobject search vervet

SubObject names must be unique across the system. If an object store and a
SubObject share the same name, ingest refuses to guess and asks for the
ambiguity to be corrected.
