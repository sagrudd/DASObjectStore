# Workloads

Workload definitions live here. Each workload should be runnable against any
provider that implements the object-service benchmark contract.

Implemented workloads:

- `large-object`
- `small-object`

Planned workloads:

- `concurrent-client`
- `crash-restart-ingest`
- `interrupted-write`
- `metadata-recovery`
- `disk-full`
- `simulated-disk-removal`
- `ssd-ingest-hdd-destage`

## Large Object

`large-object.sh` uploads one large object through an S3-compatible endpoint,
downloads it, verifies the SHA-256 hash, and writes a TSV report under
`benchmarks/output/object-services/<provider>/workloads/large-object/`.

The script uses provider defaults from `scripts/run.sh`; override endpoint,
bucket, object prefix, object size/count, and AWS credentials with environment
variables for local runs.

## Small Object

`small-object.sh` uploads and downloads many small objects through the same
provider-neutral S3 round-trip helper. It verifies each downloaded object with
SHA-256 and writes one TSV row per object.
