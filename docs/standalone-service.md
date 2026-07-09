# Standalone Service and Packaging

Status: Draft  
Scope: standalone HTTPS application and managed daemon packaging behavior

## Managed Daemon

`dasobjectstored` is the managed storage authority. Packages SHALL run it as a
service identity that owns DASObjectStore state, runtime sockets, logs, and
managed storage mutation.

Normal clients SHALL NOT write directly into managed DAS roots. The
`dasobjectstore` CLI, standalone HTTPS server, Web UI, and Synoptikon product
integration submit daemon requests or jobs and receive progress/state updates
from the daemon.

The preferred local Linux transport is a Unix-domain socket under the package
runtime directory, for example:

```text
/run/dasobjectstore/dasobjectstored.sock
```

The daemon SHALL enforce writer/admin policy before accepting storage-mutating
jobs. Local Unix groups such as `mnemosyne` authorize job submission; they are
not intended to grant ordinary users raw write access to mounted DAS member
filesystems.

Initial Linux package assets live under `packaging/linux/`:

- `systemd/dasobjectstored.service`
- `sysusers.d/dasobjectstore.conf`
- `tmpfiles.d/dasobjectstore.conf`
- `etc/dasobjectstore/daemon.json`

These assets define the `dasobjectstore` service user/group, the daemon runtime
socket directory, state directory, log directory, configuration directory, and
managed storage ownership expectations. The Rust daemon crate keeps tests that
pin those assets to the packaged Linux runtime defaults.

The Debian build script SHALL install these assets and the `dasobjectstored`
binary into the package. Its maintainer script SHALL reject an existing
`/srv/dasobjectstore` root unless it is owned by `dasobjectstore:dasobjectstore`;
ordinary ingest users must receive store writer-group authorization rather than
direct write ownership of managed DAS roots.

## Permanent Port Policy

DASObjectStore standalone packages SHALL reserve HTTPS port `8448`.

The compiled fallback standalone listener remains loopback-only for local
development:

```text
https://127.0.0.1:8448
```

Linux appliance packages SHALL install `/opt/dasobjectstore/config.json` and
enable the standalone Web UI/API by default. The packaged appliance
configuration binds the same port on every interface:

```text
bind_address = 0.0.0.0
https_port = 8448
```

Synoptikon-integrated deployments SHALL NOT expose `8448` as a public listener.
They SHALL be reached through Synoptikon's public HTTPS listener and the product
mounts declared by the Synoptikon catalogue:

```text
/products/dasobjectstore
/products/dasobjectstore/api
```

## Standalone Server Command

`dasobjectstore-server` is the standalone Web UI and API entry point. Packages
SHALL run it with the same configuration model that the CLI validates:

```text
dasobjectstore-server \
  --config /opt/dasobjectstore/config.json \
  --generate-missing-tls
```

Operators and packages SHOULD validate generated service configuration before
first start:

```text
dasobjectstore-server --config /opt/dasobjectstore/config.json --check-config
dasobjectstore-server --config /opt/dasobjectstore/config.json --check-config --json
```

The server owns local standalone authentication, local audit posture, and the
standalone GUI/API surface. Storage-mutating routes call `dasobjectstored`
rather than writing managed storage directly. It does not own Synoptikon login,
Synoptikon session cookies, tenant selection, entitlement checks, or Synoptikon
public TLS.

Standalone appliance administrator authority is OS-local. A host user with sudo
rights is a DASObjectStore administrator, and host group membership authorizes
ordinary store writer/admin job submission. The product-local auth store is a
transitional Web session layer until OS-local actor discovery is implemented; it
must not supersede sudo-derived administrator status. The full decision is
recorded in [Standalone Authentication Decision](standalone-auth.md).

## Web Assets

The packaged standalone Web UI assets are prepared through the `make web`
target:

```text
make web
```

`make deb` and `make rpm` depend on `make web`, and the package scripts also
prepare the assets defensively when called directly. Production package builds
always run `trunk build --release` and validate that the resulting `dist/`
contains the WebAssembly and JavaScript bundles for the operator interface.
Install the required build tools before packaging:

```text
rustup target add wasm32-unknown-unknown
cargo install trunk
```

The developer fallback page is available only through the explicit
`packaging/web/prepare-web-dist.sh --allow-fallback` escape hatch and must not
be used for `make deb` or `make rpm` artifacts.

Formal performance-report rendering in the Web Activity page depends on the
packaged DASObjectStore Grammateus handoff wrapper
`/usr/libexec/dasobjectstore/gnostikon-workflow-control` and a Docker-compatible
container runtime. `make pull` still fetches the upstream
`gnostikon-workflow-control` sibling checkout for local development, while the
DEB/RPM manifests install the DAS-owned wrapper and declare the container
runtime as a runtime dependency so appliance installs do not silently expose a
broken reporting surface.

The formal PDF report container is initialised through the Grammateus-owned
`grammateus_report_provider` installer. This mirrors the Mnematikon runtime
asset pattern: the report provider is a semantically versioned Grammateus
runtime asset, while DASObjectStore supplies the product report content and
invokes the shared provider. The installer uses Docker Buildx because the
Grammateus report-provider Dockerfile consumes the sibling floundeR build
context. On Ubuntu this is supplied by `docker-buildx`; Docker CE based
installations commonly use `docker-buildx-plugin`. For source checkouts, run:

```text
make pull
make report-provider
```

`make report-provider` uses the sibling `../grammateus` and `../floundeR`
contexts by default and creates the configured `grammateus/report:0.8.1` image.
`make deb` and `make rpm` run the same target before package assembly so local
appliance builds are report-ready before installation. During package
configuration, DASObjectStore prewarms the installed provider and reports the
repair command if the Grammateus installer or container image is missing.

Package configuration adds the `dasobjectstore` service user to the `docker`
group and restarts `dasobjectstore-server.service`. If Docker is repaired or
reinstalled later, run `sudo usermod -aG docker dasobjectstore` and restart the
Web service so the process receives the updated supplementary group.

Web report rebuild scratch space lives under
`/var/lib/dasobjectstore/report-rebuild` rather than `/tmp`. This path is
visible to both `dasobjectstore-server.service` and the Docker daemon even when
the systemd unit uses `PrivateTmp=true`.

## Telemetry State

Managed appliance telemetry lives under the package state tree:

```text
/var/lib/dasobjectstore/telemetry
```

Packages create this directory as `dasobjectstore:dasobjectstore` with mode
`0750`. Telemetry state files written inside it SHALL use mode `0640` and the
same owner/group. This keeps host performance data, disk identity, and Web
session counts readable to DASObjectStore services without making them
world-readable.

The initial telemetry state file is:

```text
/var/lib/dasobjectstore/telemetry/appliance-telemetry.v1.json
```

Writers SHALL update the file atomically by writing a schema-valid document to
a temporary file in the same directory, fsyncing the file, applying final
ownership and permissions, renaming it over the current state file, and fsyncing
the telemetry directory. Temporary files SHOULD use a hidden same-directory name
such as `.appliance-telemetry.v1.json.tmp-<pid>`.

Readers SHALL ignore incomplete temporary files. On startup, the daemon MAY
remove stale temporary files whose writer no longer exists. If the current JSON
file is missing, the daemon SHALL start from an empty schema-valid telemetry
history. If the current file is corrupt or fails schema validation, the daemon
SHALL preserve it for operator inspection using a timestamped
`corrupt-*.json` name in the telemetry directory, then start a fresh
schema-valid file rather than merging partial data.

Future schema versions SHALL retain the version in the payload and write major
versions to distinct filenames such as `appliance-telemetry.v2.json`. A daemon
that sees an unknown future major version SHALL leave it untouched and fall back
to the newest version it understands. Migrations SHALL write the target version
atomically and keep the source version until migration succeeds, preserving a
rollback and audit path.

## Remote Client Packages

Remote upload hosts do not need the full appliance package. Use the remote-only
Makefile targets from a source checkout:

```text
make remote
make remote-deb
make remote-rpm
```

`make remote` builds only the release `dasobjectstore-remote` binary. It is the
fast local check for the upload client and does not build the Web UI,
`dasobjectstored`, systemd units, Garage orchestration assets, or full
appliance packages.

`make remote-deb` and `make remote-rpm` build packages named
`dasobjectstore-remote`. Those packages install the remote client binary and
remote-client documentation only. They intentionally do not install the daemon
service identity, local appliance configuration, managed storage directories,
Web UI assets, or object-service lifecycle units.

The remote Debian package requires `ca-certificates` and suggests `awscli`.
The remote RPM requires `ca-certificates` and recommends `awscli`. AWS CLI is
still required for concrete `stores list` and `upload` transfers unless a
future transfer engine replaces the AWS CLI adapter; it is soft-listed because
many sites install AWS CLI v2 from Amazon's installer rather than from the OS
package repository.

Easyconnect browser launch is a runtime expectation, not a package-managed
desktop dependency. `dasobjectstore-remote easyconnect <host-or-ip>` attempts
to open the appliance login URL with the host opener (`open` on macOS,
`xdg-open` on Linux, and `cmd /C start` on Windows). Headless upload hosts,
SSH sessions, and minimal containers should use `--no-browser`; the client then
prints the login URL and continues waiting for browser-approved pairing.

## TLS Assets

Default standalone TLS assets live under the product root:

```text
/opt/dasobjectstore/tls/server.crt
/opt/dasobjectstore/tls/server.key
```

Packaging MAY pre-provision these paths. Development or appliance bootstrap MAY
generate self-signed local assets when both files are absent:

```text
dasobjectstore-server --check-config --generate-missing-tls
```

The top-level runtime status command SHALL expose the configured Web UI and
object-service ports:

```text
dasobjectstore status
dasobjectstore status --json
```

Partial TLS state is invalid: if either the certificate or private key is
missing, the package or operator must repair the pair before the service starts.

## Package Profiles

### Local Development

Local development SHOULD keep the listener on `127.0.0.1:8448`. Developers may
use `--check-config` and `--generate-missing-tls` to validate packaging inputs
without starting the long-running listener.

### Customer Standalone Appliance

A customer appliance MAY bind `0.0.0.0:8448` when the operator has configured
host firewalling, TLS trust, and local admin access. The package should make the
external bind explicit in service configuration rather than changing the default.

### Synoptikon Integrated

Synoptikon integrated mode SHALL use the product catalogue, host-mode boundary,
and product UI bootstrap. It SHALL NOT install a public standalone listener on
`8448`; any internal product port must be assigned by Synoptikon catalogue
policy.
