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
0.179.0 custody-substrate source release part of that candidate.

The parser rejects noncanonical or duplicate JSON, unknown/missing fields,
bad envelope magic/version/lengths, overflow, truncation, trailing bytes,
unsorted or duplicate inventory facts, ambiguous paths or media types, member
digest mismatches, a substituted profile/product/package tuple, and an absent
or inconsistent custody binding. It uses the exact 43-byte domain prefix
defined by the Programme contract when calculating the external subject
digest.

## Retention seam and non-authority

`retain_s6_dossier_corpus` is a pure port seam. A caller supplies separate
writer and reader ports plus redacted, fixed-peer grant/channel facts. Before
it calls the writer, the adapter requires distinct writer/reader identities,
sessions, principals, entitlement assignments, credential bindings, processes,
caches, upload handles, and staging identifiers. It accepts the canonical
`expedition/release-trains/<train>/<corpus-digest>` key only.

The writer port can report `created`, `already exists`, or conflict. Both a
new object and an equal replay require a complete readback through the separate
reader port. The reader must reparse and rehash the complete envelope; a
provider name, ETag, cache, size, or digest summary alone cannot complete the
operation. On success the adapter produces strict JCS receipt bytes and the
two raw DAS reference attachments, cross-checked against the custody binding.

The ports are deliberately not implemented by DASObjectStore 0.179.0. A later,
separately approved daemon/Pistis delivery must bind the grant shape to actual
fixed Unix peers and live Pistis/Prosopikon authority, retain the raw receipt
attachments in the selected immutable store, and obtain the independent
Jenkins review. Until then this module has no authority to contact a NUC or
DGX, open a transport, provision or write an ObjectStore, issue durable
evidence, append S6, sign a package, or permit S8.
