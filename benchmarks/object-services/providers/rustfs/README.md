# RustFS Provider

This provider scaffold runs a single-node single-disk RustFS instance for
object-service benchmark development.

RustFS is a candidate because it is S3-compatible, built in Rust, and has a
simple local container mode. The benchmark starts with a single data volume so
it can be compared against the Garage single-node scaffold before later DAS
layout and destage compatibility tests are added.

## Files

- `compose.yml`: local benchmark Compose service.

## Local Run

```sh
docker compose -f benchmarks/object-services/providers/rustfs/compose.yml up -d
docker compose -f benchmarks/object-services/providers/rustfs/compose.yml ps
docker compose -f benchmarks/object-services/providers/rustfs/compose.yml down
```

The service binds S3 API traffic to `127.0.0.1:9000`, binds the console to
`127.0.0.1:9001`, and stores generated data under
`benchmarks/output/object-services/rustfs/`.

## Notes

- The default benchmark credentials are `rustfsadmin` / `rustfsadmin`.
- The provider wrapper pre-creates the benchmark bucket directories under the
  RustFS data path before startup because this single-node container profile
  may reject S3 `CreateBucket` calls from the default credentials.
- The image is pinned to `rustfs/rustfs:1.0.0-beta.8-glibc` for repeatable
  benchmark runs.
- RustFS containers run as UID/GID `10001`, so the Compose setup includes a
  short-lived permission step for local bind mounts.

Sources:

- [RustFS Docker installation](https://docs.rustfs.com/installation/docker/)
- [RustFS Docker image](https://hub.docker.com/r/rustfs/rustfs)
