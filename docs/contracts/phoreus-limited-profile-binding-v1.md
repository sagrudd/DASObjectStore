# DASObjectStore limited Phoreus profile-binding contract v1

This producer-owned contract defines the only DASObjectStore evidence that the
limited Monas-authenticated Phoreus entry profile may consume: a path-free,
read-only point-in-time readiness result for the logical `phoreus` store. It
is not a capability, provider credential, backend root, package selection,
runtime qualification, governed-work admission, or compute-submission route.

The normative declaration is `phoreus-limited-profile-binding-v1.json`. Its
contract identifier is `dasobjectstore.phoreus-limited-profile-binding.v1` and
compatible consumers accept `>=1.0.0,<2.0.0`. The existing public readiness
schema remains `dasobjectstore.profile_readiness.v1` on
`/api/v1/profile-readiness/stores/{store_id}`. Only a preverified
`mnemosyne-monas` peer for store `phoreus` may supply the associated binding
request; DASObjectStore itself retains authority for those checks.

The readiness result has no embedded timestamp. Monas is the freshness owner:
it signs the observed result into its short-lived host context. Missing,
substituted, incompatible, unready, or unauthorized evidence must therefore
be refused before it is usable by a Phoreus consumer.

Kanon later resolves the declaration against immutable merged-main source
revisions and manifest digests. This declaration does not create a package,
lockset, artefact, or deployment entitlement.
