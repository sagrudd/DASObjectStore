Managed compute workspace host broker
=====================================

DASObjectStore keeps mutable compute workspaces separate from immutable
ObjectStores. Workspace filesystem mutation is performed by the narrowly
privileged, socket-activated ``dasobjectstore-workspace-host`` service. The
normal daemon remains unprivileged.

Security boundary
-----------------

The broker accepts only versioned ``provision``, ``inspect``, and ``rollback``
requests from members of the ``dasobjectstore`` service group. Requests name
opaque workspace, disk, branch, and project identities. They cannot supply
absolute paths, commands, mount options, users, groups, or arbitrary quota
arguments.

The administrator-owned file
``/etc/dasobjectstore/workspace-host.json`` is the only disk-root authority. It
must be owned by ``root``, must not be group/world writable, and must not be a
symlink. A minimal configuration is::

  {
    "schema_version": 1,
    "aggregate_root": "/srv/dasobjectstore/workspaces",
    "disks": {
      "qnap-1057": {
        "root": "/srv/dasobjectstore/hdd/qnap-1057",
        "workspace_directory": ".workspaces"
      }
    }
  }

Do not add a root until its identity and mount are managed by DASObjectStore.
Unknown disks and changed or symlinked roots fail closed.

Quota prerequisite
------------------

Every configured filesystem must support Linux project quotas and be mounted
with project-quota enforcement enabled. Provisioning assigns a globally unique
project identity and bounded per-branch share of the workspace quota. The
broker sets and verifies the project inheritance attribute, then applies the
hard byte limit with the operating-system quota interface. Unsupported or
inactive project quotas cause provisioning to fail and roll back newly created
empty branches.

The package installs the ``quota`` runtime dependency but deliberately does not
rewrite filesystem mount options. Filesystem quota enablement is an
administrator-controlled storage operation and must be completed before a disk
is entered in the broker configuration.

Markers, rollback, and recovery
-------------------------------

Each branch contains one root-created
``.dasobjectstore-workspace.json`` marker binding it to the exact workspace,
disk, branch, project identity, and quota. Replayed provisioning is accepted
only when the marker and quota state match exactly.

Inspection reports only bounded states such as ``absent``, ``ready``,
``marker_missing``, ``marker_conflict``, ``quota_missing``, or
``unsafe_filesystem_entry``. It never returns host paths.

Rollback removes only an exact marker-owned branch that contains no workspace
data. A non-empty branch, conflicting marker, symlink, quota ambiguity, or
unexpected filesystem entry is retained for operator review. Package removal
and service restart never authorize workspace-data deletion.

Closing and cleaning a workspace
--------------------------------

Closing freezes the mutable lifecycle but does not delete data. Closure is
refused until clients are detached, operations are terminal, and every retained
checkpoint has completed promotion evidence. Expiry is reported without
automatic application or deletion.

Cleanup is a separate audited operation requiring the exact confirmation
``CLEAN WORKSPACE <workspace-id>``. The daemon unmounts the managed aggregate;
the root broker then validates every registered branch and removes only exact
marker-owned regular directory trees. Symlinks, special files, foreign markers
and incomplete evidence stop cleanup. Capacity is released only after every
branch is proven absent.

Cancellation is safe before execution starts. After deletion may have begun,
the operation is retained for review rather than presenting a false rollback.
The path-redacted report at
``/var/lib/dasobjectstore/workspace-operations/cleanup-latest.json`` records
expiry candidates and recovery outcomes without exposing managed host paths.

Operator control surface
------------------------

Workspace lifecycle control is exposed through the typed daemon socket. The
CLI is a client of that contract and never opens ``live.sqlite`` or contacts
the privileged host broker directly.

Authenticated local users can list and inspect only workspaces whose
``owner`` matches their operating-system username::

  dasobjectstore workspace list
  dasobjectstore workspace inspect analysis-a --json
  dasobjectstore workspace cleanup-plan analysis-a --json
  dasobjectstore workspace expiry-report --json

Root, ``sudo`` members, and ``dasobjectstore-admin`` members can perform the
audited lifecycle mutations. Request identities must be stable across retries;
the CLI derives a request digest from the operation and those identities::

  sudo dasobjectstore workspace close analysis-a \
    --request-id close-analysis-a-20260726

  sudo dasobjectstore workspace expiry-apply analysis-a

  sudo dasobjectstore workspace cleanup-request analysis-a \
    --operation-id cleanup-analysis-a-20260726 \
    --request-id cleanup-request-analysis-a-20260726 \
    --confirmation "CLEAN WORKSPACE analysis-a"

  sudo dasobjectstore workspace cleanup-cancel analysis-a \
    --operation-id cleanup-analysis-a-20260726

``list``, ``inspect``, ``expiry-report``, and ``cleanup-plan`` are non-mutating
inspection operations. Closing does not delete data. Cleanup remains a
separate exact-confirmation operation, and cancellation is refused after the
broker has crossed the branch-release boundary.

The daemon derives the actor from Unix-socket peer credentials. Client JSON
cannot select an actor, claim administrator status, supply host paths, or
address the broker.

Privileged Linux acceptance
---------------------------

``deploy/acceptance/workspace-mergerfs-nfs.sh`` is an opt-in, read-only
acceptance check for an already provisioned synthetic workspace. It refuses
non-Linux hosts, non-root execution, non-``codex-`` workspace identities,
missing mergerfs process evidence, and missing root-squashed NFS export
evidence. It does not create, delete, mount, or unmount anything. This makes it
safe to run after the daemon's synthetic provisioning fixture without
weakening the normal host boundary.

Service inspection
------------------

The socket is enabled during package installation and starts the broker only
when the daemon connects::

  systemctl status dasobjectstore-workspace-host.socket
  systemctl status dasobjectstore-workspace-host.service
  journalctl -u dasobjectstore-workspace-host.service

Do not send hand-written requests to the socket. Durable workspace operations
and recovery checkpoints remain the daemon's orchestration authority.

Daemon provisioning and restart recovery
----------------------------------------

The daemon's bounded worker claims queued provision operations with a lease and
asks the broker to inspect every branch before changing the host. An absent
branch or a matching branch whose quota needs repair may be provisioned
idempotently. The workspace becomes ``ready`` only after a second inspection
proves the exact ownership marker and project quota on every branch.

After restart, expired operations are first classified by the durable operation
repository. Only explicitly idempotent operations or operations with a durable
resume checkpoint are replayed. An active lease is not stolen. Marker
conflicts, missing markers, symlinks, unexpected filesystem entries, ambiguous
cancellation, and exhausted retries remain ``needs_review`` and are not
deleted.

The latest path-redacted worker reconciliation report is written to::

  /var/lib/dasobjectstore/workspace-operations/recovery-latest.json

The report contains operation and workspace identities, bounded branch states,
aggregate health evidence, and the reason for completion, deferral, or review.
It never contains managed host paths. Inspect it alongside the daemon and
broker journals::

  sudo cat /var/lib/dasobjectstore/workspace-operations/recovery-latest.json
  journalctl -u dasobjectstored -u dasobjectstore-workspace-host.service

Rollback is attempted only for an exact marker-owned branch and the broker
will remove it only when the marker is its sole entry. Any branch containing
workspace data is retained for explicit operator review.

Managed aggregate namespace
---------------------------

After every branch is quota-ready, the daemon asks the broker to mount one
mergerfs namespace beneath the configured ``aggregate_root``. The package
creates ``/srv/dasobjectstore/workspaces`` as a root-owned namespace and
installs mergerfs as a runtime dependency.

The broker uses a fixed reviewed profile: ``category.create=mfs``, the
workspace minimum-free floor, ``inodecalc=path-hash``, ``cache.files=off``,
``dropcacheonclose=true``, and ``moveonenospc=mfs``. It assigns a distinct
``fsname`` for the workspace and accepts no caller-supplied option or path.

Readiness proves all of the following together:

* the exact configured branches are mounted in their planned order;
* every branch still has its exact ownership marker and project quota;
* the live process is mergerfs and targets the configured aggregate identity;
* its source set and reviewed options match the durable workspace plan.

If the process disappears, the broker may remount only when the hidden
mountpoint reveals the exact aggregate marker. A foreign mount, changed source
set, changed option, missing marker, symlink, or unavailable branch is retained
for review and prevents ``ready`` publication. Unmount similarly requires
complete identity proof and never removes files from workspace branches.
