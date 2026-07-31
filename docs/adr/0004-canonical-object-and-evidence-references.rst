ADR 0004: Canonical non-secret ObjectRef and EvidenceRef
========================================================

:Status: Proposed
:Date: 2026-07-31
:Deciders: Project owner, security reviewer, protocol reviewer, persistence reviewer, DASObjectStore and cross-project maintainers
:Related issue: `DASObjectStore #31 <https://github.com/sagrudd/DASObjectStore/issues/31>`_
:Related consumer issue: `Oikodome #53 <https://github.com/sagrudd/oikodome/issues/53>`_

Proposal status
---------------

This record is owner-contract review material only.  It authorizes no
production implementation, migration, readiness claim, package activation, or
consumer revision pin.  Acceptance, implementation, fixtures, security review,
and exact-revision qualification remain separate gates under issue #31.

Context
-------

Oikodome and other Monas or Synoptikon components need to retain durable
references to images, manifests, checksums, convergence evidence, approval
evidence, and recovery exports.  DASObjectStore owns durable object identity
and resolution.  A consumer-local reference grammar would transfer that
ownership incorrectly and could make a string look like both immutable
identity and read authority.

The current catalogue already models one immutable logical object version as
an ObjectStore, logical key, version, size, and checksum, with native and
provider records treated as placements of that identity.  That internal model
does not by itself define a portable, non-secret wire reference.  Internal
database identifiers, provider locations, filesystem paths, bucket details,
and bearer capabilities must not escape merely because a consumer needs to
remember an object.

The contract must also distinguish two hashes:

``content_digest``
   The SHA-256 digest of the exact raw object bytes.  It verifies content but
   does not identify the installation, authority scope, ObjectStore, logical
   object, version, or evidence purpose.

``domain_digest``
   A domain-separated SHA-256 digest over the canonical reference identity.
   It binds raw content evidence to its DASObjectStore authority scope and
   immutable logical identity.  It is not a second payload checksum, a
   signature, or a bearer capability.

Decision
--------

DASObjectStore will own two strict JSON contracts:
``dasobjectstore.object_ref.v1`` and
``dasobjectstore.evidence_ref.v1``.  They are immutable, non-secret reference
values.  Possession proves neither existence nor authorization.

Every decoder first applies the encoded-size bound and a duplicate-aware
streaming token pass to the raw JSON.  The pass maintains a separate set of
decoded member names for every object and rejects a duplicate before any
generic map or typed decoder can collapse it.  Escaped and literal spellings
that decode to the same member name are duplicates; for example, ``schema``
and ``\u0073chema`` conflict.  The same rule applies recursively to
``authority_scope``, digest objects, and nested ``object_ref`` values.

Only after that pass may a decoder inspect ``schema``.  It does so before
semantically interpreting any other field, rejects unknown schemas, and then
applies strict typed decoding with unknown fields denied.  Version 1 has no
extension map.

Canonical serialization uses UTF-8 JSON in RFC 8785 JSON Canonicalization
Scheme form.  Producers emit only the member set defined below.  Consumers may
receive members in any JSON order but equality and hashing use the canonical
bytes.  Numeric fields are deliberately restricted to the ECMAScript
interoperable safe-integer range used by RFC 8785/JCS.  Floating-point,
exponent, negative, negative-zero, and string-encoded number forms are invalid.
A decoder validates each raw number token against the field's lexical form and
bound before converting it to a generic JSON number or native integer, so no
parser may round an out-of-range value into an accepted value.

Common lexical grammar and bounds
---------------------------------

Before semantic resolution, a v1 decoder enforces all of these limits:

* one encoded reference is at most 8192 UTF-8 bytes;
* nesting is at most four objects deep and arrays are forbidden;
* all identifiers are 1 through 128 ASCII bytes and match
  ``^[a-z0-9][a-z0-9._-]{0,127}$``;
* ``store_id`` is additionally limited to 64 ASCII bytes;
* schema names, digest algorithms, and evidence kinds are lowercase canonical
  values and are never case-folded by a decoder;
* digest algorithms equal exactly ``sha256``;
* digest values contain exactly 64 lowercase hexadecimal characters, without
  an algorithm prefix;
* ``object_version`` and ``evidence_revision`` are integers from 1 through
  ``9007199254740991``;
* ``size_bytes`` is an integer from 0 through ``9007199254740991`` so the
  contract preserves true zero-byte objects;
* before typed decoding, version and revision number tokens match
  ``^[1-9][0-9]*$``, the size token matches ``^(0|[1-9][0-9]*)$``, and checked
  decimal accumulation rejects a value above ``9007199254740991``;
* every required member is present and non-null; and
* strings containing whitespace, control characters, non-ASCII bytes,
  ``/``, ``\``, ``:``, URL syntax, traversal syntax, or percent-encoding are
  invalid where the identifier grammar applies.

These bounds deliberately make a reference neither a path nor a URI.  A JSON
reference must never be accepted from a string by trimming, case conversion,
URL decoding, path normalization, or best-effort repair.

AuthorityScopeV1
----------------

Both reference types carry one nested ``authority_scope`` object:

.. code-block:: json

   {
     "installation_id": "019d4f74-41af-7dc0-8c2a-1ad58387e488",
     "site_trust_domain_id": "site-berlin-01",
     "tenant_id": "019d4f74-41af-7dc0-8c2a-1ad58387e489",
     "project_id": "oikodome-images"
   }

``installation_id`` is always required.  The other members are present exactly
when the authoritative host binding scopes the ObjectStore by that dimension.
They are omitted, not serialized as null, only when that dimension genuinely
does not exist in the owning binding.  A producer must not omit a known
dimension to create a wider reference.

The exact presence and values form part of reference identity.  Resolution
must compare them with the independently authenticated installation, Site
Trust Domain, tenant, and project context.  A consumer-supplied scope never
creates authority.  Standalone mode does not fabricate tenant, project, or
Site Trust Domain identifiers merely to fill these fields.

ObjectRefV1
-----------

The exact v1 shape is:

.. code-block:: json

   {
     "schema": "dasobjectstore.object_ref.v1",
     "authority_scope": {
       "installation_id": "019d4f74-41af-7dc0-8c2a-1ad58387e488",
       "site_trust_domain_id": "site-berlin-01",
       "tenant_id": "019d4f74-41af-7dc0-8c2a-1ad58387e489",
       "project_id": "oikodome-images"
     },
     "store_id": "oikodome-default",
     "object_id": "obj-0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
     "object_version": 1,
     "size_bytes": 4096,
     "content_digest": {
       "algorithm": "sha256",
       "value": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
     },
     "domain_digest": {
       "algorithm": "sha256",
       "value": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
     }
   }

``object_id`` is the DAS-owned stable opaque logical-object identifier
published by the owner contract.  Every immutable version of that same logical
object retains the same ``object_id``.  It is opaque to consumers.  Consumers
must not derive it from a key, filename, provider object ID, path, digest, or
database row.  ``object_version`` is non-zero and identifies one immutable
version of that logical object; it is not a mutable catalogue revision.
Consequently
``(authority_scope, store_id, object_id, object_version)`` identifies exactly
one immutable version.  Creation of a different logical object allocates a new
``object_id``.  Replacement or correction of an existing logical object
retains ``object_id`` and advances ``object_version`` monotonically under the
accepted mutation precondition; a deleted version and its number are never
reused.

``size_bytes`` and ``content_digest`` describe the exact raw bytes accepted by
DASObjectStore.  Equal content digests in different scopes, ObjectStores,
logical objects, or versions do not imply equal references and do not
authorize deduplication or substitution.

The ObjectRef domain digest is:

.. code-block:: text

   SHA-256(
     UTF8("DASOBJECTSTORE_OBJECT_REF_V1") || 0x00 ||
     JCS(identity_projection)
   )

``identity_projection`` is the complete ObjectRef object above with
``domain_digest`` omitted.  The stored ``domain_digest.algorithm`` is
``sha256`` and ``domain_digest.value`` is the lowercase hexadecimal digest.
No producer may hash a display string, reordered tuple, URI, provider locator,
or structure that includes ``domain_digest`` itself.

EvidenceRefV1
-------------

An EvidenceRef binds an ObjectRef to one immutable evidence purpose and
subject.  It does not create a second object or weaken ObjectRef resolution.
Its exact v1 shape is:

.. code-block:: json

   {
     "schema": "dasobjectstore.evidence_ref.v1",
     "object_ref": {
       "schema": "dasobjectstore.object_ref.v1",
       "authority_scope": {
         "installation_id": "019d4f74-41af-7dc0-8c2a-1ad58387e488",
         "site_trust_domain_id": "site-berlin-01",
         "tenant_id": "019d4f74-41af-7dc0-8c2a-1ad58387e489",
         "project_id": "oikodome-images"
       },
       "store_id": "oikodome-default",
       "object_id": "obj-0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
       "object_version": 1,
       "size_bytes": 4096,
       "content_digest": {
         "algorithm": "sha256",
         "value": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
       },
       "domain_digest": {
         "algorithm": "sha256",
         "value": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
       }
     },
     "evidence_kind": "oikodome.convergence",
     "evidence_revision": 1,
     "subject_digest": {
       "algorithm": "sha256",
       "value": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
     },
     "domain_digest": {
       "algorithm": "sha256",
       "value": "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
     }
   }

``evidence_kind`` names a governed registry entry and follows the common
identifier grammar.  Registering a kind and defining its subject canonical
form are separate owner-reviewed contract changes.  An arbitrary free-text
label is invalid.

``subject_digest`` is the SHA-256 digest specified by that evidence-kind
contract over the exact external subject to which the evidence applies.  It
does not replace the raw evidence object's ``content_digest``.
``evidence_revision`` identifies the immutable evidence assertion revision;
a correction creates a new object version and EvidenceRef.

The EvidenceRef domain digest is:

.. code-block:: text

   SHA-256(
     UTF8("DASOBJECTSTORE_EVIDENCE_REF_V1") || 0x00 ||
     JCS(identity_projection)
   )

For EvidenceRef, ``identity_projection`` is the complete EvidenceRef object
with its outer ``domain_digest`` omitted.  The nested ObjectRef remains
complete, including its already verified domain digest.

Reference issue and immutable put
---------------------------------

DASObjectStore may issue ObjectRef only after it has atomically committed one
immutable logical object-version identity, the exact size and raw content
digest, and the durability state required by the ObjectStore's acknowledgement
policy.  A staged payload, upload reservation, provider key, placement, or
uncommitted catalogue row cannot produce a reference.

Every immutable-put request carries a canonical lowercase UUID
``put_operation_id``.  It is a non-secret, caller-scoped idempotency identity,
not part of ObjectRef identity and not a capability.  Its uniqueness key is
exactly
``(stable_authenticated_caller_application_id, put_operation_id)``.  The
stable caller/application identity is an immutable principal or workload
identity established independently of the request; it is not a grant ID,
grant revision, role revision, entitlement revision, credential generation,
or other mutable authorization state.  Equal UUIDs in different stable caller
namespaces are unrelated and neither caller can observe the other's binding.

The idempotency binding includes one canonical operation projection.  It is the
complete semantic immutable-put command after strict validation, represented
by the accepted versioned put API schema, with exactly:

* that command's schema identity;
* the complete authority scope and exact ObjectStore;
* the operation kind;
* the complete logical-target selector;
* every replacement, expected-version, or other mutation precondition;
* the exact size and raw content digest; and
* no ``put_operation_id``, payload bytes, transport headers, credentials,
  timestamps, tracing fields, or other non-semantic request metadata.

The put API schema must define the exact member set and canonical form of its
logical-target selector, operation kind, and preconditions; an implementation
cannot substitute a display name, provider key, path, best-effort normalized
request, or generic extension map.  The operation digest is:

.. code-block:: text

   SHA-256(
     UTF8("DASOBJECTSTORE_IMMUTABLE_PUT_OPERATION_V1") || 0x00 ||
     JCS(operation_projection)
   )

The daemon retains both the canonical projection bytes and the lowercase
``sha256`` operation digest.  The digest may index comparison, but replay
equality is byte-for-byte equality of the canonical projection plus equality
of the exact payload bytes.  Any changed semantic member is deterministic
drift even if a transport field or field order differs.

The daemon atomically binds the exact
``(stable_authenticated_caller_application_id, put_operation_id)`` key to the
canonical operation projection and digest, exact payload bytes, server-owned
object identity, version, and acknowledgement state in one transaction.  The
binding records the authorization revision used for audit, but that mutable
revision is neither part of the namespace key nor accepted as continuing
authority.  Every replay still performs both current exact-grant checks
defined below.  The binding is retained for at least the complete retained
lifecycle of the object and tombstone, so deletion cannot make an operation
identifier reusable within that caller namespace.

After strict request validation and before consulting an existing operation
binding, object identity, catalogue row, or provider placement, the daemon
re-resolves current authority and requires the exact
``immutable_object_put`` action for the authenticated scope and ObjectStore.
An absent, stale, revoked, cross-scope, or cross-store grant returns
``not_authorized`` without revealing whether the operation identifier or
object already exists.

The daemon re-resolves that same exact grant again immediately before either
returning an ObjectRef from an existing replay binding or atomically committing
a new put binding, object identity, version, and acknowledgement state.  If
this second check fails, it returns ``not_authorized``, issues no ObjectRef, and
commits no mutation.  Bytes staged before the second check remain
unacknowledged and are handled only by the ordinary orphan and recovery rules.

Put is immutable:

* exact replay of one caller-namespace key with byte-identical canonical
  operation projection, operation digest, and payload bytes returns the same
  ObjectRef after both current authorization checks, including after the
  original response was lost;
* reuse of ``put_operation_id`` within the same stable caller namespace with
  any changed canonical operation projection, operation digest, or payload
  bytes is ``identity_conflict`` and changes nothing;
* equal ``put_operation_id`` values in different stable caller namespaces are
  independent operations; authorization and lookup remain namespace-scoped,
  so neither produces a replay or conflict observable by the other caller;
* the same immutable object identity with different size, content digest, or
  bytes is also ``identity_conflict`` and changes nothing;
* equal raw bytes under a different logical identity produce a different
  domain digest even when physical storage deduplicates them;
* replacement creates a new non-zero object version and a new reference; and
* no put API may accept a caller-computed ObjectRef as proof that catalogue
  commit or durability has occurred.

Before resolving the supplied ObjectRef, consulting the catalogue, or invoking
an evidence-kind validator, the daemon independently resolves authority and
requires the exact ``evidence_ref_issue:<evidence_kind>`` action for the
authenticated scope and ObjectStore.  Ordinary immutable-put or read authority
does not imply evidence issuer authority.  An unauthorized issuer receives
``not_authorized`` before catalogue or validator lookup and cannot learn
whether matching bytes or an ObjectRef exist.

After that preauthorization, EvidenceRef may be issued only when its ObjectRef
resolves to the same immutable evidence bytes and the registered evidence-kind
validator accepts the subject binding.  Immediately before issuance, the
daemon re-resolves the same exact evidence-kind grant.  If this second check
fails, it returns ``not_authorized``, persists no evidence assertion, and
issues no EvidenceRef.

The evidence-kind contract defines the permitted issuer class and the exact
content, subject, signature, and lineage validation needed for that kind;
where an intrinsic signature is required, successful cryptographic verification
is part of issuance rather than caller metadata.  DASObjectStore stores no
private signing key, approval response, access token, or secret merely because
evidence is signed.

Authorized resolution and read-back
-----------------------------------

A reference and an authenticated read grant are independent inputs.
DASObjectStore resolves the current actor or application authority and
capability at action time, then:

#. strictly decodes and verifies both domain digests;
#. compares every reference scope dimension with the independently
   authenticated context and requires the exact ObjectStore read grant,
   returning ``not_authorized`` on any mismatch before catalogue lookup;
#. resolves the immutable catalogue identity without exposing a backing path
   or provider locator;
#. checks tombstone, quarantine, retention, availability, and protection
   state;
#. reads through the daemon-owned storage adapter;
#. verifies byte count and raw content digest before successful completion;
   and
#. for EvidenceRef, also verifies the evidence kind, subject digest, revision,
   nested ObjectRef, and outer domain digest.

An unauthorized caller, including one authenticated for a different scope or
ObjectStore, receives ``not_authorized`` without object-existence, tombstone,
quarantine, operation-identity, or scope-oracle detail.  Only after exact
authorization may a caller receive the narrower lifecycle outcomes below.
``scope_mismatch`` is reserved for an already-authorized diagnostic or for a
conflict between the decoded reference and an independently authoritative host
or catalogue binding discovered after authorization; it is never returned for
a caller/grant mismatch and never precedes authorization.  A successful read
is always scoped to the exact immutable version; resolution never follows a
latest-version alias.

Lifecycle and failure semantics
-------------------------------

References never mutate.  Deletion and availability change resolution state,
not reference identity.

``tombstoned``
   An authoritative deletion marker exists.  Ordinary read-back is denied.
   The reference remains valid historical identity, and retained recovery
   bytes do not make it available.

``not_found``
   No authoritative catalogue identity is available after authorized
   resolution.  This must not be silently translated into tombstoned,
   unavailable, or a newly minted reference.

``quarantined``
   Identity or integrity evidence requires operator review.  Ordinary read and
   replacement are blocked.

``temporarily_unavailable``
   The identity is known but required catalogue, placement, provider, or
   durability evidence cannot currently be proven.  It is not equivalent to
   empty content or deletion.

``integrity_conflict``
   Resolved size, content digest, identity, or bytes disagree.  The daemon
   fails closed, quarantines according to policy, and retains diagnostic
   evidence.

``manual_recovery_required``
   Automated resolution cannot choose one authoritative interpretation.
   Existing references and bytes remain untouched.

``invalid_reference``, ``unsupported_schema``, ``scope_mismatch``,
``identity_conflict``, ``bounds_exceeded``, and ``not_authorized`` are stable
error classes.  Messages, provider errors, paths, and internal record details
are not part of the wire contract.  Retries may be suggested only for
``temporarily_unavailable``; every other class requires a new authorization,
new reference, contract upgrade, or explicit recovery action as applicable.

Tombstone garbage collection may remove bytes after policy allows it, but
cannot erase the retained tombstone or make the old reference identify a later
object.  Reuse of an object ID and version after deletion is forbidden.

Orphans and recovery
--------------------

Payload bytes without one authoritative catalogue identity are orphans, not
objects.  Catalogue identities without verifiable bytes are unavailable or
integrity conflicts, not successful zero-byte objects.

Normal startup, scanning, repair, or retry must never fabricate an ObjectRef,
guess scope, infer a logical identity from a path/provider key, adopt an
orphan, or substitute equal-digest bytes automatically.  Orphans remain
quarantined and visible only to authorized diagnostics.

Recovery is an explicit daemon-owned operation.  It must preserve the original
bytes and records, produce a non-mutating inspection first, require the
documented operator authority for any adoption or catalogue repair, validate
scope and complete content, commit atomically, and retain provenance linking
old evidence to any newly issued reference.  Ambiguity leaves the operation in
``manual_recovery_required``.  A consumer must never invent a replacement
reference to make recovery proceed.

No-secret and presentation rules
--------------------------------

Neither reference may contain:

* bearer capabilities, access or renewal tokens, cookies, passwords, private
  or symmetric keys, Pistis approval responses, or secret leases;
* presigned URLs, endpoints, bucket names, provider account/key identifiers,
  filesystem paths, mount points, placement locations, hostnames, or network
  addresses;
* mutable lifecycle, health, availability, retention, or latest-version
  projections; or
* free-text user, filename, provenance, error, or display content.

Debug and display implementations must be explicitly safe.  Default logs
record the schema and outer domain digest, not the complete authority scope or
nested reference.  Browser projections may display authorized labels resolved
separately, but must retain the immutable reference digest for accuracy and
must never turn a reference into a clickable storage URL.

Compatibility and migration
---------------------------

Version 1 is frozen once accepted.  Because unknown fields are rejected, adding
a member, changing required scope, changing digest construction, widening an
identifier grammar, or changing identity/equality semantics requires a new
schema version and owner review.

Readers support only explicitly implemented schemas.  Unknown future versions
fail with ``unsupported_schema`` before any scope lookup or existence
disclosure.  They are never parsed as v1 prefixes or downgraded.

A migration does not rewrite a stored reference in place.  It must:

* decode the old schema with its original strict rules;
* resolve it under current independent authority;
* construct and validate a new immutable reference;
* retain a durable, non-secret migration record linking both domain digests,
  reason, authority, and result;
* leave the old reference resolvable according to its retained lifecycle; and
* fail closed with manual recovery when identity, scope, content, or authority
  is ambiguous.

Legacy internal logical-version records are not portable ObjectRefs until the
owner implementation has validated scope and content and issued the v1
contract.  Consumers may not serialize current database columns into a
lookalike reference during the migration window.

Consequences and acceptance gates
---------------------------------

The proposal gives Oikodome, Monas, and Synoptikon a portable identity grammar
without granting storage authority or exposing storage topology.  It requires
an owner implementation seam, strict fixtures, and resolution behavior before
any consumer can rely on it.

Issue #31 remains open until at least:

* security, protocol, persistence, and cross-project reviewers accept or amend
  this ADR;
* owner-side types and JSON Schemas have strict positive and adversarial
  fixtures, including literal/escaped duplicate names at the root and every
  nested object, and every lexical/bounds failure;
* RFC 8785 cross-language fixtures agree on canonical bytes and both domain
  digests at ``9007199254740991`` and reject ``9007199254740992``,
  ``9007199254740993``, ``9223372036854775807``, exponent, fractional,
  negative, negative-zero, leading-zero, and string-encoded forms before
  generic numeric conversion;
* immutable-put tests prove canonical operation-projection and digest fixtures,
  byte-identical replay despite JSON member reordering or transport-metadata
  changes, deterministic conflict for every changed semantic member or payload
  byte within one stable caller namespace, independent and non-observable equal
  UUIDs in different caller namespaces, atomic caller-plus-operation binding,
  exact retry after response loss, retained non-reuse after tombstone, and no
  mutation or existence disclosure for an unauthorized scope or store;
* object-identity fixtures prove that replacement and correction retain one
  logical ``object_id`` while monotonically advancing ``object_version``, that
  a different logical object receives a different ``object_id``, and that no
  deleted identity/version pair is reused;
* immutable-put authorization tests prove exact-grant denial before operation,
  catalogue, or provider lookup and same-grant re-resolution immediately
  before replay return or new commit, including revocation between both checks
  with no mutation or ObjectRef issuance;
* evidence issuance tests prove an ordinary object writer cannot issue an
  EvidenceRef, exact evidence-kind issuer authority is checked before ObjectRef,
  catalogue, or validator lookup and re-resolved immediately before issuance,
  revocation between both checks causes no mutation or issuance, and required
  subject/content/signature validation fails closed;
* immutable put, authorized read-back, cross-scope denial, digest substitution,
  tombstone, unavailable, quarantine, orphan, and manual-recovery behavior are
  implemented and restart-tested;
* logs, browser projections, packages, and API errors prove the no-secret and
  no-path boundary;
* compatibility and migration tests preserve old identity without guessing;
  and
* Oikodome consumes an exact permanent reviewed revision only after those
  owner gates pass.

This Proposed record must not be cited as production readiness or as a
candidate permanent revision.
