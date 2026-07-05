# Bioinformatics Reference Workflow

Status: Draft
Scope: MVP reference workflow for `reproducible_cache` and `generated_data`
stores

## Intent

This workflow describes the first bioinformatics development shape that
DASObjectStore should support without making Mnemosyne a required dependency.

The reference case is a local DAS-backed object store that separates public or
reproducible inputs from derivative outputs:

- public reference datasets, genomes, indexes, and third-party archives belong
  in a `reproducible_cache` store;
- derived datasets, analysis outputs, and pipeline artefacts belong in a
  `generated_data` store;
- manifests, checksums, provenance, and object-service credential references
  remain metadata or `critical_metadata`, not ad hoc files hidden beside data.

The goal is to preserve expensive-to-redownload public data while giving
stronger protection to locally generated work.

## Store Layout

A minimal bioinformatics pool should define at least two user-facing stores:

```toml
# Policy sketch, not a finalized config-file schema.
[store.public_reference_cache]
class = "reproducible_cache"
copies = 1
ingest_mode = "ssd_first"
acknowledgement_policy = "after_ssd_ingest"
repair_policy = "redownload_or_rehydrate"
capacity_behavior = "mark_redownload_required"
export_policy = "s3"

[store.pipeline_outputs]
class = "generated_data"
copies = 2
ingest_mode = "ssd_first"
acknowledgement_policy = "after_hdd_placement"
repair_policy = "restore_from_copy"
capacity_behavior = "backpressure_by_priority"
export_policy = "s3"
```

`public_reference_cache` is allowed to trade redundancy for capacity because the
source is reproducible. It still records checksums, source URLs, and provenance
so an operator can redownload or rehydrate missing objects later.

`pipeline_outputs` uses a stronger acknowledgement and copy policy because
rerunning analysis may be more expensive, impossible, or scientifically
undesirable once upstream code and parameters have moved on.

## Object Boundaries

DASObjectStore should expose object boundaries rather than raw POSIX path
boundaries. A workflow runner, Synoptikon integration, or direct S3 client can
map domain artefacts to object keys, but the store policy remains the authority
for placement and redundancy.

Example key conventions:

```text
public_reference_cache/
  ensembl/release-113/homo_sapiens/genome.fa.zst
  ensembl/release-113/homo_sapiens/genome.fa.zst.sha256
  ncbi/sra/SRR000001/raw.fastq.zst

pipeline_outputs/
  runs/2026-06-25/alignment/sample-a.bam
  runs/2026-06-25/alignment/sample-a.bam.bai
  runs/2026-06-25/qc/multiqc-report.json
  runs/2026-06-25/provenance/run-manifest.json
```

Object metadata should preserve:

- source URL or accession where available;
- expected content hash;
- store class and store policy version;
- pipeline, parameter, and container image references for generated data;
- import time and settlement state;
- redownload-required state for reproducible cache objects.

## Ingest Flow

Normal writes use SSD-first ingest:

1. The client writes an object to the S3-compatible service.
2. DASObjectStore records the ingest job and computes the content hash.
3. The object is settled onto HDDs according to the store policy.
4. The object becomes SSD-eviction-eligible after policy-satisfying copies are
   verified.

Large public downloads may use a CLI-managed direct-to-HDD path when the store
policy and action-time confirmation allow it. That bypass is only appropriate
for reproducible objects with an expected digest and source metadata. It is not
the default write path for generated data.

```bash
dasobjectstore ingest direct-import ensembl-release-113-human-genome \
  --disk-id disk-a \
  --source /downloads/homo_sapiens/genome.fa.zst \
  --destination /mnt/disk-a/objects/public_reference_cache/ensembl/release-113/homo_sapiens/genome.fa.zst \
  --expected-sha256 <sha256> \
  --source-uri https://example.org/ensembl/release-113/homo_sapiens/genome.fa.zst \
  --policy-file policies/public-reference-cache-direct.json \
  --allow-direct-to-hdd-import \
  --confirm "confirm direct-to-hdd import"
```

The direct path writes the object once to the selected HDD destination, verifies
the resulting bytes against the expected hash, and reports the bypass warning in
the command output. Metadata commit and object-service exposure can then build
on the verified import report.

## Disk Failure Behavior

When a disk becomes suspect:

- `generated_data` objects are treated as protected data and should be drained
  or restored from verified copies before safe disk removal;
- `reproducible_cache` objects may be copied opportunistically when capacity is
  available;
- cache objects that cannot be evacuated may be marked redownload-required
  rather than blocking disk retirement indefinitely.

This keeps old-disk failure handling practical: important generated work is
protected first, while public reference data remains recoverable through its
source metadata.

## Mnemosyne Boundary

The Mnemosyne adapter should export object-store definitions and binding
snippets only. DASObjectStore should not leak raw local paths into the public
Mnemosyne contract.

Synoptikon and Mneion-facing workflows should treat DASObjectStore as an
S3-compatible object backend with per-store credentials and policy-aware
placement behind that boundary. Limen-mediated artefact ingress and egress
remain object-style.

## MVP Demonstration

A coherent MVP demonstration should show:

1. A `public_reference_cache` object imported with a known digest and source
   URL.
2. A `pipeline_outputs` object written through SSD-first ingest.
3. Store policy validation for both stores.
4. Object inspection showing store class, hash, settlement state, and copy
   status.
5. A simulated suspect disk where generated data is protected and reproducible
   cache data can be marked redownload-required.
6. A Mneion-compatible export that references the object service endpoint
   rather than host-local disk paths.

This is sufficient to prove the DAS-based object-store shape for
bioinformatics development without claiming that DASObjectStore is a backup
system or a full production archive.
