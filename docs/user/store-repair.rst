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

An apply rebuilds the complete registered store set. A store identifier may be
provided for a read-only report, but filtered ``--apply`` is rejected so that
repair cannot accidentally replace metadata for other stores.

The daemon owns this mutation. It creates and integrity-checks a replacement
SQLite database, preserves the previous database as a timestamped
``live.sqlite.pre-repair-*`` backup, and atomically installs the replacement.

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
Payload files are never moved or deleted. Export/protection should remain
disabled for recovered entries until a subsequent hash-verification workflow
has completed.

If a repair reports partial duplicates, keep the source media and the payload
files intact and investigate the corresponding ingest job before retrying.

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
