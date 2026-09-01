# Remote Site Trust provisioning v1

This is the shared authority/client contract for the normal
`dasobjectstore-remote trust provision HOST` workflow. It is deliberately
separate from appliance HTTPS: HTTPS consumes the resulting trust record and
cannot bootstrap it.

## Client source record

The remote package reads one root-owned, non-group/world-writable JSON record
at:

```text
/etc/dasobjectstore-remote/site-trust-sources.d/<canonical-host>-<https-port>.json
```

The source schema is `dasobjectstore.remote_site_trust_source.v1`. Its
`transport` is exactly `pinned-ssh-domain-cert-public-export-v1`; an HTTPS
transport is invalid. The record must bind the requested HTTPS endpoint,
expected lower-case 32-hex `site_uuid`, pinned `ssh_known_hosts_file`, and a
private `ssh_identity_file`. Both the record and known-hosts file are
root-owned and not group/world writable; the identity file is a bounded,
regular file with no group/other access.

The machine-readable schema is
[`dasobjectstore.remote-site-trust-source.v1.schema.json`](../schemas/dasobjectstore.remote-site-trust-source.v1.schema.json).

The SSH account is fixed: `mnemosyne-site-trust-export`. The remote client uses
OpenSSH with an empty global configuration, strict host-key checking, only the
pinned known-hosts file, the configured identity, no password or keyboard
interactive authentication, no agent forwarding, no TTY, and no host-key
updates. It supplies no stdin and applies a 30-second timeout.

## Authority SSH command

The client requests exactly one remote command:

```text
/usr/libexec/mnemosyne-domain-cert-site-trust-export-v1 SITE_UUID
```

`SITE_UUID` is copied only after client validation from the root-owned source
record and is exactly 32 lower-case hexadecimal characters. The authority must
configure `mnemosyne-site-trust-export` as a forced-command account: it accepts
only that absolute path and exactly one valid Site UUID argument. It grants no
shell, port forwarding, subsystem, arbitrary command, agent forwarding, or
authority-custody access.

The constrained exporter is authority-owned. It validates the Site UUID against
the current committed authority state, invokes the existing Domain Cert public
export machinery with its own least-privilege service boundary, and never
discloses a private key, receipt key, GitHub identity, Pistis identity, S3
credential, socket path, or other authority material.

## Response framing

On success the command exits zero and writes exactly one raw `PXCE/v1` byte
sequence to stdout. Stdout contains no JSON, base64 wrapper, PEM, prefix,
trailing newline, digest, progress, or diagnostic data. The response is at
most 9,000 bytes. Diagnostics go to stderr and must not contain secret
material.

On a missing/unsupported Domain Cert public-export capability, the command
exits 78 and writes the exact non-secret marker below to stderr:

```text
__DAS_SITE_TRUST_EXPORT_CAPABILITY_UNAVAILABLE__
```

The remote client maps that response to a precise upgrade remediation and does
not create or change a Site Trust record. SSH host-key or authentication
failure, timeout, malformed/oversized output, a substituted envelope, a
foreign Site UUID, an expired envelope, an invalid proof, or an inactive root
all fail before any local trust mutation.

## Client verification and persistence

The client computes SHA-256 of the exact stdout bytes locally and verifies the
PXCE envelope against the source record's Site UUID. It verifies envelope
framing, the current receipt-key registration, signed action, authority
generation, expiry, root fingerprint, and CA certificate. Only an active
`Install` or `Replace` action is accepted.

Success creates only the process-local JSON record and PEM CA bundle in
`/etc/dasobjectstore-remote/site-trust.d/` (or an explicit mounted output
path). It never changes an operating-system CA store, opens appliance HTTPS
before verification, starts a daemon, or creates a Monas, Pistis, GitHub, or
S3 session. The subsequent normal login remains separately Pistis-approved.

## Air-gapped compatibility

The explicit `--air-gap --site-uuid --envelope
--authenticated-envelope-sha256` form remains supported for a separately
authenticated mounted bundle. It is not a fallback from a missing pinned SSH
source, and it must never be implemented by downloading the bundle over the
untrusted appliance HTTPS endpoint.
