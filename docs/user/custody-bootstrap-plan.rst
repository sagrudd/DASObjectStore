Custody bootstrap input planning
========================================

This source-only 0.182.0 tool checks supplied JSON for internal consistency.
It does not inspect a NUC, establish custody, authorize execution, or create
release evidence. All observations and evidence digests remain untrusted,
including affirmative independence, exclusion and availability booleans.

The accepted first slice is documented in programme PR #251. Kanon source
coordinates are proposed in Kanon PR #331; immutable r239 DAS 0.181.0 package
selection and r237/r7 remote 0.177.4 remain unchanged. No package installation
of this source tool is selected by this change.

The only command is::

   dasobjectstore-custody-bootstrap-plan plan --manifest /absolute/manifest.json --observation /absolute/observation.json

Inputs must be regular, bounded files at absolute paths without symlink path
components, ``.`` or ``..``. Reading may update filesystem access metadata;
the adapter does not open files for writing, create output files, access
credentials, start services or contact a network. Only redacted output is
written to stdout; failures use fixed public denial codes on stderr. An exit
status of zero means consistency only, never permission to execute.

Wire schemas are ``dasobjectstore.custody-bootstrap-manifest.v1``,
``dasobjectstore.custody-bootstrap-observation.v1`` and
``dasobjectstore.custody-bootstrap-plan.v1``. Every field is required;
unknown and duplicate JSON keys, including inherited nested custody fields,
are denied. Integers are bounded, floats are denied and each input is limited
to 1 MiB. Exact raw input bytes, including whitespace, are SHA-256 bound.

The closed manifest names the target, canonical source/executable/image and
qualification digests, compiler and literal locked build/test commands,
separate role-labelled data/metadata/catalogue/ledger/configuration paths,
isolated endpoints and service identities, external marker, finite per-store
inventory, retained read-only continuation, and independent verifier/TLS/
journal/exclusion evidence digests. Existing custody store and retention types
remain authoritative; no second retention schema is introduced. Only the
builder-corpus, NUC-delivery and terminal-receipt store purposes are accepted.

The planner rejects contradictory freshness, path/endpoint aliases, role reuse,
changed target, invalid time/retention/hold, missing continuation and asserted
independence contradictions. A supplied evidence hash is only a binding slot;
this tool neither reads nor verifies the named evidence or actual isolation.
Its output always says ``live_target_verified: false`` and
``execution_authorized: false``. No apply mode or runtime capability exists.

Programme PR #253 corrects terminal-receipt planning: corpus/delivery stores
use ``content_policy.kind=preknown_inventory`` with exact current-batch objects.
The terminal store instead uses ``generated_terminal_receipt``: one future
executor-only receipt, fixed schema, 65536-byte bound and exact target/companion/
attempt/marker/outcome/time/inventory bindings. It has no precomputed digest,
payload or signature. Unknown future package bytes are not preknown inventory;
later batches need separate bounded review and an as-yet-unimplemented consumer.
The planner's ``object_count`` excludes that future receipt and reports
``generated_receipt_limit: 1`` separately. No receipt is generated or accepted.

Formal runtime composition, independently measured old-client exclusion,
credentials, one-use markers and the exact execution companion remain outside
this slice. A real package still requires complete Kanon/Terraform provenance
and all formal installation gates; this utility is not an escape hatch.

Connected service composition
-----------------------------

Source version 0.183.0 extracts the existing daemon custody operations into a
custody-only service controller. The existing daemon admission and retention
entry points delegate to that controller. It requires explicit custody and
excluded ordinary-plane configuration, a resolved catalogue and the existing
attended credential authorities. It has no ordinary registry, ingest or service
start/stop operation, and no default custody configuration or catalogue.

Programme PR #265 accepts this connected source extraction only; Kanon PR #339
coordinates the prospective version. The planner remains non-authoritative,
the selected 0.181.0 package is unchanged, and no apply command or execution
admission is added. Configuration comparisons are not evidence of actual
network, filesystem, old-client or administrator exclusion. The inert service
templates remain inert: this change does not establish restart policy,
retained read-only service continuation or a live execution companion.
