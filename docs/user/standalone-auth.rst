Standalone Authentication
=========================

DASObjectStore standalone appliances use OS-local identity as the administrator
authority.

An OS user with sudo rights is an appliance administrator. Store writer groups,
such as ``mnemosyne`` or a future store-specific group, authorize ordinary users
to submit daemon jobs for allowed stores. These groups do not give users direct
write access to managed DAS disks.

The Web UI may still use a product-local browser session while standalone UI
support is being built. That session is not the source of administrator
authority. For administrator workflows, DASObjectStore will use OS-local
identity and sudo-derived administrator status, then submit the action to
``dasobjectstored``.

Storage mutation must still go through the daemon:

* disk preparation, drain, replacement, and retirement;
* ObjectStore and SubObject creation or policy changes;
* service start, stop, and configuration;
* ingest, destage, repair, and migration jobs.

Synoptikon-integrated deployments are different: Synoptikon supplies account,
entitlement, audit, correlation, and governance-domain context.
