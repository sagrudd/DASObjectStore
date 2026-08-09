# Jenkins dossier evidence settlement

Status: daemon-owned production read-back boundary

`dasobjectstore.jenkins_dossier_evidence_settlement.v1` is the sole DAS daemon
operation which returns a retained Jenkins dossier evidence result. Its input
contains an exact canonical dossier projection and a peer-bound verified
Pistis subject. The request is accepted only from the packaged DAS GUI/API
service peer; direct root, sudo, local account, PAM, cookie, application
capability, and POSIX-delegated authority forms are absent and denied.

Before returning a response, `dasobjectstored` derives the canonical
`jenkins.dossier` `EvidenceRefV1`, obtains the object only through the existing
provider-stream boundary, reads at most the declared object size plus one byte,
and compares the independently observed byte count and SHA-256 digest with the
immutable reference. Provider metadata disagreement, short/extra bytes,
digest drift, unverified scope, a wrong peer, and every unsupported request
shape fail closed.

The response is non-secret and contains the exact EvidenceRef, verified byte
count, content digest, request identifier, and daemon observation time. It is
not a credential, approval, Monas session, custody signature, promotion,
retention policy, or workflow authority. Jenkins must retain it only alongside
its exact Monas-authenticated dossier and DAS object reference. Monas/Pistis,
Site Trust, Thesaurophylax custody, and Jenkins approval remain separate
mandatory producers and consumers.
