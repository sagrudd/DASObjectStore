Web Interface
=============

DASObjectStore has two network-facing surfaces that are easy to confuse:

* the standalone Web UI/API surface; and
* the S3-compatible object-service endpoint used for remote uploads.

Packaged Linux appliances enable the standalone Web UI/API service by default.
The packaged listener is HTTPS on port ``8448``:

.. code-block:: text

   https://<das-host>:8448

The public ``/api/v1/liveness`` route is intentionally independent of daemon
round trips. It reports Web service readiness and instance metadata so a load
balancer or operator probe can distinguish an unavailable Web process from a
daemon-dependent dashboard that is temporarily degraded.

The packaged appliance configuration lives at:

.. code-block:: text

   /opt/dasobjectstore/config.json

The default packaged bind address is ``0.0.0.0`` so the Web UI is reachable from
other hosts on the appliance network. Local development without the package may
still use the compiled fallback of ``127.0.0.1``.

Package Builds
--------------

``make deb`` and ``make rpm`` must include the full Trunk-built WebAssembly
operator interface. A package build should fail if the Trunk toolchain or
``wasm32-unknown-unknown`` Rust target is missing; it must not silently install
the developer placeholder page. Prepare a packaging host with:

.. code-block:: console

   sudo apt-get install dpkg clang libclang-dev libpam0g-dev
   rustup target add wasm32-unknown-unknown
   cargo install trunk

On AlmaLinux or RHEL package builders, install the native build tools with:

.. code-block:: console

   sudo dnf install rpm-build clang libclang-devel pam-devel

If the Web page says to install the Trunk WebAssembly toolchain, the installed
package contains the developer fallback page and should be rebuilt from a
toolchain-complete checkout.

The packaged standalone configuration also declares the authentication
authority. The DAS appliance default is local user authentication:

.. code-block:: json

   {
     "authentication": {
       "authority": "local_user",
       "session_ttl_seconds": 3600
     }
   }

``local_user`` enables the standalone login, session validation, and logout
routes under ``/products/dasobjectstore/api``. ``synoptikon`` and ``monas`` are
external authority modes; those deployments should mount DASObjectStore behind
the host product surface so account, entitlement, audit, and correlation context
come from that host.

Standalone login verifies the supplied username and password against the
appliance OS through PAM using the packaged ``dasobjectstore`` PAM service. The
product-local file under ``/opt/dasobjectstore/users.json`` stores only
DASObjectStore browser session tokens; users do not need to be pre-created in
that file before logging in. OS-local sudo status and daemon policy remain the
authority for administrative storage mutation.

Standalone browser sessions are intentionally process-scoped. When
``dasobjectstore-server`` starts, it revokes existing browser session tokens so
a restarted Web service requires a fresh sign-in. The browser also checks the
active session periodically: invalid or expired sessions are cleared from local
browser storage and the user is returned to the login page. If the Web server is
temporarily unreachable, the browser clears the active session, shows a
disconnection message, and polls the public health route until the interface can
reach the server again.

Packaged appliances keep the Web service unprivileged and perform the PAM check
through ``/usr/libexec/dasobjectstore/dasobjectstore-local-auth-helper``. The
helper must be owned by ``root:dasobjectstore`` with mode ``4750`` so
``pam_unix`` can verify local OS passwords without running the whole Web server
as root. The packaged ``dasobjectstore-server.service`` therefore sets
``NoNewPrivileges=false``; otherwise Linux would block the helper's setuid
transition and PAM would report valid local users as failed logins.

The server can also be started manually with explicit overrides:

.. code-block:: console

   dasobjectstore-server \
     --bind-address 0.0.0.0 \
     --https-port 8448 \
     --public-base-url https://<das-host>:8448

The S3-compatible upload endpoint is separate. Its default local endpoint is
``http://127.0.0.1:3900`` and it is not the Web UI.

Operator Navigation
-------------------

The redesigned Web UI is intended to be the normal operator console for a
standalone appliance and the embedded DASObjectStore surface when mounted behind
Synoptikon. After login, the first screen is the Home dashboard rather than a
marketing or setup page.

The Web console is live-data first. The Home, Enclosures, ObjectStores,
Activity, and Bioinformatics pages request authenticated API payloads from the
appliance and show loading, empty, permission-denied, transport-error, and
stale-data states explicitly. They must not present bootstrap fixtures, mock
hardware, or placeholder store cards as though they were live appliance state.
If a daemon writer or data source is not implemented yet, the page should show a
clear unavailable-source warning, idle category, or reserved-workflow message.

The primary navigation is:

``Home``
   Operational dashboard for appliance health, usable capacity, ingest and
   destage pressure, current service state, object-service status, and actions
   that need operator attention.

``Enclosures``
   Hardware and media view. It groups disks by enclosure when that information
   is available and shows role, health, mount, preparation, placement
   eligibility, SMART or USB warnings, and disk lifecycle actions.

``ObjectStores``
   Store policy and capacity view. It lists managed object stores, writer
   policy, class defaults, redundancy, ingest behavior, endpoint/export state,
   and store lifecycle actions.

``Activity``
   Daemon job and queue view. It shows administrator jobs, enclosure
   preparation, ObjectStore/SubObject creation, ingest, destage, repair, and
   endpoint validation categories from the shared daemon job model, plus any
   active task rows and queue summaries reported by the API.

``Users/Groups``
   Standalone appliance identity and writer-policy view. It is shown in primary
   navigation when the DASObjectStore host mode uses local-user authentication
   and reports OS authority, product-local browser users, local groups, writer
   groups, administrator readiness, and warnings.

``Bioinformatics``
   Workflow-readiness view for common sequencing and analysis object families.
   It presents daemon-provided cards for BAM, CRAM, POD5, FASTQ/FASTQ.GZ,
   FASTA, VCF/BCF, GFF/GTF, and ENA/SRA data, including handoff intent and
   metadata expected before orchestration.

Regardless of labels, storage mutation must still be submitted to
``dasobjectstored`` and must use the same job model as CLI and API operations.

Implementation Boundaries
-------------------------

The Web UI is a client surface. It may authenticate an operator, display live
inventory, collect workflow parameters, request action plans, and render job
progress, but it must not mutate managed DAS roots, format media, rewrite store
registries, change group policy, or move object data directly from browser
code. Those operations belong behind ``dasobjectstored`` so CLI, Web, TUI, and
future Synoptikon/Mneion adapters share the same policy checks, confirmation
phrases, audit metadata, cancellation, and recovery behavior.

The browser also does not choose the storage landing mode. Normal CLI ingest
submitted from the DAS appliance host is classified as ``local_server`` and may
write directly to daemon-selected HDDs only when the target ObjectStore policy
permits direct HDD ingest. Web-origin upload workflows are classified as
``web_upload`` and always stage bytes to the selected ObjectStore SSD first.
Paired remote-agent/S3 uploads are classified as ``remote_s3`` and also always
stage to SSD before daemon-owned HDD settlement. These origin labels and
landing modes are daemon/API contract values, not browser-controlled options.

Milestone 19 removes the old holder-page pattern from the primary browser
experience. The active Web console surfaces are:

* ``Home`` for daemon-backed health and attention state;
* ``Enclosures`` for live DAS and drive inventory;
* ``ObjectStores`` for registry-backed store cards and writer-policy
  readiness;
* ``Activity`` for daemon job categories, active task rows, ingest queue, and
  destage queue state;
* ``Users/Groups`` for standalone local-user authority, writer-policy
  readiness, and administrator capability when host mode permits it; and
* ``Bioinformatics`` for object-type workflow-readiness cards and handoff
  metadata expectations.

Legacy ``workspaces/stores`` remains a compatibility API endpoint only. The
``workspaces/users-groups`` route is now consumed by the first-class
``Users/Groups`` page in standalone local-user host mode; Synoptikon or Monas
integrated deployments should continue to omit this page until the host product
supplies the authority surface. Milestone 20 continues with concrete
bioinformatics workflow-readiness cards.

Home Dashboard
--------------

The Home dashboard should answer the operator's first questions without
requiring JSON inspection:

* whether the appliance is healthy enough to accept writes;
* how much protected and usable capacity is available;
* whether ingest, destage, repair, or object-service work is backlogged;
* which disks, enclosures, stores, or services require action; and
* whether the current user can administer storage or only inspect it.

The dashboard is informational by default. Risky actions should lead to the
specific Enclosures or ObjectStores workflow where the plan, policy allowance,
and confirmation can be shown next to the affected resource.

The redesigned Home page loads its live summary from
``/products/dasobjectstore/api/v1/dashboard/home`` using the browser session
issued at login. The page shows authenticated loading, permission-denied, and
transport-error states instead of presenting fixture metrics as live appliance
state. Once loaded, the visible cards cover drive count, mounted enclosure
count, usable capacity, selected-window throughput, memory pressure, SMART
warnings, visible ObjectStores, and operator attention items from the daemon
health payload. Telemetry-backed deployments add the operational cards that
operators normally check first: Capacity, Throughput, Disk IO, CPU usage,
Memory Stress, and Logged-in users.

The current Home API aggregator reads the managed SSD root
``/srv/dasobjectstore/ssd`` and managed HDD root ``/srv/dasobjectstore/hdd``
by default, honours ``DASOBJECTSTORE_SSD_ROOT`` and
``DASOBJECTSTORE_HDD_ROOT`` overrides, and reads the system ObjectStore
registry through the same registry model used by the CLI and daemon. When
daemon appliance telemetry is present, the Capacity, Throughput, and
Memory Stress cards prefer that telemetry so the cards match the daemon-owned
view of managed disks, IO rates, and host memory pressure. The Home page also
shows telemetry-backed Disk IO, CPU, and Logged-in users cards when appliance
samples are available; those cards report explicit unavailable telemetry state
instead of bootstrap values when the daemon has not produced samples yet.

Read unavailable telemetry as an instrumentation state, not as a healthy zero.
An unavailable card means the daemon did not provide a sample for that metric
and time range, the telemetry state file could not be read, or the local
platform does not expose the counter. A sparse chart shows only the samples the
daemon reported; the Web UI leaves gaps and empty states visible instead of
interpolating missing throughput, CPU, memory, disk, or user activity into a
continuous line. When a managed disk is still awaiting its first counter pair,
the Disk IO card says that rates are warming up and names the disk and mapped
device; if another disk is unavailable while valid disks report rates, the card
retains the valid totals and shows an elevated per-disk diagnostic.

Operators can use the Home telemetry window selector to view the latest
``1 hour``, ``1 day``, ``10 days``, or ``3 months`` of daemon appliance
telemetry. The browser sends the selected value as
``telemetry_window=one_hour``, ``one_day``, ``ten_days``, or
``three_months`` on ``/products/dasobjectstore/api/v1/dashboard/home`` so the
Capacity, Throughput, Disk IO, CPU, Memory Stress, and Logged-in users cards
are derived from a consistent sample window. The selected window affects both
card summaries and chart samples; changing it does not change stored telemetry
or daemon retention policy. Operators can override the telemetry source with
``DASOBJECTSTORE_WEB_APPLIANCE_TELEMETRY_PATH``. If appliance telemetry is not
available, the aggregator falls back to filesystem capacity, the optional
seven-day throughput JSON input at
``/var/lib/dasobjectstore/telemetry/throughput-7d.json``, and Linux memory
pressure from ``/proc/meminfo``. SMART warning summaries remain an optional
JSON input at ``/var/lib/dasobjectstore/health/smart-warnings.json``; until
those daemon writers are present, the dashboard reports explicit
unavailable-source warnings rather than bootstrap fixture text.

The Disk IO payload also retains the latest sample timestamp/age and per-disk
identity, mapped device, rates, and missing reason. This lets an operator
distinguish a single missing disk from a healthy aggregate; collection-quality
and raw missing-data markers are included in the same Home payload, while the
authenticated daemon telemetry contract remains the authoritative full history.

The Home page refreshes its selected-window telemetry payload every 30 seconds
while the page is open. The throughput telemetry chart uses a stable SVG view
box, fixed axes, bounded labels, and an explicit empty-sample state so updates
do not resize cards or interpolate missing data into the visible line. The
throughput summary identifies whether its samples came from daemon disk-IO
retention, the legacy throughput file, or an unavailable source, and carries a
diagnostic message for the latter two cases.

Any authenticated operator can use Home as a read-only appliance status page.
Administrator status only affects whether the same session can enter
daemon-owned mutation workflows from pages such as Enclosures, ObjectStores,
Users/Groups, or Activity. Home telemetry should therefore be treated as
shared operational evidence: if a card is unavailable or stale, inspect the
daemon telemetry writer and service state before acting on browser-side
assumptions.

The redesigned Home, Enclosures, ObjectStores, and Bioinformatics pages share a
single Yew API loading contract. Each page renders explicit loading, success,
empty, permission-denied, transport-error, and stale-data states so operators
can distinguish an empty appliance from an authentication problem or a transport
failure.

Home attention cards are derived from the daemon Home payload rather than from
static placeholder text. The current Web layer surfaces appliance health,
failed or suspect drives, ingest pressure, destage backlog, capacity pressure,
memory pressure warnings, enclosure warnings, SMART warnings, ObjectStore
warning state, object-service export readiness, and empty enclosure or
ObjectStore inventories. Ingest and destage cards are rendered when the
backward-compatible optional queue summaries are present in the Home payload;
the daemon/API aggregator remains responsible for populating those summaries
from live queue state.

Enclosures Page
---------------

The Enclosures page is the Web counterpart to disk inspection and preparation
workflows. It should show stable device identities, enclosure or bay grouping
when known, DAS role, mounted path, filesystem, capacity, health state, and
warnings that affect placement eligibility.

The redesigned Enclosures page loads its inventory from
``/products/dasobjectstore/api/v1/dashboard/enclosures`` using the browser
session issued at login. It shows explicit loading, empty-inventory,
permission-denied, and transport-error states instead of presenting fixture
hardware as though it were live. When enclosure data is present, the page
renders cards with connection topology, mount path, drive counts, capacity,
warning count, and health, plus a selected detail panel for enclosure identity
and bay membership when the daemon provides it.

The current Enclosures API aggregator reads the same managed SSD and HDD roots
as the Home dashboard. HDD roots are included only when their
``.dasobjectstore/device.env`` marker declares ``role=hdd:<disk-id>``. The
dashboard reports mounted drive counts, capacity, marker health, and detail
slots from those root markers; managed disk IDs beginning with ``qnap-`` are
presented as a QNAP TL-D800C enclosure until the deeper physical bay probe is
available from the daemon.

Selected enclosure detail panels render each SSD and HDD member as a drive card
with bay label, role assignment, capacity, mount path, device path, filesystem,
health, SMART warning count, and the daemon-managed actions currently available
for that member. These controls are informational until the administrator
workflow routes submit confirmed daemon jobs.

The ``Add enclosure`` card is exposed only when the dashboard payload advertises
a valid unprepared DAS enclosure candidate for the authenticated administrator
session. Existing managed DAS roots are inventory, not preparation candidates:
if DASObjectStore already knows the enclosure, the Web UI must not show a
preparation workflow for it. Deliberate destructive re-preparation, removal, or
replacement of an existing enclosure is a CLI-only administrative workflow.

When the affordance is ready, the browser presents a preparation wizard for the
selected enclosure. The wizard derives candidate SSD and HDD devices from the
live enclosure detail payload, asks the administrator to choose SSD landing
media and HDD settlement media, records mount-root, filesystem, and optional
owner inputs, and shows a destructive data-loss review before any plan is
accepted. The administrator must explicitly allow formatting and type the
confirmation phrase ``confirm prepare das``. The administrator must also
acknowledge that existing data on selected devices may be destroyed. The Web API
validates these same fields server-side before forwarding the confirmed request
to the daemon; callers cannot submit an enclosure-preparation job without SSD
media, at least one HDD, format allowance, existing-data acknowledgement, and
the confirmation phrase. The Web API also rejects preparation requests whose
mount root already contains DASObjectStore managed SSD or HDD marker metadata.

The daemon API now exposes a typed enclosure-preparation request and response
contract for that handoff. The request includes SSD media, HDD media, mount
root, filesystem, optional mounted-root owner, optional administrator actor,
destructive format allowance, existing-data acknowledgement, and the
confirmation marker
``confirm prepare das``. The daemon client validates the request before
transport submission, including absolute device paths and duplicate HDD
rejection, so browser and API callers do not pass raw shell fragments or write
directly to managed roots.

Standalone Web deployments expose the authenticated submission route at
``/api/v1/workspaces/enclosures/prepare``. The route requires a valid local Web
session and a sudo-derived local administrator account before forwarding the
request to ``dasobjectstored``. Missing sessions, non-admin users, empty HDD
selections, missing destructive format allowance, missing existing-data
acknowledgement, and daemon submission errors are returned as explicit Web API
errors. The browser wizard displays accepted daemon job metadata when submission
succeeds and shows the daemon error message when submission fails. After
submission, the wizard polls the daemon-owned job status route and renders the
latest state, stage, byte or unit progress, daemon message, failure text,
submitted and updated timestamps, and cancellation result. Operators can refresh
status manually, request cancellation with a recorded reason, or reset the
wizard for another attempt after terminal completion, failure, cancellation, or
a status-refresh error without losing their selected media and risk-review
inputs.

Administrator jobs accepted by the daemon are also exposed through the
standalone Web API at ``/api/v1/workspaces/admin/jobs/<job_id>``. This status
route and the companion cancellation route
``/api/v1/workspaces/admin/jobs/<job_id>/cancel`` require the same local session
and sudo-derived administrator authority. The routes forward to typed daemon
job status and cancellation commands; the browser must treat daemon responses as
the source of truth for job progress, terminal state, and cancellation result.
The packaged daemon persists this administrator job state beneath
``/var/lib/dasobjectstore/admin-jobs/jobs.json``. Until asynchronous
administrator execution is introduced, synchronous service, local group, and
enclosure-preparation submissions are recorded as completed job summaries; a
cancellation request against a completed job returns the current terminal state
without reopening the job.

Administrative disk actions, such as preparing media, locking down managed
roots, drain, replacement, retirement, or repair, are admin-only workflows. The
Web UI may collect parameters and present plans, but it must submit the
operation to ``dasobjectstored`` for policy checks and execution. Destructive
or data-moving operations require an explicit plan review and confirmation.

ObjectStores Page
-----------------

The redesigned ObjectStores page loads its inventory from
``/products/dasobjectstore/api/v1/dashboard/object-stores`` using the browser
session issued at login. The route reads the same system ObjectStore registry
used by the CLI and object-service orchestration layer, so visible cards are
registry-backed rather than placeholder fixtures.

Each ObjectStore card shows the store name, store class, object type, required
copy count, placement strategy, S3/export state, writer group, public/writeable
state, object count, used capacity, warning count, and last-ingest timestamp.
Registry fields come from ``/etc/dasobjectstore/stores.json`` on Linux unless
the packaged environment overrides the registry path. Object count, used
capacity, object type, and last-ingest time are read from live SQLite at
``/srv/dasobjectstore/ssd/.dasobjectstore/live.sqlite`` by default, or from
``DASOBJECTSTORE_WEB_LIVE_SQLITE_PATH`` when that override is set.

Writer groups are read server-side from ``/opt/dasobjectstore/groups.json`` by
default, or from ``DASOBJECTSTORE_GROUPS_PATH`` when the appliance overrides the
location. The browser never reads this file directly. The Web API returns the
group registry path, managed writer groups, current-user membership when known,
and each ObjectStore card's writer-policy readiness. If a store references a
writer group that is not present in the registry, the card remains visible but
reports the missing group as an explicit readiness state.

If live SQLite is unavailable, the card remains visible from the registry but
reports an explicit usage warning rather than hiding the ObjectStore or
presenting fixture data. The ``Create ObjectStore`` card remains disabled for
non-admin users. Administrator sessions can open the browser-side creation form
and prepare a reviewed action plan.

The ObjectStores page is the Web counterpart to ``dasobjectstore store`` and
managed store policy. It should list each store with class, writer group,
copy/redundancy policy, ingest mode, bucket or endpoint identity, capacity
behavior, and current health.
Ingest mode is shown as a policy summary, not as a per-upload disk selector:
local-server ingest may use direct-to-HDD placement when policy allows, while
Web and remote/S3 uploads remain SSD-first regardless of the store's local
direct-ingest allowance.
It shows explicit loading, empty-inventory, permission-denied, and
transport-error states instead of using fixture store cards. The create card
reflects the daemon/API create affordance, including whether creation is
currently available or blocked by administrator requirements.

The ``Browse objects`` panel on the ObjectStores page reads
``/api/v1/object-stores/<endpoint>/browser`` through the authenticated Web
session. Operators can select an ObjectStore endpoint, navigate folder prefixes
with breadcrumbs, search by object name or path, and sort by name, size, or
modified time. Folder and file rows show daemon-reported size, object type,
readiness, lifecycle state, copy count, and placement badges. Placement badges
name the managed disk label or external endpoint but do not expose writable
managed-disk paths to the browser.

Placement summaries distinguish SSD landing copies, verified settled HDD
copies, external endpoint records, pending placements, and degraded or missing
copies. A multi-copy object shows both the individual disk labels and the
verified HDD copy count. ``SSD only``, ``redownload required``, ``degraded``,
and ``unavailable`` readiness states are highlighted so operators can tell
whether an object is still settling, needs repair/redownload, or has no usable
local copy.

File download buttons are enabled only when the browser metadata reports an
available object with a verified settled HDD placement. Folder archive buttons
are enabled only for available folders. Large folder archives prompt for
confirmation using the browser's current object-count and size summary before
requesting the daemon-backed archive route. Downloads use the authenticated Web
session headers, so permission failures are shown in the panel rather than
falling back to direct managed-disk paths or anonymous links.

Browser and Download Semantics
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

The ObjectStore browser is an authenticated metadata view, not a filesystem
explorer. Browser requests are sent to
``/api/v1/object-stores/<endpoint>/browser`` and then forwarded to
``dasobjectstored``. The daemon chooses the visible metadata according to the
store's read policy: administrator authority, writer-group membership,
reader-group membership, or public-store read access. The browser never accepts
or displays a caller-supplied DAS disk path.

Folder navigation is prefix based. Breadcrumbs move between known prefixes, the
search field filters object names and paths server-side, and the sort selector
requests daemon-side ordering by name, size, or modified time. The response is
bounded by the browser API page size, so very large stores may report that more
entries are available even though only the first bounded page is rendered.
Operators should narrow large datasets with folder navigation or search rather
than expecting the browser to materialize an entire tree at once.

File downloads use
``/api/v1/object-stores/<endpoint>/objects/download/<object-id>``. The daemon
must resolve the object to an existing verified settled copy on a managed HDD
root before the Web API opens the source file. SSD-only objects, unsettled
objects, redownload-required cache objects, degraded or missing placements, and
objects outside the selected store fail with an explicit not-found or
unavailable message.

Folder downloads use
``/api/v1/object-stores/<endpoint>/folders/download/<folder-prefix>``. Before
the response body is streamed, the daemon checks every object under the prefix
and verifies that each archive member has a usable settled copy. If any member
is unavailable, the archive request fails before a partial archive is offered.
Successful folder downloads stream a ``tar.gz`` archive directly through the
Web service without staging the whole archive on SSD or HDD. If the browser
disconnects or cancels the download, archive generation stops rather than
continuing to write a hidden temporary payload.

Expected failure states are visible in the panel:

``Permission denied``
   The browser session is valid, but the authenticated user is not allowed by
   administrator, writer-group, reader-group, or public-store policy.

``Empty``
   The selected prefix has no folders or files visible to the current user.

``Unavailable``
   Metadata exists, but the object cannot currently be served because no usable
   verified settled copy is available.

``Transport error``
   The Web service could not reach the daemon, the daemon request failed, or
   the browser session could not complete the API request.

``Stale``
   A previous payload remains visible while a refresh failed or is in progress;
   operators should treat the visible rows as read-only until the state returns
   to success.

Creating or changing an object store is an admin-only workflow. The Web UI
presents controls for store name, writer group, mounted enclosure, object type,
redundancy, store class, export mode, and public visibility. Bucket name and
SSD root are derived from the store name and selected enclosure and are displayed
as immutable outcomes rather than operator-entered fields. New ObjectStores are
writeable by the selected writer group until explicitly locked after population;
retention is until explicit deletion, and capacity behavior is the appliance
placement policy for available space and requested redundancy. The workflow
first calls the GUI
action-plan endpoint and renders the generated ``dasobjectstore store create``
plan for administrator review. Submission then requires the exact phrase
``confirm create objectstore`` and is forwarded to ``dasobjectstored`` through
the daemon ObjectStore creation contract. For a non-dry-run request, the daemon
persists the validated definition in its managed store registry before it
records the administrator job complete. The browser displays the accepted
administrator job identifier, dry-run state, administrator actor, client request
identifier, and policy summary after the daemon accepts the request.
The Web route validates this request against the same store-service definition
shape used by CLI registry creation, so unsupported policy vocabulary or invalid
store-class/copy/retention combinations are rejected before any administrator
job is accepted.

Existing ObjectStores can be selected in the ``Configure ObjectStore`` card.
The policy editor is populated from the live ObjectStore card and lets an
administrator review changes to redundancy, writer group, public/writeable
state, retention, capacity behavior, export mode, store class, and SSD root.
The browser requests a distinct ``store_configure`` action plan and validates
store class, copy count, retention, capacity behavior, and export mode against
the supported domain vocabulary before displaying the planned command. This is
a review surface only until the matching daemon execution endpoint is wired.

The ``Create SubObject`` card provides the Web counterpart to
``dasobjectstore subobject create``. Administrators can create a top-level
SubObject under an ObjectStore or enter an existing parent SubObject name for a
nested prefix. The form previews the registry path, object prefix,
object-type inheritance or override, S3 routing mode, and SSD-root mirror
target before calling the ``subobject_create`` action-plan route. Object type
and S3 routing are validated for review even though the current CLI registry
persists only the SubObject name, parent, store ID, and derived object prefix.

The browser must still not edit store registry files directly. When a creation
form includes a writer group, the daemon remains responsible for validating the
group, applying ACL or policy changes, and recording the store in managed
metadata. Non-admin users may inspect stores and submit writes only when store
writer policy allows it.

Activity Workspace
------------------

The ``Activity`` primary navigation entry loads
``/products/dasobjectstore/api/v1/workspaces/activity`` and renders the shared
daemon activity model. The page requests the live daemon administrator job
registry through the packaged daemon socket and maps recorded jobs into task
rows. It also reads the daemon-owned live ingest queue metadata from the SSD
metadata database and derives the current ingest and HDD settlement summaries
from those rows. Repair activity is read from the same live metadata database:
``Repairing`` and ``Degraded`` pool states are surfaced as repair task rows
with explicit operator warnings. Endpoint-validation activity is read from the
same endpoint inventory registry as the ``Endpoints`` workspace. The page always
shows the supported activity categories so operators can distinguish an idle
appliance from an unimplemented browser holder. Categories currently include
administrator jobs, enclosure preparation, ObjectStore creation, SubObject
creation, ingest, destage, repair, and endpoint validation.

When daemon sources report work, the page shows active task rows with task ID,
kind, state, label, update timestamp, and daemon progress when available.
Remote upload jobs are ordinary daemon activity rows: their stage, byte
counters, unit counters, percent complete, and daemon message are read from the
persisted daemon job registry. A browser refresh, disconnect, or reconnect
therefore recovers the latest daemon-recorded transfer progress while the
paired CLI upload agent continues running. Ingest and destage queue summaries
are rendered separately so SSD upload pressure and HDD settlement activity are
visible even when no administrator job is active. If the daemon returns no task
rows, the page must state that no active tasks are currently reported rather
than implying completion. If the daemon socket or job registry cannot be read,
the API returns the category view with an explicit ``daemon_activity_unavailable``
warning instead of silently falling back to fixture data.
If the live ingest queue database cannot be read, the API returns the category
and task views with an explicit ``activity_ingest_queue_unavailable`` warning
while leaving the page observational and usable.
If live repair metadata cannot be read, the API returns
``activity_repair_events_unavailable`` rather than hiding the source failure.
If the endpoint inventory registry is missing or invalid, both ``Activity`` and
``Endpoints`` expose registry warnings rather than silently presenting fixture
data.

The Activity page is observational. Operators may navigate from submitted
administrator workflows to their daemon job status, but the page itself must not
cancel, mutate, or retry storage operations without using the same
authenticated daemon job routes and risk gates as the originating workflow.

Activity Reporting
~~~~~~~~~~~~~~~~~~

The ``Activity`` page includes a ``Reporting`` card for rebuilding formal
DASObjectStore performance-test reports from existing JSON artifacts. The card
uses the same compact drag-and-drop metaphor as the Mnematikon sample-ingress
surface, but it accepts only DASObjectStore benchmarking JSON files generated by
``dasobjectstore performance-test``. It does not ingest POD5, BAM, or ObjectStore
payload data.

The browser posts the selected JSON artifact to
``/products/dasobjectstore/api/v1/workspaces/activity/reporting/performance-report``
using the authenticated Web session headers. Standalone appliances require the
logged-in local user to have sudo-derived DASObjectStore administrator authority
before report rendering is attempted. The API validates the JSON schema
``dasobjectstore.performance_test.recommendation.v1``, writes the artifact into
a host-visible rebuild directory under
``/var/lib/dasobjectstore/report-rebuild``, invokes
``dasobjectstore performance-report`` with a bounded renderer timeout, and
removes the per-upload temporary directory after completion or failure. The
state-directory location is intentional: packaged ``dasobjectstore-server`` uses
systemd ``PrivateTmp=true``, and Docker cannot bind-mount paths that exist only
inside the Web service's private ``/tmp`` namespace.

On success, the API streams the regenerated PDF directly back to the browser with
``Content-Type: application/pdf`` and an attachment filename derived from the
uploaded JSON file. The Web UI creates a browser download for the returned PDF
automatically. If the JSON schema is unsupported, the renderer is unavailable, or
the Grammateus provider fails, the card reports the failure inline and no
partial PDF is offered for download.

The packaged appliance installs a DASObjectStore-owned
``/usr/libexec/dasobjectstore/gnostikon-workflow-control`` compatibility wrapper
and depends on a Docker-compatible container runtime for this Web report rebuild
path. These are critical runtime dependencies, not optional developer tools:
package installation should fail dependency resolution if the configured system
cannot provide the container runtime.

Package configuration adds the ``dasobjectstore`` service user to the
``docker`` group and restarts ``dasobjectstore-server.service`` so the Web API
can access ``/var/run/docker.sock``. If a manual Docker repair recreates the
group or socket after package installation, restore access with:

.. code-block:: console

   sudo usermod -aG docker dasobjectstore
   sudo systemctl restart dasobjectstore-server.service

The formal report-provider container is a Grammateus-owned runtime asset. Build
or repair it with ``make report-provider`` from a source checkout, or directly
with:

.. code-block:: console

   grammateus_report_provider install --image grammateus/report:0.8.1

The installer uses packaged ``/opt/grammateus`` and ``/opt/floundeR`` contexts
when present, or explicit source paths supplied by the build target.

Endpoints Workspace
-------------------

The standalone ``Endpoints`` navigation entry loads
``/products/dasobjectstore/api/v1/workspaces/endpoints``. It reads endpoint
inventory from ``/opt/dasobjectstore/endpoints.json`` by default, or from the
path named by ``DASOBJECTSTORE_ENDPOINTS_PATH`` when that environment variable
is set. The registry may be either an object with an ``endpoints`` list or a
bare list of endpoint records. Each endpoint record includes:

* ``endpoint_id`` and ``display_name``;
* ``kind`` such as ``dasobjectstore_das``, ``dasobjectstore_nfs``, or
  ``s3_compatible``;
* ``object_service_url``;
* ``validation`` with ``state``, optional ``checked_at_utc``, and optional
  ``message``;
* optional ``active_bindings`` with binding ID, governance domain, ObjectStore
  ID, and readiness.

Endpoint validation states are ``draft``, ``pending_validation``,
``validated``, ``degraded``, ``rejected``, and ``unknown``. Degraded, rejected,
unknown, draft, and pending states generate visible warnings.

Standalone administrator sessions can submit endpoint inventory creation or
updates from the ``Endpoints`` page. The form records endpoint identity, kind,
object-service URL, validation state, optional validation timestamp/message,
manager product ID, and optional active ObjectStore/governance-domain binding
controls. It shows the daemon acceptance result inline and reports
permission-denied responses without editing browser-side state.

The form submits to ``POST /api/v1/workspaces/endpoints/upsert``. The route
requires the same standalone session headers as other Web administrator routes
and the current OS user must have sudo-derived administrator authority. Live
submissions must include the exact confirmation marker
``record endpoint inventory``; dry runs may omit it. The request body carries
the endpoint identity, kind, object-service URL, validation state, optional
validation timestamp/message, manager product ID, optional active bindings,
dry-run flag, and optional client request ID.

The Web route validates the request and forwards it to ``dasobjectstored`` as an
``upsert_endpoint_inventory`` daemon request. The daemon writes the shared
registry and records an ``endpoint_validation`` administrator job so Activity
can show the accepted work. Browser code must not edit
``/opt/dasobjectstore/endpoints.json`` directly.

Users/Groups Workspace
----------------------

Users/group state is surfaced through the coherent product console rather than
a second standalone holder page. In standalone local-user host mode, the
``Users/Groups`` primary navigation entry loads
``/products/dasobjectstore/api/v1/workspaces/users-groups`` and presents the
current OS authority, product-local users, local group memberships,
administrator capability, and daemon-submitted group-management operations. The
same server-side groups registry used by the ObjectStores dashboard is included
as ``writer_groups`` so operators can see which managed writer groups exist and
whether the current local user is a member.

Administrator sessions can use the Users/Groups page to create a local writer
or administrator group, or assign a local user to a managed writer group. Both
forms submit first as a dry run through the daemon-backed local group
administration routes, then require the exact confirmation phrase
``confirm local group administration`` before a live daemon job is accepted.
Creating a group is idempotent: if the OS group already exists, the daemon
adopts that group instead of failing, and the Web API records the group in
``/opt/dasobjectstore/groups.json`` so ObjectStore writer-group policy and the
Users/Groups workspace remain consistent. Assigning a user to a group also
reconciles the same registry for cases where the group was prepared before the
Web console knew about it.
Non-admin sessions keep the forms visible as unavailable controls and receive a
clear permission-denied response if they attempt to submit through the API.

Legacy ``workspaces/stores`` Web holder components are not part of the primary
browser navigation. Operators should use ``Home``, ``Enclosures``,
``ObjectStores``, ``Remote Upload``, ``Activity``, ``Users/Groups``, and
``Bioinformatics`` as the canonical Web console surfaces.

Remote Upload Page
------------------

The ``Remote Upload`` navigation item is the browser side of the easyconnect
upload workflow. It is reached after a local-user Web login and requests
``/products/dasobjectstore/api/v1/workspaces/remote-upload`` through the same
authenticated session as the rest of the standalone console.

The page lists only ObjectStores that the authenticated user can read through
the daemon's easyconnect ObjectStore grant filtering. Each visible ObjectStore
shows the derived S3 bucket, ObjectStore class, object type, capacity summary,
writer group, public/restricted state, export mode, current warnings, and
whether upload is currently allowed. Upload readiness is stricter than
visibility: the store must be writable, exported through S3, and writable by
the current user's writer-group policy.

The page includes a drag/drop file and folder selection panel. Browser
filesystem metadata is used to summarize the selected target, file count,
folder count, total bytes, largest file, and representative paths before any
transfer begins. The browser does not perform the byte transfer itself:
confirmed uploads are handed to the paired ``dasobjectstore-remote`` local
agent so appliance S3 credentials do not need to be exposed in browser UI.

The handoff contract is loopback-only by default. The browser prepares relative
display paths and byte counts, validates the selected ObjectStore, and requires
an explicit confirmation phrase such as ``confirm upload to <ObjectStore>``
before posting the handoff manifest to the paired local agent. Absolute local
paths are intentionally excluded from the browser contract. If the agent is not
reachable, the page must report an ``agent_unreachable`` state and keep the
selection available for retry or cancellation.

The daemon classifies the Web-mediated path as ``web_upload`` and the paired
agent transfer as ``remote_s3``. Both origins advertise ``ssd_first`` landing
mode. This means selected bytes stage through the chosen ObjectStore's managed
SSD first, then settle to daemon-selected HDD targets according to store policy;
the browser and remote agent never choose or write individual disks directly.
The remote-upload workspace API also returns these origin and landing-mode
values so Web, CLI-agent, and future product-integration clients can make the
same placement assumptions before transfer begins.

Bioinformatics Workspace
------------------------

The Bioinformatics navigation item is the read-only workflow-readiness surface
for object families that DASObjectStore can classify for downstream
orchestration. It binds DASObjectStore concepts to reproducible reference data,
generated pipeline outputs, and Mnemosyne/Mneion storage definitions without
performing browser-side storage mutation.

The redesigned Bioinformatics page now requests
``/products/dasobjectstore/api/v1/workspaces/bioinformatics`` through the same
authenticated browser session as the other operator pages. The daemon-backed
payload controls whether the page is presented as workflow-ready or reserved
and lists the object types currently understood by the product workspace. The
payload also carries readiness cards for:

* ``BAM`` and ``CRAM`` alignment data, including reference and index metadata;
* ``POD5`` nanopore signal data for basecalling and run QC handoff;
* ``FASTQ/FASTQ.GZ`` read folders for QC, alignment, assembly, profiling, and
  transcriptome quantification;
* ``FASTA`` reference or assembly data;
* ``VCF/BCF`` variant data;
* ``GFF/GTF`` annotation data; and
* ``ENA/SRA`` public repository datasets with accession and manifest metadata.

The cards report category, readiness state, primary workflow intent, handoff
target, and required metadata. Later lineage and provenance work will derive
these states from ObjectStore/SubObject metadata and Mneion bindings; the Web
page already consumes the card contract so that transition can happen behind
the API boundary.

The page also renders read-only Bioinformatics context views:

* sequencing run provenance, covering run identifiers, sample/instrument
  context, flowcell or lane state, kits, and acquisition metadata;
* object lineage, covering raw signal, reads, alignments, variants, references,
  and annotations as parent/child workflow concepts;
* workflow handoff state for basecalling and genome/transcriptome analysis; and
* Mnemosyne project/governance-domain binding state.

These cards are deliberately informational. They describe the state required
for orchestration and auditability, but they do not launch workflows or mutate
storage from browser code. A later API slice will populate the cards from
ObjectStore/SubObject metadata, object type assignments, and Mneion export
bindings.

Bioinformatics readiness is also backed by explicit derivation source records.
The browser renders these records generically and does not hard-code metadata
paths or workflow-specific ObjectStore names. Source records identify:

* whether the evidence came from ObjectStore metadata, SubObject metadata, or a
  Mneion/Mnemosyne binding;
* the source identifier and optional parent source;
* the object type assignment and endpoint/export mode;
* the Mneion binding state and optional governance domain; and
* workflow roles plus evidence strings that explain why the source contributes
  to readiness.

This contract is the handoff point for live metadata aggregation. Once daemon
metadata exposes ObjectStore/SubObject object-type assignments and Mneion
bindings, the API can populate these source records without changing the Yew
page.

Administrator Workflow Operations
---------------------------------

Administrator Web workflows follow the same operational pattern regardless of
page:

#. the browser loads authenticated inventory from the Web API;
#. the operator enters workflow parameters in the page that owns the resource;
#. the Web API validates required fields and policy vocabulary;
#. destructive or persistent changes require an explicit review and exact
   confirmation phrase;
#. the Web API forwards the accepted request to ``dasobjectstored``; and
#. the browser renders the daemon job identifier, dry-run/live state, actor,
   progress, terminal state, and failure text reported by the daemon.

The browser never edits managed roots, registry JSON, groups files, endpoint
inventory, or object metadata directly. It also must not construct shell
procedures for mutations that have a formal daemon-backed command path.

The currently documented Web administrator workflows are:

``Enclosure preparation``
   Select supported DAS media, choose SSD and HDD roles, review destructive
   format risk, acknowledge existing data loss, type ``confirm prepare das``,
   and submit the preparation job to ``dasobjectstored``.

``ObjectStore creation``
   Enter store policy, writer group, object type, redundancy, public/writeable
   state, export mode, bucket, class, retention, and SSD root. Review the
   generated plan, type ``confirm create objectstore``, and submit to the
   daemon ObjectStore creation contract.

``ObjectStore configuration``
   Select an existing store and review policy edits for redundancy, writer
   group, public/writeable state, retention, capacity behavior, export mode,
   class, and SSD root. The current surface is a review/action-plan workflow
   until the matching daemon execution endpoint is introduced.

``SubObject creation``
   Define a named prefix under an ObjectStore or existing SubObject, review
   parentage, object-type inheritance or override, S3 routing, registry prefix,
   and SSD mirror path. The browser requests a ``subobject_create`` action plan;
   daemon execution remains the required mutation boundary.

``Users/Groups administration``
   Create local writer/admin groups or assign local users to managed writer
   groups. Dry-run review is available first; live submission requires
   ``confirm local group administration`` and sudo-derived authority.

``Endpoint inventory``
   Create or update DAS, NAS/NFS, S3-compatible, or Mnemosyne-governed endpoint
   records. Live submission requires ``record endpoint inventory`` and records
   endpoint-validation activity through the daemon.

Permission Boundaries
---------------------

Standalone appliances authenticate Web sessions through local OS users and PAM.
The Web session proves who is using the browser; it does not by itself grant
storage mutation authority. Administrator workflows additionally require
sudo-derived local administrator status as reported by the server-side
authority check.

Non-administrator users should still receive useful inventory. The UI should
show unavailable administrator controls with explicit blocked reasons rather
than hiding all operational context. API routes that mutate state must reject
missing sessions, expired sessions, non-admin users, missing confirmation
phrases, unsupported hardware, invalid policy vocabulary, and daemon transport
failures with clear messages that the browser can display.

Integrated Synoptikon or Monas deployments should not expose standalone local
login as the authority surface. In those modes, account identity, entitlement,
audit correlation, and governance context are supplied by the host product and
DASObjectStore remains an embedded storage surface.

Audit Expectations
------------------

Every accepted Web administrator mutation should be auditable through daemon
job state. Operators should be able to identify:

* the daemon job ID;
* job kind and dry-run/live state;
* submitted and updated timestamps;
* administrator actor where the host mode provides one;
* client request ID when supplied;
* requested policy or resource summary;
* current stage, progress, daemon message, terminal state, and failure message;
  and
* related Activity row or endpoint-validation state when the workflow affects
  shared activity.

The daemon job registry under
``/var/lib/dasobjectstore/admin-jobs/jobs.json`` is the packaged source of
truth for administrator job status. Browser status cards and Activity task rows
must reflect daemon state rather than reconstructing success from local form
state.

Recovery from Failed Web Jobs
-----------------------------

When a Web-submitted job fails, treat the daemon response as authoritative and
recover through the owning workflow:

#. Open ``Activity`` or the originating page and inspect the job state, stage,
   daemon message, and failure text.
#. If the job is still running or waiting, refresh status before retrying. A
   stale browser card is not evidence that the daemon is idle.
#. If the job supports cancellation, submit a cancellation reason through the
   job cancellation route and wait for the daemon terminal state.
#. For enclosure preparation failures, keep the selected media and risk-review
   inputs visible, correct the failed precondition, and use the wizard retry
   state rather than retyping unsafe shell commands.
#. For ObjectStore, SubObject, Users/Groups, and endpoint failures, correct the
   rejected policy, confirmation, or authority condition and submit a fresh
   reviewed request.
#. If the daemon socket, registry, live SQLite database, or endpoint inventory
   cannot be read, resolve that service or metadata source first. The Web UI
   should report unavailable-source warnings rather than treating missing data
   as success.

Do not manually edit ``/opt/dasobjectstore/groups.json``,
``/opt/dasobjectstore/endpoints.json``, the store registry, or live metadata to
recover from a failed Web operation unless a documented emergency repair
procedure explicitly instructs you to do so.

Bioinformatics Readiness Semantics
----------------------------------

The Bioinformatics page is read-only orchestration context. It identifies
object families and metadata requirements so downstream workflow systems can
decide whether data is ready for basecalling, genome analysis, transcriptome
analysis, variant analysis, annotation work, or public repository ingestion.

Readiness states are advisory and API-owned:

``workflow_ready``
   The object family has enough default metadata semantics for the named
   workflow handoff, subject to project/governance policy.

``metadata_required``
   The object family is recognised, but reference, sample, run, index,
   accession, or provenance metadata must be attached before automatic
   orchestration.

``catalogue_ready``
   Repository-style datasets can be catalogued and tracked, but download
   manifests, accessions, and study/project identity remain part of the
   evidence contract.

``binding_required``
   Mneion/Mnemosyne project or governance-domain binding is required before the
   data can be treated as auditable workflow input.

``planned``
   The product has a stable surface for the state, but live metadata derivation
   is not yet connected.

Bioinformatics cards must not imply that analysis has run. They describe
classification, evidence, provenance, lineage, and governance readiness only.
Workflow execution remains outside the browser and must use daemon-owned or
host-product-owned orchestration paths.

Login and Footer Branding
-------------------------

The standalone login screen and application footer should make the product
identity clear while preserving the Mnemosyne family branding used by sibling
surfaces. Operators should see that they are signing in to DASObjectStore, that
local appliance authentication is being used in standalone mode, and that the
surface belongs to the Mnemosyne Biosciences product family.

The Web application uses the shared ``DasObjectStoreFooter`` component on both
the login page and authenticated console pages. The footer uses the Mnemosyne
green ``#1c2b0b`` surface, the approved reversed wordmark, one cropped
decorative partial mark, compact ``DASObjectStore v<version>`` provenance,
"Developed by", a ``https://mnemosyne.co.uk`` Mnemosyne link, and 2026
Mnemosyne Biosciences attribution. It remains a shared flex-shell footer on
short and long pages.

This footer is a product provenance requirement, not decorative page copy.
Approved Mnemosyne branding files are copied into the Web package at build
time; the decorative partial mark is registered as a local Trunk asset and
its source SHA-256 is pinned by the Web workspace contract test. The partial
mark is not fetched from a sibling checkout at runtime.
Future pages, dialogs that own a full operator route, and standalone error
states should keep the shared footer visible unless they are embedded inside a
host product that already supplies an equivalent Mnemosyne provenance footer.

Shared panes, tables, and status primitives
-------------------------------------------

The console uses shared Web primitives for task panes, dense tables, status
badges, capacity bars, segmented controls, icon buttons, inspector drawers,
and risky-operation confirmations. These primitives keep keyboard focus,
labels, disabled states, and responsive behavior consistent across workflows;
page-owned styles should add only feature-specific layout or data treatment.
Tables are wrapped in a shared horizontally scrollable surface so wide object
metadata remains usable on small screens. Task-pane forms prevent accidental
browser submission while callers own the daemon mutation and confirmation
boundary.

Screenshot regression coverage is available through:

.. code-block:: console

   make web-screenshots

The check builds the real Trunk/WebAssembly interface, serves it under
``/products/dasobjectstore/`` with deterministic mocked API payloads, captures
login plus viewer/admin Home, Enclosures, ObjectStores, Activity,
Users/Groups, and Bioinformatics screenshots at desktop and mobile widths, and
fails if the footer, primary navigation, major cards, or page headers visibly
overlap. The desktop pass also exercises role-aware Web workflows: non-admin
sessions must keep enclosure preparation, ObjectStore/SubObject creation, and
local group assignment disabled, while administrator sessions must be able to
open the preparation wizard, review and submit daemon-backed ObjectStore
creation, review SubObject routing, dry-run/apply local group administration,
see Activity task progress, and render API-derived Bioinformatics readiness.
Generated screenshots are written under ``target/web-screenshots/`` for review
and are not committed. The check expects the same Web packaging prerequisites
as ``make web`` plus Node.js and Playwright with Chromium installed.

Checking the Web Server
-----------------------

Use the top-level status command to inspect the daemon, Web UI, and object
service endpoints together:

.. code-block:: console

   dasobjectstore status
   dasobjectstore status --json

The ``object_service`` section of the JSON output is the CLI healthcheck surface
for S3-compatible access. It reports whether the service is active, the bind
address, the active port, whether the endpoint is remote-ready, and the
``remote_url`` that remote workers should use. The Web Home page shows the same
information in the S3 service card so operators can see at a glance whether
Mnemosyne ecosystem clients can reach the object store endpoint.

The managed storage daemon, ``dasobjectstored``, is separate from the standalone
Web UI service. Check the web service and listener explicitly when diagnosing
access issues:

.. code-block:: console

   systemctl status dasobjectstore-server
   ss -ltnp | grep ':8448'

The Debian and RPM packages install and enable these systemd units:

.. code-block:: console

   dasobjectstored.service
   dasobjectstore-server.service

Validate the resolved standalone server configuration without starting a long
running listener:

.. code-block:: console

   dasobjectstore-server --config /opt/dasobjectstore/config.json --check-config
   dasobjectstore-server --config /opt/dasobjectstore/config.json --check-config --json

The JSON output includes ``auth_host_mode`` so operators can confirm whether the
server is exposing local standalone auth routes or expecting an integrated host
authority.

Self-signed TLS assets may be generated for standalone bootstrap when both the
certificate and private key are missing:

.. code-block:: console

   sudo dasobjectstore-server \
     --config /opt/dasobjectstore/config.json \
     --check-config \
     --generate-missing-tls

Synoptikon-Integrated Mode
--------------------------

Synoptikon-integrated deployments must not expose ``8448`` as the public product
listener. In that mode, DASObjectStore is mounted behind Synoptikon's HTTPS
surface under:

.. code-block:: text

   /products/dasobjectstore
   /products/dasobjectstore/api
