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
