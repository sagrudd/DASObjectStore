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

After a user is added to a group, existing login sessions may not show the new
membership. The user must start a new login session before DASObjectStore or
other host processes can reliably see the updated group list.

Storage mutation must still go through the daemon:

* disk preparation, drain, replacement, and retirement;
* ObjectStore and SubObject creation or policy changes;
* service start, stop, and configuration;
* ingest, destage, repair, and migration jobs.

Synoptikon-integrated deployments are different: Synoptikon supplies account,
entitlement, audit, correlation, and governance-domain context.
