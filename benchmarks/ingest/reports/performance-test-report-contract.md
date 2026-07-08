# Performance Test Report Contract

This contract covers reports produced by `dasobjectstore performance-test`.
It is intentionally limited to report reproducibility and presentation so the
CLI parser and benchmark execution path can evolve independently.

## Canonical Artifact

The Markdown report is the source of record. A finished report should be
auditable from the Markdown alone and should not require the PDF artifact.

The report should start with a two-column table before the first narrative
section:

| Field | Required value |
| --- | --- |
| Brand | `Mnemosyne Biosciences` |
| Product | `DASObjectStore` |
| Report type | `Performance test` |
| Report status | `draft` or `final` |
| Run ID | Stable run identifier used in raw output paths |
| Generated at (UTC) | RFC 3339 UTC timestamp |
| Repository revision | Git commit or `unknown` |
| CLI version | DASObjectStore workspace package version |
| Command | Shell-escaped command line used for the run |
| Markdown artifact | Markdown report path |
| PDF artifact | Sibling PDF path |
| QR artifact | Sibling SVG path |
| QR status | QR renderer or fallback status |
| Reproduction payload SHA-256 | Hash of the exact JSON payload |
| Reproduction QR payload | Reference to the payload section |

## Reproduction QR Payload

The QR payload should be minified UTF-8 JSON using schema
`dasobjectstore.performance_test.reproduction.v1`. It should be stable enough
for scanners and short enough for practical QR generation. Use paths as the
operator supplied or as resolved by the command; do not redact them unless the
report is explicitly prepared for external distribution.

Required top-level fields:

| Field | Meaning |
| --- | --- |
| `schema` | `dasobjectstore.performance_test.reproduction.v1` |
| `brand` | `Mnemosyne Biosciences` |
| `product` | `DASObjectStore` |
| `run_id` | Stable run identifier |
| `generated_at_utc` | RFC 3339 UTC timestamp |
| `repository_revision` | Git commit or `unknown` |
| `cli_version` | DASObjectStore version |
| `command` | Full command argument array |
| `parameters` | File size/count, roots, concurrency, and temp policy |
| `artifacts` | Markdown/PDF/QR paths |

Markdown reports should include both the QR image path and the exact JSON
payload text. The text keeps the report reproducible in terminals, code review,
and systems that strip images.

## PDF Strategy

PDF is a distribution artifact derived from the Markdown report. Generate it
after the Markdown is finalized and place it beside the Markdown with the same
basename and a `.pdf` extension.

Do not make an external PDF renderer a prerequisite for benchmark execution. If
a host lacks the preferred PDF renderer, leave the Markdown report complete and
write a local fallback PDF artifact.

Raw metrics remain outside the PDF under `benchmarks/output/ingest/` or the
operator-selected output root. Reports should link those paths rather than
embedding bulky raw TSV/profiling content.
