# Workloads

Workload definitions live here. Each workload should be runnable against any
provider that implements the object-service benchmark contract.

Implemented workloads:

- `large-object`
- `small-object`
- `concurrent-client`
- `crash-restart-ingest`
- `interrupted-write`
- `metadata-recovery`
- `disk-full`

Planned workloads:

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

## Concurrent Client

`concurrent-client.sh` starts multiple provider-neutral S3 round-trip clients in
parallel. Each client writes distinct object keys and an isolated per-client
report directory under `benchmarks/output/object-services/<provider>/`.

## Crash/Restart During Ingest

`crash-restart-ingest.sh` starts a large S3 upload, restarts the selected
provider's Compose service while that upload is in flight, then verifies that a
post-restart upload/download round trip succeeds. If the interrupted upload
reports success, the script also downloads and verifies that object.

## Interrupted Write

`interrupted-write.sh` starts a large S3 upload and interrupts the client
process without restarting the service. A retrievable interrupted object is
allowed only if its checksum matches the source payload. The script then proves
the service still accepts a clean post-interruption upload/download round trip.

## Metadata Recovery

`metadata-recovery.sh` uploads and verifies a seed object, stops the selected
provider, snapshots its benchmark bind-mounted state, restores that state, then
restarts the provider and verifies the object remains readable with the
expected SHA-256 hash.

## Disk Full

`disk-full.sh` creates a configurable allocated filler file inside the
provider's benchmark output tree, then attempts an S3 upload/download under
that pressure. A rejected write is recorded as acceptable behavior; an accepted
write must download with the expected SHA-256 hash.
