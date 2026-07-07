# Changelog

All notable DASObjectStore release changes are recorded here.

This project follows semantic versioning. Patch and minor version bumps may be
made automatically for compatible work; major version bumps require explicit
agreement before landing.

## 0.1.2 - 2026-07-07

- Wire `dasobjectstored` to handle `SubmitIngestFiles` requests by resolving
  store/SubObject endpoints, discovering managed HDD roots, staging source files
  through the SSD, and writing verified HDD copies.
- Add `dasobjectstore ingest files --tui` as an embedded upload view driven by
  daemon progress events while file ingest runs.
- Remove the standalone `dasobjectstore-tui` command surface and packaging path;
  terminal graphics are embedded niceties for long-running CLI actions.
- Relax the packaged systemd service to `ProtectHome=read-only`, allowing the
  daemon to read user-provided source paths when Linux filesystem permissions
  allow it.
- Surface daemon API error responses directly in the client, so unwired daemon
  commands report their daemon error code and message instead of a generic
  unexpected-response error.
- Corrected `TODO.md` entries that incorrectly marked production daemon ingest
  dispatch and peer-credential authorization as complete.

## 0.1.1 - 2026-07-07

- Established the first maintained semantic version for the Rust workspace,
  CLI, daemon, GUI/API, Mnemosyne adapter, object-service, platform, metadata,
  and TUI crates.
- Updated packaged/product metadata to advertise `0.1.1` instead of the
  pre-release placeholder `0.0.0`.
- Documented the release discipline in `AGENTS.md` and the versioning policy.
