# Standalone Authentication Decision

Status: Draft  
Scope: standalone appliance host mode and administrator authority

## Decision

Standalone DASObjectStore appliances SHALL use OS-local identity as the
administrator authority.

An OS-local user with sudo rights is a DASObjectStore administrator. OS-local
group membership, such as membership in `mnemosyne` or a store-specific writer
group, authorizes daemon job submission for ordinary storage work. Neither sudo
membership nor writer-group membership grants direct write access to managed DAS
roots; all storage mutation still goes through `dasobjectstored`.

The current product-local auth store remains a transitional Web session layer
for standalone development and early appliance UI work. It is not the durable
administrator authority for standalone appliances. Before broader standalone
administrator workflows are exposed, the Web/API layer should discover the
current OS user, map sudo-derived administrator status, expose current-user
metadata, and pass that actor context to the daemon.

## Rationale

DASObjectStore is an appliance that manages disks, mounts, services, and
long-running storage jobs on a host. Binding administrator authority to the host
OS keeps package behavior, sudo policy, service ownership, and daemon
authorization aligned. It also avoids creating a second local administrator
database that can drift from the host's real privilege model.

Product-local sessions are still useful as browser sessions, but they must not
be confused with root-equivalent authority. A browser login proves access to the
standalone UI. Sudo-derived OS status proves appliance administration.

## Boundary

- `dasobjectstored` remains the final storage authorization point.
- The standalone Axum API and Yew UI are clients of the daemon for all
  storage-mutating work.
- OS-local sudo status authorizes administrator workflows such as store
  management, disk preparation, disk retirement, and service administration.
- OS-local writer groups authorize store write job submission.
- Product-local sessions may gate browser access while OS-local actor discovery
  is implemented, but they do not supersede OS-local authority.
- Synoptikon-integrated mode is unchanged: Synoptikon remains authoritative for
  account, entitlement, audit, correlation, and governance-domain context.

## Implementation Implications

Next implementation slices should add:

- local-user discovery for the standalone API;
- sudo-rights administrator detection;
- current-user metadata in protected API responses;
- tests for administrator and non-administrator OS-local actors;
- clear permission-denied responses when an authenticated browser session lacks
  OS-local authority for an administrator action.
