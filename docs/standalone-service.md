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

## Permanent Port Policy

DASObjectStore standalone packages SHALL reserve HTTPS port `8448`.

The default standalone listener SHALL remain loopback-only:

```text
https://127.0.0.1:8448
```

Linux appliance packages MAY expose the same port on every interface only when
the operator explicitly selects appliance mode:

```text
dasobjectstore-server --bind-address 0.0.0.0 --https-port 8448
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
  --bind-address 127.0.0.1 \
  --https-port 8448 \
  --public-base-url https://127.0.0.1:8448 \
  --product-root /opt/dasobjectstore
```

Operators and packages SHOULD validate generated service configuration before
first start:

```text
dasobjectstore-server --check-config
dasobjectstore-server --check-config --json
```

The server owns local standalone authentication, local audit posture, and the
standalone GUI/API surface. Storage-mutating routes call `dasobjectstored`
rather than writing managed storage directly. It does not own Synoptikon login,
Synoptikon session cookies, tenant selection, entitlement checks, or Synoptikon
public TLS.

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
