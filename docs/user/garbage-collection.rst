Daemon garbage collection
=========================

``dasobjectstored`` starts a bounded, asynchronous garbage-collection pass on
every daemon start and repeats it hourly through ``disk_housekeeping``. Socket
service is not delayed while a pass runs. The collector first inventories
candidates, then repeats every durability check before reclaiming anything.

What the daemon may reclaim
---------------------------

The collector recognizes only daemon-owned namespaces:

* every completed remote-provider reconciliation snapshot whose objects have
  independently verified managed placements;
* terminal ingest staging after the retention grace, when catalogue and
  placement state prove the staging copy is no longer required; and
* performance-test directories carrying the versioned DASObjectStore ownership
  marker and a terminal state. ``--keep-temp`` remains authoritative.

Incomplete reconciliation manifests, active ingest jobs, legacy unmarked
performance directories, unknown files, symlinks, hard links, mount crossings,
and any candidate with incomplete durability evidence are retained. Age alone
never authorizes deletion.

The daemon also runs this reconciliation cleanup immediately before and after
each S3 reconciliation. If completed non-resumable staging cannot be proven
safe and reclaimed, the route hard-fails before another snapshot can compound
the retained data. A successful reconciliation must leave no completed source
snapshot behind; the managed SSD placement and its durable HDD destage job are
the only permitted transient SSD state.

Evidence and visibility
-----------------------

The latest general and reconciliation reports are written below
``<state_dir>/garbage-collection/``. Reports use managed relative paths; the Web
``Live Status`` response exposes the versioned
``dasobjectstore.staging_inventory.v1`` projection. It groups path-free counts
and bytes by managed staging kind, disposition, and typed retention reason.
The daemon refreshes this read-only inventory every 30 seconds.

This staging inventory is distinct from the native SSD-residency report at
``<state_dir>/disk_housekeeping/ssd-residency.json``. The latter reconciles
physical native payloads with SSD placement and HDD-destage evidence, so it can
separate safely landed-but-uncleared bytes from bytes still awaiting HDD and
from orphaned or unexplained data. Neither report authorizes manual deletion.

The inventory covers ingest jobs, performance fixtures, reconciliation
checkpoints, direct-S3 uploads and multipart work, registered folder staging,
and interrupted garbage-collection quarantine. ``observed_bytes`` always
equals ``accounted_bytes + unaccounted_bytes``. A scan limit, unsafe entry,
unreadable root, or unsupported journal changes coverage to ``partial`` or
``unavailable`` instead of silently presenting a complete total. The Web Live
Status page renders these results under **Staging & attention**.

A collection error fails closed, leaves uncertain data in place, and appears
as a Live Status warning. Inventory is evidence, not deletion authority.

Reclamation uses a same-filesystem quarantine rename followed by directory
synchronization. When SSD placement metadata must be changed, a failed metadata
update restores the quarantined directory before returning an error. Do not
manually remove staging trees: doing so bypasses catalogue and placement proofs
and may destroy the only durable copy.
