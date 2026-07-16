Repairing ObjectStore metadata
==============================

``store repair`` verifies the relationship between managed HDD payloads and
the live SQLite metadata index. It is intentionally read-only by default:

.. code-block:: console

   dasobjectstore store repair
   dasobjectstore store repair xenognostikon --json

The report includes payload counts, recovered object counts, duplicate/partial
payloads omitted by size selection, and the metadata path. A repair does not
claim cryptographic verification; recovered objects remain in a settling state
until their content hashes are validated.

To rebuild the index after an interrupted or historically incomplete ingest,
use the explicit administrator confirmation phrase:

.. code-block:: console

   dasobjectstore store repair --apply \
     --confirm "confirm store repair"

For appliance metadata, an apply rebuilds the complete registered store set.
A store identifier may be provided for a read-only appliance report, but
filtered appliance ``--apply`` is rejected so repair cannot accidentally
replace metadata for other stores. Registered bounded-folder profiles use the
targeted semantics below.

The daemon owns this mutation. It creates and integrity-checks a replacement
SQLite database, preserves the previous database as a timestamped
``live.sqlite.pre-repair-*`` backup, and atomically installs the replacement.

For a registered bounded-folder profile, a targeted repair has narrower and
safer semantics. ``store repair STORE`` compares the authoritative private
profile catalogue with shared SQLite and reports drift without mutation.
``store repair STORE --apply --confirm "confirm store repair"`` republishes
the exact private catalogue through the crash-safe handoff. It does not scan
user files, rebuild appliance placements, or expose the backend root. Profile
repair cannot be combined with ``--reconcile-s3``.

Recover uncatalogued Garage uploads
-----------------------------------

An S3-compatible bucket is not the ObjectStore catalogue.  A successful
direct Garage upload is not accepted by DASObjectStore until the daemon has
copied it through the SSD-first ingest pipeline, calculated checksums while
copying, settled the configured HDD copies, and committed live metadata.

If objects are known to exist in a provisioned Garage bucket but are absent
from the browser or ``store contents``, use the guarded repair mode for that
one store:

.. code-block:: console

   sudo dasobjectstore store repair alleleanchor_mvp --reconcile-s3 --apply \
     --confirm "confirm store repair"

Use ``--s3-prefix variants/chm13/v2.0/chr20`` to recover a bounded prefix.
Without ``--apply`` the command only reports the private SSD staging location
and does not contact Garage.  The command uses the daemon's provisioned
credentials; do not copy access keys into a shell command.  It never creates
catalogue rows solely from a bucket listing and it never deletes bucket data.
Payload files are never moved or deleted. On a successful apply, normal ingest
registers the recovered objects atomically; the daemon deliberately does not
run a filtered live-index rebuild afterwards, because that rebuild could
replace unrelated catalogue state. Export/protection should remain disabled
for recovered entries until a subsequent hash-verification workflow has
completed.

Each apply records a durable per-key manifest in the private SSD staging tree.
Administrator cancellation is checked between provider transfers and leaves
the in-progress checkpoint intact. A later invocation does not yet discover
older job manifests automatically or continue a partial ``aws s3 cp`` at byte
range level; retain the staging tree and use the recorded job evidence until
stable rediscovery and true byte-level resume are delivered.

If a repair reports partial duplicates, keep the source media and the payload
files intact and investigate the corresponding ingest job before retrying.

Acceptance path for a recovered upload
---------------------------------------

After a successful reconciliation, prove the catalogue and browser handoff in
this order before treating the upload as product-ready:

.. code-block:: console

   dasobjectstore store contents alleleanchor_mvp --json
   dasobjectstore store verify alleleanchor_mvp --hash --json

The contents response must contain the recovered relative key, and verification
must report the recorded checksum without an orphan or size-mismatch warning.
Then use the authenticated standalone Web browser to refresh that
ObjectStore, open the recovered folder, and download one recovered object from
the ObjectBrowser download action. The download response must be served by the
daemon-authorized endpoint and match the verified checksum; never select a
filesystem path or a provider URL in the browser. If the object is still
SSD-only or lacks a verified settled copy, the daemon must return an unavailable
state and the operator should wait for settlement/repair rather than bypassing
the authority boundary.

Verification and checksum cleanup
---------------------------------

Use the read-only health check to find missing payloads, orphan payloads,
size/hash mismatches, and duplicate placement rows:

.. code-block:: console

   dasobjectstore store verify xenognostikon
   dasobjectstore store verify xenognostikon --hash --json

``--hash`` reads each landed payload and compares its SHA-256 checksum with
metadata. To record those checksums and remove only checksum-identical
placement rows on the same disk, run a dry run first and then confirm the
explicit metadata-only cleanup:

.. code-block:: console

   dasobjectstore store deduplicate xenognostikon --json
   dasobjectstore store deduplicate xenognostikon --apply \
     --confirm "confirm store deduplicate"

Deduplication never deletes payload files. Any removed metadata row is therefore
reported as an orphan on the next verification until an operator separately
reviews the physical file.
