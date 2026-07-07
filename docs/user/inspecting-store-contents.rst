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
