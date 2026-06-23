# AGENTS.md

This file defines working rules for AI coding agents contributing to DockerDAS.

## Repository Discipline

- Commit after each user prompt that results in repository changes.
- Push regularly after meaningful commits, especially when work completes a
  prompt or leaves the repository in a useful review state.
- Keep commits focused and reviewable.
- Avoid unrelated formatting, renames, dependency changes, or cleanup while
  implementing a scoped request.
- Prefer surgical modifications and additions over broad rewrites.

## Versioning

- Maintain semantic versioning once releases begin.
- Use version changes intentionally:
  - patch for compatible fixes;
  - minor for backward-compatible features;
  - major for breaking changes.
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

## Project Architecture Preferences

- Keep DockerDAS public-core first.
- Keep Mnemosyne/Synoptikon integration in an adapter layer.
- Keep storage profiles and object-service providers abstract enough to evolve
  without breaking pool metadata.
- Treat persistent metadata formats as compatibility-sensitive public surfaces.
- Prefer explicit state machines for disk, pool, object, ingest, and repair
  lifecycle behavior.

## Safety

- Never hide data-loss risk behind convenience.
- Risky operations must require both policy allowance and action-time
  confirmation.
- Health, repair, drain, and recovery paths should favor clear user-facing state
  over silent automation.

