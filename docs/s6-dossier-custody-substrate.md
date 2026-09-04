# S6 dossier-custody substrate

Version 0.179.0 provides a source-only Rust core adapter for the proposed S6
dossier-custody contract. It is intentionally not a service operation. The
adapter has no socket listener, command, provider configuration, storage
backend, filesystem path, credential, Pistis client, package-builder hook, or
release-train mutation.

## Narrow input contract

The adapter accepts an external strict-JCS
`mnemosyne.expedition.s6-dossier-subject.v1` and streams one `MNS6DCRP` corpus.
The subject is intentionally external to the corpus: placing it in the
complete inventory would include its own raw digest and create the fixed point
which the programme contract forbids.

It accepts only this interoperability tuple:

- profile: `dasobjectstore-0180-nuc-debian`;
- selected products: exactly `dasobjectstore`;
- format and architecture: `deb` and `amd64`;
- corpus package coordinate: `dasobjectstore` version `0.178.0`.

This validation is not an assertion that the 0.178.0 package has been built,
signed, selected, installed, or accepted. In particular it does not make the
0.179.0 custody-substrate source release part of that candidate. The
historical r237/r7 Remote profile, product identity, `0.177.4` package
coordinate, and arm64 architecture are each rejected.

The parser rejects noncanonical or duplicate JSON, unknown/missing fields,
bad envelope magic/version/lengths, overflow, truncation, trailing bytes,
unsorted or duplicate inventory facts, ambiguous paths or media types, member
digest mismatches, a substituted profile/product/package tuple (including any
selection of DASObjectStore Remote), and an absent or inconsistent custody
binding. It keeps the two authority roles distinct: the release authority must
equal the fixed public Kleidophylax S6 PEM and the S3 authority must equal the
fixed public Expedition S3 PEM. Both raw public anchors are retained under
`trust/` and are separately checked as canonical Ed25519 SPKI. The captured
S2 plan must be strict-JCS
`mnemosyne.terraform.sealed-successor-plan.v2` with an exact
`mnemosyne.terraform.s6-signing-authority-selection.v1` that binds the raw
authority-record and PEM digests. A v1 plan, a self-consistent caller key, a
selection mismatch, or an attempt to reuse one PEM for both roles is denied.
Continuity is
unambiguous: a signed predecessor carries an explicit JSON `null` reason and
a source fallback carries a member reference; absence is rejected. It uses the
exact 43-byte domain prefix defined by the Programme contract when calculating
the external subject digest.

## Retention seam and non-authority

`inspect_s6_dossier_custody`, `preflight_s6_dossier_custody`, and
`retain_s6_dossier_corpus` all return `typed_stage_validation_required` for a
structurally valid corpus. Inspection deliberately does not return a usable
preflight or durable evidence. Its internal canonical object-key calculation
is derived only from the complete envelope digest:
`expedition/release-trains/<corpus-digest>`.

This is a fail-closed boundary, not an opt-in switch: raw S0--S5 member
inventory, even with the S2-v2 authority selection, and a caller Boolean or
ad-hoc validator cannot authorise custody. The Programme requires the full
typed S0--S5 cross-bindings and the Jenkins typed-evidence path; those are not
yet available in this source release. Therefore this module cannot create,
read back, receipt, append S6, or claim S6 completion.

The port traits are deliberately not implemented by DASObjectStore 0.179.0.
A later separately approved delivery must first add the real typed S0--S5
validators, bind the fixed-peer grant shape to actual Unix peers and live
Pistis/Prosopikon authority, retain the raw receipt attachments in the selected
immutable store, and obtain independent Jenkins review. That later work also
requires its own Kanon candidate/lock and package provenance. Existing Kanon
registration identifies `dasobjectstore`; it does not register this source
module as a candidate, profile, lock, package, or delivery-ready S6 path.
Until then this module has no authority to contact a NUC or DGX, open a
transport, provision or write an ObjectStore, issue durable evidence, sign a
package, or permit S8.
