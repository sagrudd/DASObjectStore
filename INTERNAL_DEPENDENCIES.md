# Internal dependency catalogue

This catalogue records the Mnemosyne-owned sources required to build a
release package. Package builders fail closed unless the pinned sibling
checkouts below are exact and clean; they never fetch private source during a
release build.

| Local path | Purpose and reference | Required revision / branch | Remote | Observed local state |
| --- | --- | --- | --- | --- |
| `../prosopikon` | Identity contracts and Yew components. Patched by `Cargo.toml`; package preflight is `packaging/pinned-mnemosyne-package-sources.sh`. | `89a168bd95785e62369be9e76fecda517d859ce9` (`main`, Prosopikon #48) | `https://github.com/sagrudd/prosopikon.git` | Primary checkout state is not used for release packaging; package builds require a clean checkout at the pinned revision. |
| `../pistis` | Canonical, COSE, crypto, and protocol contracts transitive through Prosopikon. Patched by `Cargo.toml`; package preflight is `packaging/pinned-mnemosyne-package-sources.sh`. | `6d52ae2c1551e45eb970124418f18b8b0e84d407` (detached package checkout) | `https://github.com/sagrudd/pistis.git` | Primary checkout: `feature/mnemosyne-mobile-branding` at `e948369c6f22696228897e6ea0f59c5b5b9ed0e9`, dirty/unsafe for release packaging. |

`make pull` discovers both repositories but does not rewrite a sibling
checkout to a historical commit. For release packaging, create clean detached
worktrees at the revisions above beside the DASObjectStore checkout, then run
the package builder. The builder records both revisions in the adjacent
`*.dependencies.json` evidence file.
