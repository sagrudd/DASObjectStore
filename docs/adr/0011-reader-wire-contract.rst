Reader continuation records and finite wire contract
====================================================

Status: ACCEPTED SOURCE CONTRACT / NO ACTIVATION AUTHORITY. Issue #210 / PR #211.
Lead agent ``/root`` and independent ``/root/release_capability_audit`` reviewed
exact annex SHA-256
``d3357335da5e5a5970543c5cf172115519ea29cbb9e37685c498310203b4427d``
at source ``04cbfec9a694f81ebbd21086ed15ff9eb2e8a265`` under delegated
source-work authority, not a personal owner signature. Programme PR #267
supplies the accepted source adapter amendment. Implementation is authorized
after both source-contract PRs merge; Kanon #339 coordination
``e81c950cfab7116b0094b795f64291b7a19a1643`` records minor 0.186 and these
source coordinates. Runtime/lifecycle remains unimplemented and the NUC
custody NO-GO is not changed. No production
credential names, paths, accounts, keys or sockets are selected. Schema names
below are source-coordinated, not installed or selected lockset identities.

Encoding and common limits
--------------------------

Every new JSON record defined here is UTF-8 RFC8785 JCS, without BOM, trailing newline or bytes.
Reject duplicate/unknown fields at every level, noncanonical input, invalid
UTF-8 and unsupported schema. Do not silently reserialize received input into
validity. All SHA-256 fields are 64 lowercase hexadecimal characters over
the named exact raw bytes; no prefix or self-digest is implicit. Strings are
nonempty, at most 256 UTF-8 bytes unless stated otherwise, and contain no
control characters. UUIDs use lowercase hyphenated canonical form. Integers
are nonnegative exact JSON integers, at most 2^53-1, with no exponent syntax;
positive fields cannot be zero. Times use UTC YYYY-MM-DDTHH:MM:SSZ exactly.
These scalar restrictions apply only to the new binding/current/seal/private
and response records, not retroactively to existing signed request or receipt
schemas. Existing signed request decoding, signature, u64 sequence, string and
timestamp validation remain exactly the existing custody_attestation contract.
The 1 MiB envelope ceiling is an explicit transport resource limit, not a change
to its signature or scalar semantics. Selecting this new profile must satisfy
its new binding limits; it does not claim all historical configurations fit.

Public binding and current selection are at most 16 KiB each. Seal and signed
request are at most 1 MiB each. The selected existing inventory contains 1--4096
objects, unique by content-addressed key; the existing shared inventory checks
size and aggregate overflow. Object size is additionally limited by the
server-owned nonzero maximum_bytes, representable by usize/isize. This is a
resource limit, never a caller policy override. Every call has a server-selected
deadline of 1--300 seconds covering authentication, SQLite, child, framing and
response write; new progress does not reset it. One connection carries one
request. No retry, redirect, compression or pipelining is part of this contract.

Immutable continuation binding
------------------------------

The exact flat field set for schema ``das.custody.reader_binding.v1`` is::

    schema, companion_sha256, host_identity_sha256, service_identity, uid,
    executable_sha256, package_provenance_sha256, credential_name,
    encrypted_source_sha256, credential_generation, backend_key_id,
    reader_identity, store_id, configuration_sha256, endpoint_authority_sha256,
    tls_peer_sha256, read_adapter_profile, frontend_authority_sha256,
    frontend_tls_peer_sha256, stores_namespace_sha256, bucket_name,
    object_lock_policy_sha256, inventory_sha256, seal_sha256,
    verifier_authority_sha256, verifier_tls_identity_sha256,
    not_before_utc, expires_at_utc

uid is a nonzero platform-valid UID; credential_generation is positive.
credential_name matches ``[A-Za-z0-9_-]{1,128}``, without path or scheme. Other
identity strings match the existing independently selected values exactly.
read_adapter_profile is exactly ``das.custody.exact_object_frontend.v1`` for
this proposed opt-in profile. endpoint_authority_sha256 and tls_peer_sha256
retain the actual S3 backend meanings of existing signed requests; the separate
frontend fields bind this TLS HTTP service. Neither measurement substitutes
for the other. A consumer lacking explicit support/selection of this profile
must deny; historical direct-S3 behavior is unchanged.
backend_key_id is a public backend identifier, not a secret or hash of a secret.
encrypted_source_sha256 commits ciphertext only. Binding expiry does not exceed
the admitted continuation interval and must cover the claimed availability;
replacement before expiry requires the admitted lifecycle update, not reader
autorenewal. No private material appears in a binding, seal, response or log.

Encrypted plaintext reuses the existing bounded systemd handoff key=value
decoder, restricted to version=1 and role=reader, with required store_id,
configuration_sha256, identity, aws_access_key_id, aws_secret_access_key and
optional aws_session_token. Its existing duplicate/unknown/blank-field checks
remain; compare decoded identity/store/configuration to the binding and access
key ID to backend_key_id. The separate continuation resolver never calls or
modifies consume_one_use. The manager creates the continuation ciphertext in
its admitted provisioning transaction, not by recovering consumed handoff files.
No plaintext fixture or secret value is included in these public vectors.

The service must run in its own mount namespace with its private systemd
credential directory; the selected systemd encryption mode must be explicitly
host, tpm2 or host+tpm2 as qualified, never auto/null or downgrade on failure.
Unit sandboxing prevents access to other credential directories, source
ciphertexts, writer/provisioner material and credential-manager control. This
is a native isolation requirement, not an assumption based on an environment
variable named CREDENTIALS_DIRECTORY. Missing protections deny activation.

The immutable binding is selected by one protected current record with exactly::

    schema = "das.custody.reader_current.v1"
    binding_sha256, credential_generation, state, updated_at_utc

state is ``active`` or ``revoked``. Generation must equal the selected binding;
active->revoked is terminal for that generation, and replacement increases it.
The manager retains the sequence/audit outside service write and restore scope;
an atomic replacement alone is not anti-rollback. Reader opens the configured
record via its protected parent, checks owner/mode/no symlink and exact bytes,
and rechecks current selection before acquisition and before successful return.
No public endpoint writes these files or selects their paths. Unavailable
manager/currentness/time evidence fails closed. The source implementation must
provide the real protected manager installation/revocation composition before
claiming deployed restart safety; a caller-provided generation is not one.

Backend revocation must invalidate the old key independently of restored local
files. Revocation first prevents successful reads and future activation, then
records durable completion; uncertain intermediate state is unavailable, never
silently active. Generation replacement revokes old backend access before new
activation, accepting a bounded outage rather than overlapping authority.
Deletion of retained objects or legal holds is never part of this transition.

Stable identity is not credential generation
--------------------------------------------

Existing receipts and sealed profile both compare reader_identity exactly;
``validate_reader_identity`` and ``verify_custody_readback_existing`` retain
that behavior. It is the stable logical reader identity, not backend_key_id or
generation. A new continuation generation must preserve it, store/profile,
inventory and policy. The trusted manager verifies actual backend read-only
scope and binds its new key to that logical identity before installation.
The adapter then constructs the existing CustodyRuntimeCredential with that
verified logical identity and the separately delivered backend credential.
Arbitrary relabeling of caller credentials is forbidden. No old receipt,
configuration hash or one-use handoff marker is rewritten during rotation.
A change to logical identity or sealed profile is not a compatible rotation.

Completed bootstrap seal
------------------------

Schema ``das.custody.reader_seal.v1`` has exactly::

    schema, companion_sha256, bootstrap_transaction_id, store_id,
    configuration_sha256, inventory_sha256, ledger_head_sha256,
    receipt_jcs_sha256, completed_at_utc

receipt_jcs_sha256 is the sorted, unique array of hashes of the existing exact
CustodyIntegrityReceiptV1 JCS records. Cardinality equals selected inventory
cardinality, with an independently verified bijection by key/hash/size, not
merely a matching count. This is a completion witness, not a parallel receipt
inventory or new signing authority. The exact raw selected inventory remains
the existing companion input. No cyclic binding: the seal commits the prior
companion/actual receipts; the continuation binding commits the seal; current
selection commits the binding. None contains its own digest.
The prior companion selects inputs and output schemas/transaction, not future
raw seal or continuation-binding hashes. Those are derived outputs retained
after execution. Requiring the prior companion to contain either future digest
would create a cycle and is forbidden. Fixture companion_sha256 is an external
synthetic prior-input digest, not a hash of later records in this directory.

The admitted manager publishes a seal once, create-only and durable, only after
verifying the completed transaction and existing ledger chain. Unknown/partial
bootstrap cannot publish it. Restart validates the exact seal and receipts
against the read-only ledger; it cannot create or repair a seal. Read recovery
does not open the initial private bootstrap channel or reconsume handoffs.

Private bootstrap channel
-------------------------

One accepted connection belongs to the exact lifecycle-created socket and
kernel-authenticated writer peer. UID alone is insufficient: the manager pins
the admitted process instance (PID plus start identity on Linux) and transaction;
if that process dies or identity cannot be verified, deny, never accept a reused
PID. Only the lifecycle manager can install the socket/peer expectation.

Frame = four-byte unsigned big-endian JCS length followed by that many bytes,
then EOF from the sender. Limit request to 4096 bytes. Extra bytes, second frame,
no EOF before deadline or length mismatch deny before backend access. Readback
clients explicitly shutdown the write half after sending the one frame; the
server requires that EOF within the same deadline. The response uses the still
open read half. This private Unix behavior must be exercised with real sockets.
The readback request
has exactly ``schema="das.custody.bootstrap_read.v1"``,
``bootstrap_transaction_id``, ``object_key``, ``content_length``. The key and
length must match the selected inventory; digest is derived from that exact
selection, not a caller field. seal has exactly
``schema="das.custody.bootstrap_seal.v1"``, ``bootstrap_transaction_id`` and
``inventory_sha256``. Same transaction on all requests. Only one call in flight;
duplicate object readback, seal before all readbacks, or any error terminally
denies this bootstrap session. No reconnect reconstructs its state.

Successful readback response = four-byte BE metadata length, metadata JCS,
then exactly content_length bytes and EOF. Metadata has exactly
``schema="das.custody.bootstrap_read_result.v1"``, ``object_key``,
``content_length``, ``content_sha256``. Server validates complete bytes before
sending success metadata. Successful seal returns length-prefixed exact seal
JCS and EOF, no object body. The receiving retainer uses actual returned bytes
for its existing readback algorithm, never metadata alone.

All private failures use length-prefixed JCS
``{"code":"denied","schema":"das.custody.reader_error.v1"}`` and EOF,
with no secret-bearing detail. If a success response has begun, disconnect
instead of appending an error frame. Partial response never counts as success.

HTTP operation
--------------

Propose HTTP/1.1 over mutually authenticated TLS only, ALPN http/1.1, one exact
POST /custody/v1/read-object, no query/fragment or path normalization aliases.
Host must equal the configured authority. Require one Host, Content-Type:
application/json, Content-Length and Connection: close; bound all headers to
16 KiB and 64 fields. Reject Transfer-Encoding, Content-Encoding, Expect,
Upgrade and duplicate framing headers. The HTTP parser rejects malformed
framing headers before dispatch and reads exactly the bounded Content-Length
body. It never dispatches a second/pipelined request and closes the connection
after the response. This does not promise detection of arbitrary future bytes
before the first backend call; HTTP does not require a client write-half EOF.
No TLS early data. Client pins the server TLS identity;
server pins the verifier identity and existing Ed25519 authority separately.

Body is exact existing CustodySignedPreReadRequestV1 JCS, with unchanged schema,
signature and validation. A matching transport peer does not replace signature
checking. The real client calls only inside off-NUC perform_pre_read after its
durable first-attempt transition. Across server restarts that journal remains
the attempt authority; restarting never retries a failed or uncertain request.

HTTP 200 Content-Type: application/octet-stream body uses the same four-byte
metadata-length prefix, followed by metadata JCS and exact raw object bytes.
Metadata has exactly ``schema="das.custody.reader_result.v1"``,
``request_sha256``, ``receipt_jcs_sha256``, ``configuration_sha256``,
``ledger_head_sha256``, ``content_length``. request_sha256 is the raw signed
envelope digest; receipt is already independently selected by that request and
need not be retransmitted or reserialized. The client verifies metadata against
the actual selected existing receipt and request, then verifies every byte.
Metadata <=4096 bytes. Content-Length is exactly 4+metadata length+object length,
checked for overflow. One extra/missing byte or disconnect fails the attempt.

Authenticated application rejection returns HTTP 403, application/json and the
same exact error JCS above. Malformed HTTP may return 400 with empty body or
close; failed TLS closes without HTTP. No redirect, retry-after, challenge or
debug body. A late failure closes the connection; no partial success attestation.
Disable automatic retries in both HTTP and backend clients. The server must
check full selected raw ledger digest and all existing signed request bindings,
not just fields passed through the response metadata.

Accepted source programme adapter clarification
-----------------------------------------------

The local class's direct DAS-supported S3 read requirement would gain an explicit
opt-in ``das.custody.exact_object_frontend.v1`` adapter profile backed by a
real bounded S3 GET. The verifier independently binds endpoint/TLS/routing,
selected receipt, actual full bytes, ledger/policy and declared executable
provenance. It does not accept an ordinary daemon read, a proxy's self-asserted
measurement, or raw Garage credentials. Existing signed s3_endpoint_authority,
endpoint_authority_sha256 and tls_peer_sha256 continue to identify the actual
S3 backend, never this custom HTTP frontend. The continuation binding separately
commits frontend authority/TLS identity and the explicit adapter profile. The
existing routing measurement must bind the independently verified association
between them. Both frontend transport identity and backend S3 identity/route
must be independently measured by the selected verifier, not obtained solely
from the frontend's assertions. If the isolated deployment cannot provide those
independent facts, it cannot select this profile or produce a passing attestation.
The companion must explicitly select this profile in its reviewed configuration
and supported source closure; an old companion/direct-S3 consumer does not
automatically accept it merely because existing signature fields parse. This is
the explicit source programme amendment in PR #267, not a default substitution
for historical consumers. Its merge is required before implementation; no
live companion or custody eligibility follows from source-contract acceptance.

Conformance and qualification matrix
------------------------------------

Frozen public-only vectors under fixtures/0011-reader-wire define JCS bytes,
acyclic hashes and private/HTTP response framing. Synthetic hashes are not
admitted inventory or live provenance. Existing signed request fixtures remain
the signature oracle; this proposal adds no cryptographic fixture authority.
Implementation must run those genuine signature positives and negatives through
the actual HTTP parser and off-NUC journal, not merely a request DTO helper.

Test every field substitution after canonical resealing, unknown/duplicate keys,
non-JCS, integer overflow, wrong schema, expired/current generation mismatch,
old receipt identity under valid key rotation, arbitrary credential relabeling,
write/delete denial, revoked key after restored files, missing/partial seal,
reboot and restart with fresh requests, and replay of an already-started request.
Exercise fragmented/oversized/truncated frames, extra frame/body, duplicate HTTP
framing headers, chunked/compressed input, TLS wrong peer and early data,
concurrent calls, actual child/SQLite deadline, disconnect after successful GET,
and response overrun. Failure counters prove no backend access for ingress
denials and no success signature or gate consumption after partial results.
Native tests prove private credential-directory/mount isolation, no writer or
provisioner credential, actual host protection mode and no automatic downgrade.
Root-only ciphertext on disk is not a substitute for those isolation proofs.
