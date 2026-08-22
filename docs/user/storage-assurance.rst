Background storage assurance
============================

``dasobjectstored`` continuously protects settled appliance data when the
server is otherwise idle. This is deliberately a low-priority, one-object-at-a-
time service rather than a bulk shuffle.

The service waits for ten uninterrupted minutes with:

* no active or queued ingest/destage work;
* no live ingest connection;
* no startup garbage collection; and
* less than 1 MiB/s of aggregate host block-device IO.

After that quiet period it performs exactly one action, then returns to the
idle gate. Primary ingress always has precedence. If ingest becomes active
during a relocation, the temporary destination is discarded and the
authoritative source placement remains untouched.

At daemon startup, structurally verified capacity reservations belonging to a
stopped direct-ingest worker are released before assurance begins. This keeps
abandoned reservations from suppressing a valid rebalance. Durable destage,
workspace, repair, evacuation, and unfamiliar claims remain authoritative and
are not cleared by that recovery pass.

Action order
------------

The daemon chooses work in this order:

1. evacuate one verified placement from a disk marked ``Draining`` or
   ``Suspect``;
2. rebalance one placement from the fractionally fullest disk to the
   fractionally freest eligible disk when their free-space difference is at
   least five percentage points;
3. re-hash the oldest placement whose verification is at least 30 days old.

Destinations must be ``Healthy`` or ``Watch``, contain no other copy of the
same object, and have room for the complete file. Fractional free space is
compared exactly; filesystem discovery order and absolute free bytes do not
override the policy.

Relocation safety
-----------------

A relocation is ordered as:

``journal -> claim -> verify source -> copy -> hash destination -> fsync ->
metadata swap -> unlink source -> clear journal``

The operation identity and phase are persisted atomically with mode ``0600``.
On restart, the daemon independently verifies the placement metadata, capacity
claim, destination file, and checksum before continuing. A crash before the
metadata swap leaves the source authoritative. A crash after it may leave a
redundant source file; recovery removes that source only after proving the
verified destination is authoritative. Changed or ambiguous evidence fails
closed and retains both data and the journal for inspection.

Hashing and copying check for primary ingest, destage, and garbage-collection
work between bounded chunks. Preemption releases the capacity claim and leaves
the source placement unchanged. Rebalancing searches the ordered set of
feasible source/destination pairs, so an ineligible freest disk does not hide a
valid second choice. Object size is not used to silently exclude evacuation or
reverification.

When the last placement leaves a disk already marked ``Draining``, one SQLite
transaction proves the disk empty and changes it to ``Retired``. A disk with
any remaining placement stays draining and is reported as blocked; operators
must not take it offline.

If a scrub finds a checksum mismatch, DASObjectStore withdraws the placement's
verification, marks the object ``Degraded``, and promotes a ``Healthy`` or
``Watch`` disk to ``Suspect``. It does not copy, delete, or silently accept the
damaged bytes.

Status and configuration
------------------------

The latest durable result is written to:

``/var/lib/dasobjectstore/storage-assurance/latest.json``

An interrupted relocation checkpoint is held at
``/var/lib/dasobjectstore/storage-assurance/operation.json``. Do not edit or
remove it manually; daemon restart recovery owns it.

Defaults are enabled for packaged Linux appliances. Operators may override
them through the service environment:

* ``DASOBJECTSTORE_ASSURANCE_ENABLED``
* ``DASOBJECTSTORE_ASSURANCE_POLL_SECONDS``
* ``DASOBJECTSTORE_ASSURANCE_IDLE_GRACE_SECONDS``
* ``DASOBJECTSTORE_ASSURANCE_VERIFY_AFTER_SECONDS``
* ``DASOBJECTSTORE_ASSURANCE_IMBALANCE_BASIS_POINTS``
* ``DASOBJECTSTORE_ASSURANCE_MAX_OBJECT_BYTES``
* ``DASOBJECTSTORE_ASSURANCE_IDLE_IO_BYTES_PER_SECOND``

Disabling the service stops new background work; it does not alter placements.
Disk drain, repair, and force-retirement safety rules remain authoritative.

Garage S3 storage is separate
-----------------------------

The assurance loop governs verified DASObjectStore placements recorded in
``live.sqlite``. Garage's S3 block tree is a separate provider storage plane;
it is not retrospectively moved by the assurance loop.

Garage 2.3 can place new blocks natively across several HDD directories. Use
the same explicit directory specifications when rendering its Compose mounts
and its secret-free ``data_dir`` TOML stanza. A legacy path can be retained
read-only while new writes are distributed across managed member disks:

.. code-block:: console

   dasobjectstore service render-garage-data-config \
     --garage-data-directory \
       /srv/dasobjectstore/hdd/garage=/var/lib/garage/data-legacy=read-only \
     --garage-data-directory \
       /srv/dasobjectstore/hdd/qnap-1057/garage=/var/lib/garage/data/qnap-1057=4T \
     --garage-data-directory \
       /srv/dasobjectstore/hdd/qnap-1058/garage=/var/lib/garage/data/qnap-1058=4T

Repeat the writable specification for every reviewed managed HDD and pass the
identical specifications to ``service render-compose``. The command rejects
relative paths, duplicates, invalid capacities, and layouts with no writable
directory. Garage capacity values are placement weights, not filesystem quotas;
review actual free space and DASObjectStore's own reservations before applying
the configuration.

Changing the directory list does not move existing blocks. Keeping the legacy
directory read-only is the source-preserving transition: Garage continues to
read its blocks there and sends new blocks only to writable member disks. An
active ``garage repair rebalance`` may later evacuate legacy blocks, but it is
a separate, monitored operation. Do not launch it during package installation,
remove the legacy path, or delete any residual files without a completed repair
and explicit operator approval.
