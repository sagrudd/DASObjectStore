# Contributing

Thanks for helping build DASObjectStore.

DASObjectStore is a Rust-first, SSD-ingest-first object appliance for portable
mixed-disk DAS storage. Contributions should keep the project modular,
compatibility-aware, and useful without requiring DAS hardware for ordinary
development tasks.

## Start Here

Before making changes, read:

- [AGENTS.md](AGENTS.md) for repository discipline and architecture rules;
- [ROADMAP.md](ROADMAP.md) for milestone scope;
- [TODO.md](TODO.md) for discrete implementation tasks;
- [docs/architecture.md](docs/architecture.md) for crate boundaries;
- [docs/versioning.md](docs/versioning.md) before changing public or persistent
  formats.

## Development Baseline

Use the Rust workspace from the repository root.

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The CLI is implemented with `clap`:

```bash
cargo run -p dasobjectstore-cli -- --help
```

## Contribution Shape

Prefer small, focused changes:

- one task or coherent task slice per commit;
- tests or validation with implementation changes;
- minimal formatting churn;
- no opportunistic refactors;
- no hardware-dependent behavior in default tests.

If a task requires connected DAS hardware, split out any non-hardware work first
and document the remaining blocker.

## Compatibility-Sensitive Areas

Treat these as public or future-public surfaces:

- CLI command names and `--json` output;
- metadata schemas and manifests;
- store policy keys and semantics;
- object lifecycle state names;
- generated Docker/Compose configuration;
- Mnemosyne/Mneion export formats.

Changes to those areas should update documentation or include a clear design
note before implementation lands.

## Safety

DASObjectStore assumes old disks fail.

Do not hide data-loss risks. Risky operations such as force import, force
retire, direct-to-HDD import, and destructive metadata migration must require
both policy allowance and explicit action-time confirmation.

