# Maintained custody provider source candidate

The 0.187.5 candidate carries the existing local-authority receipt boundary
forward on P-256 0.14. It retains rejection of high-S receipt signatures and
the existing canonical receipt, key and challenge checks. It selects Pistis
B01, Prosopikon Core and Yew from revision `3092d8c7d39bf2ee02bd98819e9f1ade78eebb16`,
and Proxenos 0.69.3 from revision `dec0c3aafbcb76be40b8ea4eba879583ff48cd36`.

The generated lock selects a single Thesaurophylax 0.93.1 source for its API,
core and policy packages. Package source validation checks each exact lock
record and rejects extra Thesaurophylax Git records, so the existing v2
single-revision provenance field remains complete. It does not substitute
source identity for signing authority or operational acceptance.

A private Linux Rust 1.94 run passed the GUI API, Mnemosyne and GUI web
all-target checks; the high-S twin, exact source/closure and package-source
guard regressions also passed. The resolved lock SHA-256 is
`421b7d86903d483c9c04d1783d4e04a4ebd52b7a55d0cdd15d950d59f92d1fd9`.
The tested source snapshot preceded this documentation-only evidence update.

Kanon source-edge preparation is in [Kanon #819](https://github.com/sagrudd/kanon/pull/819).
Dependency security, MSRV, native package checks, a supported lockset and
release qualification remain open; the diagnostic is not an RPM or signing
approval.
