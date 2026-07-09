# floundeR Telemetry Chart Grammar

Status: Draft
Scope: product-neutral telemetry chart grammar for Mnemosyne products

## Intent

DASObjectStore emits appliance telemetry, but the plotting grammar is not
DASObjectStore-specific. The `mnemosyne.flounder.telemetry_chart_contract.v1`
contract is the reusable boundary that lets floundeR render telemetry charts
for DASObjectStore, Monas, Synoptikon, Mnematikon, and future Mnemosyne
products without each product inventing its own plotting semantics.

The Rust source of truth in this repository is
`crates/dasobjectstore-mnemosyne/src/flounder_telemetry.rs`. The
DASObjectStore-specific wrapper
`mnemosyne.flounder.appliance_telemetry.v1` can be converted into the
product-neutral chart contract by attaching the intended render audiences.

## Contract Envelope

The product-neutral envelope contains:

- `schema_version`: currently
  `mnemosyne.flounder.telemetry_chart_contract.v1`;
- `generated_at_utc`: UTC generation timestamp for the chart contract;
- `producer`: product identity, optional display name, and optional component
  id;
- `audiences`: render targets such as `web_dashboard`, `grammateus_report`, or
  `api_export`;
- `window`: selected time range, display label, optional start/end timestamps,
  source cadence, and downsample interval; and
- `charts`: ordered chart descriptions.

Consumers should treat the envelope as a render contract, not as raw telemetry
storage. Products may keep their own telemetry state files and APIs, but the
data passed to floundeR should be normalized through this grammar before
plotting or report generation.

## Chart Grammar

Each chart declares a stable `chart_id`, human title, layout, axes, series,
optional threshold bands, optional missing intervals, and optional
small-multiple panels. The current layouts are:

| Layout | Use |
| --- | --- |
| `line_with_gaps` | Continuous time-series lines that must break at missing samples. |
| `point_summary` | Point-in-time observations such as active users. |
| `step_summary` | Step-like counters or session counts. |
| `capacity_band` | Capacity or threshold-oriented charts with optional warning bands. |
| `per_disk_io_trace` | Device-scoped IO traces. |
| `small_multiple` | Repeated panels for comparable devices or subsystems. |

Series declare a `series_id`, label, role, unit, optional device identity, and
ordered points. Current roles are `line`, `point`, `step`, `band`, and
`trace`. Current units are `time_utc`, `percent_basis_points`, `bytes`,
`bytes_per_second`, `operations_per_second`, `count`, and `tib`.

Percent values are basis points, not floating-point percentages. For example,
`4200` means `42.00%`. This keeps Web dashboards and Grammateus PDF reports
consistent and avoids product-specific rounding drift.

## Missing Data Rules

Missing data is first-class. A point with `value = null` is not zero and must
not be bridged by a line segment. Point quality describes why the value is
missing:

| Point quality | Meaning |
| --- | --- |
| `missing_sample` | No sample was collected for the timestamp. |
| `unavailable_counter` | The platform or product could not expose that counter. |
| `service_restart` | Collection restarted and a delta cannot be calculated safely. |
| `unknown_device` | A referenced device was not known at render time. |

Charts may also provide `missing_intervals` with a start, end, reason, label,
and affected series ids. Renderers should label those gaps and split observed
points into separate line segments. Missing interval reasons are
`no_samples`, `service_stopped`, `counter_unavailable`, `device_unknown`, and
`collection_error`.

## Device and Small-Multiple Semantics

Device identity is optional for product-level series and required for
device-oriented traces. A device may include:

- `device_id`: stable product identity for the device or subsystem;
- `label`: display label;
- `enclosure_id`: optional physical or logical enclosure; and
- `bay_label`: optional bay, slot, worker, or partition label.

Small multiples use `series_ids` to bind panels to existing series rather than
duplicating points. DASObjectStore uses this for per-disk IO, but the same
shape should also work for Monas service traces, Synoptikon tenant or worker
panels, and Mnematikon report-generation stages.

## Cross-Product Guidance

Monas should use this grammar for local appliance and service telemetry where
standalone dashboards and reports need identical chart behavior.

Synoptikon should use it for platform and product-host telemetry exports so
embedded products can render comparable gap-aware charts under the same
dashboard and report rules.

Mnematikon should use it for report-pipeline health, render-stage throughput,
queue depth, and retry telemetry so Web previews and Grammateus PDFs share the
same missing-data semantics.

Future Mnemosyne products should add product-specific telemetry collectors and
domain-specific chart ids, but should reuse this envelope, window model,
layout vocabulary, unit vocabulary, device model, and missing-data rules before
adding a new chart contract.

## Compatibility Rules

The `v1` grammar is additive within its major version:

- new optional fields may be added when older renderers can ignore them;
- new enum values require renderer review because they may affect plotting;
- missing-data semantics must remain strict: null is never zero and gaps are
  never silently interpolated;
- product-specific wrappers should convert into
  `mnemosyne.flounder.telemetry_chart_contract.v1` rather than forking the
  chart vocabulary; and
- breaking grammar changes require a new schema version and a new registry
  entry.
