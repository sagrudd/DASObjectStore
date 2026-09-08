Custody reader continuation source boundaries
================================================

Source 0.186 follows accepted DAS ADR0011 (merged #211) and programme #267,
with Kanon #339 source coordination e81c950. It selects no live credential,
unit, key, provider, host action or release gate. The qualified release path
still needs the real independently admitted companion decoder and lifecycle.

The first connected source checkpoint supplies closed binding/current/seal
codecs and verify_reader_seal_existing. The latter opens a real existing
quiesced SQLite ledger read-only, shares the existing schema/configuration/
event-chain/receipt algorithms, and checks the complete selected object and
receipt bijection with a deadline. It creates/migrates/repairs no ledger and
fetches no object. This is not a fresh content read or off-NUC attestation.

Inventory byte encoding remains the existing companion's contract. The caller
supplies its trusted selected digest and object list together; this low-level
verifier does not claim to authenticate arbitrary raw inventory bytes or mint
admission from self-agreement. Actual release composition must produce those
inputs from the independently admitted companion, not a caller boolean.

The contract requires stable logical reader identity across backend key
generation changes; this checkpoint does not implement generation rotation.
Codec transition validity is not proof of backend revocation.
No initial source checkpoint claims qualified systemd continuation delivery,
backend rotation, endpoint authentication or implemented complete lifecycle.

Checkpoint validation comprises three codec tests against the accepted public
goldens and four independently authored real SQLite tests: unchanged successful
inspection, head/hash/object-set conflicts, missing/sidecar denial without
creation, and oversized corrupt receipt denial without repair. These are source
tests, not systemd delivery, Garage content or installed custody qualification.

Continuation implementation boundary
---------------------------------------

The next daemon source slice inspects effective ciphertext protection without
decrypting or selecting a key. Constants and field layout are from systemd v259
commit ``9ca433482f2281d71718718705ca8cd3bf562ad6``, files
``src/shared/creds-util.h``, ``src/shared/creds-util.c`` and
``src/creds/creds.c``. Only the host, TPM2 HMAC and combined host/TPM2 HMAC
stored identifiers are supported. Null, scoped, unknown and unsupported modes
deny. Header classification is not authentication: actual systemd activation
must authenticate the entire credential, name and lifetime. The stored header
cannot prove which command-line creation option was used; the independently
admitted provisioning transaction must select an explicit mode, not auto.

Backend endpoint and AWS helper path/hash are explicit trusted companion
installation inputs. The helper's actual bytes are hashed before acquisition.
The reader-process executable is separately compared with the binding. Existing
backend authority and TLS digests retain their independently measured meanings;
they are not inferred from the URL or replaced by its hash. Actual admitted
companion decoding, backend authority/TLS measurement and platform-positive
qualification remain required before release eligibility can be claimed.

Initial publication uses an existing manager-owned, non-service-writable
directory. The directory must initially be empty. An exclusive, synced
``manager.claim`` precedes a second complete ledger/source check. Create-only
``seal-<raw-sha256>.jcs``, ``binding-<raw-sha256>.jcs`` and
``state-<raw-sha256>.jcs`` retain the exact accepted record bytes; ``current.jcs``
is published last, then the claim is removed and the directory synced. An
incomplete operation retains its claim and public prefix. Failure after unlink
may instead leave the complete publication with a lost return. Both deny manager
re-entry; no adoption, cleanup, recovery or retry path is supplied. A durability failure cannot establish a
power-loss persistence guarantee. Reader loading requires exactly those four
final files and no claim or extra history. This is initial-only publication,
not a replacement/revocation state machine or an anti-rollback store against
a malicious manager. The admitted manager directory remains outside the
service's write and restore scope.

Child file operations use a retained directory descriptor and no-follow opens.
Reads reject hardlinks, nonregular files, unsafe owner/modes, oversized bytes
and changed descriptor/path metadata. Credential/ciphertext files additionally
deny group/other permissions. Path guards assume the independently trusted
manager/root is not malicious; they do not grant trust to a service-owned path.
No production endpoint invokes this source library in this release slice.

The Linux inspection adapter is intentionally restricted to systemd v259's
typed credential properties, cgroup v2 and a numeric/local passwd service user.
It requires the selected actual MainPID/cgroup, PrivateMounts, no pending unit
reload, a stable own mount namespace and an actual dedicated read-only
credential mount. It does not read PID1's inaccessible namespace descriptor.
The process start timestamp must be later than both the manager's latest reload
start and its boot unit-load finish; those two observations must remain unchanged
across the probe. They are not a paired interval: the initial reload timestamp
may be zero, and the boot finish remains earlier than a later reload start.
This conservatively denies an old reader after any daemon-reload until
an independently admitted new activation; the adapter never performs a restart.
Actual activation admission and native qualification remain necessary. Unsupported
metadata shapes or missing protections deny rather than falling back to ordinary
files. The probe reads no ciphertext or secret and grants no companion authority.

Thirteen focused daemon tests pass on macOS: five metadata/format negatives,
two synthetic header-classification cases, three real protected-file cases and
three independently authored publication cases using actual ledger APIs. The
latter cover success/replay, preflight inventory denial and preexisting partial
claim rejection, not a process crash between every fsync/publication step. The
original checkpoint did not cover the interruption matrix or Linux-only process
tests. Later source/native receipts identify their exact successor heads; real
systemd positive qualification remains separate and no power-loss durability
claim is made by these process tests.

The subsequent manager-reload refinement has six focused metadata tests passing,
including legitimate initial zero-reload and reload-start-after-boot-finish
cases, changed observations and old activation denial. This is still portable
parser/ordering evidence, not a successful native systemd credential load.

Publication interruption successor
----------------------------------------

Publication now holds a nonblocking advisory exclusive lock for the complete
call, before empty-directory preflight through claim removal and final sync.
Each call opens a fresh description of the same verified directory, including
threads sharing one manager. Contention denies without waiting or retry. This
prevents a stale preflight contender from creating a new claim after another
publisher completed. It coordinates cooperating publishers, not a malicious
authorized manager writing directly to the protected directory.

The test-binary-only matrix terminates a child without Rust destructors, or
returns an injected error, at 27 boundaries: exclusive file creation, partial
write, full write, file sync and directory sync for each of five files, plus
claim unlink and its directory sync. Every one of the 54 attempts is followed
by a new process attempting publication. Incomplete prefixes retain a claim
and deny record loading. Post-unlink loss leaves the complete four-record
publication readable but still denies publication re-entry. Every re-entry
must preserve all existing public bytes and the exact ledger bytes.

Additional tests cover four competing processes, a deterministically paused
empty-preflight contender and threads sharing one manager, including an already
held lock. The pause has a finite ten-second rendezvous; test child guards kill
and reap unfinished children on failure. No injection/environment branch exists
in a production build. These tests exercise process loss and error boundaries,
not physical power loss, disk-controller caches or a simulated kernel reboot.

Finite wire codec successor
----------------------------------------

The object-service ``custody_reader::wire`` module implements the accepted
bootstrap read/seal/result and reader result records, exact private length
prefixes and fixed denial responses. It reuses the existing binding/seal record
machinery and the existing signed pre-read parser, signature verification and
time validation. Journal issuance and HTTP ingress share that validation sequence;
the legacy signature schema, u64 sequence and string rules are unchanged.

These are complete-buffer codecs, not an endpoint or network reader. Private
decoding requires separately observed EOF; actual collection must enforce the
server-owned byte/deadline limit before allocation. HTTP checks the exact route,
configured Host, required headers, field/header/body bounds and prohibited
framing, returning only the first request's consumed length. The eventual
connection owner must close after that operation without dispatching pipelined
bytes. No claim is made that HTTP detects arbitrary future bytes before dispatch.
Complete responses bind independently selected metadata and actual content
bytes; partial/extra responses and missing EOF cannot count as success.

Tests match the frozen public private/HTTP frames byte-for-byte, exercise
fragmentation and write-half shutdown using real Unix sockets, and run genuine
existing Ed25519 fixtures through HTTP ingress inside the actual off-NUC
``perform_pre_read`` journal transition. Reopening that journal cannot repeat an
already-started request. Synthetic signatures and sockets are source conformance,
not a qualified frontend. TLS identity/early-data enforcement, server-owned
request-to-ledger/receipt binding, authenticated private peer/session sequencing,
actual bounded transport collection, and admitted lifecycle composition remain
required before an executable endpoint can assert custody eligibility.

Server-owned signed read binding
----------------------------------------

``ExactObjectServer`` now composes an actually loaded ``ReaderContinuation``
with fixed existing receipt bytes and existing request measurement templates.
It accepts no caller-provided reader implementation or admission boolean. The
authority input is the exact selected JCS ``CustodyEd25519AuthorityV1`` record;
its raw digest must match the reader binding. This source refinement names the
authority bytes explicitly rather than substituting a key hash or a URL hash.
Receipt selection is a complete bijection with the protected seal. Every
non-attempt request field must match the server-owned template; only request ID,
nonce, sequence, previous request digest and issue/expiry times vary under the
unchanged signed-request validator. Actual server time is used, not an HTTP
clock field. Backend endpoint selection remains distinct from frontend Host
and independently measured frontend/backend provenance.

Used request IDs and nonces are retained for the entire reader process, including
after expiry. They are claimed before backend access; later failure never removes
them. A 4096-entry bound denies further attempts without eviction, retry or reset.
This bounded local memory is not cross-restart anti-replay: the existing off-NUC
journal remains the durable attempt authority. The server is mutable and handles
one call at a time; a future concurrent listener must serialize access to it.

The new bound read path checks the actual complete raw SQLite file digest inside
the existing verified read transaction, both before GET and before success.
The daemon supplies its no-follow opened descriptor; the object-service verifier
compares that descriptor to the guarded path, streams fixed-size chunks under
the remaining deadline, and checks identity/metadata again. It never substitutes
the event-chain head for the raw file digest. Historical unbound read behavior
is unchanged. Response configuration/head/receipt data comes from the verified
snapshot, with fresh protected current-selection checks before return.

This is not yet a TLS listener. The public library operation assumes its caller
has bound the actual transport peer before invocation; the subsequent concrete
TLS adapter must enforce that boundary. Policy tests exercise synthetic selected
data, not a fabricated successful continuation load. Actual raw-ledger tests
exercise real SQLite with wrong digest/descriptor before-effect denial and
same-length post-acquisition mutation denial. Platform-positive continuation
loading, native credentials and live admission remain separately qualified.

The dedicated systemd memory-credential path now recognizes only the exact
root-owned selected-UID read ACL emitted by the qualified systemd version, or
its selected-UID-owned private fallback. It rejects extra ACL principals or
permissions; generic private-file checks are not relaxed. The secret byte buffer
is fixed-size and wrapped in ``zeroize::Zeroizing`` from allocation through
decode, including all errors, without plaintext reallocations. The same existing
deadline is checked around each bounded memory-file read. Kanon #339
``9842f59`` coordinates the direct existing locked zeroize 1.9.0 dependency;
no dependency version, credential schema or production identity changes.
The separate VM evidence binds the actual probe it ran, not this full loader
or a future authenticated endpoint.

Exact outgoing journal binding
----------------------------------------

The additive ``perform_pre_read_exact`` client entry point validates the existing
signed envelope and compares its original bytes and digest to the retained issued
row inside the same transaction that writes ``started``. Denormalized target,
nonce, sequence, predecessor and time columns must also match those exact bytes.
A differently signed envelope with the same ID cannot borrow an older permit.
The callback receives the checked original bytes only after commit. The prior
request-ID-only entry point and persisted journal schema are unchanged.

This new path uses the existing whole-call deadline, opens an existing journal
only, denies lock contention without a busy wait, and installs the SQLite progress
hook. Callback failure is retained as incomplete while the budget permits; if
the deadline is exhausted, the durable started marker remains and still denies
retry. No deadline or journal is created afresh for failure settlement. Tests use
real SQLite and synthetic Ed25519 records, including resigned substitutions,
corrupt columns/digests, legacy-start reuse, missing/busy journal and deadline
expiry after the callback. This establishes the real client attempt boundary;
it does not itself make a network connection or produce an attestation.
