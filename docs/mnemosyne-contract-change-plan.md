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

### C6: Add Managed Mutation Request Context

Every storage-mutating DASObjectStore action exposed through Synoptikon, Mneion,
the standalone Axum API, or the Web UI must be submitted to `dasobjectstored`
through the daemon API. Mneion and Synoptikon product code may request or render
storage operations, but they must not write managed DAS roots, update portable
metadata, settle object copies, drain disks, retire disks, or change store
policy by bypassing the daemon.

Add a managed mutation envelope shared by DASObjectStore product routes and
Mneion adapters:

- `actor_id`;
- `actor_display`;
- `actor_roles`;
- `entitlements`;
- `governance_domain_id`;
- `project_id` where applicable;
- `storage_definition_id`;
- `storage_binding_id` where applicable;
- `client_request_id` for idempotency;
- `correlation_id`;
- `audit_origin`: `standalone`, `synoptikon`, `mneion`, or future product host;
- `requested_action`;
- `requested_at_utc`.

Standalone deployments populate the envelope from local sessions and local
writer/admin groups. Synoptikon-integrated deployments populate it from the host
request context. `dasobjectstored` remains the final storage authorization point
and emits authoritative storage-mutation audit events. Synoptikon remains the
authority for account/session, entitlement, product routing, central audit
correlation, and governance-domain binding.

## Storage-Definition Schema Change Inventory

These are the required Mneion storage-definition schema changes identified from
the DASObjectStore side. They are additive unless noted otherwise.

### Storage Definition Record

Add these fields to the Mneion storage-definition or object-store definition
record:

| Field | Type | Required | Notes |
| --- | --- | --- | --- |
| `endpoint_owner` | enum | yes for new records | Values: `mneion`, `dasobjectstore`; default existing rows to `mneion`. |
| `endpoint_kind` | enum | yes for new records | Values: `s3_compatible`, `nfs`, `posix`, `dasobjectstore_das`, `dasobjectstore_nfs`. |
| `manager_product_id` | string or null | required for managed endpoints | Must be `dasobjectstore` for DASObjectStore-managed endpoints. |
| `manager_api_path` | string or null | required for managed endpoints | Synoptikon-relative API mount, initially `/products/dasobjectstore/api`. |
| `object_contract` | enum | yes | `object_style` for DASObjectStore endpoints; raw path contracts are not allowed for managed endpoints. |
| `validation_contract` | string or null | recommended | Schema id for validation evidence, initially `dasobjectstore.nas_nfs_endpoint.v1` or later DAS pool equivalent. |
| `health_contract` | string or null | recommended | Schema id for endpoint health summaries exposed by DASObjectStore. |
| `mutation_contract` | string or null | required for managed endpoints | Schema id for daemon-backed mutation requests, initially `dasobjectstore.managed_mutation.v1`. |
| `capabilities` | string array | yes | Examples: `ssd_ingest`, `hdd_destage`, `copy_redundancy`, `disk_health`, `direct_reproducible_import`. |

Existing `backend_kind` should be retained during the transition for backwards
compatibility, but new code should derive behavior from `endpoint_kind` and
`endpoint_owner`.

### Managed Endpoint Location

DASObjectStore-managed endpoint location metadata should be represented as a
typed object rather than a raw path:

| Endpoint kind | Location fields | Raw path exposure |
| --- | --- | --- |
| `dasobjectstore_das` | `pool_id`, `service_endpoint` | none |
| `dasobjectstore_nfs` | `export_id`, `service_endpoint` | raw NFS server/export remain validation-only evidence, not tenant context |
| `s3_compatible` | `provider_id`, `endpoint` | none |

### Validation Evidence

Add a validation-evidence record or payload accepted by Mneion admin/storage
routes:

- `schema_version`;
- `storage_definition_id`;
- `governance_domain_id` where validation is binding-specific;
- `endpoint_id`;
- `manager_product_id`;
- `validated_at_utc`;
- `validated_by`;
- `validation_state`: `draft`, `pending_validation`, `validated`, `degraded`,
  or `rejected`;
- `health_state`;
- `object_service_reachable`;
- `policy_compatible`;
- `warnings`;
- references to DASObjectStore health details rather than inline disk internals.

Rejected endpoints must remain visible for audit but must not become eligible
for new active bindings.

### Resolved Storage Context

Extend resolved Mneion storage context for products with managed endpoint
metadata:

- `endpoint_owner`;
- `endpoint_kind`;
- `manager_product_id`;
- `object_access_profile`;
- `namespace_prefix`;
- `credential_ref`;
- `validated_at_utc`;
- `health_state`;
- `manager_api_path`.

Resolved context must not include DASObjectStore local paths, NFS mount paths, or
raw DAS member paths.

## Coordinated Implementation Plan

This repository has implemented the DASObjectStore-side endpoint model, export
tests, NAS/NFS validation model, runtime validation planning, binding export,
GUI API view models, and CLI endpoint-definition validation. The remaining
schema work should be implemented in the Mnemosyne repositories in this order:

1. `../mnemosyne/mneion-api-types`: add endpoint-owner, endpoint-kind,
   managed-location, validation-evidence, and resolved-context types. Include
   compatibility mapping from existing `backend_kind` values.
2. `../mnemosyne/mneion-server`: add nullable persistence columns or a versioned
   JSON metadata column for the additive fields, with migrations that default
   existing rows to `endpoint_owner = "mneion"` and infer `endpoint_kind` from
   existing backend values.
3. `../mnemosyne/mneion-server`: update admin storage-definition create/update,
   validation-evidence ingest, and governance-domain binding readiness checks.
4. `../mnemosyne/mneion-web`: update Authority and Runtime storage workbenches
   to show managed appliance state, validation state, health state, and the
   DASObjectStore management link.
5. `../mnemosyne/mnemosyne-product-sdk`: add host helpers for managed storage
   appliance links, manager API paths, and endpoint health bootstrap metadata.
6. `../mnemosyne-docs`: update the Mneion SRS, storage-binding implementation
   notes, admin guide, and architecture decision records.
7. `../DASObjectStore`: update export payloads only after the accepted
   Mnemosyne schema is available, keeping compatibility tests for the prior
   export shape until the platform migration is complete.

No sibling repository changes should be committed from DASObjectStore
automation. This plan is the handoff boundary for coordinated Mnemosyne work.

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
- Add product-route and adapter tests proving storage-mutating actions construct
  daemon API requests with actor, entitlement, correlation, and audit context
  rather than mutating DAS filesystems directly.

## Non-Changes

- Do not change Synoptikon request-context ownership: Synoptikon remains the
  authority for account/session, entitlement, project, audit, and correlation.
- Do not let Synoptikon, Mneion, Axum routes, Yew actions, or CLI client paths
  bypass `dasobjectstored` for storage-mutating operations.
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
