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

Formal runtime composition, independently measured old-client exclusion,
credentials, one-use markers and the exact execution companion remain outside
this slice. A real package still requires complete Kanon/Terraform provenance
and all formal installation gates; this utility is not an escape hatch.
