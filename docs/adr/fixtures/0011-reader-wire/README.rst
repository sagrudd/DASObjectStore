Proposed ADR0011 public wire vectors
====================================

No deployment identity, key, credential, actual receipt or authority is present.
All repeated-letter digests and fixture identities are deliberately synthetic.
These are format/linkage positives, not admission or signed-request positives.
The existing custody_attestation.rs genuine Ed25519 tests remain the signature
oracle and must be integrated with transport tests before source acceptance.

JSON files are exact canonical bytes with no final newline. hashes.jcs.json
lists raw file SHA-256 and byte count for the other frozen JSON/hex files; it
does not hash itself or this explanatory document. Hex files contain lowercase
hex with no newline: decode to obtain the actual transport bytes. Their listed
hashes cover the hex text, not decoded bytes. No TLS record encoding is implied.

seal -> binding -> current is acyclic. revoked names the same binding/generation
and is terminal for it. Public result and readback frames use object bytes abc
and the existing custody/sha256/<digest> key derivation. The HTTP positive is
the successful response framing only, not proof of an authenticated request.

binding-generation-2 and current-generation-2 preserve the exact logical reader,
sealed configuration and seal while changing the public backend key identifier,
ciphertext digest and generation. They follow generation-1 revocation; this is
a compatibility-format positive, not evidence that any key was provisioned.

Mandatory deterministic negative derivations from these frozen positives:

* Change generation in current only: deny before credential/backend access.
* Select revoked as current: deny despite otherwise valid ciphertext.
* Change logical reader identity in binding, reseal current: old receipt/profile
  identity checks still deny; a new backend key cannot relabel logical identity.
* Delete/add/duplicate any key, append LF, uppercase a digest, replace an integer
  with exponent syntax: closed canonical decoding denies.
* Increase/decrease a BE length by one, append another frame or omit EOF:
  framing denies; no bootstrap readback effect for malformed requests.
* Change any abc byte or append a byte: exact body/hash validation denies.
* Duplicate HTTP Content-Length, add Transfer-Encoding or alter its length:
  parser denies; no retry or second request follows.

These derivations specify future executable tests; this docs-only commit does
not claim runtime rejection tests have run.
