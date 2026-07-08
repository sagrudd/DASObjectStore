Performance Testing Ingest Storage
==================================

Use ``dasobjectstore performance-test`` to measure how the local DAS storage
path behaves when DASObjectStore lands a benchmark workload on the managed SSD,
drains the resulting SSD backlog to managed HDD members, and compares that
SSD-first route with direct source-to-HDD landing.

The command is intended for appliance commissioning, regression evidence, and
capacity planning. It is an administrative command, not a service and not a
normal ingest command, and it does not add objects to an object store.

What It Measures
----------------

For either generated files or an existing source folder, the command records:

* SSD-only landing throughput directly into the DASObjectStore SSD benchmark
  area;
* SSD read throughput from staged SSD payloads during real HDD drain work;
* ``ssd-stage-then-drain`` throughput where all selected files are landed on
  SSD first and FIFO HDD drain workers start only after staging completes;
* ``ssd-overlap-drain`` throughput where source files continue landing on SSD
  while FIFO HDD drain workers settle already staged files;
* direct source-to-HDD throughput for the same concurrency range;
* per-disk assigned bytes and write rates from the scheduler's actual placement
  decisions;
* redundancy effects when each logical file is landed to one, two, or three
  distinct HDD members;
* a recommendation for future ingest strategy and HDD settlement concurrency.

The benchmark discovers the managed HDD members under the configured HDD root
and tests concurrency from ``1`` up to ``--max-hdd-concurrency`` or the number
of available disks, whichever is lower. Use ``--max-hdd-concurrency 5`` to test
the requested one-to-five worker range on a sufficiently populated DAS.

Safety and Resource Warnings
----------------------------

This command deliberately creates sustained disk IO. Do not run large
performance tests while production ingest, repair, drain, or other storage-heavy
work is active unless that contention is the test scenario.

When ``--file_size`` and ``--file_count`` are used, DASObjectStore first
creates every generated random source file under:

.. code-block:: text

   <tmp-dir>/dasobjectstore-performance-source-<run-id>/

Those files are then used as a fixed source workload for the SSD and HDD
benchmark scenarios. This keeps random-data generation out of the measured
upload/landing phases and avoids per-file generation lag during scenario runs.
The generated source folder is removed on normal completion or cancellation.

HDD scenarios create temporary benchmark files under the selected disk's
managed ``.dasobjectstore/performance-test/<run-id>/`` directory. By default,
each logical file is assigned to one selected HDD for a scenario; with
``--redundancy 2`` or ``--redundancy 3``, each logical file is landed to that
many distinct HDD members. The benchmark does not write the same file to every
disk merely to inflate throughput. On normal completion these temporary
benchmark files and run directories are removed.

If the process is killed or the host loses power, temporary benchmark files may
remain. Inspect only the matching ``performance-test/<run-id>`` directories
from the report output before deleting anything manually.

Large runs should be planned against available free space and expected elapsed
time. For example, ``--file_size 2GiB --file_count 100`` writes 200 GiB of
logical payload per scenario and substantially more total IO while testing SSD
only, SSD-first HDD drainage, and direct-to-HDD landing.

Redundancy increases the physical HDD write volume. ``--redundancy 2`` writes
two HDD copies for each logical file in HDD-writing scenarios; ``--redundancy
3`` writes three copies. Values above ``3`` are rejected, and the command also
rejects redundancy greater than the number of managed HDD members.

Pressing ``Ctrl-C`` asks the active benchmark operation to stop and allows the
temporary objectstore cleanup guard to remove the run directories after the
current file operation returns. ``SIGKILL``, host power loss, or filesystem
failure can still leave temporary files behind.

Workload Modes
--------------

The benchmark supports two mutually exclusive workload modes:

* generated payloads, using ``--file_size`` and ``--file_count``;
* extant source data, using ``--source <DIR>``.

When ``--source`` is used, DASObjectStore recursively enumerates regular files
under the supplied directory, preserves each relative path inside the temporary
benchmark objectstore, and uses sorted relative path order as the FIFO source
order. This mode is intended for measuring real datasets already present on
NVMe, external disk, network mounts, or any other readable local path. Do not
combine ``--source`` with ``--file_size`` or ``--file_count``.

Use ``--cap <SIZE>`` with ``--source`` to benchmark a deterministic prefix of a
large existing dataset:

.. code-block:: console

   sudo dasobjectstore performance-test \
     --source /data/zymo_fecal_2025.05 \
     --cap 750GiB \
     --max-hdd-concurrency 5 \
     --tui \
     --report /var/lib/dasobjectstore/reports/performance-zymo-source-750GiB.pdf \
     --json-artifact /var/lib/dasobjectstore/reports/performance-zymo-source-750GiB.json

The cap is whole-file only. Files are selected in the same sorted relative-path
FIFO order used by the benchmark. Selection stops before the next file would
exceed the cap; files are not split. If the first FIFO file is larger than the
cap, the command exits without running a benchmark.

Basic Smoke Test
----------------

Run a small generated-data test first to confirm that the prepared SSD and HDD
roots are discoverable and that report generation works:

.. code-block:: console

   dasobjectstore performance-test \
     --file_size 100MiB \
     --file_count 2 \
     --max-hdd-concurrency 2 \
     --report /tmp/dasobjectstore-performance-smoke.pdf

The command prints progress while it runs and finishes with the PDF report and
JSON artifact paths:

.. code-block:: text

   Report: /tmp/dasobjectstore-performance-smoke.pdf
   JSON: /tmp/dasobjectstore-performance-smoke.json

The JSON and QR artifacts are written beside the PDF report using the same base
name unless an explicit JSON artifact path is supplied:

.. code-block:: text

   /tmp/dasobjectstore-performance-smoke.qr.svg

Redundancy Testing
------------------

Use ``--redundancy`` to model replicated landing of each logical file. The
default is ``1``. Accepted values are ``1``, ``2``, and ``3``:

.. code-block:: console

   sudo dasobjectstore performance-test \
     --source /data/zymo_fecal_2025.05 \
     --cap 750GiB \
     --max-hdd-concurrency 5 \
     --redundancy 2 \
     --tui \
     --report /var/lib/dasobjectstore/reports/performance-zymo-r2.pdf \
     --json-artifact /var/lib/dasobjectstore/reports/performance-zymo-r2.json

The concurrency limit remains the total number of active HDD write workers, not
the number of workers per copy. For example, ``--max-hdd-concurrency 3
--redundancy 2`` allows at most three simultaneous HDD writes while each
logical file is eventually landed on two distinct disks. The internal FIFO
write queue is bounded so a fast SSD producer cannot create an unbounded HDD
backlog inside the benchmark process.

Commissioning Test
------------------

A larger commissioning run can be used to model whether SSD read throughput can
support more than one concurrent HDD writer:

.. code-block:: console

   sudo dasobjectstore performance-test \
     --file_size 2GiB \
     --file_count 100 \
     --max-hdd-concurrency 5 \
     --report /var/lib/dasobjectstore/reports/performance-100x2GiB.pdf \
     --authoritative

To benchmark an existing folder instead of generated data, pass the source
directory. The benchmark will explore the same SSD-only, SSD-first FIFO drain,
and direct-to-HDD paths using the actual file sizes and folder structure:

.. code-block:: console

   dasobjectstore performance-test \
     --source /data/zymo_fecal_2025.05 \
     --cap 1TiB \
     --max-hdd-concurrency 5 \
     --tui \
     --report /var/lib/dasobjectstore/reports/performance-zymo-source.pdf \
     --json-artifact /var/lib/dasobjectstore/reports/performance-zymo-source.json \
     --authoritative

Authoritative Policy
--------------------

Add ``--authoritative`` only for commissioning or re-commissioning runs whose
results should govern future ingest behavior on the appliance:

.. code-block:: console

   sudo dasobjectstore performance-test \
     --source /data/zymo_fecal_2025.05 \
     --cap 1TiB \
     --max-hdd-concurrency 5 \
     --tui \
     --report /var/lib/dasobjectstore/reports/performance-zymo-source.pdf \
     --json-artifact /var/lib/dasobjectstore/reports/performance-zymo-source.json \
     --authoritative

The command still writes the requested JSON artifact beside the PDF report, and
also writes the same structured recommendation to the daemon's persistent
policy location:

.. code-block:: text

   /var/lib/dasobjectstore/performance/authoritative-recommendation.json

Restart ``dasobjectstored`` after the authoritative run. New ingest jobs after
the restart use the persisted benchmark recommendation to size the SSD-to-HDD
settlement worker pool. Remote S3 uploads and ingress from external disks remain
SSD-first; the authoritative result controls how staged SSD backlog is drained
to HDD from that point onwards. The JSON also records the recommended route for
NVMe/local-source ingest so future planner work can distinguish local NVMe
sources from external media without changing the remote/external safety rule.

Use ``--tmp-dir`` when the default report location under ``/tmp`` is unsuitable:

.. code-block:: console

   dasobjectstore performance-test \
     --file_size 1GiB \
     --file_count 20 \
     --tmp-dir /srv/dasobjectstore/tmp \
     --report /var/lib/dasobjectstore/reports/performance-20x1GiB.pdf

Use ``--ssd-root`` only when testing a non-default prepared SSD root:

.. code-block:: console

   dasobjectstore performance-test \
     --file_size 1GiB \
     --file_count 10 \
     --ssd-root /srv/dasobjectstore/ssd \
     --report /var/lib/dasobjectstore/reports/performance-explicit-ssd.pdf

Terminal View
-------------

Add ``--tui`` for the embedded terminal benchmark view during a long-running
administrative run:

.. code-block:: console

   dasobjectstore performance-test \
     --file_size 2GiB \
     --file_count 100 \
     --max-hdd-concurrency 5 \
     --tui \
     --report /var/lib/dasobjectstore/reports/performance-100x2GiB.pdf

The TUI is an operator convenience for the same command. It should show the
current benchmark phase, elapsed time, generated data volume, SSD write/read
activity, HDD concurrency activity, and artifact paths without changing the
benchmark workload or report content.

Keeping Temporary Files
-----------------------

``--keep-temp`` leaves benchmark files in place for post-run inspection. This is
useful for debugging path ownership or filesystem behavior, but it consumes SSD
and HDD capacity until the matching run directories are removed. Generated
source files under ``--tmp-dir`` are still removed when the command exits.

.. code-block:: console

   dasobjectstore performance-test \
     --file_size 100MiB \
     --file_count 1 \
     --keep-temp \
     --report /tmp/dasobjectstore-performance-keep-temp.pdf

Use this option sparingly on production appliances.

Report Outputs
--------------

Every successful run writes a final PDF report. ``--report`` must point to a
``.pdf`` path. The report has a tabular header containing Mnemosyne Biosciences
branding, DASObjectStore product identity, run ID, generation timestamp,
repository revision, CLI version, command line, artifact paths, and the
reproduction QR payload reference.

The PDF report includes:

* a scenario summary;
* the exact reproduction command;
* a JSON reproduction payload;
* median SSD write and read throughput;
* the recommended ingress strategy, redundancy setting, and HDD worker count;
* SSD-only, SSD-first pipeline, and direct-HDD scenario summaries;
* per-file SSD timing tables;
* per-disk landed-file tables, including the redundant copy index;
* the concurrency result table;
* the names of the tidy quantitative plot datasets embedded in the JSON
  artifact;
* the generated recommendation.

The command also writes:

* ``<report>.qr.svg`` as the reproduction QR SVG artifact;
* ``<report-stem>-*.svg`` quantitative bar-chart artifacts embedded into the
  PDF report when the renderer supports local images;
* a temporary Markdown source under ``--tmp-dir`` only while rendering the PDF.

The temporary Markdown source is removed after PDF generation. It is not a
supported report artifact.

Use ``--json-artifact`` to write the structured benchmark artifact beside the
human-readable report bundle:

.. code-block:: console

   dasobjectstore performance-test \
     --file_size 2GiB \
     --file_count 100 \
     --max-hdd-concurrency 5 \
     --report /var/lib/dasobjectstore/reports/performance-100x2GiB.pdf \
     --json-artifact /var/lib/dasobjectstore/reports/performance-100x2GiB.json

The JSON artifact is intended for automation and audit ingestion. It should
include the run ID, generation timestamp, CLI version, repository revision,
input parameters, discovered disks, PDF/QR artifact paths, per-file SSD
measurements, per-disk assigned bytes and HDD write rates, concurrency scenario
rows, and the generated recommendation. Keep it with the PDF and QR SVG files
for a complete evidence bundle.

Recommendation JSON Contract
----------------------------

The structured artifact uses schema
``dasobjectstore.performance_test.recommendation.v1``. It is the contract that
future ingress planners should consume rather than scraping PDF report tables.

The artifact records:

* run identity, including run ID, generation time, repository revision, CLI
  version, command arguments, benchmark parameters, and related artifact paths;
* hardware roots, including SSD root, HDD root, report temporary root, and
  discovered managed HDD member roots with disk IDs;
* SSD-only metrics, including generated bytes, SSD write/read rates, file
  count, nominal file size, total source bytes, workload kind, optional source
  path, optional source cap, discovered source totals, and per-file rates;
* separated SSD-stage-then-HDD-drain metrics for every tested concurrency value
  from ``1`` to ``N``;
* overlapping SSD+HDD drain metrics for every tested concurrency value from
  ``1`` to ``N``, including whether HDD drainage started before all selected
  files finished staging to SSD;
* aggregate assigned bytes, aggregate write rate, slowest member duration,
  selected members, and per-disk assigned bytes/rates for HDD-writing routes;
* direct-to-HDD pipeline metrics for every tested concurrency value from ``1``
  to ``N``, with the same aggregate and per-disk fields;
* a ``plot_data`` block with tidy bar-chart rows for strategy landing rate,
  elapsed time, physical HDD write volume, HDD write operations, and per-disk
  HDD write rates;
* the recommended ingress strategy, HDD concurrency, estimated aggregate rate,
  whether SSD drain-read throughput appears limiting, and short rationale
  strings;
* a ``daemon_policy`` block that records whether the artifact is authoritative,
  when it becomes effective, the fixed SSD-first route for remote and external
  disk ingress, the recommended route for NVMe/local-source ingest, and the
  HDD settlement concurrency consumed by the daemon.

Numeric byte counts and rates are emitted as JSON numbers in bytes and
bytes-per-second. Display-friendly strings in the PDF report are not a
substitute for these numeric fields. A representative contract fixture is
maintained at
``docs/user/examples/performance-recommendation.v1.json``.

During ``--tui`` runs the dashboard shows the active scenario objective and
SSD residency bounds. SSD-backed scenarios are bounded by measured available
SSD capacity and the default SSD high-water policy, so datasets larger than
the SSD are benchmarked in safe resident batches rather than requiring the
whole selected workload to fit at once. ``ssd-only`` writes a resident batch
sequentially, then reads that same batch back sequentially before the next
batch. ``ssd-stage-then-drain`` stages a resident batch to SSD before HDD
drain begins for that batch, then frees the batch before continuing.
``ssd-overlap-drain`` stages to SSD while FIFO HDD drain workers consume the
queue, but source staging pauses when the measured safe SSD residency budget is
full and resumes as drained files are removed. ``direct-hdd`` bypasses SSD for
the benchmark scenario.

The TUI separates active operation state from averages. The HDD landing panel
lists active file-copy writes with the file number, copy number, target disk,
landed bytes, total file size, and relative path. The rates panel reports SSD
write/read averages, aggregate HDD average, and per-disk HDD write rates only
for disks that are actively writing at that moment. Completed per-disk
performance remains available in the PDF and JSON report artifacts.

Scenario completion snapshots show aggregate scenario rates; detailed completed
per-disk rates are reserved for the report artifacts so they are not confused
with live active-write rates.

Daemon file ingest uses a bounded split SSD pipeline by default. Source reads
write staged payload bytes to SSD first; a bounded side worker then syncs the
staged payload and calculates SHA-256 before the file is allowed to enter HDD
settlement. This avoids blocking the next source file on the previous file's
SSD sync or checksum calculation while preserving the rule that HDD settlement
only consumes synced and checksummed SSD payloads.

When ``qrencode`` is available on the host, the QR SVG is a scan-ready code
for the reproduction payload. If ``qrencode`` is unavailable, DASObjectStore
still writes a fallback SVG artifact and records that fallback in the report's
``QR status`` field; install ``qrencode`` before formal commissioning runs that
require a scannable QR code.

When ``grammateus_markdown_pdf`` is available, the PDF is rendered with the
standard ``dasobjectstore-performance`` Mnemosyne report template, including
the title-panel metadata table, provenance QR payload, and signature fields. If
Grammateus is unavailable, DASObjectStore tries ``pandoc`` and then writes a
built-in fallback PDF artifact so that the benchmark run has a complete local
evidence bundle.

Reproducibility Notes
---------------------

Keep the PDF, JSON, and QR SVG artifacts together. The PDF report is the
primary human-reviewable artifact, while the JSON artifact is the preferred
machine-ingestion artifact.

For reproducible comparisons between runs:

* record the DAS appliance host, enclosure, and disk population used for the
  run;
* keep the generated report artifacts with the same run ID;
* compare runs produced by the same DASObjectStore CLI version where possible;
* use the same generated workload settings or the same ``--source`` tree,
  ``--max-hdd-concurrency``, and ``--tmp-dir`` settings;
* avoid concurrent ingest or repair work unless the comparison is explicitly
  testing contention.

The report recommendation should be treated as an operational starting point,
not a permanent policy. Re-run the benchmark after replacing disks, changing the
SSD, moving the appliance to a different DAS enclosure, changing filesystems, or
upgrading DASObjectStore storage placement behavior.
