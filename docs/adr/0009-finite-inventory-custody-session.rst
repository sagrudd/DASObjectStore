Finite-inventory custody session
================================

Status: ACCEPTED bounded source design; no execution authority.

Lead-agent independent full review accepted raw design SHA-256
``28a05eb6d949976db3448cee2de3dd4b1480f695679bf762051df6d67a47794f``
on 2026-09-08, with the timestamp clarification below. This is delegated
source-design review, not a personal owner signature or live companion.
Kanon #339 source coordination: ``930e58f`` for proposed source 0.184.0.

Date: 2026-09-08. Decision owner: project owner, with lead-agent source
design review. Issue: https://github.com/sagrudd/DASObjectStore/issues/205.
Baseline: ``26e375ca6380df596dfc7bc94da0a1d451803a01`` (DAS #204).
Proposed completed capability release: 0.184.0; Kanon coordination precedes
code/version changes. This documentation-only proposal changes no version.

Context
-------

The extracted ``CustodyServiceController::retain_custody_object`` consumes
the sealed writer and reader references for each object. The actual systemd
resolver permanently consumes each reference once. Repeating that method
cannot retain a multi-object inventory. The lower-level
``retain_custody_object_with_readback`` already accepts reusable writer and
reader instances and performs the actual conditional write, fresh readback
and immutable ledger append. Reuse that implementation, not a new provider.

The programme ``R239_CUSTODY_BOOTSTRAP_DESIGN_V1.md`` and
``R237_S4_LOCAL_DAS_OBJECT_LOCK_CUSTODY_CLASS_V1.md`` remain controlling.
The extracted controller is accepted; lifecycle runtime and read continuation
remain separately proposed. This batch does not establish custody eligibility.

Decision proposed
-----------------

Add an in-process, synchronous, consuming finite-batch operation in a child
module of ``runtime/custody_service.rs``. No HTTP/CLI operation or serialized
batch protocol is added. Existing single-object method and request remain
unchanged; do not extend the sealed profile, ledger, catalogue or credential
handoff wire. Do not add a new durable batch marker.

The concrete API shape is::

    CustodyFiniteInventory { store_id, objects }
    CustodyInventoryObject { content_sha256, size_bytes }
    controller.retain_custody_inventory(
        expected: &CustodyFiniteInventory,
        inputs: Vec<CustodyObjectInputV1>,
    ) -> Result<Vec<CustodyIntegrityReceiptV1>, CustodyBatchError>

These are library data types, not serde authority or owner-approval types.
``expected`` is supplied by trusted service composition from its selected
finite inventory, just as the existing controller receives explicit catalogue
and plane configuration. It must not be populated from a transport request
claiming its own expected hashes. Construction is usable and concrete, but
does not prove an independently admitted execution companion. There is no
production invocation in this slice. Actual future lifecycle composition must
establish that admission before calling this method.

The operation derives writer/reader references solely from the exact sealed
catalogue definition; it takes no reference, policy, endpoint, bucket, ledger,
retention, hold or identity override. Inputs retain the existing object type,
bytes and retained-at fields and their current validation semantics. The
selected store fixes retention/hold/policy; timestamps cannot override them.

Limits and matching
-------------------

Reuse the planner's preknown-inventory rules: 1..4096 entries, distinct valid
lowercase SHA-256 digests, and strictly positive u64 sizes. The planner retains
its exact ``sha256:<64 lowercase hex>`` wire; its adapter validates and strips
exactly that prefix. The batch library uses bare 64-character hex, matching
existing custody receipts. Neither adapter accepts the other's representation.
Aggregate-size overflow denial is a bounded new validation tightening, not a
changed wire schema. Share the inventory validator
with the planner rather than duplicate it. Generated terminal receipts are
not preknown inventory and are excluded from this operation.

Require exactly one input per expected digest/length; reject missing, extra,
duplicate or changed bytes. Input order need not be trusted: execute in the
server-owned expected inventory order, returning receipts in that order.
Use checked aggregate-size arithmetic and checked usize/u64 conversions.
The planner's 1,048,576-byte limit bounds its JSON metadata, not object payloads;
do not misrepresent it as an approved total payload limit. This in-process API
accepts already-owned byte vectors and performs no transport allocation. It
does not clone all payloads. A later transport must separately specify bounded
streaming/admission limits; no unbounded transport is introduced here.

Ordering and failure
--------------------

1. Before either handoff, validate the complete expected inventory and every
   actual input, including the existing input/type/time validation. Hash all
   bytes, check exact membership and order, and resolve/recheck catalogue,
   sealed ledger, store, bucket, configuration, distinct identities and sealed
   retention-versus-input-timestamp validation using the existing routines.
   This is not actual wallclock freshness; actual time authority and lifecycle
   admission remain the future companion's responsibility. Invalid input has zero
   credential consumption and zero Garage writes.
2. Consume writer, then reader, once through the existing actual resolver,
   with sealed role/store/configuration bindings. If reader resolution fails
   after writer consumption, preserve the consumed writer marker; do not
   restore authority. No object operation has yet occurred.
3. Construct the actual ``GarageCustodyS3Writer`` and
   ``GarageCustodyS3Reader`` once inside the operation. Iterate the finite
   sequence using ``retain_custody_object_with_readback``. Keep all existing
   per-object validation and fresh readback; do not weaken checks because
   prevalidation passed earlier. No batch-wide SQLite transaction is claimed.
4. First error stops the batch immediately. Drop credentials/adapters and
   return a structured error containing a fixed phase, failed index when
   applicable, and only the verified completed-prefix receipts. Do not leak
   credentials, command environments or raw backend diagnostics. Completed
   receipts are partial evidence, never a successful whole-batch assertion.
5. A failed object may already exist without a committed ledger receipt.
   Preserve it, the ledger, catalogue, namespace claims and consumption
   markers. Never delete, compensate, adopt an orphan, retry, re-admit or
   return another session capability. Existing consumption markers deny a
   competing/restarted invocation before Garage effects. Prevalidation errors
   have consumed no authority and may be corrected by the caller; this is not
   replay of a started session.

Error phases are ``prevalidation``, ``writer_handoff``, ``reader_handoff``,
``adapter_construction`` and ``object_retention``. An uncertain error is not
evidence of absence. Existing exact-content ledger idempotence remains an
internal per-object invariant; it grants no new batch or credential lifetime.

Success drops both adapters after all receipts have been returned. It does
not retain a reader, issue an attestation, extend credential lifetime, reopen
consumed handoffs, restart a unit or advance a release gate. Failure durability
is only that of the underlying existing durable operations; no hardware-proof
claim is added.

Compatibility and security
--------------------------

This is an additive source capability. No historical API, sealed record,
database schema, identity or migration changes; no dependency expected.
Single-object callers retain their exact behavior. Reusable secret material
exists only for this synchronous finite call, not in an externally accessible
session object. No raw credentials are accepted from a batch caller or emitted
in results. Existing trusted-administrator-overlay limits remain unchanged.

Rejected alternatives: repeated single-object calls (consume-once conflict),
fresh credentials for each item (new provisioning/lifetime semantics), generic
mutable session handle (unbounded operations), and rollback deletion (violates
custody retention). A new marker-only abstraction does not solve the problem.

Implementation and validation plan
----------------------------------

Own a narrowly scoped controller child module and tests; factor shared pure
inventory validation in object-service/planner without changing planner wire.
Keep credential resolution and real adapter construction inside the existing
daemon boundary. Update source evidence and user documentation honestly.

Required tests before source review:

* Actual existing ledger plus conditional-retain/readback integration for
  multiple different objects; exactly one writer and one reader consumption,
  receipts/inventory order and fresh readback per object.
* Every inventory boundary: empty, 4096 and 4097, invalid/duplicate digest,
  zero size, overflow, missing/extra/duplicate/tampered payload, wrong store,
  altered sealed binding, invalid type/time and expired retention; no handoff
  or backend effect on prevalidation denial.
* Writer denial and reader denial after writer success; exact counters and
  durable non-reuse. Competing invocations and resolver reconstruction using
  real temporary one-use marker files produce at most one effects-bearing
  batch, not only a mock boolean.
* Failure at each object and at conditional put, post-put inspection,
  independent readback and ledger commit: preserve complete prior receipts,
  stop later calls, preserve partial objects and deny replay/re-admission.
* Whole success followed by repeated call/restart fails before backend work;
  existing single-object tests and planner conformance remain unchanged.
* Full relevant tests, formatting, strict Clippy, documentation and scoped
  Kanon/packaging checks. No native Garage/systemd qualification claim from
  an in-process command fixture.

Remaining connected work is explicitly separate: reader endpoint and its
authenticated transport; protected reader reopening/lifetime; one-attempt
lifecycle/containment; actual off-NUC verifier and execution companion. This
proposal neither selects those contracts nor claims they already exist.
