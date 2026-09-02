# Internal dependency catalogue

This catalogue records the Mnemosyne-owned sources required to build a
release package. Package builders fail closed unless the pinned sibling
checkouts below are exact and clean; they never fetch private source during a
release build.

| Local path | Purpose and reference | Required revision / branch | Remote | Observed local state |
| --- | --- | --- | --- | --- |
| `../prosopikon` | Identity contracts and Yew components. Pinned by `Cargo.toml`; package preflight is `packaging/pinned-mnemosyne-package-sources.sh`. | `f09749273ef382c1b42bf04a77d96189dd7361b3` (`main`, Prosopikon core 0.28.9) | `https://github.com/sagrudd/prosopikon.git` | Primary checkout state is not used for release packaging; package builds require a clean checkout at the pinned revision. Monas and DASObjectStore retain this exact actor-type source revision so the reconciled installation owner's typed `AudienceBoundActorContext` crosses the embedded boundary without translation. |
| `../pistis` | Canonical, COSE, crypto, and protocol contracts transitive through Prosopikon. Pinned by `Cargo.toml`; package preflight is `packaging/pinned-mnemosyne-package-sources.sh`. | `14e481497d3838d3310df3b0a21232f5d01d6f9f` (protected `main`) | `https://github.com/sagrudd/pistis.git` | Primary checkout state is not used for release packaging; package builds require a clean checkout at the pinned revision. |
| `../proxenos` | Site Trust consumer used by the appliance and standalone `dasobjectstore-remote` client. Pinned by `Cargo.toml`, checked in `Cargo.lock`, and recorded by package preflight. | `e4ff70dcc25fdea6949b779ede2c39394be2991b` (`main`, Proxenos 0.57.0) | `https://github.com/sagrudd/proxenos.git` | Builders require the exact clean checkout and reject a substituted or unlocked Proxenos source. |
| `../thesaurophylax` | Custody API, core, policy, and store transitive through Proxenos. Its exact API declaration and all four Cargo lock records are verified by package preflight. | `0bfb16857d135d2830de2cf53d245b68ed2d051f` (`main`, Thesaurophylax 0.72.3) | `https://github.com/sagrudd/thesaurophylax.git` | Builders reject a stale, split, or substituted custody graph before compiling either appliance or remote payload. |

`make pull` discovers repositories but does not rewrite a sibling
checkout to a historical commit. For release packaging, create clean detached
worktrees at the revisions above beside the DASObjectStore checkout, then run
the package builder. The builder records all four direct or transitive
Mnemosyne source revisions in the adjacent `*.dependencies.json` evidence
file.
