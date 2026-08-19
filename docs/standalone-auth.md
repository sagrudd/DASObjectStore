# Standalone Authentication Decision

Status: Superseded by the Pistis-only host authentication contract
Scope: standalone appliance host mode and administrator authority

## Decision

Standalone DASObjectStore appliances use Monas as their host authentication
authority. Monas establishes the operator identity through Pistis and
Prosopikon; DASObjectStore receives only the verified, credential-free host
context. Synoptikon-integrated deployments use the same contract with
Synoptikon as the host authority.

OS-local users and groups remain transport, ownership, and ordinary writer-job
policy inputs where required by the daemon. They are not human authentication,
administrator authority, or a substitute for a verified Pistis subject.
Neither sudo membership nor writer-group membership grants direct write access
to managed DAS roots; all storage mutation still goes through
`dasobjectstored`.

The retired product-local login/session store, password helper, and PAM service
are not part of the standalone package or runtime authority surface. The
`local_user` configuration value is retained only as a decode-only migration
marker and is rejected by validation; it cannot re-enable the old authority.

## Rationale

DASObjectStore manages disks, mounts, services, and long-running storage jobs
on a host. Human authority belongs to the host's Pistis ceremony, while
storage mutation remains daemon-owned. This avoids creating a second local
administrator database that can drift from the site's authoritative identity
and entitlement state.

Pistis authentication proves the operator's host identity and roles. It does
not by itself grant storage access: the daemon still evaluates ObjectStore
policy, entitlement, writer scope, and operation-specific confirmation.

## Boundary

- `dasobjectstored` remains the final storage authorization point.
- The standalone Axum API and Yew UI are clients of the daemon for all
  storage-mutating work.
- Monas or Synoptikon Pistis roles authorize the explicitly exposed host
  workflows.
- OS-local writer groups may authorize ordinary daemon job submission after
  the host route has established its verified actor context.
- Local cookies, passwords, PAM results, and claimed OS identities never
  create a DASObjectStore human session.
- Synoptikon-integrated mode remains authoritative for account, entitlement,
  audit, correlation, and governance-domain context.

## Implementation Implications

The implementation must keep the Monas/Synoptikon host adapter, Pistis session
freshness checks, CSRF binding, and daemon-side storage authorization in one
contract. Package installation must leave the service ready for the attended
host onboarding flow without requiring a second DASObjectStore login or manual
PAM configuration.
