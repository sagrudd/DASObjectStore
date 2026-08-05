# Retained Jenkins dossier DAS prerequisite

Status: non-production verifier contract

`dasobjectstore.jenkins_retained_dossier_prerequisite.v1` is a pure,
fail-closed local verification seam for one retained Jenkins dossier candidate.
It pins an immutable Jenkins Git revision and one fully validated
`EvidenceRefV1`.  The nested `ObjectRefV1` provides the exact immutable object
version, byte size, and SHA-256 content digest.

An adapter may call `verify_readback` only after Monas has verified the
session-bound read capability and the DAS daemon boundary has supplied bytes.
The function requires the exact pinned EvidenceRef, exact byte count, and exact
SHA-256 digest. Schema/revision/reference errors, substitution, short or extra
bytes, and digest drift fail closed.

This contract does **not** establish a Monas session, validate Site Trust,
issue or verify a capability, resolve a DAS catalogue entry, read storage,
persist a Jenkins dossier, issue a receipt, or create an approval. It performs
no storage mutation. In particular, passing this check is never authority to
promote an object, publish an artifact, retry a workflow, schedule work, or
delete retained evidence. Promotion remains a separate manually approved,
daemon-owned operation with a fresh Monas/Pistis approval.

The existing `synthetic_scoped_readback` module remains the separate
non-production capability-verifier seam. A future production integration must
define and qualify a versioned Monas capability adapter, authenticated DAS
resolver/settlement flow, retention/recovery policy, and exact Jenkins
Expedition dossier before it can make a retained-dossier or storage-health
claim.
