Daemon garbage collection
=========================

``dasobjectstored`` starts a bounded, asynchronous garbage-collection pass on
every daemon start. Socket service is not delayed while the pass runs. The
collector first inventories candidates, then repeats every durability check
before reclaiming anything.

What the daemon may reclaim
---------------------------

The collector recognizes only daemon-owned namespaces:

* older completed remote-provider reconciliation snapshots that exactly match
  a newer retained checkpoint and whose objects have verified managed
  placements;
* terminal ingest staging after the retention grace, when catalogue and
  placement state prove the staging copy is no longer required; and
* performance-test directories carrying the versioned DASObjectStore ownership
  marker and a terminal state. ``--keep-temp`` remains authoritative.

Incomplete reconciliation manifests, active ingest jobs, the newest provider
checkpoint, legacy unmarked performance directories, unknown files, symlinks,
hard links, mount crossings, and any candidate with incomplete durability
evidence are retained. Age alone never authorizes deletion.

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
