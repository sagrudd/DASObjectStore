Standalone Authentication
=========================

DASObjectStore standalone appliances use OS-local identity as the administrator
authority.

An OS user with sudo rights is an appliance administrator. Store writer groups,
such as ``mnemosyne`` or a future store-specific group, authorize ordinary users
to submit daemon jobs for allowed stores. These groups do not give users direct
write access to managed DAS disks.

The Web UI verifies local usernames and passwords against the appliance OS
through PAM. DASObjectStore stores only its browser session token locally after
PAM succeeds; users do not need a separate product-local password account. That
session is not the source of administrator authority. For administrator
workflows, DASObjectStore uses OS-local identity and sudo-derived administrator
status, then submits the action to ``dasobjectstored``.

Administrator capability is evaluated for the authenticated local username
from the browser session, not for the unprivileged Web service account. For
example, when ``stephen`` signs in and belongs to ``sudo`` or another supported
administrator group, the Web dashboard and administrator submission routes
should expose administrator-capable affordances for ``stephen``.

The packaged service runs the Web process as the unprivileged
``dasobjectstore`` user. Local password verification is therefore delegated to
the root-owned ``/usr/libexec/dasobjectstore/dasobjectstore-local-auth-helper``
binary, which is executable only by the ``dasobjectstore`` group and uses the
packaged ``/etc/pam.d/dasobjectstore`` PAM service. The packaged Web systemd
unit must allow this setuid transition with ``NoNewPrivileges=false``.

Local group administration is also daemon-backed. Creating a local writer or
administrator group, and assigning a local user to one of those groups, is
accepted by the Web UI only as a request to ``dasobjectstored``. The daemon is
responsible for validating the operation, enforcing sudo-derived administrator
authority, and running the host-local system change when policy allows it. The
frontend must not mutate OS users or groups directly.

Group creation and assignment actions are sudo-administrator gated and support a
dry-run path before mutation. When a request would change the host, the UI must
present the daemon plan and require explicit confirmation before submitting the
confirmed operation. The confirmation marker for host-local group mutation is
``confirm local group administration``.

If a requested writer group already exists on the host, DASObjectStore treats
group creation as an adoption/reconciliation operation rather than as a fatal
error. A successful live create or assignment records the group in
``/opt/dasobjectstore/groups.json`` so Web ObjectStore policy and local OS group
membership stay aligned.

After a user is added to a group, existing login sessions may not show the new
membership. The user must start a new login session before DASObjectStore or
other host processes can reliably see the updated group list.

Storage mutation must still go through the daemon:

* disk preparation, drain, replacement, and retirement;
* ObjectStore and SubObject creation or policy changes;
* service start, stop, and configuration;
* ingest, destage, repair, and migration jobs.

For local CLI ingest on Linux, the daemon authorizes the connecting OS account
from Unix-socket peer credentials. ``dasobjectstored`` resolves the peer UID and
group set from the host account database and compares those groups with the
ObjectStore writer-group policy in the system-managed store registry. The
managed SSD and HDD roots remain daemon-owned; ordinary users submit jobs but
do not receive direct filesystem write permission to DAS media.

Browsing and download authorization use the same daemon identity boundary but a
separate read policy. A store may define a reader group for users who can browse
or download objects without ingest privileges. A store marked public is readable
by any authenticated DASObjectStore user, not by anonymous HTTP clients. Writer
group members can also read the store so existing private stores remain usable
without a separate reader group.

The Web UI follows the same boundary. Admin-only actions such as enclosure or
disk lifecycle changes, object store creation, writer-group assignment, and
store policy changes may be initiated from Web forms, but the frontend only
submits requests. ``dasobjectstored`` validates authority and policy, prepares
the mutation plan, and performs confirmed host or storage changes.

Host-federated Web authentication
---------------------------------

The target Web authentication authority is Monas for a standalone product host
and Synoptikon for an integrated deployment. Both hosts pass the same versioned
authenticated context to DASObjectStore. On every request the host adapter must
validate its live session and revocation state before constructing the verified
context accepted by the GUI API. DASObjectStore additionally rejects unknown
schemas, mismatched issuer or audience, expired or overlong contexts, malformed
CSRF bindings, and raw contexts that have not crossed that verification
boundary.

The host context supplies subject, roles, expiry, correlation, and a digest
binding to the host's CSRF state. It does not contain a storage permission.
Daemon-owned local group, administrator, ObjectStore, pairing, and action policy
still decide whether an authenticated actor may read or mutate storage.

The Monas adapter reads the host's pinned Prosopikon session store directly and
verifies the browser session for each adaptation. The bearer token is never
placed in the context, response, or audit identity: DASObjectStore receives an
opaque SHA-256-derived session identifier and a context valid for at most five
minutes and never longer than the host session. Monas caller input cannot mint
administrator or storage roles; the adapter emits only ``authenticated``.

The ready-to-mount Monas router consumes the host's HttpOnly
``monas_session`` cookie in process, inserts the verified actor, and serves the
normal DASObjectStore operational API without mounting ``/api/login``,
``/api/logout``, registration, or product session-issuer routes. The Monas and
DASObjectStore routers must share the same ``ProsopikonAuthStore`` instance or
root. A missing, malformed, expired, or logged-out cookie returns ``401`` before
the product handler runs.
Application mTLS/token endpoints are not mounted beneath this browser-cookie
middleware; they remain on their separately authenticated service boundary.

Monas ``0.6.0`` commit ``adfbef19e1e8bad0e503dcdf8584b7a7b0131020``
mounts this router directly at ``/products/dasobjectstore``. Both projects pin
Prosopikon commit ``c1dd477270e3ab34f5444e2c0565af845a930a70`` so the shared
store is type- and schema-identical in a clean checkout.

Federated HTML must include
``<meta name="dasobjectstore-host" content="monas">`` (or ``synoptikon``).
The Yew client then checks ``/api/v1/host-session`` with the HttpOnly host
cookie, never creates a DAS token in browser storage, returns an invalid session
to the host login, and sends logout to the host. Standalone pages without this
marker retain the compatibility DAS login during migration.

Monas serves the Trunk bundle from ``DASOBJECTSTORE_WEB_ROOT`` and injects the
marker itself; browsers cannot opt into trusted host mode through a request
header or query parameter. Its local browser acceptance uses generated data
only and covers login return, logout/revocation, and the 390 px layout.

Federated mutation requests also require ``x-dasobjectstore-csrf``. The value
is derived from and checked against the live host session, returned only by the
authenticated host-session endpoint, and retained by the Web application only
in process memory. It is not a storage credential. Missing or mismatched
values fail before a mutation handler runs; session invalidation and logout
clear the browser copy.

Monas standalone subjects occupy the same appliance-local OS username
namespace as direct standalone sessions. DASObjectStore therefore resolves a
Monas subject through the local user provider for dashboards, administrator
actions, EasyConnect, and ObjectStore browsing. Only current OS groups and
sudo-derived administrator status authorize storage access; Monas roles remain
audit context and cannot grant storage authority. Synoptikon subjects are not
assumed to be local usernames and fail closed until an explicit central-to-
local identity mapping is configured.

The Synoptikon adapter first validates the integrated request/session boundary,
then requires Synoptikon to confirm the live entitlement and revocation state.
Its governance storage binding is not copied into the GUI authentication
context and therefore cannot become DASObjectStore storage authority.
Its router composer requires the host to insert the typed session envelope and
CSRF binding before dispatch; missing or revoked context returns ``401``.

The intrinsic DASObjectStore login remains a compatibility path during the
migration. Operators must run exactly one authority mode for a deployment; do
not proxy host authentication while also exposing the standalone login routes.
Removal follows only after the Monas and Synoptikon adapters, identity/session
migration, browser behavior, package rollback, and recovery evidence pass.

Migrating the intrinsic registry to Monas
-----------------------------------------

The intrinsic registry already uses the pinned Prosopikon schema. Migration
therefore preserves users, password hashes, groups, rights, device tokens, and
session records without translating or printing secrets. It does not mutate or
remove the source registry.

Stop the intrinsic DASObjectStore Web server and the deployed Monas process
before migration so the source is quiescent. Monas's service name is owned by
its deployment and is intentionally not assumed here. First inspect the plan
using explicit roots::

   sudo systemctl stop dasobjectstore-server.service
   sudo dasobjectstore-auth-migrate \
     --source-root /var/lib/dasobjectstore/auth \
     --target-root /var/lib/prosopikon \
     --json

The dry run creates nothing. Review the user/session counts and checksum, then
apply with the exact confirmation phrase::

   sudo dasobjectstore-auth-migrate \
     --source-root /var/lib/dasobjectstore/auth \
     --target-root /var/lib/prosopikon \
     --apply \
     --confirm "confirm auth migration" \
     --json

The target must be absent, empty, or contain the byte-identical interrupted
copy. A different target registry fails closed. Symlinked roots/registries and
a source that changes during copying are rejected. The target receives a
mode-0600 registry and ``dasobjectstore-auth-migration.json`` checksum marker.
Repeating the same command is idempotent.

Configure Monas to use the target root and verify login before disabling the
intrinsic authority. Existing session records remain available to the shared
store, but browsers must authenticate once through Monas: a DAS header or
local-storage bearer is never converted into an HttpOnly Monas cookie.

For rollback, stop Monas and restore the prior package/configuration pointing
at the retained ``/var/lib/dasobjectstore/auth`` source. Do not delete either
registry until the package upgrade, Monas login, and rollback acceptance have
passed on the deployment host.
