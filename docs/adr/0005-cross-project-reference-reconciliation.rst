ADR 0005: Cross-project ObjectRef and EvidenceRef reconciliation
=================================================================

:Status: Proposed owner reconciliation
:Date: 2026-07-31
:Deciders: DASObjectStore owner, Oikodome owner, Phoreus Registry owner, security reviewer, protocol reviewer, persistence reviewer
:Related issue: `DASObjectStore #31 <https://github.com/sagrudd/DASObjectStore/issues/31>`_
:Related Oikodome issues: `#53 <https://github.com/sagrudd/oikodome/issues/53>`_, `#61 <https://github.com/sagrudd/oikodome/issues/61>`_
:Related Registry issue: `Phoreus Registry #27 <https://github.com/sagrudd/phoreus-registry/issues/27>`_

Purpose and status
------------------

This record reconciles the Proposed DASObjectStore ADR-0004 reference
contract with the first consumer-side seams in Oikodome and Phoreus Registry.
It is a documentation and review artifact only.  It does not make ADR-0004
accepted, issue a reference, add a transport, change a Registry schema,
create a storage authority, or promote a package revision to production.

The reconciliation is intentionally exact about the revisions reviewed on
2026-07-31:

.. list-table:: Reviewed revisions
   :header-rows: 1

   * - Repository and change
     - Revision
     - Review state
   * - DASObjectStore PR #32
     - ``4ba3bf5cd3a1e7224709fcb021c1bce5560c4b43``
     - Merged to ``main``; ADR-0004 remains Proposed.
   * - Oikodome PR #68
     - ``bd0c8d6d9cc395fb74fb72089b5c4775d31159cc``
     - Merged to ``main``; provider-neutral foundation and DAS settlement port.
   * - Oikodome PR #69
     - ``43c62414dfd34889641b835fff63fc516597a56c``
     - Draft at review time; receipt replay validation is not a permanent dependency.
   * - Phoreus Registry PR #60
     - ``cb72124595f9ce5270d7bc1773454a337365e3ce``
     - Merged to ``main``; PRG-T10 evidence seam remains planning-only.

If Oikodome #69 is subsequently merged, its merge commit must be recorded in
the consuming Jenkins qualification evidence.  A draft head, branch name, or
local checkout is never an acceptable permanent revision.

Reference-vector binding
------------------------

The dependency-free vectors in ``docs/adr/fixtures/`` remain the only bytes
used for this reconciliation.  They bind to the consumer seams as follows:

.. list-table:: Canonical vector consumers
   :header-rows: 1

   * - Vector or seam
     - Required binding
     - Boundary that remains unchanged
   * - ``object-ref-v1.json``
     - Oikodome ``DasObjectStorePort`` must decode the exact owner-defined
       ``ObjectRefV1`` and use it as input to immutable-object resolution.
     - No local path, URL, bearer capability, or provider location is accepted.
   * - ``object-ref-v1-max-safe-integer.json``
     - The Oikodome adapter and any Registry-facing adapter must preserve
       ``9007199254740991`` without floating-point conversion or rounding.
     - Values above the JCS safe-integer bound fail before storage lookup.
   * - ``evidence-ref-v1.json``
     - Oikodome ``settle_das_output`` must retain the nested ObjectRef and
       verify the evidence kind, subject digest, revision, and outer digest.
     - An EvidenceRef is evidence identity, not approval, a signature, or a
       read capability.
   * - Registry PRG-T10 fixture from PR #60
     - ``object_ref_shape`` and ``evidence_ref_shape`` remain opaque,
       provider-neutral observations until the DAS owner contract is accepted.
       The fixture's settlement/read-back, digest, signature, Pistis, and
       Jenkins columns are pre-admission observations.
     - Registry does not dereference objects, validate storage credentials,
       or add those observations to its closed publication request schema.

The Registry ``opaque-token`` shape must not be interpreted as permission to
synthesize an ObjectRef from a string.  A future adapter may carry a canonical
reference as an opaque value only after strict owner decoding and authorization
have succeeded; no consumer may copy the fixture fields into a lookalike
reference.

Required acceptance gates
-------------------------

No gate below is satisfied merely by the existence of a fixture or a merged
documentation PR.  Each owner must retain machine-readable evidence against a
permanent revision.

.. list-table:: Owner gates
   :widths: 19 19 42 20
   :header-rows: 1

   * - Gate
     - Owner
     - Required evidence
     - Fail-closed result
   * - Grammar and canonical bytes
     - DASObjectStore
     - Strict duplicate-aware decoding (including escaped names), unknown
       member rejection, RFC 8785 bytes, digest vectors, and every numeric /
       lexical negative case in ADR-0004.
     - ``unsupported_schema`` or validation error before scope or existence
       lookup.
   * - Issuance and persistence
     - DASObjectStore
     - Immutable put atomically persists the catalogue identity, payload
       metadata, and caller-operation binding before issuing ObjectRef;
       replay, conflict, tombstone, and restart tests pass.
     - No ObjectRef or EvidenceRef is issued on ambiguous or partial commit.
   * - Authenticated transport
     - DASObjectStore and Oikodome adapter
     - The eventual adapter uses an authenticated, bounded transport and
       transmits metadata-only references.  Transport fields are not part of
       canonical identity and are never normalized into it.
     - Reject malformed, unauthenticated, stale, or scope-inconsistent input;
       disclose no path, credential, or existence detail.
   * - Scope authorization and resolution
     - DASObjectStore
     - Current caller grant and installation/Site Trust Domain/tenant/project
       scope are checked before lookup and rechecked immediately before replay
       or new issuance.  Object and evidence resolution is restart-tested.
     - Deny without catalogue/provider lookup where possible; never issue a
       reference or disclose existence after revocation or scope mismatch.
   * - Independent read-back and settlement
     - DASObjectStore adapter and Oikodome
     - Read-back independently obtains immutable object version, byte count,
       content digest, and settlement state.  Oikodome compares exact scope,
       version, size, digest, evidence kind, and subject digest.
     - Return an unsettled/manual-recovery result; do not advance a queue,
       attempt, or workflow on mismatch, outage, or ambiguous acknowledgement.
   * - Evidence issuance authority
     - DASObjectStore with Pistis/Forge consumers
     - Exact evidence-kind issuer authority is checked before object lookup;
       required subject/content/signature validation and revocation tests pass.
     - Ordinary object writers cannot issue EvidenceRef; no evidence is issued
       on failed validation or changed authority.
   * - Publication boundary
     - Phoreus Registry
     - PRG-T10 rows remain planning-only until the DAS contract, Forge dossier,
       external Pistis approval, and qualified Jenkins evidence are retained.
       Registry's existing closed v1 request and persistence contracts remain
       unchanged.
     - Reject the evidence set before publication; never persist a partial
       aggregate or treat a planning row as publication authority.
   * - Permanent revision qualification
     - Jenkins/Expedition and programme owners
     - Capture exact merge commits for DASObjectStore, Oikodome, Registry and
       all authority dependencies, then run cross-language vector, transport,
       persistence, resolution, and read-back acceptance on that lockset.
     - Draft heads, local branches, and unverified vectors cannot qualify a
       release or become a consumer pin.

Resolution and read-back sequence
---------------------------------

Once the owner gates are accepted, the minimum safe sequence is:

#. Authenticate the caller and establish the expected installation/Site Trust
   Domain/tenant/project scope independently of the supplied reference.
#. Decode the exact ObjectRef or EvidenceRef with ADR-0004 duplicate, bounds,
   canonical-byte, and domain-digest checks.
#. Resolve under the independent scope.  A consumer-supplied scope or digest
   never creates authority and does not bypass the owner grant.
#. Perform an independent metadata read-back from DASObjectStore.  Compare
   immutable version, byte count, content digest, and lifecycle/settlement
   state; for EvidenceRef also compare kind, subject digest, revision, nested
   ObjectRef, and outer digest.
#. Only after all comparisons succeed may Oikodome return a metadata-only
   settlement receipt to Monas or a future Registry adapter observe a positive
   pre-admission condition.  This receipt is not a queue transition, approval,
   entitlement, signature, secret, or storage capability.

Any transport timeout, provider disagreement, revocation, digest substitution,
partial persistence, or ambiguous acknowledgement leaves the operation
unsettled and requires the existing manual, fail-closed recovery path.

Issue #31 exit criteria
-----------------------

ADR-0004 may move beyond Proposed only when the owner records:

* accepted security, protocol, persistence, and cross-project review;
* owner-side strict parser and schema implementation with the vectors and
  adversarial fixtures;
* atomic issuance, restart, authorization, and no-existence-disclosure
  evidence;
* an authenticated transport adapter with bounded metadata-only envelopes;
* independent resolution/read-back and Oikodome settlement evidence;
* Registry/Forge evidence showing the planning-only boundary is preserved; and
* a Jenkins/Expedition qualification lockset containing permanent merge
  commits, including a merged Oikodome #69 if that follow-up is adopted.

Until then, this reconciliation is review material.  It authorizes no runtime
mutation, production storage authority, object dereference, secret transport,
or consumer revision pin.
