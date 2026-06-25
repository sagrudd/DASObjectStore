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
benchmarks/object-services/scripts/provider.sh garage up
benchmarks/object-services/scripts/provider.sh garage ps
benchmarks/object-services/scripts/provider.sh garage down
```

The service binds S3 API traffic to `127.0.0.1:3900` and stores generated data
under `benchmarks/output/object-services/garage/`.

## Notes

- `provider.sh garage up` creates `benchmarks/output/object-services/garage/garage.env`
  with generated `GARAGE_DEFAULT_ACCESS_KEY` and `GARAGE_DEFAULT_SECRET_KEY`
  values when the file does not exist.
- Garage v2.3.0 starts with `--single-node --default-bucket`, then the provider
  wrapper provisions the benchmark buckets and grants the generated key read,
  write, and owner permissions.
- The image is pinned to `dxflrs/garage:v2.3.0` for repeatable benchmark runs.
- The Garage documentation recommends fixed image tags rather than `latest`.
- Garage documentation recommends SSD-backed metadata and HDD-backed data
  directories for this storage shape.

Sources:

- [Garage deployment guide](https://garagehq.deuxfleurs.fr/documentation/cookbook/real-world/)
- [Garage Docker image](https://hub.docker.com/r/dxflrs/garage)
