Expedition retained-dossier writer
==================================

Status
------

Accepted, 2026-08-13. Tracks `issue 157
<https://github.com/sagrudd/DASObjectStore/issues/157>`_.

Decision
--------

``dasobjectstore.expedition_retained_dossier_write.v1`` extends only the
bounded provider-stream upload envelope. The daemon accepts it only when Unix
peer credentials identify the packaged ``mnemosyne-expedition`` service and
the envelope carries the live Prosopikon projection for the same Pistis
session: non-zero authority revision, principal and session identifiers,
expiry, exact entitlement assignment, Operate/Administer source entitlement,
and the normalized ``dasobjectstore.retained-evidence.write`` capability.

The writer is constrained to one configured ObjectStore and a canonical
dossier prefix. It cannot combine local, PAM, root, delegated-user, application
capability, browser cookie, or human CLI authority. Serialized peer identity
alone is never authority.

The daemon reserves capacity and streams the declared bytes through its
existing authoritative SSD ingress. It verifies size and SHA-256, commits the
catalogue and configured HDD acknowledgement, constructs ``EvidenceRefV1``
from committed facts, and reopens and hashes the object independently before
returning the terminal receipt. Exact replay is idempotent. Different content
at the same immutable key conflicts. No path, credential, bearer, or storage
implementation detail is returned.

Recovery
--------

Loss of the terminal response is handled by replaying the identical request.
Expired, changed, ambiguous, or revoked authority requires a fresh Pistis
session and Prosopikon decision; it is never repaired from DAS local state.
