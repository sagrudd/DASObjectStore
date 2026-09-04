r237 bootstrap observer
=======================

Version 0.178.0 contains a deliberately narrow, source-only local observer
for the reviewed r237 NUC bootstrap-storage transaction. The observer is
**not** a dry run, planner, installer, or approval. It accepts no arguments,
input files, target overrides, credentials, or configuration. It is a separate
binary, not a ``dasobjectstore store`` subcommand:

.. code-block:: console

   dasobjectstore-r237-bootstrap-observer

It is intended only for a separately authorised, attended local-root read on
the NUC. This release does not authorise running it on any host. It always
returns a non-zero status and a redacted, JCS-canonical JSON report with
``disposition: "denied"``. There is deliberately no ``ready`` or ``eligible``
state and no operation that can consume its report.

Reviewed binding
----------------

The binary contains no generic bootstrap inputs. Its report is fixed to the
following reviewed event and identifies the proofs accurately:

* target NUC IP ``192.168.0.193``;
* store ``r237_s4_bootstrap_custody`` and bucket
  ``dos-r237-s4-bootstrap-custody``;
* ``critical_metadata`` with key prefixes ``corpus/sha256`` and
  ``receipts/sha256``, 256 MiB per object, and writer group
  ``mnemosyne-r237-custody``;
* the non-WORM one-use marker-root purpose, recorded as a purpose only and
  never as a filesystem path;
* canonical Programme-main merge ``ab4c7319ad398621052643a0eef07551f7ba969f``;
* transaction document source revision
  ``34b44650b22606f1dcc9fc7383d847513c670805`` and its SHA-256;
* the reviewed capacity threshold: 16 GiB allocation plus 24 GiB residual,
  so each of three distinct healthy HDDs must have at least 40 GiB available.

The source revision is labelled as the document source only. It is not used as
a substitute for the canonical-main merge prerequisite.

Read-only evidence boundary
---------------------------

On Linux, the observer independently obtains only bounded local evidence: the
configured target IP, ``machine-id``, DAS appliance identity, store-registry
collision state, marker-root state, local-files-only NSS group state, and
physical disk facts. Protected files use descriptor-relative
``O_NOATIME | O_NOFOLLOW`` reads; marker inspection uses ``openat`` plus
``fstatat``. File, directory, FIFO, device, symlink, replacement, malformed,
or missing evidence fails closed. Disk facts require a WWN and serial,
rotational physical disk, writable verified mount mapping, ``statvfs``
capacity, and a read-only ``smartctl --json --health --attributes`` result.

The binary does not contact a NUC or DGX remotely, Garage, Docker, S3, a DAS
daemon, or a service socket. It does not create or change a registry, ACL,
credential, admin job, intent, marker, file, service, mount, or process.

Deliberate denial
-----------------

Current authoritative interfaces cannot safely prove either a complete Garage
bucket inventory or that a later Garage provision will bind placement to the
three exact physical HDDs. The observer therefore reports both as
``unavailable`` and is denied even when its local observations succeed. It
does not infer those facts from ``critical_metadata``, replication settings,
Docker, S3, or a partial registry.

Every report explicitly says ``not_s4``, ``not_custody_acceptance``,
``not_remote_deployment``, and ``not_service_activation``. It is non-WORM and
cannot provide custody, retention, S4--S8, package, installation, deployment,
or service-activation evidence. A future guarded provisioner needs a separate
reviewed authority contract for complete Garage inventory, physical placement,
one-use approval, and transaction-time target binding.

Digest and retention
--------------------

The observer emits canonical JCS JSON. ``report_sha256`` is self-excluding: it
is the SHA-256 of the report body, not the outer wrapper. The fixed reviewed
tuple is included with its own JCS digest. The report retains only local
evidence digests; it never writes raw local evidence or emits disk paths,
serials, mount points, account data, registry contents, credentials, or marker
paths. Durable receipt retention is outside this source-only release.
