# Garage Provider

This provider scaffold runs a single-node Garage instance for object-service
benchmark development.

Garage is a candidate because it is S3-compatible, lightweight, and supports
separate metadata and data directories. That matches the DASObjectStore model
where metadata and ingest state should sit on SSD while bulk object blocks can
sit on larger HDD-backed storage.

## Files

- `compose.yml`: local benchmark Compose service.
- `garage.toml`: benchmark-only Garage configuration.

## Local Run

```sh
docker compose -f benchmarks/object-services/providers/garage/compose.yml up -d
docker compose -f benchmarks/object-services/providers/garage/compose.yml ps
docker compose -f benchmarks/object-services/providers/garage/compose.yml down
```

The service binds S3 API traffic to `127.0.0.1:3900` and stores generated data
under `benchmarks/output/object-services/garage/`.

## Notes

- The benchmark client defaults to `garageadmin` / `garageadmin` when
  `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY` are not set. Provision
  matching Garage keys before running real S3 workloads.
- The image is pinned to `dxflrs/garage:v2.3.0` for repeatable benchmark runs.
- The Garage documentation recommends fixed image tags rather than `latest`.
- Garage documentation recommends SSD-backed metadata and HDD-backed data
  directories for this storage shape.

Sources:

- [Garage deployment guide](https://garagehq.deuxfleurs.fr/documentation/cookbook/real-world/)
- [Garage Docker image](https://hub.docker.com/r/dxflrs/garage)
