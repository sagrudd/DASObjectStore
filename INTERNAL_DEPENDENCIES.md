# Internal dependency catalogue

This catalogue records the Mnemosyne-owned sources required to build a
release package. Package builders fail closed unless the pinned sibling
checkouts below are exact and clean; they never fetch private source during a
release build.

| Local path | Purpose and reference | Required revision / branch | Remote | Observed local state |
| --- | --- | --- | --- | --- |
| `../prosopikon` | Identity contracts and Yew components. Pinned by `Cargo.toml`; package preflight is `packaging/pinned-mnemosyne-package-sources.sh`. | Candidate `6c421a6e1ddaee63692e88c9b46095c652befe04` (Prosopikon #93, core 0.34.0; Yew 0.1.4) | `https://github.com/sagrudd/prosopikon.git` | Source successor for DASObjectStore #1 / Monas #178, independently reviewed under Prosopikon #13. Both workspace dependencies use one exact Rust source identity. Downstream resolution, normal delivery gates and linked Kanon binding remain separate; this is not release or runtime qualification. |
| `../pistis` | Canonical, COSE, crypto, and protocol contracts. The direct `pistis-canonical` dependency and the five resolved Prosopikon contract crates use the proposed #526 source. Pinned by `Cargo.toml`; package preflight is `packaging/pinned-mnemosyne-package-sources.sh`. | Candidate `e36275d9fba8cfbfcd8d92d25b0cebe6199d4b85` (Pistis #526, proposed 0.16.1; required check pending) | `https://github.com/sagrudd/pistis.git` | The candidate Cargo lock resolves Pistis from one source revision; `package_assets` checks the old `14e481...` source is absent. Pistis #526 remains an unmerged draft input. No immutable lockset or release qualification is claimed. |
| `../proxenos` | Site Trust consumer used by the appliance and standalone `dasobjectstore-remote` client. Pinned by `Cargo.toml`, checked in `Cargo.lock`, and recorded by package preflight. | `56d4853f57c59a3303124ae4f33800170ed9be44` (protected `main`, Proxenos 0.62.0; merge of PR #290) | `https://github.com/sagrudd/proxenos.git` | Builders require the exact clean checkout and reject a substituted or unlocked Proxenos source. |
| `../thesaurophylax` | Custody API, core, policy, and store transitive through Proxenos. Its exact API declaration and all four Cargo lock records are verified by package preflight. | `eb2f180ee7b8cb8673fa325a9235a5ba2709adb9` (`main`, Thesaurophylax 0.79.9) | `https://github.com/sagrudd/thesaurophylax.git` | Builders reject a stale, split, or substituted custody graph before compiling either appliance or remote payload. |

`make pull` discovers repositories but does not rewrite a sibling
checkout to a historical commit. For release packaging, create clean detached
worktrees at the revisions above beside the DASObjectStore checkout, then run
the package builder. The builder records all four direct or transitive
Mnemosyne source revisions in the adjacent `*.dependencies.json` evidence
file.
