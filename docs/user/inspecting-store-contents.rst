Inspecting Store Contents
=========================

Use ``dasobjectstore store contents`` to inspect the logical contents recorded
for an object store. The command reads live metadata and does not walk or mutate
managed HDD payload directories.

Show a du-style summary
-----------------------

The default view is a size summary similar to ``du -h -d 1``:

.. code-block:: console

   dasobjectstore store contents zymo_fecal_2025.05

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

Downloads stream from an existing verified settled HDD copy selected by the
daemon. The Web API does not accept a disk path from the caller and does not
read managed HDD roots directly. If the object is missing, redownload-required,
SSD-only, or otherwise lacks a verified managed HDD placement, the API returns a
clear unavailable/not-found error instead of serving an arbitrary filesystem
path.
