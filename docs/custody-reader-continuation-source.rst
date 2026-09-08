Custody reader continuation source boundaries
============================================

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
