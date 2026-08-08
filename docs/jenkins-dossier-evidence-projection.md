# Jenkins dossier EvidenceRef projection

Status: owner-side canonical projection foundation; not a live authority path

`dasobjectstore.jenkins_dossier_evidence_projection.v1` is the single DAS
owner-side construction seam for the `jenkins.dossier` EvidenceRef required by
the Jenkins retained-dossier contract. It accepts only facts for an already
committed immutable object version and a canonical Expedition
`sha256:<lowercase-hex>` dossier digest. The projection places the digest's
hexadecimal payload in `EvidenceRefV1.subject_digest`, so the assertion is
bound to the exact canonical dossier Jenkins retains.

The projected reference is deliberately non-secret. It contains no backend
path, bucket, URL, capability, credential, token, session cookie, local user,
or OS role. Any malformed scope, object identity, size, digest, revision, or
dossier digest fails closed before a reference is returned.

This module does not itself issue or persist a reference. A formal live
implementation must first, in one daemon-owned transaction:

1. verify the fixed-peer Monas/Pistis subject and an exact retained-evidence
   grant;
2. resolve the committed logical object version and re-check its authoritative
   scope, immutable size, content SHA-256, and durability state;
3. project and persist the canonical EvidenceRef with the immutable evidence
   assertion; and
4. independently read the object back through the daemon boundary before
   Jenkins records or qualifies the dossier.

The projection grants no read, write, retention, promotion, workflow, or
service authority. Missing, stale, replayed, foreign-scope, or ambiguous
authority remains denied and requires explicit manual recovery.
