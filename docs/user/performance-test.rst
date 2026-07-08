Performance Testing Ingest Storage
==================================

Use ``dasobjectstore performance-test`` to measure how the local DAS storage
path behaves when DASObjectStore creates generated test files, stages them
through the managed SSD, reads them back from SSD, and models concurrent HDD
settlement across discovered managed HDD members.

The command is intended for appliance commissioning, regression evidence, and
capacity planning. It is not a normal ingest command and it does not add objects
to an object store.

What It Measures
----------------

For each generated test file, the command records:

* random source-file generation throughput in the temporary source directory;
* SSD write throughput into the DASObjectStore SSD benchmark area;
* SSD read throughput from the staged SSD payload;
* per-disk HDD write throughput for each tested concurrency level;
* aggregate HDD write throughput and the slowest member at each concurrency
  level;
* a recommendation for initial HDD settlement concurrency.

The benchmark discovers the managed HDD members under the configured HDD root
and tests concurrency from ``1`` up to ``--max-hdd-concurrency`` or the number
of available disks, whichever is lower.

Safety and Resource Warnings
----------------------------

This command deliberately creates sustained disk IO. Do not run large
performance tests while production ingest, repair, drain, or other storage-heavy
work is active unless that contention is the test scenario.

The test creates one source file at a time under ``--tmp-dir`` and one SSD
payload at a time under:

.. code-block:: text

   <ssd-root>/.dasobjectstore/performance-test/<run-id>/

For each HDD member, it creates temporary benchmark files under that disk's
managed ``.dasobjectstore/performance-test/<run-id>/`` directory. On normal
completion these temporary benchmark files and run directories are removed.

If the process is killed or the host loses power, temporary benchmark files may
remain. Inspect only the matching ``performance-test/<run-id>`` directories
from the report output before deleting anything manually.

Large runs should be planned against available free space and expected elapsed
time. For example, ``--file_size 2GiB --file_count 100`` creates 200 GiB of
source payloads over the run and writes substantially more data while modelling
multiple HDD concurrency levels.

Basic Smoke Test
----------------

Run a small test first to confirm that the prepared SSD and HDD roots are
discoverable and that report generation works:

.. code-block:: console

   dasobjectstore performance-test \
     --file_size 100MiB \
     --file_count 2 \
     --max-hdd-concurrency 2 \
     --report /tmp/dasobjectstore-performance-smoke.md

The command prints progress while it runs and finishes with the Markdown and PDF
artifact paths:

.. code-block:: text

   Report: /tmp/dasobjectstore-performance-smoke.md
   PDF: /tmp/dasobjectstore-performance-smoke.pdf

The QR artifact is written beside the Markdown report using the same base name:

.. code-block:: text

   /tmp/dasobjectstore-performance-smoke.qr.svg

Commissioning Test
------------------

A larger commissioning run can be used to model whether SSD read throughput can
support more than one concurrent HDD writer:

.. code-block:: console

   dasobjectstore performance-test \
     --file_size 2GiB \
     --file_count 100 \
     --max-hdd-concurrency 3 \
     --report /var/lib/dasobjectstore/reports/performance-100x2GiB.md

Use ``--tmp-dir`` when ``/tmp`` is too small or is backed by storage that should
not participate in the source-generation part of the test:

.. code-block:: console

   dasobjectstore performance-test \
     --file_size 1GiB \
     --file_count 20 \
     --tmp-dir /srv/dasobjectstore/tmp \
     --report /var/lib/dasobjectstore/reports/performance-20x1GiB.md

Use ``--ssd-root`` only when testing a non-default prepared SSD root:

.. code-block:: console

   dasobjectstore performance-test \
     --file_size 1GiB \
     --file_count 10 \
     --ssd-root /srv/dasobjectstore/ssd \
     --report /var/lib/dasobjectstore/reports/performance-explicit-ssd.md

Keeping Temporary Files
-----------------------

``--keep-temp`` leaves benchmark files in place for post-run inspection. This is
useful for debugging path ownership or filesystem behavior, but it consumes SSD
and HDD capacity until the matching run directories are removed.

.. code-block:: console

   dasobjectstore performance-test \
     --file_size 100MiB \
     --file_count 1 \
     --keep-temp \
     --report /tmp/dasobjectstore-performance-keep-temp.md

Use this option sparingly on production appliances.

Report Outputs
--------------

Every successful run writes a Markdown report. The report has a tabular header
containing Mnemosyne Biosciences branding, DASObjectStore product identity, run
ID, generation timestamp, repository revision, CLI version, command line,
artifact paths, and the reproduction QR payload reference.

The Markdown report includes:

* a scenario summary;
* the exact reproduction command;
* a JSON reproduction payload;
* median SSD write and read throughput;
* the best observed aggregate HDD write throughput;
* per-file SSD timing tables;
* per-disk HDD write tables;
* the concurrency model table;
* the generated recommendation.

The command also writes:

* ``<report>.qr.svg`` as the reproduction QR SVG artifact;
* ``<report>.pdf`` as the final PDF report artifact.

When ``qrencode`` is available on the host, the QR SVG is a scan-ready code
for the reproduction payload. If ``qrencode`` is unavailable, DASObjectStore
still writes a fallback SVG artifact and records that fallback in the report's
``QR status`` field; install ``qrencode`` before formal commissioning runs that
require a scannable QR code.

When ``pandoc`` and its PDF engine are available, the PDF is rendered from the
Markdown report with ``pandoc``. If those tools are unavailable, DASObjectStore
still writes a fallback PDF artifact so that the benchmark run has a complete
local evidence bundle.

Reproducibility Notes
---------------------

Keep the Markdown, QR SVG, and PDF artifacts together. The Markdown report is
the primary machine-readable and reviewable artifact because it contains the
full reproduction payload.

For reproducible comparisons between runs:

* record the DAS appliance host, enclosure, and disk population used for the
  run;
* keep the generated report artifacts with the same run ID;
* compare runs produced by the same DASObjectStore CLI version where possible;
* use the same ``--file_size``, ``--file_count``, ``--max-hdd-concurrency``, and
  ``--tmp-dir`` settings;
* avoid concurrent ingest or repair work unless the comparison is explicitly
  testing contention.

The report recommendation should be treated as an operational starting point,
not a permanent policy. Re-run the benchmark after replacing disks, changing the
SSD, moving the appliance to a different DAS enclosure, changing filesystems, or
upgrading DASObjectStore storage placement behavior.
