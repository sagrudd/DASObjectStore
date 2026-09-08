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
is published last, then the claim is removed and the directory synced. A failed
operation retains its claim and any public prefix; no adoption, cleanup,
recovery or retry path is supplied. A durability failure cannot establish a
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
Current unit properties cannot prove that an already running process restarted
after an earlier daemon-reload: the independently admitted activation manager
must bind that lifecycle, and native qualification remains necessary. Unsupported
metadata shapes or missing protections deny rather than falling back to ordinary
files. The probe reads no ciphertext or secret and grants no companion authority.

Thirteen focused daemon tests pass on macOS: five metadata/format negatives,
two synthetic header-classification cases, three real protected-file cases and
three independently authored publication cases using actual ledger APIs. The
latter cover success/replay, preflight inventory denial and preexisting partial
claim rejection, not a process crash between every fsync/publication step. The
full interruption matrix, Linux-only process tests and real systemd positive
qualification remain outstanding; no exhaustive durability or platform claim
is made by this checkpoint.
