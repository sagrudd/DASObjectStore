Finite authenticated custody reader session
==========================================

Status: PROPOSED source architecture; no implementation or activation authority.
Date: 2026-09-08. Owner: project owner with independent source-design review.
Issue: https://github.com/sagrudd/DASObjectStore/issues/210.
Baseline: ``a6228f1626d2870d2464e1a59955dbf00f01fd4e`` (DAS #209).

Purpose and existing contracts
------------------------------

Connect the accepted finite batch and bounded read primitives to an actual
reader-only process and the existing off-NUC verifier journal. Do not introduce
another receipt-inventory authority, signed-attempt format or generic service
manager. The programme custody bootstrap design requires a distinct reader
service identity, full declared-retention availability and fail-closed fresh
verification for formal S4--S8. This design does not narrow that period to the
first S4 call or to the writer's short execution window.

Reusable source is ``CustodyServiceController`` and its immutable catalogue;
``retain_custody_object_with_readback``; ``verify_custody_readback_existing``;
``BoundedGarageCustodyReader``; ``CustodySignedPreReadRequestV1``;
``CustodyOffNucJournal::perform_pre_read``; ``CustodySignedAttestationV2`` and
``consume_for_formal_gate``. Existing signed record schemas and Ed25519
algorithm remain unchanged. Their availability does not establish a live
signer, independently measured target or admitted execution companion.

Selected lifetime proposal
---------------------------

Use one continuously running, reader-only process for the explicitly finite
declared retention period. It consumes the existing sealed Reader handoff
exactly once on its initial start and owns the resulting credential only in
memory. No new credential, saved secret, refreshed reference, resealed profile
or credential reopening is proposed. Restart is disabled. A lost reader
process or reboot causes loss of custody eligibility and denial of subsequent
gates; it does not authorize reusing the consumed handoff. Objects/holds/ledger
remain preserved. Availability monitoring may detect this failure, not repair
or convert it into a new attempt.

This is viable only if the companion's availability plan genuinely permits
that continuous process to remain available until at least the declared
retention end. It is not a claim of reboot continuity. If operational needs
require restart/replacement without loss of eligibility, a separately reviewed
reopening contract is actually necessary; this proposal does not smuggle one
in. Legal holds remain after any process expiration, and stopping a reader
never authorizes deletion or clearing a hold.

The reader service identity differs from the bounded writer/provisioner
identity, as the planner already requires. Its mounts expose only its exact
catalogue/ledger read-only, private scratch and private backend connection.
It has no writer/provisioner credential, normal registry, management socket,
service-control or arbitrary filesystem authority.

Initial handoff and connected batch
-----------------------------------

The existing 0.184 batch consumes and drops both handoffs. It therefore cannot
be followed by this reader process using the same reference. Preserve that
API unchanged and add a connected composition, rather than pretend otherwise:

1. After independent companion/lifecycle admission, start the separately
   constrained reader once, consuming Reader in its own boundary. It validates
   exact catalogue/store/profile, selected finite inventory and writer peer.
   It starts in ``BootstrapReadback`` with a bounded private local channel.
2. The writer consumes only Writer once. Its actual conditional retainer uses
   a concrete channel-backed ``CustodyObjectReader`` connected to that reader,
   not a new fake provider or a caller-supplied readback assertion. Reader
   credentials never leave the reader process.
3. During ``BootstrapReadback``, the private reader accepts only exact selected
   content-addressed keys and expected lengths from the fixed authenticated
   writer peer. This readback precedes the ledger receipt and therefore cannot
   incorrectly call existing-receipt verification. It uses the concrete bounded
   GET adapter and exact expected digest/length. There is no public access in
   this state and no arbitrary key or policy selection.
4. After actual completed retention, a one-shot local seal operation verifies
   the entire selected committed ledger/receipt set, closes the bootstrap
   channel permanently and enters ``ReadOnly``. Public verification reads use
   the existing-only snapshot operation from #209. Writer/provisioner may then
   stop without stopping the reader or Garage/data plane.
5. Any partial batch, uncertain seal, deadline or binding mismatch enters
   ``Denied``. No public read success or automatic retry follows. Consumed
   handoffs and retained state survive, even though the in-memory process state
   itself is not a recovery journal. A fresh process cannot reconstruct a
   Reader credential from those public records.

The source protocol for the private channel is deliberately small: a
length-bounded request discriminated as readback (exact key and u64 size) or
seal (exact selected inventory digest), with fixed result/error responses.
Peer identity is kernel-authenticated on a lifecycle-created private Unix
socket, bound to the selected writer UID and exact process transaction.
The lifecycle-supplied private socket and admission remain real prerequisites;
an arbitrary UID string or caller boolean cannot create them. No normal DAS
API gains this channel. Exact frame bytes and conformance vectors require
review before protocol implementation; this ADR selects architecture only.

Finite remote endpoint and authentication
----------------------------------------

Propose a dedicated TLS read-only endpoint, separate from normal daemon routes,
with one bounded operation: ``POST /custody/v1/read-object``. Only the
independently selected verifier's pinned TLS client identity is accepted;
that peer is bound to the already pinned ``CustodyEd25519AuthorityV1``.
The request body is the existing exact JCS ``CustodySignedPreReadRequestV1``,
not a new signed-attempt envelope. Validate its existing signature, time,
purpose and complete target/store/namespace/policy/ledger/inventory bindings
against server-owned admitted configuration before reading any object.

The real off-NUC client invokes this operation only inside
``CustodyOffNucJournal::perform_pre_read``, after durable attempt consumption.
Mutual peer authentication and that reviewed executable composition provide
the boundary; the endpoint need not independently prove the remote journal's
state by inventing another marker/signature format. A bare captured signed
request without the pinned verifier transport identity is insufficient.
Disable transport retries and redirects. Reject duplicate attempts within the
reader process before backend access. A process restart cannot restore its
credential; the independent journal remains the durable attempt authority.

Each request selects exactly one existing ``CustodyIntegrityReceiptV1`` by
its already bound ``receipt_jcs_sha256``, within the independently selected
finite backend inventory. The server derives key and size from that receipt;
no additional caller key or policy is accepted. Return that existing receipt,
the exact raw object bytes and verified configuration/ledger-head bindings.
No aggregate custody receipt or attestation format is added. #209's exact byte
and deadline bounds apply. The driver processes the complete selected object
set with a separate journal-minted attempt and existing attestation for each.
An incomplete or failed set cannot qualify the whole custody inventory.
Exact bounded HTTP framing is to be frozen with conformance vectors before
protocol code, not implemented as an unbounded body meanwhile.

Reuse r237 inventory and receipt semantics
-----------------------------------------

For the initial r237 NUC-delivery selection, reuse Jenkins 0.112/0.113 rather
than invent an inventory codec. ``r237_manifest`` already checks the 13 ordinary
roles and receipt attachments; ``r237_marker::delivery_digest`` hashes JCS of
the exact 17 sorted members, each ``{name,sha256,size}``. Include manifest and
custody receipt bytes exactly as that algorithm does. The accepted
``nuc-custody-receipt.jcs.json`` binds ordinary and attachment inventories and
terminal schemas. Its raw digest is already computed by ``verify_custody``.
These are 17 consumer-visible files, not a mandate for 17 Garage objects.
There is likewise no accepted two-aggregate-object serialization to assume.

The exact companion must select the backend object mapping. Prefer the simple
1:1 mapping of those same named bytes to existing content-addressed objects
when it fits the selected inventory, rather than invent an archive parser.
That is a proposed companion choice, not a new interpretation of the r237
contract. Any aggregation would require an actually specified/reviewed encoding.

Keep each signed request's ``receipt_jcs_sha256`` bound to its actual retained
DAS integrity receipt, not the unrelated NUC delivery custody receipt. Bind
``inventory_sha256`` to the companion's exact approved backend inventory;
do not silently substitute the 17-file consumer digest when mappings differ.
Reuse existing selected raw inventory/receipt bytes and validate any digest
prefix conversion exactly once. Do not introduce a generic receipt-inventory
schema to fill an unselected mapping.

A strict in-process complete collector requires one accepted existing proof
for every selected backend object, rejects duplicate/missing/foreign proofs,
and reconstructs the exact 17 named raw consumer files using only the selected
mapping. Then run the existing Jenkins delivery validator and digest algorithm.
The collector is not itself a signed receipt, admission token or new canonical
serialization. Individual proof success never stands for complete-corpus
success. Formal consumption must cover the full expected set; partial
consumption/failure remains terminal and cannot authorize a gate or read retry.

Off-NUC verification and authority limits
----------------------------------------

The client checks all selected bytes, sizes, receipts, policy/hold/retention,
inventory and raw receipt bindings and independently measured endpoint/source
facts. Target-reported executable/configuration claims are not independent
measurements. Existing journal first-attempt and terminal rules apply on every
error, timeout, crash or incomplete stream. Only the actual selected off-NUC
signer may produce the existing successful attestation, and the existing
formal consumer must atomically consume it. This design does not supply that
production signing/measurement authority by a test callback.

An r237 delivery read does not qualify all three required custody stores or
authorize S5--S8. Fresh selected evidence and the actual gate's independently
admitted companion remain necessary. The local class currently says direct
DAS-supported S3 endpoint: the proposed dedicated TLS object operation is
a supported DAS custody-read adapter, not raw Garage or an S3-compatibility
claim. That terminology/adapter boundary needs explicit programme review,
not silent substitution in an implementation.

Decisions versus execution choices
----------------------------------

Source review must settle this connected reader-before-writer topology,
private-channel and TLS framing/limits, the exact existing-inventory mapping,
and the supported endpoint adapter boundary. No code before those concrete
choices and vectors are accepted; no new cryptographic purpose is required.

The owner execution companion selects actual service identities/UIDs, private
socket, configuration, namespace/network isolation, endpoint TLS identities,
the existing handoff references, finite inventory and full retention/availability
window, off-NUC signer/measurement/journal and failure containment. Nothing
here selects or provisions a key/account/path, grants a pre-package execution
exception or supplies absent authority. New stable coordinates must be Kanon
coordinated before implementation or packaging, not invented as active IDs.

Required connected qualification
--------------------------------

Real separate synthetic processes must prove Reader consumption once, Writer
consumption once, exact private peer binding, actual conditional batch readback,
seal and public read of actual committed bytes. Test all old normal clients
and writer attempts against the public reader, wrong TLS peer/signature/target,
inventory substitution, partial retention/seal, deadline/overrun/disconnect,
concurrent/repeated attempts, and off-NUC journal restart denial. Stop writer
and prove the reader still serves fresh reads; stop reader and prove subsequent
formal verification fails without credential reuse. Preserve held objects and
ordinary synthetic data throughout. Native isolation/lifetime qualification
and admitted target capability remain distinct from these source tests.
