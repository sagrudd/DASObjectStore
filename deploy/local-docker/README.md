# Local Docker DASObjectStore profile

This profile is the canonical macOS development deployment for exercising the
DASObjectStore S3 adapter with persistent state on an attached volume. It keeps
the storage authority boundary intact:

- `dasobjectstored` runs in a container and owns the Unix-socket control plane,
  registry, credential lifecycle, and Garage lifecycle;
- Garage runs as the daemon-owned nested Compose service;
- `/etc/dasobjectstore` is a container-internal configuration mount, so the
  Linux package paths remain valid without writing to the macOS host `/etc`;
- all persistent profile state is under the configured Seagate root.

This is a local single-node development profile, not an appliance durability or
multi-disk-redundancy claim. The generated store uses `generated_data` policy
with one copy because the profile has one Garage node and one USB volume.

## Prerequisites

1. Docker Desktop with the Compose plugin.
2. `/Volumes/Seagate` added under Docker Desktop **Settings > Resources >
   File Sharing** for the attached-drive profile. When the drive is unavailable,
   use the dedicated generated-data validation root
   `$HOME/.dasobjectstore-codex-validation`; the helper refuses arbitrary home
   folders and enforces a 1 TiB safety ceiling.
3. The DASObjectStore checkout and its sibling `prosopikon` checkout under one
   build context (the default is the parent of this repository).
4. A built host CLI, or a `dasobjectstore` binary on `PATH`.

The script never downloads data, stores credentials in Git, or prints secret
values. Generated files are written below:

```text
/Volumes/Seagate/DASObjectStore/alleleanchor-mvp/
  config/       daemon.json, Garage config, nested Compose file
  state/        daemon metadata, writable store registry, and private Garage credential registry
  credentials/  mode-0600 Garage and AlleleAnchor credential files
  garage-meta/  Garage metadata
  garage-data/  Garage object data
```

## Build and start

From the DASObjectStore checkout:

```bash
cargo build --locked -p dasobjectstore-cli
./deploy/local-docker/local.sh up
```

`up` renders the profile, builds the daemon image, starts the daemon, starts
Garage through the daemon, provisions the `alleleanchor_mvp` store (bucket
`alleleanchor-mvp`) and scoped key, and writes an AlleleAnchor adapter config.
It prints only paths and
non-sensitive status.

Useful lifecycle commands:

```bash
./deploy/local-docker/local.sh status
./deploy/local-docker/local.sh config
./deploy/local-docker/local.sh down
```

`down` stops Garage through the daemon before stopping the daemon container.
It does not delete the Seagate profile. There is deliberately no destructive
reset command in this helper.

## AlleleAnchor validation

Point AlleleAnchor at the generated config path returned by `config` (or copy
that path into an ignored local configuration file). The endpoint is:

```text
http://127.0.0.1:3900
```

Run the existing tiny-fixture DAS smoke test only after `up` succeeds. Keep
object IDs, credentials, raw reads, profiles, and generated payloads outside
Git. This local profile can close the S3-compatible adapter integration gate;
the remote DAS appliance remains a separate deployment/soak acceptance.

## Overrides

The defaults target the attached volume and can be changed without editing the
repository:

```bash
DASOBJECTSTORE_LOCAL_ROOT=/Volumes/Seagate/DASObjectStore \
DASOBJECTSTORE_LOCAL_PROFILE=alleleanchor-mvp \
DASOBJECTSTORE_BUILD_CONTEXT=/Users/stephen/Projects \
./deploy/local-docker/local.sh up
```

Set `DASOBJECTSTORE_BIN` when using a release binary from another checkout.
Set `DASOBJECTSTORE_LOCAL_API_PORT` if port 3900 is already occupied.

## Platform boundary

Docker Desktop bind mounts cross a Linux VM. USB disconnects, sleep, Docker
Desktop restarts, and APFS/bridge behavior are not equivalent to native Linux
appliance semantics. The daemon container receives `/var/run/docker.sock` so
that it can own the nested Garage lifecycle; this is a local-development
authority boundary, not appliance hardening. Use this profile for deterministic
local contract and adapter validation; keep throughput, SMART, repair, and
multi-disk claims on a Linux DAS host. AlleleAnchor's local FileStore remains a
consumer-side substitute and must not become a second storage authority.
