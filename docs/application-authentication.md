# Application Authentication and Authoritative Tokens

Status: approved architecture decision, 2026-07-13.

This document defines how unattended applications authenticate to
DASObjectStore. It applies to Synoptikon, Mneion, AlleleAnchor, Mnemosyne
adapters, and standalone integrations. It does not make an application a
storage authority: `dasobjectstored` remains authoritative for paths, profiles,
quotas, catalogue state, placement, credentials, and health.

## Credential classes

Long-lived application identities are supported. Long-lived, broadly scoped
bearer access tokens are not.

| Credential | Lifetime and use | Authority |
| --- | --- | --- |
| Application identity key or certificate | Long-lived, rotatable identity used only for token exchange | Identifies an application; grants no storage operation by itself |
| Short-lived access token | Target of a token exchange; normally 5–15 minutes | Grants narrowly scoped reads, writes, listings, or verification |
| Upload completion capability | Single-use and upload-scoped | Allows one provider completion claim after daemon verification |
| Renewal token | Used only at the renewal endpoint | Cannot read, write, complete uploads, or mutate policy |

Applications are registered as daemon-owned service principals. A registration
records the owner, purpose, allowed ObjectStores, allowed prefixes or object
types, operations, ingress origin, optional byte limits, expiry, and audit
metadata. Application credentials never convey private host paths.

Rotatable key descriptors may carry base64-encoded public-key material alongside
its SHA-256 fingerprint. The daemon's ring-backed verifier supports Ed25519 and
P-256 detached signatures over the canonical proof-free exchange payload; an
mTLS certificate descriptor must be verified by the transport layer.

## Token scope and completion

Access tokens are audience-bound and include an application identity, token
identifier, issued/expiry times, allowed ObjectStore and namespace, operation
set, ingress origin, and optional byte/object limits. The daemon rejects missing
or excessive scope before touching a backend.

An upload initiation response may include a short-lived, single-use completion
capability bound to the paired session, ObjectStore, upload ID, object key,
expected length, checksum, audience, expiry, and nonce. A client submits that
capability only after the provider transfer. The daemon independently checks
provider state, size, checksum, reservation state, and catalogue conflicts
before atomically committing completion. Replays are rejected or return the
same idempotent terminal result. The daemon now has a durable,
expiry-pruned replay registry for capability IDs and nonces; proof verification
and catalogue completion wiring remain authority-side work. Core token issuance
requires an explicit proof-verifier implementation and rejects unverified
proofs. A failed
catalogue commit never reports success.

Renewal tokens are not accepted as bearer credentials for this operation.

## Development self-signing mode

For software development and generated-data tests, the workspace provides an
explicitly feature-gated self-signed development identity helper. This is a
constrained testing convenience, not production authority:

- it is accepted only in an explicit development/local-Docker mode;
- it is limited to loopback or the canonical local container network;
- it is mapped to synthetic stores and prefixes under
  `$HOME/.dasobjectstore-codex-validation`;
- it has bounded read/write/list/verify rights and a finite byte budget;
- development access tokens expire quickly (at most 24 hours; shorter test
  lifetimes are preferred) and keys rotate with the validation profile;
- the private signing key is kept in the OS-private configuration area, not in
  the generated object-data root;
- it is rejected for appliance profiles, production configuration, non-local
  listeners, and real user/project stores; and
- startup and audit output identify development authentication clearly.

The helper is compiled only with the local workspace Cargo feature
`development-self-signing`; normal daemon builds and all package build scripts
use the default feature set. The helper's certificate/private-key material is
returned only to the local test process and is not persisted by the package
build.

Development self-signing is excluded from RPM and DEB artifacts. Packaging
must not ship a development private key, development issuer, development
configuration, or an installation-time switch that enables this mode. Package
tests must inspect the built contents and reject those markers; only local
workspace/test builds may exercise the self-signing path.

The daemon, not the self-signing key, remains the authority. A self-signed token
is accepted only when the daemon's development policy maps its issuer and
claims to the constrained development service principal.

## Lifecycle and safeguards

The implementation backlog covers service-principal registration, key/cert
rotation, short-lived token exchange, one-time completion capabilities,
revocation, replay protection, audit events, and contract fixtures. Every
credential class must support explicit expiry and revocation. Secrets and
private keys are never written to normal logs or exported consumer manifests.

## Versioned fixtures

The core crate publishes non-secret JSON fixtures under
`crates/dasobjectstore-core/fixtures/application-auth/` for adapter contract
tests. They cover the identity, public-key descriptor, exchange request,
scoped access token, renewal-only token, and upload-completion capability
shapes. The exchange fixture deliberately contains only a placeholder proof;
the daemon must perform cryptographic verification before issuing a token.

The public HTTPS gateway must use TLS and the existing daemon authorization
boundary. The exact route names and wire schema are implementation work under
the versioned API contract; this decision does not authorize a general-purpose
bearer endpoint.
