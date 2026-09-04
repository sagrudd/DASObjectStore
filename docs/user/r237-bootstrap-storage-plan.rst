r237 bootstrap-storage plan
===========================

Version 0.178.0 adds a narrow source-only assessment for the reviewed r237
NUC bootstrap-storage transaction. It is **not** ``service provision --dry-run``.
It has no daemon client, network, Garage, registry, credential, marker, job,
or apply path; it cannot contact a NUC or DGX and cannot change any state.

The command is deliberately fixed to this reviewed tuple:

* NUC host ``192.168.0.193``;
* logical store ``r237_s4_bootstrap_custody``;
* bucket ``dos-r237-s4-bootstrap-custody``;
* ``critical_metadata``, exactly three copies, and the non-human
  ``mnemosyne-r237-custody`` writer group;
* a 16 GiB namespace body limit, 256 MiB per-object limit, and the two
  content-addressed key prefixes ``corpus/sha256`` and ``receipts/sha256``.

It also binds the exact canonical Programme main merge and transaction-source
revision/document digest that were reviewed for this event. A different target
or namespace needs a separately reviewed interface; there are no command-line
overrides for these values.

Read-only inventory assessment
------------------------------

An attended read-only audit first produces a strict
``dasobjectstore.r237_bootstrap_plan_inventory.v1`` JSON inventory. The
inventory must contain only redacted identifiers and facts: the NUC identity
digest, complete store and bucket enumerations, and post-alias physical-HDD
facts. It must state that both namespace enumerations are complete. An unknown,
missing, duplicate, existing, unhealthy, degraded, unwritable, unmounted, or
SMART-warning fact is a denial rather than a reason to inspect or modify the
host.

On the NUC's Linux environment, assess that inventory with:

.. code-block:: console

   dasobjectstore store r237-bootstrap-plan --inventory /path/to/redacted-inventory.json

The command uses ``O_NOATIME`` and ``O_NOFOLLOW`` and refuses to fall back to
an ordinary read. It therefore fails closed on platforms that cannot provide
those protections. The inventory file must be regular and no larger than 1 MiB.
The file path is never emitted in output.

The JSON result has ``ready: true`` only when the requested store and bucket
are proven absent and at least three distinct physical HDD members are mounted,
writable, non-degraded, SMART-passed, and each have at least 24 GiB free. Its
plan contains only the count and a JCS SHA-256 digest of selected member IDs,
not disk identifiers, paths, topology, credentials, marker location, or input
file details. ``plan_sha256`` is self-excluding: it is the JCS SHA-256 of the
plan payload only.

Boundaries
----------

``ready: true`` is **not** approval to provision. The result is explicitly
``non_worm_bootstrap_only: true`` and
``later_guarded_apply_compatible: false``. It cannot provide WORM, custody,
retention, S4, S5--S8, release, package, remote-client installation, service
activation, or deployment evidence. In particular, ``critical_metadata`` and
Garage replication do not prove a placement on the exact physical HDD members
selected by the inventory.

Do not turn this result into a guarded provision input. A future provisioner
must first be separately designed and reviewed to prove target, physical-media
placement, guard, and transaction binding at mutation time.
