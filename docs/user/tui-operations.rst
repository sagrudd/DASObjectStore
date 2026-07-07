Console TUI Operations
======================

The Milestone 18 console TUI is the planned operator surface for parallel file
ingress. It will use the same ``dasobjectstored`` job model as the CLI and Web
UI. Until the TUI binary or subcommand lands, use the current CLI commands for
real ingest and treat the TUI controls below as the supported design contract,
not executable commands.

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

Planned TUI Launch
------------------

These commands are planned for the Milestone 18 TUI entry point and are not
executable until that implementation is merged:

.. code-block:: console

   dasobjectstore tui import zymo_fecal_2025.05 \
     --source /mnt/external/zymo_fecal_2025.05

   dasobjectstore tui attach <job-id>

   dasobjectstore tui queue

The import view is expected to show the target ObjectStore or SubObject, source
paths, file count, scaled data volume, import description metadata, resource
policy, and a confirmation step before launch.

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
     - Planned
   * - ``r``
     - Resume a paused import when the daemon supports resume.
     - Planned
   * - ``c``
     - Request cancellation, followed by an explicit confirmation prompt.
     - Planned
   * - ``R``
     - Retry failed files or a failed job when daemon policy allows retry.
     - Planned
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

If the terminal disconnects or the TUI exits, the daemon job should continue
according to daemon policy. Operators should be able to attach to a running job
later and see a coherent state derived from daemon job events and metadata, not
from client-local screen state.

Error states should be explicit for authentication failure, permission denial,
lost daemon or event connection, stalled jobs, SSD pressure, HDD write failure,
verification failure, retry exhaustion, cancellation, and completed imports.
