# Web GUI and Mnemosyne Plugin Specification

Status: Draft  
Scope: DASObjectStore as standalone HTTPS application, Synoptikon product/plugin,
and Mneion-native storage endpoint

## Product Position

Priority: DASObjectStore SHALL be brought under the Synoptikon umbrella as a
formal Mnemosyne product/plugin first. Standalone operation is still a required
delivery mode, but it must reuse the same domain model and must not fork the
product into a separate non-Mnemosyne architecture.

DASObjectStore SHALL be implemented as both:

- a standalone HTTPS application for non-Mnemosyne users; and
- a formal Mnemosyne/Synoptikon product plugin registered through the product
  catalogue and product UI bootstrap conventions.

Standalone mode owns local runtime state, local authentication, local audit, and
direct hardware workflows. Synoptikon-integrated mode SHALL use the common
Mnemosyne account, entitlement, audit, correlation, and storage-binding context
provided by the host.

Current Synoptikon and Mneion contracts are design inputs, not immutable
constraints. DASObjectStore SHALL initially align with existing conventions, but
the Mnemosyne platform may be changed where a better integrated storage
architecture requires it, provided every affected product, contract, migration,
test, and documentation surface is updated coherently. The goal is an integrated
platform for core facilities and artisanal bioinformatics service providers, not
a bolt-on appliance that preserves weak boundaries for their own sake.

## Port Policy

The permanent standalone HTTPS default port for DASObjectStore SHALL be `8448`.

Standalone deployments SHALL default to:

```text
https://127.0.0.1:8448
```

Linux appliance packages MAY bind `0.0.0.0:8448` when explicitly configured by
the operator.

Synoptikon-integrated deployments SHALL NOT use `8448` as a public listener.
They SHALL run behind Synoptikon's public HTTPS listener, currently `9443`, and
use a catalogue-assigned internal product port in the Synoptikon product range.
The public product surface SHALL be mounted under:

```text
/products/dasobjectstore
/products/dasobjectstore/api
```

This keeps DASObjectStore compatible with Synoptikon packaging while preserving a
stable standalone endpoint for customers and clients.

Standalone packaging, TLS asset paths, service validation commands, and Linux
appliance binding rules are defined in
[Standalone Service and Packaging](standalone-service.md).

## Authentication Model

The server SHALL use `axum` for HTTP/API routing and SHALL support two host
modes:

- `standalone`: local login, logout, session validation, and local user storage;
- `synoptikon_integrated`: no product-owned login endpoints; the host supplies
  authenticated user context, roles, entitlement, correlation ID, and audit
  authority.

Standalone local authentication SHOULD follow the Mnematikon pattern:

- local users and sessions stored under `/opt/dasobjectstore`;
- password hashes, registration tokens, and session token hashes persisted
  outside the Web bundle;
- default session TTL of one hour, configurable later;
- startup may revoke stale local sessions where required for safety;
- risky operations still require operation-specific confirmation even after
  login.

Integrated authentication SHALL treat Synoptikon as authoritative. Product API
handlers SHALL reject mutating actions when required host context, entitlement,
or role claims are absent.

## Daemon-Owned Mutation Boundary

`dasobjectstored` SHALL be the storage authority in standalone and
Synoptikon-integrated modes. Every storage-mutating action from the CLI, Axum
API, Yew UI, Synoptikon product mount, or Mneion adapter SHALL call the daemon
API and submit a request or job. Product code SHALL NOT write managed SSD/HDD
roots, edit live portable metadata, settle objects, drain disks, retire disks,
or change store policy by touching DAS filesystems directly.

Standalone mode maps local sessions and local user groups into daemon actor
claims. Synoptikon-integrated mode maps host-provided account, role,
entitlement, project, governance-domain, correlation ID, and audit context into
the same daemon request envelope. The daemon enforces storage permissions and
emits the authoritative storage-mutation audit trail; Synoptikon remains the
authority for login, entitlement, product routing, central audit correlation,
and governance-domain binding.

Read-only views MAY consume daemon health summaries, endpoint inventories, job
status, and object/store view models. Mutating views SHALL use daemon job
submission and progress streams so the Web GUI, CLI, and Synoptikon paths share
the same safety, authorization, audit, and cancellation semantics.

## Storage Endpoint Model

DASObjectStore SHALL be a native storage endpoint across Mneion. It SHALL support
at least these endpoint forms:

- `dasobjectstore_das`: local DAS-backed object service managed by
  DASObjectStore;
- `dasobjectstore_nfs`: external NAS/NFS-backed endpoint registered and
  validated by DASObjectStore but governed through Mneion storage definitions;
- `s3_compatible`: exported S3-compatible service endpoint produced by
  DASObjectStore's selected object-service provider.

All Mneion-facing contracts SHALL remain object-style. Even when the backing
endpoint is NFS or a local DAS filesystem, tenant-facing and product-facing
surfaces SHALL NOT expose raw filesystem paths as the durable storage contract.

External NAS endpoints SHALL be formal, validated storage definitions. They
SHALL include endpoint identity, export path or service URL, credential or mount
credential reference, TLS/CA material where relevant, validation status, and
governance-domain binding eligibility.

DASObjectStore-native endpoints SHALL NOT be treated as generic Mneion `posix`
storage definitions. The distinction is defined in
[Mnemosyne Contract Change Plan](mnemosyne-contract-change-plan.md):
DASObjectStore owns the appliance identity, health, ingest, destage, validation,
repair, and runtime mount leases, while Mneion owns governance-domain binding and
object-style product access.

## Web UI Design Language

The DASObjectStore interface SHALL feel like a storage operations console, not a
marketing page. It should be quiet, dense, and precise, with the visual weight
placed on operational state, risk, capacity, and pending actions.

Design principles:

- Information density over decorative cards.
- Tables, split panes, inspector drawers, segmented controls, and status badges
  as primary UI primitives.
- Stable dimensions for disk rows, capacity bars, object rows, and action
  buttons so live state changes do not shift layout.
- Restrained color: neutral base, green/amber/red/blue status accents, and no
  one-hue dominant palette.
- Buttons use icons for common actions and text only where the action carries
  risk or policy meaning.
- Destructive or risky operations use explicit confirmation panels and explain
  exactly what will happen before enabling the action.
- The same IA should work embedded in Synoptikon and as a standalone appliance.

Primary navigation:

- Overview: capacity, ingest pressure, destage urgency, health posture,
  endpoint state, and required actions.
- Disks: enclosure grouping, disk health, SMART/USB warnings, benchmark drift,
  drain/replace/retire workflows, and placement eligibility.
- Stores: store policies, redundancy, mutability, endpoint export mode, capacity
  behavior, and resizing/tiering plans.
- Objects: object inventory, store membership, state, copy locations, hashes,
  reproducibility source, export/download paths, and repair/redownload actions.
- Endpoints: DAS pools, external NAS/NFS definitions, S3 service status,
  Mneion storage-definition export, and governance-domain binding readiness.
- Activity: ingest queue, destage queue, repair tasks, audit/provenance events,
  and long-running operations.

The first screen after login SHALL be the Overview workspace, not a landing page.
It SHALL surface storage safety, usable capacity, ingest pressure, and required
operator actions without requiring the user to inspect JSON.

## Mnemosyne Integration Requirements

DASObjectStore SHALL provide a product manifest aligned with
`mnemosyne.product.manifest.v1` and support:

- `standalone = true`;
- `synoptikon_integrated = true`;
- product API mount `/products/dasobjectstore/api`;
- product Web mount `/products/dasobjectstore`;
- local hardware workflows in standalone mode;
- Synoptikon account, entitlement, central audit, object-store artifact, and
  project/RDBMS context in integrated mode where required.

The repository SHALL keep Mnemosyne-specific integration in the
`dasobjectstore-mnemosyne` boundary crate and avoid making the public core depend
on Mnemosyne runtime crates.

Contract changes required to make DASObjectStore a native Mneion storage
appliance are tracked in
[Mnemosyne Contract Change Plan](mnemosyne-contract-change-plan.md). That plan
is the coordination gate before changing Mneion storage-definition,
storage-binding, product SDK, or Limen contracts.
