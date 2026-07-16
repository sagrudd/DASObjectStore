Inspecting Store Contents
=========================

Use ``dasobjectstore store contents`` to inspect the logical contents recorded
for an object store. The command reads live metadata and does not walk or mutate
managed HDD payload directories.

The equivalent ``dasobjectstore store objects`` and
``dasobjectstore store list-contents`` aliases are available when a shorter
listing-oriented command is more convenient.

Show a du-style summary
-----------------------

The default view is a size summary similar to ``du -h -d 1``:

.. code-block:: console

   dasobjectstore store contents zymo_fecal_2025.05

Scope directly to a folder or file by appending its relative path to the
store target. Paths in the scoped report are rebased beneath that target:

.. code-block:: console

   dasobjectstore store contents xenognostikon/PRJEB33511
   dasobjectstore store contents xenognostikon/PRJEB33511 --tree -d 2

Text output labels entries explicitly as ``[DIR]`` or ``[FILE]``. JSON output
also includes ``kind: "file"`` for each object entry; aggregate directory
entries are represented by the text ``du`` view.

Use ``--depth`` or ``-d`` to control aggregation depth:

.. code-block:: console

   dasobjectstore store contents zymo_fecal_2025.05 --du -d 2

Show a tree
-----------

Use ``--tree`` to render directories and object leaves:

.. code-block:: console

   dasobjectstore store contents zymo_fecal_2025.05 --tree -d 4

Filter paths
------------

Use ``--filter`` with a Rust regular expression to limit output to useful
object IDs or relative paths:

.. code-block:: console

   dasobjectstore store contents zymo_fecal_2025.05 --tree --filter '\.(pod5|fastq\.gz)$'
   dasobjectstore store contents zymo_fecal_2025.05 --du -d 2 --filter '^raw/PAW10254/'

Export JSON
-----------

Use ``--json`` when another tool should consume the object list:

.. code-block:: console

   dasobjectstore store contents zymo_fecal_2025.05 --json --filter '\.bam$'

Download Objects From the Web API
---------------------------------

The standalone Web API exposes file downloads through the daemon boundary:

.. code-block:: text

   GET /api/v1/object-stores/<store>/objects/download/<object-id>

``<object-id>`` may contain slash-separated folder structure, for example
``ENA/Xenognostikon/Vervet/metadata.tsv``. The API requires an authenticated
browser session and asks ``dasobjectstored`` to authorize the request using the
same read policy as ObjectBrowser metadata: store administrators, the writer
group, the optional reader group, or authenticated users of a public store may
download.

Downloads prefer an existing verified settled HDD copy selected by the daemon.
When no settled HDD payload exists, a bounded folder profile may instead stream
the catalogue-authoritative object through the path-free daemon provider
transport. The daemon reauthorizes the browser's delegated OS actor before it
opens that stream; neither backend paths nor provider credentials reach the Web
process. Missing objects, unsupported providers, failed verification, and
permission denials return explicit errors rather than serving an arbitrary
filesystem path.

ObjectBrowser file metadata includes the daemon-selected ``download_source``:
``hdd_settled`` for a verified managed placement or ``provider_stream`` for a
catalogue-backed bounded folder profile. The Web console uses this field to
enable its Download action; it does not infer download safety from a disk badge.

Inspect Profile Catalogue Diagnostics
-------------------------------------

The standalone Web API exposes a read-only diagnostics projection for bounded
folder profiles:

.. code-block:: text

   GET /api/v1/profile-s3/stores/<store>/diagnostics

The request requires the same authenticated browser session as profile listing.
``dasobjectstored`` compares the authoritative catalogue with a bounded backend
enumeration and reports whether the store is ``empty``, ``synchronized``,
``uncatalogued_backend_objects``, or ``catalogue_missing_backend_objects``.
The response includes catalogue/backend counts, drift counts, the last
reconciliation timestamp when available, and an actionable message. It never
returns private disk paths and does not mutate the catalogue or backend. Use
the message and timestamp to decide whether to run the documented reconciliation
workflow before treating an empty browser view as data loss.

Inspect one profile object
--------------------------

The same authenticated profile endpoint exposes catalogue-authoritative object
metadata without returning a payload or backend path:

.. code-block:: text

   HEAD /api/v1/profile-s3/stores/<store>/objects?key=<url-encoded-object-id>&version=1

The response carries the logical key, version, byte size, and checksum after
the daemon resolves the registered bounded folder profile. A missing key or
catalogue/backend failure is returned as an explicit error; clients must not
infer object existence from provider listings.

For local operator and automation use, the daemon client contract is also
available through the CLI:

.. code-block:: console

   dasobjectstore store profile-head generated-data reads/sample.fastq
   dasobjectstore store profile-head generated-data reads/sample.fastq --json

This command reports metadata only and never reads or prints private backend
paths.

Inspect profile health
----------------------

An authenticated health projection is available for bounded profiles:

.. code-block:: text

   GET /api/v1/profile-s3/stores/<store>/health

The response contains provider-neutral ``state`` and optional ``message``
fields. It does not claim appliance SMART/NVMe or enclosure health; those
remain separate deployment-gated telemetry surfaces.

The same projection is available to local automation:

.. code-block:: console

   dasobjectstore store profile-health generated-data
   dasobjectstore store profile-health generated-data --json

Download Folder Archives From the Web API
-----------------------------------------

The standalone Web API also exposes folder archive downloads through the daemon
boundary:

.. code-block:: text

   GET /api/v1/object-stores/<store>/folders/download/<folder-prefix>

``<folder-prefix>`` is normally the relative prefix shown by the ObjectStore
browser, for example ``raw/PAW10254``. For compatibility with object IDs, the
daemon also accepts a prefix that begins with the store name, such as
``zymo_fecal_2025.05/raw/PAW10254``.

Before streaming begins, ``dasobjectstored`` resolves every object under the
prefix, applies the same read policy used for the browser and file downloads,
and verifies that each archive member has an existing verified settled copy on
a managed HDD root. If any object is missing, SSD-only, unverified, degraded, or
outside the store, the request fails before an archive body is generated.

Successful responses stream a ``tar.gz`` archive without staging the full
archive on SSD or HDD. The archive contains paths relative to the selected
folder prefix, and the response includes ``X-DASObjectStore-Archive-Files`` and
``X-DASObjectStore-Archive-Source-Bytes`` headers so Web clients can show a
preflight summary before or as the download starts.
