# Metadata Compatibility and Recovery

Status: Draft  
Scope: Milestone 4 portable metadata behavior

## Compatibility Surfaces

DASObjectStore treats persistent metadata as a public compatibility surface.

The MVP metadata set is:

- live SQLite metadata on the mandatory SSD;
- canonical pool manifest;
- canonical disk manifest;
- append-only placement log;
- replicated metadata snapshots on HDD metadata directories;
- pool state markers for clean eject, dirty attach, read-only import, repair,
  and force read-write import.

Each persistent artifact carries or is tied to an explicit format version.
Readers must reject unknown future major versions rather than guessing.

## Live Metadata

The live SQLite database is the authoritative writable metadata store while a
pool is actively operating.

The live database records:

- pool identity and state;
- disk identity hints and roles;
- store, object, placement, and ingest rows;
- metadata format versions;
- migration history;
- pool state marker history.

The live database lives under the mandatory SSD metadata root. It must not be
the only durable recovery source for committed HDD objects.

Live SQLite format `0.2` adds explicit ingest job metadata for SSD-first writes:
ingest mode, acknowledgement policy, priority, staging path, expected and
received byte counts, content hash fields, failure messages, and indexes for
state/priority queue views.

## Snapshot Metadata

Snapshot export writes recovery metadata into HDD metadata directories.

The snapshot set currently includes:

- `pool-manifest.json`;
- `disk-manifest.json`;
- `placement-log.jsonl`;
- `live.sqlite`.

The JSON manifests provide portable identity and recovery anchors that can be
inspected without host-local state. The SQLite snapshot provides a direct
recovery source for committed metadata until full manifest-only reconstruction
is implemented.

## Recovery Guarantees

The MVP recovery target is conservative:

- committed HDD object metadata should be recoverable from replicated HDD
  metadata snapshots;
- pool and disk identity should be inspectable from disk-borne metadata;
- dirty attach state should be visible and explicit;
- read-only import should be the default recovery posture after unclean
  detach;
- force read-write import is a risky developer/operator action, not a silent
  default.

DASObjectStore does not guarantee recovery of pending SSD-only ingest objects if
the SSD fails before settlement to HDD.

DASObjectStore does not claim that local metadata snapshots are a backup. Users
still need independent backup for data that cannot be redownloaded or
recomputed.

## Import Behavior

Snapshot import must validate that pool and disk manifests agree on pool
identity before recovering metadata.

Recovered live metadata must match the manifest pool identity. If those
identities disagree, import must fail rather than silently choosing one source.

Read-only import is the preferred default for dirty or uncertain attach
scenarios. Repair and force read-write import must remain explicit states.

## Migration Rules

Metadata migrations must be explicit and tested.

Automatic migrations must not run during read-only attach or read-only dirty
import.

Destructive or lossy metadata changes must require user confirmation and must
document the recovery effect before implementation lands.

## Current Non-Guarantees

The current Milestone 4 implementation does not yet guarantee:

- full manifest-only reconstruction of all live SQLite rows;
- object data recovery when HDD copies were never verified;
- recovery of pending SSD ingest after SSD loss;
- automatic conflict resolution between divergent metadata snapshots;
- compatibility with future major metadata versions.

These are intentionally outside the current milestone boundary.
