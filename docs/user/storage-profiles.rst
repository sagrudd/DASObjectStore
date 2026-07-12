Storage Profiles and Host Modes
===============================

DASObjectStore is being extended beyond appliance-only storage. A deployment
profile describes the backend boundary of one ObjectStore:

* ``folder`` exposes one explicitly bounded directory;
* ``drive`` exposes one validated non-rotational SSD mount; and
* ``appliance`` uses the managed SSD/HDD placement model.

Host mode is a separate axis. ``per_user`` is intended for a user-owned local
service, ``system`` for a package-managed service, and ``integrated`` for a
host product such as Mnemosyne or Synoptikon. Existing store metadata is not
silently rewritten when these contracts evolve; profile creation, adoption,
capacity limits, and migration rules remain gated campaign work.

All profiles retain daemon-owned catalogue and ingress authority. A folder or
drive is not a license for direct writes that bypass manifests, checksums,
quota reservations, or durable finalization. The folder profile must be size
bounded, and all profiles will eventually expose the same logical capacity and
admission semantics through CLI, Web, S3, and product adapters.

The shared policy contract now carries a logical limit, backend reserve, and
warning/critical thresholds. Its reservation ledger admits bytes transactionally
before an upload and commits or releases the reservation after durable
finalization. Existing appliance policies remain explicitly unbounded until an
operator performs a compatibility-aware capacity migration.

Portable manifests identify a profile with a versioned backend reference:
folder manifests store a canonical root identity, drive manifests store stable
filesystem/device identities plus an optional mount hint, and appliance
manifests reference a pool. A path hint is never treated as the backend
identity, and an explicit migration/adoption step is required before writing a
manifest for an existing store.

Protection is independent of the profile: manifests may require ``local_only``,
``reproducible``, ``externally_replicated``, or ``appliance_protected`` policy.
The physical backend does not silently choose a protection promise; product
adapters must request and validate one explicitly.

All profiles implement the same backend capability boundary: validation,
reservation, staging, durable finalization, reads, enumeration, verification,
health, reconciliation, and removal. A profile may report an unsupported
capability explicitly; callers must not assume appliance-only placement methods
exist for folder or drive stores.
