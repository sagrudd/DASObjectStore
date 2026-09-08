# Actual Garage to protected TLS continuation — source fixture, execution held

Root reviewed the bounded join and explicit Garage-only region correction.
This extends the successful admission/batch fixture; it does not inherit a
passing joined result from the prior loopback responder or activate a service.
The immutable Garage8ced2ad3/public5e516cee selection and no-NIC, sole-KVM,
4GiB/2CPU/256PID/900s isolation remain unchanged. No metrics/admin endpoint,
guest Docker, extra host capability or new credential protocol is introduced.

## Connected path

1. Existing genuine controller admission/finite retention produces two actual
   receipts, full ledger SHA and existing definition/inventory records in a
   private guest test envelope. This envelope is not a production wire schema.
2. Stop the bootstrap test. A separate guarded UID2002 test invocation imports
   a newly generated read-only key under the sealed logical reader identity,
   grants R, revokes original W/R grants, and checks the actual bucket table.
   New key identity differs from both old keys. Original W direct PUT/GET and
   new R PUT must receive actual403; no grant is widened to make a probe pass.
   A separate private test-probe copy of W is generated before handoff, never
   read through an old consumed reference, and removed after denied probes.
3. Root copies the quiesced ledger bytes to its private manager root, requiring
   matching full hashes and original identity/owner/mode/ctime before/after.
   Complete real receipt set feeds the existing seal checker. The new secret
   binds the actual sealed-configuration digest, not the earlier provisioning
   definition digest, and is encrypted through real systemd-creds. Original
   private generation material is removed; no writer secret reaches UID2000.
4. Existing ReaderManager initial generation1 publication and actual protected
   ReaderContinuation load feed ExactObjectServer/ServerTls and the separate
   UID2001 ExactObjectClient. One actual retained object is selected for GET;
   the seal still covers both objects. AWS/FIFO contacts Garage directly.
5. Stop/start Garage preserving its data and restart the reader with the same
   encrypted source/current/seal. Require a different retained reader PID and
   unchanged full publication/ledger hashes. A new independently signed request
   uses the same preserved verifier journal. Old requests stay non-retryable.
   A subsequent binding-mismatch case must deny; no provider GET-count claim
   is made without actual instrumentation.

Actual byte success is deliberately settled through the existing Incomplete
terminal API: this fixture does not produce a formal signed attestation. That
truthful terminal record permits the next distinct request under unchanged
checkpoint semantics; it is not a retry, checkpoint reset or passing custody
attestation. Prior original records must remain terminal Incomplete on reopen.

This is initial protected continuation after bootstrap, not implementation of
existing-current credential rotation. No protected current is replaced. Reader
restart tests reuse the separately provisioned continuation source legitimately;
neither bootstrap handoff is reopened or re-admitted.

## Qualification boundaries

Prior real TLS/loopback responder tests retain their separate exact GET-count,
corrupt/disconnect and journal evidence. This actual Garage join adds backend
persistence and role checks without claiming those counters. Two guest UIDs
are functional separation, not physical off-NUC provenance. Garage remains a
local-trusted-administrator overlay, not native WORM or Compose qualification.
The finite two-object fixture is not a formal17-file storage mapping or owner
companion. Formal S4 admission, actual deployment and full attestation remain
outside this test's authority.

Static shell/source tests and native compilation are separate from execution.
The earlier uncategorized zero-winner concurrency observation remains recorded
in evidence/manager-concurrency-observation.txt; successful repeats alone do
not resolve it. Final native group results and exact frozen runner/artifact
review are required before this new guest mode is run.
