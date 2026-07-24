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

``verify source -> copy -> hash destination -> fsync -> metadata swap -> unlink source``

The metadata swap is one SQLite transaction. A crash before it leaves the
source authoritative. A crash after it may leave a redundant source file, but
never removes the newly verified placement. Such redundant files are retained
for daemon-owned garbage collection rather than guessed at during recovery.

If a scrub finds a checksum mismatch, DASObjectStore withdraws the placement's
verification, marks the object ``Degraded``, and promotes a ``Healthy`` or
``Watch`` disk to ``Suspect``. It does not copy, delete, or silently accept the
damaged bytes.

Status and configuration
------------------------

The latest durable result is written to:

``/var/lib/dasobjectstore/storage-assurance/latest.json``

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
