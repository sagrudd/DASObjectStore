# Synoptikon demonstration projection contract

Status: owner-side typed adapter foundation; inactive and not a live projection

`dasobjectstore.synoptikon_projection_request.v1` and its readiness and
settlement records replace the rejected direct `/srv/dasobjectstore` bind.
DASObjectStore owns object creation, catalogue mapping, durability settlement,
export readiness, and replay evidence. The Synoptikon plugin receives no
managed path and cannot select a bucket, endpoint, storage disposition, host,
or identity.

The deployment slice is deliberately fixed from producer `syno_plug_demo` on
`nuc-192-168-0-193` to consumer `oikodome` on `gb10-192-168-0-48`, using the
reviewed TLS endpoint `https://192.168.0.193:3900`. A live adapter must use the
existing scoped application identity, token, provider transfer, upload
completion, catalogue snapshot/group-status, and provider read boundaries.
This module does not provision credentials, start port 3900, or activate a
service.

Admission accepts readiness only through an opaque proof created after a
DAS-owned HMAC authenticates the canonical readiness record. The HMAC key is
loaded only from the fixed protected descriptor
`/var/lib/dasobjectstore/projection-authority/synoptikon-owner-hmac.key`; callers
cannot supply a key or path. The descriptor must be a non-symlink regular file
owned by the effective daemon UID/GID with exact mode 0600 and link count one.
The key is absent from consumer records. The authenticated
record carries both the owner-pinned expected TLS peer certificate digest and
the observed digest; they must match exactly and also match the independently
protected fixed consumer expectation at
`/etc/dasobjectstore/synoptikon-projection-peer.sha256`.

Admission denies an unavailable daemon or port 3900, a changed TLS peer,
foreign identities, stale generation, changed source digest, non-current
catalogue, and ambiguous mapping. In particular, the observed 330 unmapped
objects are a hard blocker unless DASObjectStore itself emits an exact
projection/generation/source-digest-bound exclusion settlement for the entire
count. The exclusion is inside the same owner-authenticated record, so a
consumer assertion or caller-deserialised readiness structure cannot exclude
them.

The authenticated readiness is object-specific. It carries the exact SSD
upload-completion receipt, catalogue snapshot and row, provider group status,
one or more verified HDD replica placements, and the HDD settlement reference.
Every record repeats and must match the request's store, object ID, version,
logical key, size, and SHA-256. Requests and readiness records are bounded by a
300-second lifetime, a 60-second observation age, a nonce, and a positive
owner-issued sequence. The terminal settlement retains those bindings and the
authenticated readiness observation time, and returns byte-identically on exact
replay; changed replay is denied. This unmounted typed foundation does not claim
to enforce sequence monotonicity: a future DAS daemon adapter must durably
advance that sequence before it can expose an operational transport.

The returned `hdd_settled` record is path-free and digest-binds the canonical
request and readiness records. Exact replay returns the same record; changed
time, generation, source, object identity, or readiness conflicts. The fixture
is a contract gate only and makes no claim that the NUC endpoint, Garage,
Oikodome, Synoptikon, or the demonstration is running.
