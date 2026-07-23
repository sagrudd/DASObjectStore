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
| Short-lived access token | Target of a token exchange; normally 5–15 minutes | Grants narrowly scoped reads, writes, listings, verification, or exact-object deletion |
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

Credential enrollment is a separate administrator operation after identity
registration. The daemon rejects an enrollment unless the referenced identity
already exists and is active, the key lifetime is contained by the identity,
the credential kind and algorithm agree, and the fingerprint is not assigned
to another application. Ed25519 material must be exactly 32 decoded bytes;
P-256 material must be a 65-byte uncompressed SEC1 point. In both cases the
daemon recomputes the SHA-256 fingerprint before persistence. An mTLS mapping
contains only the CA-verified certificate fingerprint and must not contain
certificate or private-key material.

One identity uses one credential mode. The registered Ergasterion identity
`app-7e4a31c9b260` currently selects `asymmetric_key`, so its deployment must
register an Ed25519 or P-256 public key. Selecting mTLS instead requires a
reviewed identity replacement with `mtls_certificate` plus the production
client-CA listener; registering a certificate fingerprint against the current
asymmetric identity fails closed. Controlled rotation may overlap two active
descriptors for the same identity, but a fingerprint may never cross identity
boundaries.

## Token scope and completion

Access tokens are audience-bound and include an application identity, token
identifier, issued/expiry times, allowed ObjectStore and namespace, operation
set, ingress origin, and optional byte/object limits. The daemon rejects missing
or excessive scope before touching a backend.

### Governed Ergasterion reads

The approved split-authority bridge accepts
`ergasterion.object-store-binding.v1` as a dynamically evaluated scope source;
DASObjectStore does not mirror project bindings or manufacture project
authority. The binding is part of the canonical signed exchange request. The
daemon validates its active RFC 3339 lifetime, tenant/project/binding identity
shape, exact ObjectStore, logical prefixes, and the read-only operation set
before issuing resolved claims. A missing, expired, cross-store, cross-prefix,
or excessive binding fails closed before any provider request.

The assigned opaque service principal is `app-7e4a31c9b260`, with audience
`ergasterion-governed-data-service` and audit purpose
`ergasterion.governed-data-access`. Its v1 operation vocabulary is `list`,
`read`, and `verify`; `verify` means daemon-side metadata/checksum verification
and does not grant a separate payload-read route. The registration ceiling is
64 GiB per object and 256 GiB aggregate per capability. Every exchange must
request explicit equal-or-smaller limits, which the daemon returns in the
resolved claims. Tokens remain bounded by the production 15-minute maximum;
clients should renew within the final five minutes. Revocation propagation is
therefore at most 15 minutes, with 30 seconds of clock skew accepted only for
binding boundary evaluation.

Successful registration returns a non-secret registration record alongside
the identity. It documents the contract revision, limits, correlation/audit
contract, stable `governed_scope_denied` consumer reason, public-key rotation,
incident revocation, compatibility, and deprovisioning procedures. It contains
no token, secret, private key, endpoint, bucket, or managed path. An
Ergasterion deployment still requires a separately registered public key or
mTLS mapping before it can exchange a signed request.

An upload initiation response may include a short-lived, single-use completion
capability bound to the paired session, ObjectStore, upload ID, object key,
expected length, checksum, audience, expiry, and nonce. A client submits that
capability only after the provider transfer. The daemon independently checks
provider state, size, checksum, reservation state, and catalogue conflicts
before atomically committing completion. Replays are rejected or return the
same idempotent terminal result. The daemon now has a durable,
expiry-pruned replay registry for capability IDs and nonces. Its Ed25519/P-256
verifier handles asymmetric proofs and core issuance rejects unverified proofs;
mTLS transport verification and public capability issuance/exposure remain.
Provider verification uses a provider-neutral request/result contract. Garage
implements that contract through the existing cancellable AWS CLI command
runner, avoiding a second S3 client stack; provider identity, size, and checksum
must match before catalogue publication.
The daemon-owned EasyConnect AWS CLI path now provides the concrete Garage
implementation: a completion-bearing request runs `aws s3api head-object`
after transfer, requires the exact admitted `ContentLength` and the
`dasobjectstore-sha256` object-metadata value, and atomically publishes a
provider placement through the shared SQLite catalogue transaction. The job
does not enter `complete` until that transaction succeeds. Upload producers
must therefore attach `dasobjectstore-sha256=<lowercase hex digest>` as S3
object metadata when requesting authoritative completion.
The paired remote client produces these fields automatically for single-file
daemon submissions. It hashes the source as a bounded stream and adds the
checksum metadata to the same `aws s3 cp`; directory sync remains a legacy
transfer-only path until it can supply a per-key completion manifest.
The listener-side mTLS boundary is intentionally not enabled by inference.
The approved production policy uses native daemon-enforced mTLS with an
explicitly configured CA trust reference and daemon-owned certificate
fingerprint-to-application mapping. Missing, unknown, expired, or revoked
client certificates fail closed. Controlled certificate rotation may overlap
active mappings, and a listener that requires mTLS never silently falls back
to bearer-token authentication. Unix-socket OS-peer authentication remains a
separate local boundary.
Credential registration and revocation also emit atomically persisted audit
events that retain only identity/key metadata and a SHA-256 digest of the
operator reason. The completion helper verifies provider state before consuming
a capability and releases it when catalogue commit fails, so retries remain safe.
A failed
catalogue commit never reports success.

Profile-aware remote-upload callers may additionally provide a bounded
completion metadata record containing an upload ID, relative logical object
key, exact admitted size, and SHA-256 checksum. The daemon validates that
record after provider transfer and before invoking the completion authority;
legacy multi-object jobs may omit object-level metadata and retain their
transfer-only terminal semantics during migration; release integrations must
use the completion-bearing contract.

Renewal tokens are not accepted as bearer credentials for this operation.

### Exact-object deletion

Deletion is a separate application operation and is never implied by write or
upload-completion scope. The daemon requires an active paired ObjectStore
session plus a registered application identity whose `delete` operation,
ObjectStore, prefix, byte limit, and lifetime contain the request. It then
matches object ID, version, size, SHA-256, provider, bucket, and key against the
authoritative catalogue before provider mutation.

Garage deletion uses only daemon-managed credentials. The daemon verifies the
current provider object, deletes the exact key, verifies absence, atomically
withdraws the matching catalogue row, and records a redacted audit event.
Changed evidence or an uncatalogued provider object fails closed before
mutation. Exact absence is idempotent success. Applications must not substitute
raw S3 deletion or remove their projection before the authoritative response.
The v1 request is defined at
`/api/v1/application-auth/object-deletions`; its first consumer helper and live
synthetic deployment remain separate integration work.

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

All daemon/Web exchange and administrator identity/key/revocation wrappers use
strict JSON decoding: unknown fields are rejected before validation, registry
lookup, or claim consumption. This prevents clients from smuggling unreviewed
security decisions through an otherwise valid versioned payload.

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
