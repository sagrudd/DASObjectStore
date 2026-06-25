# macOS Development and Read/Export Notes

Status: Draft  
Scope: macOS support boundaries for DASObjectStore MVP development

## Support Position

Linux is the intended full-operation target for the MVP. macOS is a supported
development, inspection, read-only import, and settled-object export platform.

macOS support is deliberately narrower than Linux support:

- inspect portable pool metadata from mounted DAS disks;
- import clean or dirty pools read-only for local inspection;
- export settled objects from verified disk placements where mounted paths are
  readable;
- render and validate service configuration for development;
- report best-effort disk health from host-visible signals.

macOS does not provide the MVP's authoritative write, placement, drain, repair,
or object-service operation path.

## Docker Desktop

Docker Desktop runs Linux containers inside a VM. DASObjectStore can render
Docker/Compose configuration on macOS, and may run development services where
paths are shared with Docker Desktop, but hardware-facing behavior remains
host-side.

Practical limits:

- bind-mounted DAS paths cross the Docker Desktop file-sharing layer;
- raw USB block devices, udev events, and Linux block-device semantics are not
  available to containers in the same way as on Linux;
- a path readable in Terminal may still be unavailable to Docker Desktop until
  it is included in file-sharing settings;
- Docker Desktop restarts, sleep, or VM resets may interrupt services without
  changing DASObjectStore pool metadata.

Use `dasobjectstore service render-compose` first and treat
`dasobjectstore service status --json --dry-run` as the safest macOS validation
path when Docker Desktop availability is uncertain.

## Service Management

macOS service management is for development only in the MVP. DASObjectStore
should not use launchd, Docker Desktop, or a container as the authority for pool
state, disk health, or repair decisions.

Expected behavior:

- generated Compose files are reviewable artifacts;
- service start/stop failures are reported as orchestration failures;
- object-service lifecycle is separate from pool metadata lifecycle;
- production service-management assumptions must be validated on Linux.

## SMART and Health

macOS exposes less disk-health detail for many USB-attached disks than Linux.
USB bridge firmware often hides SMART, serial, temperature, and reset details.

DASObjectStore health on macOS is therefore best-effort:

- `diskutil` SMART status may be unavailable or report only broad pass/fail
  state;
- missing SMART does not prove a disk is healthy;
- suspicious IO, checksum, disconnect, or benchmark-drift signals remain useful
  when available;
- CLI output should distinguish unknown health from healthy health.

Linux remains the preferred host for complete SMART-based health, drain, and
repair decisions.

## Filesystem Support

macOS can read and write APFS and HFS+ directly, but Linux-native filesystems
such as ext4 or XFS usually require extra drivers or should be accessed through
a Linux host.

For portable read/export development:

- metadata snapshots must stay in platform-neutral JSON/JSONL plus SQLite
  formats;
- settled object export requires the relevant HDD filesystem to be mounted and
  readable on macOS;
- direct write operation to HDD capacity members is not part of the macOS MVP;
- filesystem-specific behavior must not be treated as portable metadata state.

## Permissions

Mounted external disks can differ in owner, group, ACL, quarantine, and privacy
permissions between hosts.

DASObjectStore should expect:

- Terminal or app-level permissions may be required for removable volumes;
- Docker Desktop may need explicit file-sharing permission for mounted paths;
- read-only import should fail clearly when manifests or object paths are not
  readable;
- repair and write paths should not be inferred from macOS permission success.

## Performance

macOS performance is useful for development and smoke testing, not for MVP
throughput claims.

Known sources of variance:

- Docker Desktop bind mounts add latency and VM overhead;
- USB DAS bridge behavior differs by enclosure and port topology;
- APFS, exFAT, ext4-through-driver, and network-backed mounts have different
  durability and flush behavior;
- sleep, Spotlight indexing, Time Machine, and power management may affect
  external disk IO;
- old HDD inner-track performance still matters once disks fill.

Benchmark and provider-selection claims should be made on the target Linux
hardware path, with macOS results recorded only as development observations.

## Recommended macOS Workflow

1. Inspect mounted pool metadata:

   ```bash
   dasobjectstore pool inspect --metadata-path /Volumes/das-disk/.dasobjectstore/metadata
   ```

2. Import clean or dirty pools read-only into local recovery metadata:

   ```bash
   dasobjectstore pool import \
     --read-only \
     --source-path /Volumes/das-disk \
     --recovery-metadata-dir ./recovered-metadata \
     --recorded-at-utc 2026-01-05T00:00:00Z
   ```

   Read-only import creates local recovered metadata for inspection. It must not
   be treated as a repaired writable pool, and it does not recover SSD-only
   ingest data that was never settled to HDD.

3. Preview repair intent without mutation:

   ```bash
   dasobjectstore pool repair --source-path /Volumes/das-disk --dry-run
   ```

   `pool repair --dry-run` is advisory only. Any future non-dry-run repair path
   must document whether it rewrites metadata, changes pool state, or marks
   unresolved ingest work as lost.

4. Export settled objects only from readable mounted disk roots.

5. Move full write, service, drain, repair, and benchmark workflows to Linux.
