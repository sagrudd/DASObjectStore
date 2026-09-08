# Measured Garage HEAD role mismatch — accepted compatible source correction

Root independently reviewed and accepted design commit a2256844 for source
implementation under delegated coordination; no owner activation implied.
The explicit-region actual fixture measured
HEAD403 using the correctly provisioned W-only writer; see
`evidence/garage-region403-run.txt`. No change to W/R grants, wire records,
sealed policy, receipt encoding, handoff lifecycle or public generic ports.
This compatible internal correction belongs to the open0.186.0 source delivery.

## Exact minimal implementation

1. Add private `runtime/custody_garage/retention.rs` with a concrete helper
   `retain_garage_custody_object_with_readback(path, input, &mut writer, &reader)`.
   Before backend activity, require the two actual Garage adapters to match
   endpoint and bucket and have distinct identities. Existing object-service
   retainer still validates those identities against the sealed profile.
2. Helper constructs two short-lived borrowed wrappers: a writer view whose
   identity and conditional PUT delegate to the real W adapter, but whose
   `object_state` delegates HEAD to the real R adapter; and a readback view
   whose identity and GET delegate to that same R adapter. Both observer and
   readback borrow the reader immutably; no secret/environment cloning or new
   credential consumption is needed.
3. Extract the reader's existing GET body into a private shared-borrow method;
   the existing public trait implementation delegates unchanged. Extract HEAD
   parsing/policy checks into one shared helper used by legacy writer HEAD and
   the role-correct observer. Exact digest/length/retention/hold metadata and
   missing-object classification remain unchanged, with403 always an error.
4. Replace only the concrete calls in `custody_service.rs` single-object retain
   and `custody_service/batch.rs` finite retain with this helper. The existing
   writer and reader are still created from the same one-use sealed pair.
   Public legacy traits/constructors and object-service retainer stay compatible;
   no generic new observer trait or supplied admission assertion is introduced.

The object-service algorithm remains unchanged: initial HEAD, sole conditional
PUT, post-PUT exact metadata HEAD, independent GET, then immutable receipt and
ledger. Existing objects cannot be adopted; failures preserve claims/objects.
The observer is a read capability, never extra backend permission for writer.

## Regression matrix

- A role-enforcing command fixture rejects W HEAD/GET and R PUT; both single
  and finite service routes succeed only with R HEAD, W PUT, R HEAD, R GET.
- Observe both HEAD calls using exact sealed reader environment, including
  post-PUT verification; no old writer credentials leak into read operations.
- Foreign endpoint/bucket or same identity denies before any backend command.
- Missing/bad policy/retention/digest/length metadata, wrong readback bytes,
  foreign identities, unledgered existing object, failed conditional PUT and
  failed post-PUT HEAD deny with no success receipt; retained prior state stays.
- Existing one-use/reconstruction and completed-prefix batch tests remain.
- Actual pinned Garage fixture rerun uses unchanged exact W/R grants and fixed
  region, then verifies resulting catalogue/ledger/receipts. It is not a native
  WORM, Compose, independently admitted companion or physical-verifier claim.

No actual run until this source correction and its frozen executable are reviewed.

## Implemented source checkpoint

The private helper and both service routes now use the borrowed composition.
Legacy writer HEAD delegates to the same extracted parser with its original
environment; legacy reader trait delegates to the unchanged shared GET body.
Single-object credential identities now receive the same explicit sealed-role
preflight already present in the batch route. Public/wire APIs are unchanged.

Role-enforcing service fixtures now return403 for W HEAD/GET or R PUT; the
existing single and finite success tests would fail with the old composition.
New tests cover both HEAD observations using R, every required policy field,
actual selected retention rather than a default, foreign endpoint/bucket and
same identity before I/O, foreign sealed reader before backend, and403 on
either initial or post-PUT HEAD with zero receipts and preserved ledger/objects.
Final Mac daemon allfeatures library985tests passed10.83s (0failed/0ignored),
including the selected-retention assertion. Strict daemon allfeatures/alltargets
Clippy passed13.05s; workspace formatting and diff checks passed. All commands
used locked/offline dependency resolution and the existing shared cache with
incremental compilation disabled. Real Garage qualification remains separate.
