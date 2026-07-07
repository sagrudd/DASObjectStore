Console TUI Operations
======================

The Milestone 18 console TUI defines the supported operator surface for
parallel file ingress. It uses the same ``dasobjectstored`` job model as the CLI
and Web UI, so planning, launch, reconnect, progress, and completion review
remain daemon-mediated rather than client-local storage mutation.

Current CLI Path
----------------

Submit an ingest job through the daemon-backed CLI path:

.. code-block:: console

   dasobjectstore ingest files zymo_fecal_2025.05 \
     --source /mnt/external/zymo_fecal_2025.05

Inspect known ingest jobs:

.. code-block:: console

   dasobjectstore ingest queue
   dasobjectstore ingest queue --json
   dasobjectstore ingest status <job-id>

The hidden ``--local-direct`` developer mode is not a TUI workflow and should
not be used for production imports.

Standalone Package Path
-----------------------

Standalone Linux packages should install the TUI binary as:

.. code-block:: console

   /usr/bin/dasobjectstore-tui

The binary is packaged beside ``dasobjectstore`` and ``dasobjectstored``. It is
not a service and should run as the operator's login user, connecting to the
daemon for any storage mutation. Package installation must not grant the TUI
direct write access to managed DAS roots.

Current TUI Launch Preview
--------------------------

The current TUI binary provides a non-interactive launch preview. It accepts the
launch metadata, confirmation, and resource-cap fields that the interactive
import flow uses before submitting daemon-mediated work:

.. code-block:: console

   dasobjectstore-tui \
     --object-store zymo_fecal_2025.05 \
     --source /mnt/external/zymo_fecal_2025.05 \
     --description "Zymo fecal May 2025 ingest" \
     --metadata ticket=LAB-42 \
     --confirm-launch "confirm import launch"

Launch previews are blocked until a nonblank ``--description`` and the exact
``--confirm-launch "confirm import launch"`` phrase are provided. Additional
description metadata can be supplied with repeated ``--metadata KEY=VALUE``
arguments. Metadata keys must be unique, and both key and value must be
nonblank.

The planned interactive subcommands remain:

.. code-block:: console

   dasobjectstore-tui attach <job-id>

   dasobjectstore-tui queue

The import view should show the target ObjectStore or SubObject, source paths,
file count, scaled data volume, import description metadata, resource policy,
and a confirmation step before launch.

Keyboard Controls
-----------------

The planned controls are:

.. list-table::
   :header-rows: 1

   * - Key
     - Action
     - Status
   * - ``Tab`` / ``Shift+Tab``
     - Move between panels or fields.
     - Planned
   * - ``Enter``
     - Confirm the focused safe action.
     - Planned
   * - ``p``
     - Pause a running import when the daemon supports pause.
     - Planned when daemon policy allows pause
   * - ``r``
     - Resume a paused import when the daemon supports resume.
     - Planned when daemon policy allows resume
   * - ``c``
     - Request cancellation, followed by an explicit confirmation prompt.
     - Planned
   * - ``R``
     - Retry failed files or a failed job when daemon policy allows retry.
     - Planned when daemon policy allows retry
   * - ``d``
     - Open job details, including active stage, queue depths, retries, and
       current file context.
     - Planned
   * - ``q``
     - Leave the TUI view without cancelling the daemon job.
     - Planned

Risky actions such as cancellation must remain daemon-mediated and confirmation
gated. A TUI screen must not mutate managed storage directly.

Supported Terminal Sizes
------------------------

The standard layout should target terminals at least ``120x35`` cells. It can
show planning fields, resource policy, progress bars, worker/queue panels, SSD
pressure, HDD fan-out, verification, and throughput trend at the same time.

The compact layout should target ``80x24`` cells. It should preserve the
essential job state, current bottleneck, total progress, SSD pressure, HDD
backlog, verification status, and the active action prompt. Lower-priority
details can move behind the job details view.

Terminals smaller than ``80x24`` should show a clear unsupported-size message
and avoid launching new imports. Attaching read-only to an existing job may be
allowed if the status line and quit control remain visible.

Resource Policy
---------------

Before launch, the TUI should display whether resource policy is automatic or
explicitly capped. The policy summary should include:

* worker counts for scan, read, stage, write, verify, and finalization;
* memory budget for bounded read, write, and verify buffers;
* SSD reserve and current SSD pressure;
* per-HDD queue depth and write concurrency;
* verification parallelism;
* system safety reserve.

Automatic policy should use available CPU and memory headroom while preserving
explicit safety limits. Manual caps should be visible throughout the run so an
operator can explain why throughput is intentionally below device capability.

The launch preview accepts automatic or explicit caps:

.. code-block:: console

   dasobjectstore-tui \
     --cores auto \
     --memory-cap-bytes auto \
     --ssd-reserve-bytes auto \
     --hdd-write-concurrency auto

Use a positive number instead of ``auto`` to set an explicit cap. For example,
``--cores 12``, ``--memory-cap-bytes 8589934592``,
``--ssd-reserve-bytes 549755813888``, and ``--hdd-write-concurrency 6``.
Explicit zero-value caps are rejected.

Operational Expectations
------------------------

The TUI should report progress by lifecycle stage: discovered or scanned,
staged on SSD, written to HDD, verified, and finalized. Files must not be shown
as safely persisted until the daemon has completed the required HDD write and
verification work.

The live view should classify bottlenecks across CPU, memory, SSD pressure, HDD
fan-out, verification, daemon connectivity, and source-read limits. It should
show when source-to-SSD streaming is throttled by policy rather than by device
speed.

Benchmark profiling output should mirror those bottleneck dimensions. Offline
benchmark smoke runs write ``profiling.tsv`` with CPU, memory, SSD, HDD, and
verification fields set to ``not_collected``. Hardware benchmark runners should
replace those placeholders with measured values from daemon telemetry and host
profilers before a result is used for Milestone 18 acceptance.

If the terminal disconnects or the TUI exits, the daemon job should continue
according to daemon policy. Operators should be able to attach to a running job
later and see a coherent state derived from daemon job events and metadata, not
from client-local screen state.

Error states should be explicit for authentication failure, permission denial,
lost daemon or event connection, stalled jobs, SSD pressure, HDD write failure,
verification failure, retry exhaustion, cancellation, and completed imports.
