ADR-0004 review fixtures
========================

These JSON files are deterministic, non-secret review vectors for the Proposed
``dasobjectstore.object_ref.v1`` and ``dasobjectstore.evidence_ref.v1``
contracts.  They are not issued object references, do not prove that an object
exists, and do not grant read or write authority.  The maximum-safe-integer
ObjectRef vector exercises ``9007199254740991`` at both integer fields.

Run the dependency-free verifier from the repository root::

   python3 tools/verify-object-reference-fixtures.py

The verifier checks the exact field sets, bounded identifier and digest
grammar, canonical JSON bytes, domain-separated digest construction, duplicate
member rejection, unknown-member rejection, path-shaped values, and digest
drift.  It deliberately has no network, storage, capability, or authority
side effects.

The vectors remain review material until DASObjectStore issue #31 accepts the
ADR, owner-side implementation, and resolution/authorization gates.  Consumers
must not pin these files as a production revision or synthesize references from
their fields.
