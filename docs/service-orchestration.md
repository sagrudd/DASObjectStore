# Service Orchestration Notes

Status: Draft  
Scope: Docker/Compose service orchestration, with macOS development limits

## Intent

DASObjectStore uses Docker/Compose as the default orchestration path for the
S3-compatible object service selected by the benchmark milestone.

The service orchestration layer should:

- generate reviewable Compose configuration;
- keep store-to-bucket layout explicit in DASObjectStore metadata;
- use per-store credentials;
- support CLI start, stop, and status flows;
- provision S3 buckets and credentials against the running service without
  requiring object-service restarts;
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
dasobjectstore service provision
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

This example is the `garage_legacy` topology: Garage owns public port `3900`
and later reconciliation imports accepted payloads into managed storage. The
opt-in `direct_gateway` topology gives public port `3900` to the
DASObjectStore standalone server and moves Garage to a loopback-only port such
as `127.0.0.1:3901`. Garage keeps its existing metadata/data volumes, buckets,
and keys for compatibility and recovery. Never start the two listeners on the
same port, expose the private Garage port, or delete provider data as part of
the mode switch. Render the private mapping with container `--api-port 3900`,
`--published-api-port 3901`, and `--bind-address 127.0.0.1`; this produces
`127.0.0.1:3901:3900` without rewriting retained `garage.toml`. The
architecture decision and exact migration/rollback and
acceptance procedure are documented in
[Direct S3 ingress](user/direct-s3-ingress.rst).

The rendered Garage Compose file is process configuration: image, ports,
volumes, and `garage.toml`. It is not the authoritative bucket registry and
does not contain per-ObjectStore access keys. Adding an ObjectStore must not
require rebuilding or restarting Garage. After creating or changing S3-exported
ObjectStores, apply the live registry to the running service through the daemon:

```bash
dasobjectstore service provision --provider garage
```

Use `--dry-run` to inspect the number of stores, buckets, and Garage admin
commands that would be applied. The daemon runs Garage admin commands inside
the running Compose service, creating buckets and granting per-store keys
without changing `/etc/dasobjectstore/garage.compose.yml`.

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
