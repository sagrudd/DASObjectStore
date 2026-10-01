# Internal dependency catalogue

This catalogue records the Mnemosyne-owned sources required to build a
release package. Package builders fail closed unless the pinned sibling
checkouts below are exact and clean; they never fetch private source during a
release build.

| Local path | Purpose and reference | Required revision / branch | Remote | Observed local state |
| --- | --- | --- | --- | --- |
| `../prosopikon` | Identity contracts and Yew components. Pinned by `Cargo.toml`; package preflight is `packaging/pinned-mnemosyne-package-sources.sh`. | `e9d3dd75d79c63e3b68689eb7141c79294cf1305` (`main`, Prosopikon core 0.32.0) | `https://github.com/sagrudd/prosopikon.git` | Primary checkout state is not used for release packaging; package builds require a clean checkout at the pinned revision. Monas and DASObjectStore align on this actor-type source revision so the reconciled installation owner's typed `AudienceBoundActorContext` crosses the embedded boundary without translation. |
| `../pistis` | Canonical, COSE, crypto, and protocol contracts. The direct `pistis-canonical` dependency uses the proposed #526 source. Pinned by `Cargo.toml`; package preflight is `packaging/pinned-mnemosyne-package-sources.sh`. | candidate `e36275d9fba8cfbfcd8d92d25b0cebe6199d4b85` (Pistis #526, proposed 0.16.1; required check pending) | `https://github.com/sagrudd/pistis.git` | DAS's direct Pistis dependency and lock entry use #526. Prosopikon 0.32.0 still transitively resolves Pistis crates from #455 / `14e481497d3838d3310df3b0a21232f5d01d6f9f`; this candidate therefore still has two Pistis revisions and is not a unified consumer lock. The protected main checkout remains unchanged. |
| `../proxenos` | Site Trust consumer used by the appliance and standalone `dasobjectstore-remote` client. Pinned by `Cargo.toml`, checked in `Cargo.lock`, and recorded by package preflight. | `56d4853f57c59a3303124ae4f33800170ed9be44` (protected `main`, Proxenos 0.62.0; merge of PR #290) | `https://github.com/sagrudd/proxenos.git` | Builders require the exact clean checkout and reject a substituted or unlocked Proxenos source. |
| `../thesaurophylax` | Custody API, core, policy, and store transitive through Proxenos. Its exact API declaration and all four Cargo lock records are verified by package preflight. | `eb2f180ee7b8cb8673fa325a9235a5ba2709adb9` (`main`, Thesaurophylax 0.79.9) | `https://github.com/sagrudd/thesaurophylax.git` | Builders reject a stale, split, or substituted custody graph before compiling either appliance or remote payload. |

`make pull` discovers repositories but does not rewrite a sibling
checkout to a historical commit. For release packaging, create clean detached
worktrees at the revisions above beside the DASObjectStore checkout, then run
the package builder. The builder records all four direct or transitive
Mnemosyne source revisions in the adjacent `*.dependencies.json` evidence
file.
