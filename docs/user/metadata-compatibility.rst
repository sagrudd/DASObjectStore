Metadata Compatibility and Migration
=====================================

ObjectStore manifests are versioned compatibility contracts, not inferred
directory markers. The current manifest schema is version 1 and contains the
logical store identity, deployment profile, host mode, protection policy, and
backend identity.

Readers use a strict decode boundary:

* malformed JSON and missing or invalid ``schema_version`` values are rejected;
* unknown or future schema versions are rejected before profile/backend fields
  are interpreted;
* unknown fields in the v1 manifest or backend reference are rejected; and
* profile/backend identity mismatches are rejected.

Existing appliance metadata and legacy policy files are never rewritten or
reinterpreted as folder or drive manifests. A future migration must be an
explicit daemon-owned operation that backs up the prior metadata, inspects the
source identities and protection policy, writes the new manifest atomically,
and retains the prior copy until verification succeeds. Mount path hints are
local probe hints only; stable filesystem and device identities remain the
authority.

The decode helper does not itself perform migration, adoption, or filesystem
writes. Those operations remain gated by the profile adoption and catalogue
contracts in the campaign roadmap.

Migration provenance is deliberately stored outside manifest v1 and existing
appliance metadata. A daemon-owned, versioned sidecar record binds the migration
ID to the source and destination store IDs and manifest digests, destination
verification result and time, source-retention state, and the administrator and
time authorizing source retirement. The daemon must publish this record
atomically and reconcile it after restart. It may not report source retirement
complete until the durable provenance record says that retirement was
authorized.

Portable object catalogue companion
------------------------------------

Object versions and placements use a separate ``portable_object_catalogue.v1``
contract. It carries logical object/version identity, size and digest,
opaque provenance, lifecycle/protection state, and profile-neutral placement
locations for folder, drive, appliance, or provider backends. Folder and
drive locations are safe relative paths; the contract does not imply an SSD,
HDD, or replication layout.

The companion catalogue has its own strict schema boundary: unknown fields,
duplicate object versions or placements, unsafe paths, and future schema
versions are rejected before interpretation. It does not change the strict v1
ObjectStore manifest or make private folder/drive catalogue files authoritative;
daemon transaction wiring and profile adoption remain separate decisions.
Export adapters use the same validation boundary before emitting JSON, so an
export cannot contain an unsafe placement or duplicate logical version.
