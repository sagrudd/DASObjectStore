# AGENTS.md

This file defines working rules for AI coding agents contributing to DASObjectStore.

## Repository Discipline

- Commit after each user prompt that results in repository changes.
- Push regularly after meaningful commits, especially when work completes a
  prompt or leaves the repository in a useful review state.
- If another process has unrelated worktree edits, treat those paths as
  read-only isolation boundaries: continue only in untouched files, never
  stage, overwrite, reset, stash, or reformat the unrelated paths, and commit
  only the non-overlapping slice. Report a blocker only when integration with
  those edits is required.
- Keep commits focused and reviewable.
- Avoid unrelated formatting, renames, dependency changes, or cleanup while
  implementing a scoped request.
- Prefer surgical modifications and additions over broad rewrites.

## Multi-Agent Coordination

- Use multiple worker agents judiciously when the requested work is substantial
  enough to benefit from parallel delivery, especially when planning,
  implementation, documentation, and testing can be separated cleanly.
- Keep the lead agent accountable for the final design, integration, review,
  test selection, and user-facing summary. Worker agents may accelerate bounded
  slices, but they do not replace coherent technical ownership.
- Separate work by responsibility whenever practical:
  - planning/design agents should clarify scope, risks, data-loss boundaries,
    interfaces, and acceptance criteria;
  - coding agents should own specific files, modules, or behavior changes with
    minimal overlap;
  - documentation agents should update user-facing `.rst` material, examples,
    release notes, and operator guidance;
  - testing agents should add or run focused regression, integration, packaging,
    or deployment checks.
- Give each worker a concrete, non-overlapping write scope and tell workers
  they are not alone in the codebase. They must not revert unrelated edits and
  must adapt to changes made by other contributors.
- Avoid spawning workers for tiny, tightly coupled, or urgent blocking tasks
  where delegation would add coordination overhead. Prefer local execution for
  the immediate critical path and use workers for parallel sidecar tasks.
- Integrate worker output deliberately: inspect diffs, reconcile overlapping
  assumptions, run appropriate tests from the lead context, and keep the final
  commit focused.
- When the user explicitly asks for multiple agents, make a brief coordination
  plan before delegation and keep the user informed about which responsibilities
  are being handled in parallel.

## Deployment Host

- The DAS appliance currently used for deployment testing is
  `stephen@192.168.1.192`.
- Use the PEM at `~/.ssh/dasobjectstore-codex` for SSH access:
  `ssh -i ~/.ssh/dasobjectstore-codex stephen@192.168.1.192`.
- Do not commit or copy private key material into this repository; only the
  expected local key path is documented here.
- The active deployment checkout on the DAS host is usually
  `/home/stephen/src/DASObjectStore`.
- Build Linux packages on the DAS host when working from a non-Linux
  development machine, then install the generated Debian package through APT
  with `sudo apt-get install --reinstall ./dasobjectstore_<version>_amd64.deb`
  and restart `dasobjectstored`. Do not use a raw `dpkg -i` deployment because
  installations and reinstalls must remain formally managed through APT.
- Coding agents are authorized to compile on the DAS host, install the
  resulting DASObjectStore package, and restart `dasobjectstored` for
  validation.
- Coding agents may create and use a dedicated `CODEX` ObjectStore for
  automated stress and ingress tests using randomly generated data only. Keep
  all such test data below 1 TiB total, never use user/project data, and clean
  it up only through documented, explicitly confirmed management commands.

## Definition of Done

- A TODO item is complete only when its implementation is committed and pushed,
  relevant local tests pass, user/operator documentation and TODO status are
  updated, and the change is ready for validation with real-world data.
- Feature work is expected to reduce the approved TODO backlog each cycle;
  work one dependency-ordered task at a time and do not move on while a
  locally actionable gap in that task remains.

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
