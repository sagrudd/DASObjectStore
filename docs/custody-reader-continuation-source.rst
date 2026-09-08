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
