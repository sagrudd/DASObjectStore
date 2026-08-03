# Synthetic scoped read-back demonstration seam

Status: non-production fixture contract

This seam implements the bounded direction accepted in programme decisions
D-039, D-045, D-050, and D-052. It is intentionally a pure in-process
verification operation and is not the DAS daemon read API.

## Boundary

- Monas issues the session-bound capability. DASObjectStore receives only a
  redacted projection and asks an injected Monas verifier to verify it.
- Proxenos remains the Site Trust evaluator. This module neither consumes nor
  evaluates Site Trust facts.
- Thesaurophylax remains the signing/custody authority. This module has no key,
  signature, token, credential, URL, or managed-path field.
- DASObjectStore verifies that the exact `EvidenceRefV1` describes the exact
  `ObjectRefV1`, then verifies the supplied bytes against that object’s size
  and SHA-256 digest.

Verifier denial, expiry, invalid reference data, evidence/object substitution,
size mismatch, and digest mismatch deny without a settlement result. A result
is a redacted `synthetic_seven_day` observation only. It does not resolve an
object, create a durable receipt, demonstrate storage health, authorise retry,
or permit promotion/publication/workload scheduling.

The future daemon adapter must use the supported DAS service boundary to obtain
bytes and a separately versioned Monas capability format. It must retain
manual/audited deletion and never use this unencrypted synthetic path for
customer, genomic, PII, public, shared, S3, or production data.
