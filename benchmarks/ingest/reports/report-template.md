# DASObjectStore Performance Test Report

| Field | Value |
| --- | --- |
| Brand | Mnemosyne Biosciences |
| Product | DASObjectStore |
| Report type | Performance test |
| Report status | `draft | final` |
| Run ID | `...` |
| Generated at (UTC) | `YYYY-MM-DDTHH:MM:SSZ` |
| Repository revision | `...` |
| CLI version | `...` |
| Command | `dasobjectstore performance-test ...` |
| Markdown SHA-256 | `computed after final render` |
| PDF artifact | `same basename as Markdown, .pdf extension, or not generated` |
| Reproduction QR payload | See `Reproduction Payload` |

## Environment

| Field | Value |
| --- | --- |
| Host | `...` |
| OS | `...` |
| CPU | `...` |
| RAM | `...` |
| Source device | `...` |
| SSD staging device/root | `...` |
| HDD targets/root | `...` |
| Filesystem | `...` |
| Copy count | `...` |
| Verification policy | `...` |
| Resource policy | `...` |

## Reproduction Payload

Encode the exact minified JSON payload below as the QR code payload. The
Markdown remains the canonical report; the QR payload is a compact index back
to the command, revision, raw inputs, and report hashes needed to reproduce or
audit the run.

```json
{"schema":"dasobjectstore.performance_test.reproduction.v1","brand":"Mnemosyne Biosciences","product":"DASObjectStore","run_id":"...","generated_at_utc":"YYYY-MM-DDTHH:MM:SSZ","repository_revision":"...","cli_version":"...","command":["dasobjectstore","performance-test","..."],"parameters":{"file_size":"...","file_count":0,"max_hdd_concurrency":0,"ssd_root":"...","hdd_root":"..."},"artifacts":{"markdown_path":"...","markdown_sha256":"...","pdf_path":"...","pdf_sha256":"...","raw_output_root":"..."}}
```

QR code image:

`Embed image path or state "not generated".`

## Scenario Results

| Scenario | Result | Bottleneck | Source-to-SSD | HDD fan-out | Verification | Peak RSS | Recovery |
| --- | --- | --- | --- | --- | --- | --- | --- |
| small-file | not run | not collected | not collected | not collected | not collected | not collected | n/a |
| large-file | not run | not collected | not collected | not collected | not collected | not collected | n/a |
| mixed-file | not run | not collected | not collected | not collected | not collected | not collected | n/a |
| slow-hdd | not run | not collected | not collected | not collected | not collected | not collected | n/a |
| full-ssd | not run | not collected | not collected | not collected | not collected | not collected | n/a |
| interrupted-import | not run | not collected | not collected | not collected | not collected | not collected | not collected |

## Acceptance Notes

- Sustained source-to-SSD staging:
- HDD fan-out:
- Verification throughput:
- Bounded memory growth:
- Recovery after interruption:

## Caveats

-

## PDF Artifact Strategy

- Treat the Markdown report as the source of record and generate PDF only after
  the Markdown content, QR payload, and hashes are finalized.
- Store the PDF beside the Markdown using the same basename and a `.pdf`
  extension so report bundles are predictable.
- Include the renderer name/version and PDF SHA-256 in final reports when a PDF
  is generated.
- Keep raw TSV/profiling outputs under `benchmarks/output/ingest/`; link to
  them from the Markdown instead of embedding bulky raw output in the PDF.
