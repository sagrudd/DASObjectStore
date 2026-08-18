# Synoptikon demonstration projection contract

Status: owner-side local-daemon transport prerequisite; inactive and not a live projection

`dasobjectstore.synoptikon_projection_request.v1` and its readiness and
settlement records replace the rejected direct `/srv/dasobjectstore` bind.
DASObjectStore owns object creation, catalogue mapping, durability settlement,
export readiness, and replay evidence. The Synoptikon plugin receives no
managed path and cannot select a bucket, endpoint, storage disposition, host,
or identity.

The deployment slice is deliberately fixed from producer `syno_plug_demo` on
`nuc-192-168-0-193` to consumer `oikodome` on `gb10-192-168-0-48`, using the
reviewed TLS endpoint `https://192.168.0.193:3900`. The daemon now exposes a
fixed-peer Unix-socket prerequisite: prepare derives the store, object,
version, generation, nonce, and expiry; provider upload accepts at most 1 MiB
of exact digest-bound bytes; settlement derives live catalogue and verified
HDD evidence; and provider readback requires the opaque terminal settlement.
The ledger is retained under the descriptor-protected
`projection-authority` directory and advances its owner sequence durably before
readiness publication. Exact prepare and terminal settlement retry return the
same records after restart.

The separately packaged gateway adds fixed, parameter-free port-3900
intent/bytes/readback routes. The projection wire protocol is explicitly
HTTP/1.1 only because SigV4 binds the literal fixed `Host` header; HTTP/1.0 and
HTTP/2 are rejected before credential, body, or daemon processing. The package
does not provision the projection-purpose credential or client trust inputs,
start port 3900, add an enabled unit, mount a Synoptikon route, or make jobs
available. Those remain a separately reviewed consumer and activation gate.

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
replay; changed replay is denied. The local daemon ledger durably reserves each
monotonic authority sequence before readiness can be published; a failed
readiness observation consumes a sequence rather than allowing reuse.

The returned `hdd_settled` record is path-free and digest-binds the canonical
request and readiness records. Exact replay returns the same record; changed
time, generation, source, object identity, or readiness conflicts. The fixture
and local-daemon tests make no claim that the NUC endpoint, Garage, Oikodome,
Synoptikon, or the demonstration is running.
