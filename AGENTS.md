# AGENTS.md

This file defines working rules for AI coding agents contributing to DASObjectStore.

## Repository Discipline

- Commit after each user prompt that results in repository changes.
- Push regularly after meaningful commits, especially when work completes a
  prompt or leaves the repository in a useful review state.
- Keep commits focused and reviewable.
- Avoid unrelated formatting, renames, dependency changes, or cleanup while
  implementing a scoped request.
- Prefer surgical modifications and additions over broad rewrites.

## Versioning

- Maintain semantic versioning for every release; releases have begun at
  `0.1.1`.
- Keep the Rust workspace package version as the source of truth for Rust
  crates and CLI/server `--version` output.
- Keep `CHANGELOG.md` updated for every version change.
- Use version changes intentionally:
  - patch for compatible fixes and documentation-only release corrections;
  - minor for backward-compatible features;
  - major for breaking changes.
- Apply patch and minor version bumps automatically when the delivered work
  warrants them.
- Do not apply a major version bump without clear agreement from both the user
  and the coding agent in the thread.
- Document version-impacting decisions before changing public interfaces,
  persistent metadata formats, CLI behavior, or store/pool compatibility rules.

## Code Organization

- Keep code highly hierarchical and modular.
- Prefer small files with clear responsibility boundaries.
- Split modules when a file begins to mix concerns such as CLI parsing, domain
  logic, persistence, service orchestration, and presentation.
- Keep public interfaces narrow and explicit.
- Avoid circular dependencies between modules.

## Churn Control

- Minimize diff size.
- Preserve existing style unless a local convention is clearly wrong or missing.
- Do not reorder code, tables, documentation sections, or imports unless it is
  needed for the current change.
- Do not refactor opportunistically while implementing unrelated behavior.

## Redundancy

- Avoid code redundancy at all costs.
- Extract shared domain logic into well-named modules rather than duplicating
  behavior across CLI, daemon, Web UI, tests, or adapters.
- Keep configuration schemas, validation rules, and lifecycle state definitions
  single-sourced where practical.
- Prefer generated or shared types over manually duplicated API structures once
  schemas stabilize.

## User Documentation

- Maintain user-facing documentation under `docs/user/` in Sphinx/readthedocs
  `.rst` format when changing CLI workflows, disk-management behavior, store
  policy, NAS/NFS endpoint handling, service operation, or portability behavior.
- Keep user docs helpful but not excessive: explain the task, the safe command
  path, important warnings, and how to inspect the result.
- Do not document ad hoc shell procedures for storage mutations when a formal
  `dasobjectstore` management command exists or is being introduced.
- Keep examples aligned with current CLI names, defaults, confirmation phrases,
  and risk boundaries.
- Update `docs/index.rst` or `docs/user/index.rst` when adding, renaming, or
  removing user guide pages.

## Project Architecture Preferences

- Keep DASObjectStore public-core first.
- Keep the implementation Rust-first unless an integration boundary clearly
  requires another language or tool.
- Use `clap` for CLI parsing and command documentation.
- Use `axum` for GUI-facing HTTP/API surfaces and `yew` for frontend GUI work.
- Treat DASObjectStore as a server/client system. Managed storage mutation
  belongs behind the daemon/service boundary, not inside a normal-user CLI
  process.
- Prefer `dasobjectstored` as the enterprise storage authority: disk discovery,
  mount validation, placement, ingest execution, destage, health mutation, disk
  retirement, repair, and object-service orchestration should be daemon-owned.
- Treat `dasobjectstore` as a client: it may parse commands, authenticate,
  submit jobs, stream source data, and render progress, but it should not write
  directly into managed DAS roots in normal operation.
- Keep eventual GUI delivery aligned with sibling Monas and Synoptikon surfaces
  rather than introducing an unrelated UI stack.
- Keep Mnemosyne/Synoptikon integration in an adapter layer.
- Keep storage profiles and object-service providers abstract enough to evolve
  without breaking pool metadata.
- Treat persistent metadata formats as compatibility-sensitive public surfaces.
- Prefer explicit state machines for disk, pool, object, ingest, and repair
  lifecycle behavior.
- Treat file ingress as a performance-critical product surface. SSD-first
  streaming, parallel HDD fan-out, verification, bounded resource use,
  backpressure, crash recovery, and resumability must be designed together.
- Treat the console TUI as a supported operator surface, not a developer
  diagnostic. It must consume the same daemon job model/events as CLI, Web UI,
  and Synoptikon-facing adapters.
- Reconcile standalone local authentication with the product charter before
  expanding administrator workflows: local OS users and sudo-derived
  administrator status are preferred for appliance-style standalone operation
  unless a documented host-mode decision supersedes that model.

## Safety

- Never hide data-loss risk behind convenience.
- Risky operations must require both policy allowance and action-time
  confirmation.
- Health, repair, drain, and recovery paths should favor clear user-facing state
  over silent automation.
