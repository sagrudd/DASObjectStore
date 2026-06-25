# Workloads

Workload definitions live here. Each workload should be runnable against any
provider that implements the object-service benchmark contract.

Implemented workloads:

- `large-object`

Planned workloads:

- `small-object`
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
bucket, object key, object size, and AWS credentials with environment variables
for local runs.
