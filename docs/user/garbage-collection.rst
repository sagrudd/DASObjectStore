Daemon garbage collection
=========================

``dasobjectstored`` starts a bounded, asynchronous garbage-collection pass on
every daemon start. Socket service is not delayed while the pass runs. The
collector first inventories candidates, then repeats every durability check
before reclaiming anything.

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
``Live Status`` response exposes only aggregated retained reasons, counts, and
bytes. A collection error fails closed, leaves uncertain data in place, and
appears as a Live Status warning.

Reclamation uses a same-filesystem quarantine rename followed by directory
synchronization. When SSD placement metadata must be changed, a failed metadata
update restores the quarantined directory before returning an error. Do not
manually remove staging trees: doing so bypasses catalogue and placement proofs
and may destroy the only durable copy.
