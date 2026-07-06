# Mnemosyne Contract Change Plan

Status: Draft  
Scope: Milestone 13 contract-change identification  
Primary goal: make DASObjectStore a native Mneion storage appliance without
weakening existing governance-domain storage boundaries

## Summary

DASObjectStore should not be modelled as a generic POSIX backend or as a raw
NFS mount in Mneion. The cleaner platform shape is to extend Mneion's
storage-definition and storage-binding contracts with first-class
DASObjectStore endpoint kinds while preserving object-style tenant contracts.

The current Mneion direction is compatible with this:

- governance-domain storage binding is the storage authority;
- products consume resolved storage context rather than raw paths;
- POSIX backends are supported but discouraged;
- runtime mounts are leases, not durable control-plane facts.

The required contract changes are therefore additive and should be coordinated
across DASObjectStore, Mnemosyne, and Mnemosyne documentation before
implementation.

## Proposed Contract Changes

### C1: Add DASObjectStore Endpoint Kinds

Extend Mneion storage-definition contracts to distinguish:

- `dasobjectstore_das`: DASObjectStore-managed local DAS pool;
- `dasobjectstore_nfs`: DASObjectStore-managed external NAS/NFS endpoint;
- `s3_compatible`: existing S3-compatible object-service endpoint;
- `nfs`: ordinary Mneion NFS endpoint where DASObjectStore is not the manager;
- `posix`: discouraged transitional filesystem endpoint.

Rationale: DASObjectStore endpoints carry appliance health, ingest, destage,
disk, store-policy, and object-copy semantics that ordinary NFS/POSIX records do
not express.

## DASObjectStore-Native Endpoints Versus `posix`

Generic Mneion `posix` storage definitions describe a filesystem path and leave
most runtime safety semantics outside the storage contract. That is useful for
transitional integrations, but it is too weak for DASObjectStore because the
durable storage boundary is an object appliance, not a path.

DASObjectStore-native endpoints differ from `posix` definitions in these ways:

- Identity is managed by DASObjectStore and exported to Mneion as
  `manager_product_id = "dasobjectstore"` plus an endpoint kind such as
  `dasobjectstore_das` or `dasobjectstore_nfs`.
- Tenant-facing access remains object-style. Mneion products should receive an
  object access profile, namespace prefix, and credential reference, not a local
  directory, NFS export, or mount path.
- Health and validation are first-class binding inputs. Disk health, USB
  posture, NAS/NFS validation, object-service reachability, and endpoint
  degradation can block new unsafe bindings while preserving visibility of
  existing endpoints.
- Runtime filesystem or NFS mounts are implementation leases owned by
  DASObjectStore. They may be created for validation, repair, export, or local
  service operation, but they are not durable Mneion control-plane facts.
- Store policy remains attached to the managed appliance. SSD ingest, HDD
  destage, copy redundancy, direct reproducible import, object verification,
  repair, and redownload semantics cannot be represented by a bare POSIX path.
- Governance-domain binding remains authoritative. DASObjectStore does not hand
  products raw filesystem credentials or paths to bypass Mneion binding rules.

`posix` should therefore remain a compatibility or emergency endpoint class for
ordinary filesystem-backed storage. DASObjectStore-managed DAS, NAS/NFS, and
S3-compatible endpoints should use the native endpoint kinds so the platform can
reason about validation, safety, health, and binding readiness.

### C2: Extend Storage Definition Metadata

Add optional fields to Mneion object-store/storage-definition payloads:

- `endpoint_owner`: `mneion`, `dasobjectstore`, or future owners;
- `endpoint_kind`: one of the endpoint kinds above;
- `manager_product_id`: `dasobjectstore` for DASObjectStore-managed endpoints;
- `manager_api_path`: host-relative management path, for example
  `/products/dasobjectstore/api`;
- `validation_contract`: schema identifier for endpoint validation evidence;
- `health_contract`: schema identifier for endpoint health summaries;
- `capabilities`: array including values such as `ssd_ingest`,
  `hdd_destage`, `copy_redundancy`, `disk_health`, and `direct_reproducible_import`.

Rationale: Mneion needs enough typed metadata to present, validate, and bind a
DASObjectStore endpoint without learning DAS disk internals or exposing raw
paths to tenant-facing contracts.

### C3: Keep Governance-Domain Binding Authoritative

Resolved storage context should continue to be returned by Mneion only after an
active governance-domain binding exists. DASObjectStore must not bypass Mneion
by handing products raw credentials, raw NFS export paths, or local filesystem
paths as durable contracts.

For DASObjectStore-managed endpoints, resolved context should include:

- `storage_definition_id`;
- `storage_binding_id`;
- `governance_domain_id`;
- `endpoint_kind`;
- `manager_product_id`;
- `object_access_profile`;
- `namespace_prefix`;
- `credential_ref`;
- `validated_at_unix`;
- `health_state`.

### C4: Add Endpoint Validation Evidence

Mneion should accept DASObjectStore-generated validation evidence for
DASObjectStore-managed endpoints. The evidence should be object-style and should
not include durable raw paths.

Minimum validation evidence:

- endpoint identity and manager product id;
- storage definition id and governance domain id;
- validation time and actor;
- endpoint health state;
- object-service reachability result;
- policy compatibility result;
- DAS/NAS health summary reference;
- warnings that affect binding readiness.

### C5: Add UI/API Treatment for Managed Appliances

Mneion's Authority and Runtime storage workbench should present
DASObjectStore-managed endpoints as managed appliances, not just as static
object-store rows.

Required UI/API behavior:

- show endpoint owner and endpoint kind;
- show validation and health state separately;
- link to DASObjectStore management under `/products/dasobjectstore`;
- prevent raw path display as the primary durable contract;
- allow degraded endpoints to remain visible while blocking new unsafe bindings.

## Affected Repositories

- `../DASObjectStore`: endpoint kind model, Mneion export contracts, validation
  evidence, GUI/API endpoint inventory, CLI export commands, tests.
- `../mnemosyne/mneion-api-types`: storage-definition types, endpoint-kind
  enums, resolved storage context, validation evidence schemas, schema tests.
- `../mnemosyne/mneion-server`: persistence/migrations, admin storage routes,
  governance-domain binding resolution, validation workflows, runtime status.
- `../mnemosyne/mneion-web`: Authority and Runtime storage workbench updates.
- `../mnemosyne/mneion-admin`: catalogue/profile validation and migration
  support for first-party DASObjectStore product entries.
- `../mnemosyne/mnemosyne-product-sdk`: shared host adapter helpers for
  managed storage appliance links and endpoint health bootstrap metadata.
- `../mnemosyne-docs`: Mneion SRS, storage-binding implementation notes, admin
  guide, and architecture decision updates.
- `../limen`: only if resolved storage context or managed mount leases need new
  fields for DASObjectStore-managed NFS endpoints.

## Required Migrations And Tests

- Add migrations for new storage-definition fields while preserving existing
  `backend_kind` string data.
- Add compatibility mapping from existing `S3-Compatible`, `nfs`, and `posix`
  values into the new endpoint-kind model.
- Add schema tests for `dasobjectstore_das`, `dasobjectstore_nfs`, and
  `s3_compatible` definitions.
- Add governance-domain binding tests proving one active primary binding remains
  authoritative.
- Add negative tests proving product-facing contracts do not expose raw local or
  NFS paths.
- Add validation evidence tests for healthy, degraded, and unsafe endpoints.
- Add Web/API tests for managed-appliance visibility and binding readiness.

## Non-Changes

- Do not change Synoptikon request-context ownership: Synoptikon remains the
  authority for account/session, entitlement, project, audit, and correlation.
- Do not expose DASObjectStore's standalone HTTPS port `8448` through
  Synoptikon catalogue entries.
- Do not make Mneion a DAS disk manager. DASObjectStore owns disk, enclosure,
  ingest, destage, store-policy, and repair workflows.
- Do not use raw filesystem paths as tenant-facing or product-facing durable
  storage contracts.

## Implementation Order

1. Add DASObjectStore endpoint model and tests in this repository.
2. Propose `mneion-api-types` schema additions and compatibility mapping.
3. Update Mneion server persistence and storage-definition APIs.
4. Update Mneion Web Authority and Runtime workbench.
5. Wire DASObjectStore export/validation commands to the accepted Mneion
   contracts.
6. Update Mnemosyne documentation and SRS references.
