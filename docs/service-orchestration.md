# Service Orchestration Notes

Status: Draft  
Scope: Docker/Compose service orchestration, with macOS development limits

## Intent

DASObjectStore uses Docker/Compose as the default orchestration path for the
S3-compatible object service selected by the benchmark milestone.

The service orchestration layer should:

- generate reviewable Compose configuration;
- keep store-to-bucket layout explicit;
- use per-store credentials;
- support CLI start, stop, and status flows;
- keep native service management possible later.

Linux is the intended full-operation target for the MVP. macOS is a supported
development, inspection, and read/export platform, but it should not pretend to
have the same service-management or hardware-observation behavior as Linux.
The wider macOS support boundary is documented in
[macOS Development and Read/Export Notes](macos-development.md).

## CLI Surface

Current service orchestration commands:

```bash
dasobjectstore service render-compose
dasobjectstore service up
dasobjectstore service down
dasobjectstore service status --json
```

`render-compose` is the safest cross-platform command because it only produces
configuration. `status --json --dry-run` is useful on development machines when
Docker Desktop is unavailable or when the generated Compose project should be
reviewed before execution.

Production appliance renders should bind the S3-compatible API to a
non-loopback address so remote workers and Mnemosyne ecosystem services can
reach it:

```bash
dasobjectstore service render-compose \
  --project-name dasobjectstore \
  --ssd-metadata-path /srv/dasobjectstore/ssd/garage \
  --hdd-data-path /srv/dasobjectstore/hdd/garage \
  --provider garage \
  --service-name garage \
  --image dxflrs/garage:v2.3.0 \
  --bind-address 0.0.0.0 \
  --api-port 3900 > /etc/dasobjectstore/garage.compose.yml

dasobjectstore service up \
  --compose-file /etc/dasobjectstore/garage.compose.yml \
  --project-directory /var/lib/dasobjectstore/garage
```

Use the top-level runtime status command as the appliance healthcheck. Its JSON
payload includes the active S3 bind address, port, `remote_ready` flag, and
`remote_url` to hand to remote upload clients:

```bash
dasobjectstore status --json
```

## macOS Docker Desktop Limits

Docker Desktop for macOS runs Linux containers inside a VM. That makes it useful
for development, configuration validation, and some local object-service testing,
but there are important limits:

- Bind-mounted DAS paths cross the macOS file-sharing layer before reaching the
  container. Performance may be lower and less predictable than native Linux.
- External disk paths must be visible to Docker Desktop file sharing. A path
  that works in the macOS shell may still be unavailable to the Compose service.
- Raw USB block devices are not exposed to Linux containers in the same way they
  are on a Linux host. DASObjectStore should not depend on container access to
  raw disks, SMART, udev, or Linux block-device events on macOS.
- SMART and USB topology probing should run through host-side platform code,
  not through the object-service container.
- Filesystem semantics may differ between macOS-hosted bind mounts and native
  Linux filesystems. Durability and latency assumptions must be tested before
  production claims are made.
- Sleep, unplug, eject, and Docker Desktop restart behavior can interrupt
  service containers independently of DASObjectStore pool state.

These limits mean macOS service orchestration is best treated as a development
and compatibility path until the selected object service has been benchmarked
against DASObjectStore workloads.

## Recommended macOS Workflow

On macOS development machines without connected DAS hardware:

1. Render the Compose configuration.
2. Validate the generated file with `docker compose config` when Docker Desktop
   is available.
3. Use dry-run status checks to inspect the Docker command DASObjectStore would
   execute.
4. Keep benchmark and provider-selection work separate from hardware-dependent
   pool operation.

On macOS machines with external disks attached:

1. Keep SSD metadata and object-service data paths under directories Docker
   Desktop can access.
2. Prefer read-only inspection and export flows for pools created on Linux.
3. Avoid claiming Linux-equivalent throughput from macOS bind mounts.
4. Treat service start/stop failures as orchestration failures, not as proof of
   disk or pool failure.

## Linux Expectation

On Linux, DASObjectStore can eventually coordinate:

- host-side disk and SMART observation;
- direct filesystem access to mounted SSD and HDD paths;
- Docker/Compose service lifecycle;
- future native service management where Compose is not desired.

The object-service container should still remain a service boundary. Disk
identity, health scoring, placement, metadata, repair, and safety decisions
belong to DASObjectStore crates rather than provider-specific container logic.

## Validation Boundary

No DAS hardware is required to validate generated Compose syntax. Tests may
render a local Compose file and run `docker compose config` when Docker Desktop
or Docker Engine is available.

Starting a real service depends on:

- the selected object-service provider;
- Docker availability;
- accessible configured paths;
- the host platform;
- later benchmark and reliability results.

Until Milestone 8 selects the MVP object service, provider-specific runtime
claims remain provisional.
