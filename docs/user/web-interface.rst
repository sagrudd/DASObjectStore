Web Interface
=============

DASObjectStore has two network-facing surfaces that are easy to confuse:

* the standalone Web UI/API surface; and
* the S3-compatible object-service endpoint used for remote uploads.

Packaged Linux appliances enable the standalone Web UI/API service by default.
The packaged listener is HTTPS on port ``8448``:

.. code-block:: text

   https://<das-host>:8448

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

The Web console is live-data first. The Home, Enclosures, ObjectStores, and
Bioinformatics pages request authenticated API payloads from the appliance and
show loading, empty, permission-denied, transport-error, and stale-data states
explicitly. They must not present bootstrap fixtures, mock hardware, or
placeholder store cards as though they were live appliance state. If a daemon
writer or data source is not implemented yet, the page should show a clear
unavailable-source warning or reserved-workflow message.

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

``Bioinformatics``
   Placeholder workspace for the first bioinformatics-oriented workflow. Until
   the workflow is implemented, this page should clearly present itself as a
   future integration surface rather than a place where storage mutations are
   already available.

``Activity`` or equivalent status surfaces may also expose daemon jobs, ingest
queues, repair work, and audit/provenance events as the implementation expands.
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

Milestone 19 removes the old holder-page pattern from the primary browser
experience. The active Web console surfaces are:

* ``Home`` for daemon-backed health and attention state;
* ``Enclosures`` for live DAS and drive inventory;
* ``ObjectStores`` for registry-backed store cards and writer-policy
  readiness; and
* ``Bioinformatics`` for clearly reserved workflow-readiness state.

Legacy ``workspaces/stores`` and ``workspaces/users-groups`` routes remain
compatibility API endpoints only. They are not primary navigation targets and
must not be used to reintroduce parallel static holders. Milestone 20 will add
administrator workflows on top of the canonical console, including enclosure
preparation, ObjectStore/SubObject creation, Users/Groups promotion when host
mode permits local administration, Activity views, and concrete bioinformatics
workflow-readiness cards.

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
count, usable capacity, seven-day throughput, memory pressure, SMART warnings,
visible ObjectStores, and operator attention items from the daemon health
payload.

The current Home API aggregator reads the managed SSD root
``/srv/dasobjectstore/ssd`` and managed HDD root ``/srv/dasobjectstore/hdd``
by default, honours ``DASOBJECTSTORE_SSD_ROOT`` and
``DASOBJECTSTORE_HDD_ROOT`` overrides, reads the system ObjectStore registry
through the same registry model used by the CLI and daemon, and reads Linux
memory pressure from ``/proc/meminfo``. Seven-day throughput and SMART warning
summaries are optional JSON inputs at
``/var/lib/dasobjectstore/telemetry/throughput-7d.json`` and
``/var/lib/dasobjectstore/health/smart-warnings.json``; until those daemon
writers are present, the dashboard reports explicit unavailable-source
warnings rather than bootstrap fixture text.

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

The ``Add enclosure`` card is no longer static placeholder text. The dashboard
payload carries a live affordance state that combines administrator capability,
supported enclosure discovery, and daemon inventory readiness. Non-admin users
see the card disabled with an explicit reason. Administrator-capable sessions
may see the card become ready only when a supported DAS enclosure is visible to
the daemon and the inventory path is healthy enough to plan preparation.

When the affordance is ready, the browser presents a preparation wizard for the
selected enclosure. The wizard derives candidate SSD and HDD devices from the
live enclosure detail payload, asks the administrator to choose SSD landing
media and HDD settlement media, records mount-root, filesystem, and optional
owner inputs, and shows a destructive data-loss review before any plan is
accepted. The administrator must explicitly allow formatting and type the
confirmation phrase ``confirm prepare das``. The Web API validates these same
fields server-side before forwarding the confirmed request to the daemon;
callers cannot submit an enclosure-preparation job without SSD media, at least
one HDD, format allowance, and the confirmation phrase.

The daemon API now exposes a typed enclosure-preparation request and response
contract for that handoff. The request includes SSD media, HDD media, mount
root, filesystem, optional mounted-root owner, optional administrator actor,
destructive format allowance, and the confirmation marker
``confirm prepare das``. The daemon client validates the request before
transport submission, including absolute device paths and duplicate HDD
rejection, so browser and API callers do not pass raw shell fragments or write
directly to managed roots.

Standalone Web deployments expose the authenticated submission route at
``/api/v1/workspaces/enclosures/prepare``. The route requires a valid local Web
session and a sudo-derived local administrator account before forwarding the
request to ``dasobjectstored``. Missing sessions, non-admin users, empty HDD
selections, missing destructive format allowance, and daemon submission errors
are returned as explicit Web API errors. The browser wizard displays accepted
daemon job metadata when submission succeeds and shows the daemon error message
when submission fails. After submission, the wizard polls the daemon-owned job
status route and renders the latest state, stage, byte or unit progress,
daemon message, failure text, submitted and updated timestamps, and cancellation
result. Operators can refresh status manually, request cancellation with a
recorded reason, or reset the wizard for another attempt after terminal
completion, failure, cancellation, or a status-refresh error.

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
non-admin users until the administrator workflow submits confirmed daemon action
plans.

The ObjectStores page is the Web counterpart to ``dasobjectstore store`` and
managed store policy. It should list each store with class, writer group,
copy/redundancy policy, ingest mode, bucket or endpoint identity, capacity
behavior, and current health.
It shows explicit loading, empty-inventory, permission-denied, and
transport-error states instead of using fixture store cards. The create card
reflects the daemon/API create affordance, including whether creation is
currently available or blocked by administrator requirements.

Creating or changing an object store is an admin-only workflow. The Web UI
should present class defaults before creation and submit the request to
``dasobjectstored`` rather than editing store registry files directly. When a
creation form includes a writer group, the daemon remains responsible for
validating the group, applying ACL or policy changes, and recording the store in
managed metadata. Non-admin users may inspect stores and submit writes only when
store writer policy allows it.

Users/Groups Workspace
----------------------

Users/group state is currently surfaced through the coherent product console
rather than a second standalone holder page. The compatibility API
``/products/dasobjectstore/api/v1/workspaces/users-groups`` remains available
for authenticated clients and future first-class Users/Groups navigation work.
It reports the current OS authority, product-local users, local group
memberships, administrator capability, and daemon-submitted group-management
operations. The same server-side groups registry used by the ObjectStores
dashboard is included as ``writer_groups`` so operators can see which managed
writer groups exist and whether the current local user is a member.

Legacy ``workspaces/stores`` and ``workspaces/users-groups`` Web holder
components are not part of the primary browser navigation. Operators should use
``Home``, ``Enclosures``, ``ObjectStores``, and ``Bioinformatics`` as the
canonical Web console surfaces.

Bioinformatics Workspace
------------------------

The Bioinformatics navigation item is reserved for product workflows that bind
DASObjectStore to reproducible reference data, generated pipeline outputs, and
Mnemosyne/Mneion storage definitions. Until those workflows are implemented, it
should behave as a placeholder and must not imply that unimplemented data
management actions have run.

The redesigned Bioinformatics page now requests
``/products/dasobjectstore/api/v1/workspaces/bioinformatics`` through the same
authenticated browser session as the other operator pages. The daemon-backed
payload controls whether the page is presented as workflow-ready or reserved
and lists the object types currently understood by the product workspace, such
as BAM, POD5, FASTQ, and ENA/SRA-oriented data.

Login and Footer Branding
-------------------------

The standalone login screen and application footer should make the product
identity clear while preserving the Mnemosyne family branding used by sibling
surfaces. Operators should see that they are signing in to DASObjectStore, that
local appliance authentication is being used in standalone mode, and that the
surface belongs to the Mnemosyne Biosciences product family.

The Web application uses the shared ``DasObjectStoreFooter`` component on both
the login page and authenticated console pages. The footer follows the compact
dark Mnemosyne product style used by sibling Web surfaces: monospace text,
``DASObjectStore v<version>``, "Developed by", a ``https://mnemosyne.co.uk``
Mnemosyne link, and 2026 Mnemosyne Biosciences attribution.

This footer is a product provenance requirement, not decorative page copy.
Future pages, dialogs that own a full operator route, and standalone error
states should keep the shared footer visible unless they are embedded inside a
host product that already supplies an equivalent Mnemosyne provenance footer.

Screenshot regression coverage is available through:

.. code-block:: console

   make web-screenshots

The check builds the real Trunk/WebAssembly interface, serves it under
``/products/dasobjectstore/`` with deterministic mocked API payloads, captures
login, Home, Enclosures, ObjectStores, and Bioinformatics screenshots at desktop
and mobile widths, and fails if the footer, primary navigation, major cards, or
page headers visibly overlap. Generated screenshots are written under
``target/web-screenshots/`` for review and are not committed. The check expects
the same Web packaging prerequisites as ``make web`` plus Node.js and Playwright
with Chromium installed.

Checking the Web Server
-----------------------

Use the top-level status command to inspect the daemon, Web UI, and object
service endpoints together:

.. code-block:: console

   dasobjectstore status
   dasobjectstore status --json

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
