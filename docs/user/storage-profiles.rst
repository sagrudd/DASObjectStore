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
