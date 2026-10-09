ADR-0012: Conditional design for maintained PEM readers and server supplier migration
======================================================================

:Status: Accepted conditionally for design only; implementation not authorized
:Date: 2026-10-10
:Decision authority: Programme recovery lead/coordinator, under standing owner scope
:Technical review: ready_original_screen, independent AI technical analysis; no human specialist credential claimed
:Base: DASObjectStore dcc986bda43d0eaeb1c70af0573af0ef67ec5577, version 0.186.40
:Constraints: Accepted ADR-0001; ADR-0006 remains Proposed

Scope and authority
-------------------

This document is a design proposal only. It authorizes no product code,
dependency, lockfile, version, provider, native build, package, credential,
trust, deployment or Kanon mutation. The coordinator recorded standing-owner conditional design acceptance at
2026-10-09 22:16 UTC (minute precision), as relayed by the recovery lead. Independent reviewer
ready_original_screen supplied source/security/parser/supply-chain technical
analysis, not a claimed human specialist qualification. That review binds
proposal packet af375067a180d3160b8e9438d38b09c62e7f01d296c8ef37299b973a9d7d1e32
and prior document acd7ce8af2be2ea43e8fce77c2d0393297bc6517bbb449792e90f59236aa571b.
The review receipt SHA256 is
82e300ae00fce209a120a014a47e72eaad71545f3ab05ded3f8faa6ce9722b96.
Normal approved document delivery and a separate explicit implementation
lease remain required. This conditional acceptance grants design only; every
qualification and release gate below remains open. No named human approval
is invented. Existing provider-policy acceptance is not implementation qualification.

Context and verified source boundary
------------------------------------

The canonical workspace declares rustls-pemfile 2.2.0 and Rustls 0.23.42.
There are six direct internal reads across four Rust files. The resolved
axum-server 0.7.3 also depends on rustls-pemfile, so substituting internal
readers alone does not remove the obsolete dependency from the complete graph.
Use the already locked Rustls pki-types 1.15.1 APIs, not a custom PEM or
cryptographic implementation. The supplier recommendation is retained as
INDEPENDENT_SUPPLIER_DESIGN_RECOMMENDATION.json, SHA256
5d658df49eda49f19b4401e616cb54a965a2bd290e6da846d9f9ffb60e5e742e.

Proposed internal parsing contract
----------------------------------

* ``mtls_listener.rs::read_certificates`` fully collects certificate blocks.
  Preserve order, empty vector and ``MtlsListenerError::Io`` mapping; the
  existing CA caller rejects empty roots and ``with_single_cert`` validates
  server material before listener binding.
* ``mtls_listener.rs::read_private_key`` chooses the first recognized PKCS1,
  PKCS8 or SEC1 key in input order. Preserve the exact no-key TLS error and
  file/malformed-input I/O category. Do not scan trailing blocks after the
  first key or silently reject a second key that the current path never reads.
* ``s3_endpoint_probe.rs`` fully collects the configured certificate bundle,
  preserving its fixed Unavailable errors, empty-bundle rejection, first leaf,
  order and existing remaining-chain/trust-anchor selection.
* ``site_trust.rs::pem_to_der`` fully collects and requires exactly one
  certificate, preserving fixed Invalid errors for parse and cardinality.
* ``trust.rs::verify_chain_with_authority`` fully collects, requires exactly
  one CA and preserves Invalid mapping, root insertion, chain, hostname and
  signature verification.
* ``trust.rs::pem_leaf_der`` reads only the first certificate with lazy
  ``next().transpose()`` behavior. Preserve empty and initial-error mappings;
  trailing malformed PEM after a returned leaf remains unconsumed.

Use ``CertificateDer::pem_reader_iter`` or ``pem_slice_iter`` with unchanged
collection boundaries, and ``PrivateKeyDer::from_pem_reader`` with an explicit
``NoItemsFound`` adapter. Differential fixtures must prove error categories
and boundary behavior, including unsupported/malformed preceding blocks.
Do not claim byte-identical diagnostic text from a different upstream parser
unless actually established; any externally relied-on difference must be
resolved and documented before compatible implementation approval.

Proposed complete supplier removal
----------------------------------

Qualify axum-server exactly 0.8.0 as a separate bounded supplier step in the
same coordinated migration. Its declared MSRV is 1.82 and edition 2021;
verify the actual DAS supported toolchain/platform policy before adopting it.
Its Server address/connection generics and acceptor uses require exact caller
compile qualification. Retain the existing framework version rather than
assuming an Axum upgrade is required by the supplier's dev dependencies.

The supplier's ``tls-rustls`` continues selecting AWS-LC; provider-neutral
features are not permission to change the accepted process-owner/provider
contract. Preserve current TLS/mTLS, certificate and hostname checks and the
accepted policy constraints without treating existing implementation debt as
qualified policy compliance. Libraries must not gain provider installation.

The supplier's new key loader iterates the whole input and rejects multiple
valid keys, which differs from DAS private first-key semantics. Whole-input
iteration is not generic whole-file strictness: a valid key followed by a
malformed block may retain success, and a later valid key may overwrite an
earlier parse error. Differential fixtures must cover both cases and their
contrast with DAS first-key selection before any supplier migration. Inventory every
existing ``RustlsConfig::from_pem_file`` and equivalent supplier-owned load
path before implementation. Preserve their actual behavior or obtain an
explicit compatible migration decision; where reviewed, construct the
existing configuration from the private parser instead of inheriting changed
whole-file behavior. A blanket API substitution is not an approved solution.

Remove direct declarations from workspace Cargo.toml and the gui-api and
remote manifests only after exact replacements qualify. Regenerate the lock
through an authorized maintained native source route; preserve unrelated
supplier pins/checksums and all thirteen DAS self-version entries. Prove no
rustls-pemfile inverse edge remains across the full normal/build/dev graph.
Do not delete a still-consumed lock record or infer removal from text searches.
The reported 0.7.4 tag/version mismatch is not evidence of a compatible 0.7
fix; published 0.8.0 metadata is the proposed supplier coordinate.

Security, compatibility and rollback
------------------------------------

The trust boundary is configured untrusted PEM input before authenticated
TLS operation and listener binding. Preserve DER/key-certificate validation
through existing ``with_single_cert`` and all existing verifier paths. Do not
invent Monas Ring DER/SPKI checks in DAS or switch providers. No real keys,
secret stores or certificate authority state belong in fixtures or receipts.

Closure of the obsolete-dependency advisory requires an actual complete
resolved-graph proof and pinned advisory/policy checks. A source substitution,
compilation or allowlist exception cannot establish advisory closure. No
policy waiver, custom fork or generic permission repair is proposed.

A private-parser compatible correction may warrant patch 0.186.41 only after
behavior is proven. Supplier 0.8 API and whole-file parsing differences are
not automatically a compatible patch. Classify actual downstream behavior
and documented MSRV implications before allocating a release; no version is
changed by this proposal. Public API/protocol/config/store formats and
interval behavior remain unchanged, or an explicit new decision is required.

Rollback restores the exact canonical source/lock supplier tuple through
normal reviewed source delivery, not live key/trust/service manipulation.
Preserve failed migration evidence and do not reuse artifact bytes under a
new identity or rewritten historical lockset.

Qualification and delivery gates
--------------------------------

1. Preserve the recorded conditional design acceptance and independent
   technical review. Final exact-source review accepts or rejects implementation
   of these semantics,
   supplier/API/MSRV and error boundary; record reviewer qualification,
   exact source/document hashes, disposition and unresolved conditions.
2. Differential generated disposable fixtures cover PKCS1/8/SEC1, mixed and
   multiple keys, empty/certificate-only input, unsupported blocks, malformed
   base64, truncated/end-marker mismatch, initial and trailing parse errors,
   read failures, valid-key then malformed-block, earlier-error then later-key
   and exact cardinality/order. Compare full-collection and
   lazy-first paths explicitly; exercise actual old and proposed readers.
3. Invalid DER, mismatched certificate/key, bad CA, hostname/chain failures
   deny before bind/application data/trust persistence. Exercise existing
   public HTTPS/mTLS supplier load paths, shutdown and acceptor behavior.
4. Under separately granted native resources, qualify actual supported
   toolchains/platforms, full locked graph/inverse-edge removal, focused
   regressions, repository fmt, strict lint, full applicable tests, strict
   private-item docs, pinned audit/deny and package metadata checks. Missing
   inputs remain unknown/fail-closed, never borrowed from Monas evidence.
5. Coordinator binds exact source/version/dependency coordinates in Kanon,
   generates new immutable resolved release inputs and Terraform projections,
   and performs normal reviewed publication after the existing REG priority.
   SOURCE qualification does not claim installation or runtime acceptance.

References
----------

* Rustls pki-types 1.15.1 ``PemObject`` official API documentation:
  https://docs.rs/rustls-pki-types/1.15.1/rustls_pki_types/pem/trait.PemObject.html
* Published axum-server 0.8.0 changelog:
  https://docs.rs/crate/axum-server/0.8.0/source/CHANGELOG.md
* Tagged 0.8.0 manifest and TLS implementation:
  https://github.com/programatik29/axum-server/tree/v0.8.0
* Engineering SECURITY_POLICY.md requires specialist review and an accepted
  ADR before security/cryptography/parser implementation. Existing DAS
  ADR-0001 supplies constraints; Monas 0.137.2 is technical reference only.
