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

The Web UI follows the same boundary. Admin-only actions such as enclosure or
disk lifecycle changes, object store creation, writer-group assignment, and
store policy changes may be initiated from Web forms, but the frontend only
submits requests. ``dasobjectstored`` validates authority and policy, prepares
the mutation plan, and performs confirmed host or storage changes.

Synoptikon-integrated deployments are different: Synoptikon supplies account,
entitlement, audit, correlation, and governance-domain context.
