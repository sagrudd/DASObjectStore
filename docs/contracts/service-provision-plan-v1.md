# DASObjectStore service-provision plan v1 (proposal)

Schema coordinate: ``dasobjectstore.service-provision-plan.v1``.

This is a proposal for a future read-only export of the normal Garage
provisioner's *catalogue-derived intent*. It is not implemented by the current
daemon or CLI. The current ``dasobjectstore service provision --dry-run``
returns aggregate counts only; it does not enumerate the planned resources or
observe Garage. This contract does not change that behavior.

The normative schema is
``docs/schemas/dasobjectstore.service-provision-plan.v1.schema.json``. The
paired JSON is a synthetic shape fixture only:
``docs/contracts/service-provision-plan-v1.fixture.json``. Its identifiers,
digests, timestamp, and all-zero source revision are placeholders, not live
provenance and not an approved AlleleAnchor store-to-bucket mapping.

## Meaning and limits

The export would describe the complete set of normal S3-exported store
definitions read from one local DASObjectStore registry snapshot and the
idempotent resource intents derived for each store. It would not report whether
Garage already has a key, bucket, or grant. Accordingly v1 requires
``provider_visibility: "unknown"`` and ``provider_observation: null``. These
are fixed values, not defaults. A producer or consumer must reject any v1
document that claims an observed provider state or supplies provider
observation fields.

``source_snapshot.complete`` means only that every record in the bound local
store-registry byte snapshot was accounted for. The digest is SHA-256 over the
exact bytes read, before parsing. Store IDs preserve their source spelling,
including case, in both the row and the raw credential-reference path segment.
The bounded v1 row grammar accepts 1–128 ASCII letters, digits, periods,
underscores, or hyphens, beginning with a letter or digit. If an eligible
source record has an ID outside that grammar, a producer must fail the whole
export rather than omit the record or claim a complete snapshot. Bucket names
follow the current planner’s 3–63 character lowercase letter, digit, and
hyphen rules. ``record_count`` counts all validated normal
registry records, ``eligible_store_count`` counts S3-exported records included
in ``stores``, and ``excluded_store_count`` counts the remaining validated
records. Consumers must require:

* ``record_count = eligible_store_count + excluded_store_count``;
* ``stores.length = eligible_store_count``;
* every eligible store ID and bucket name occurs exactly once;
* every unresolved credential reference is the deterministic DAS slot for its
  enclosing store ID;
* ``resource_action_count`` equals the total number of actions in all store
  rows; and
* each store has exactly the three ordered actions in the schema.

The source digest and producer revision bind the input and producer identity;
they do not prove a live host, current registry beyond the capture instant,
Garage availability, or provider state. A consumer must fail closed on missing
or inconsistent provenance, incomplete snapshots, duplicate mappings,
unsupported schema versions, or unknown fields. V1 is descriptive and
non-executable: ``execution_authorized`` is always ``false`` and action rows
contain neither access-key IDs nor secret material. The ``key_name`` is
Garage's logical ``-n`` label for importing a key. It is not the ``--key``
operand used by ``allow_bucket``; that command receives the credential's
access-key ID. Because v1 reads only the store registry and does not resolve
the credential registry, both key import credentials and the grant's
access-key operand are represented by a credential binding whose status is
``unresolved``. Its reference identifies the intended DAS credential slot; it
does not assert that a record exists or reveal the Garage operand. Consumers
must not turn this descriptive row into a Garage command.

The logical actions preserve the current planner's ordering and intent:
import the store key, ensure the bucket exists, then allow the credential
identified by the unresolved binding read/write/owner access to that bucket.
``ensure`` semantics describe an idempotent requested operation; they do not
assert creation, absence, or success. The contract intentionally omits command
argv because key import requires credentials and the grant command's actual
``--key`` operand is unavailable without resolving the credential registry.

## Gates before implementation or AlleleAnchor use

This proposal alone does not satisfy the Outcome 4 per-bucket diff gate. Before
implementation, the product owner must confirm the authoritative mapping for
all twelve stores and buckets and approve which normal registry records are in
scope. An implementation must derive rows from one bound complete snapshot,
redact credential material, expose the fixed ``unknown`` provider visibility
until a supported Garage observer exists, and include focused tests for
duplicates, omissions, digest/count mismatch, schema-version mismatch, and
secret leakage. It must not call Garage during plan export.

Any future provider observation requires a separately specified, independently
bound observation contract with its own producer, endpoint/TLS identity,
capture time, completeness, and freshness rules. It cannot be inferred from a
catalogue plan, dry-run counts, an unavailable service, or this v1 schema. No
live store mapping, provider observation, bucket access, credential read, or
data transfer is represented by the fixture.
