# Public Format Registry

Status: Draft
Scope: machine-readable and schema-like formats that need explicit versioning

## Intent

This registry names DASObjectStore formats that downstream tools, recovery
flows, or sibling adapters may consume. New machine-readable formats should be
added here before their implementation is treated as stable.

The compatibility rules are defined in [Versioning Policy](versioning.md).
Persistent metadata recovery behavior is defined in
[Metadata Compatibility and Recovery](metadata-compatibility.md).

## Persistent Metadata Formats

| Format | Artifact | Current version | Version carrier | Notes |
| --- | --- | --- | --- | --- |
| Live SQLite metadata | `live_sqlite` | `0.2` | `metadata_format_versions` table | Writable live metadata on the mandatory SSD. |
| Pool manifest | `pool_manifest` | `0.1` | `format_version` object | Portable pool identity and artifact references. |
| Disk manifest | `disk_manifest` | `0.1` | `format_version` object | Composite disk identity and recovery hints. |
| Placement log | `placement_log` | `0.1` | `format_version` object per JSONL record | Append-only placement and recovery events. |

Readers must reject unknown future major versions. Minor versions may be
accepted only when the reader explicitly supports the added fields or can ignore
them safely.

## CLI JSON Outputs

The following current CLI outputs are machine-readable compatibility surfaces:

- `dasobjectstore probe --json`;
- `dasobjectstore health --json`;
- `dasobjectstore ingest queue --json`;
- `dasobjectstore disk drain --json`;
- `dasobjectstore disk replace --json`;
- `dasobjectstore service status --json`;
- `dasobjectstore mnemosyne export`.

Until dedicated schema identifiers are added to each payload, changes to field
names, enum strings, nesting, or required fields must be treated as
compatibility-sensitive and documented in the same change that introduces them.

## Policy and Service Documents

| Format | Current version | Version carrier | Notes |
| --- | --- | --- | --- |
| Store policy document | Pre-1.0 draft | Rust type and documentation version | JSON accepted by `dasobjectstore store validate`. |
| Object-service Compose request | Pre-1.0 draft | Rust type and documentation version | Internal request shape used to render Compose output. |
| Credential reference manifest | Pre-1.0 draft | Documented manifest purpose | Must never contain secret material. |
| Generated Docker Compose YAML | Provider draft | Compose schema plus provider ID | Reviewable generated deployment artifact. |

These formats need explicit schema identifiers before a release that presents
them as stable public contracts.

## Mnemosyne Adapter Formats

| Format | Current version | Version carrier | Notes |
| --- | --- | --- | --- |
| Host storage boundary | `mnemosyne.host_storage_boundary.v1` | `schema_version` field | Declares Synoptikon object-store boundary semantics. |
| Mneion object-store create request | Pre-1.0 draft | Mneion contract and adapter version | Exported by `dasobjectstore mnemosyne export`. |
| Mneion object-store link request | Pre-1.0 draft | Mneion contract and adapter version | Exported with the binding snippet. |

Mnemosyne formats must stay in the adapter boundary and must not make
DASObjectStore public-core crates depend on Mnemosyne runtime state.

## Update Checklist

Before adding or changing a schema-like format:

1. Add or update its registry entry.
2. Identify the version carrier in the payload, filename, metadata table, or
   governing contract.
3. Add fixture or round-trip tests when the format is parsed from external
   input.
4. Document compatibility or migration behavior for breaking changes.
5. Keep data-loss or secret-leak risks adjacent to commands that expose the
   format.
