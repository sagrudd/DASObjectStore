# Internal dependency catalogue

This catalogue records the Mnemosyne-owned sources required to build a
release package. Package builders fail closed unless the pinned sibling
checkouts below are exact and clean; they never fetch private source during a
release build.

| Local path | Purpose and reference | Required revision / branch | Remote | Observed local state |
| --- | --- | --- | --- | --- |
| `../prosopikon` | Identity contracts and Yew components. Pinned by `Cargo.toml`; package preflight is `packaging/pinned-mnemosyne-package-sources.sh`. | `f4ba81e893136525e961eb4bdb995e70ef6085fa` (`main`, Prosopikon 0.22.0 typed production runtime identity projection) | `https://github.com/sagrudd/prosopikon.git` | Primary checkout state is not used for release packaging; package builds require a clean checkout at the pinned revision. Monas and DASObjectStore must retain this exact actor-type source revision. |
| `../pistis` | Canonical, COSE, crypto, and protocol contracts transitive through Prosopikon. Patched by `Cargo.toml`; package preflight is `packaging/pinned-mnemosyne-package-sources.sh`. | `3bb6e96948734fbc6e7d6cf20b7805ff88011af2` (detached package checkout) | `https://github.com/sagrudd/pistis.git` | Primary checkout state is not used for release packaging; package builds require a clean checkout at the pinned revision. |

`make pull` discovers both repositories but does not rewrite a sibling
checkout to a historical commit. For release packaging, create clean detached
worktrees at the revisions above beside the DASObjectStore checkout, then run
the package builder. The builder records both revisions in the adjacent
`*.dependencies.json` evidence file.
