#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
request="$repo_root/crates/dasobjectstore-core/fixtures/synoptikon-projection/request-v1.json"
readiness="$repo_root/crates/dasobjectstore-core/fixtures/synoptikon-projection/readiness-v1.json"
schema="$repo_root/docs/schemas/dasobjectstore.synoptikon-projection.v1.schema.json"

python3 - "$request" "$readiness" "$schema" <<'PY'
import json, pathlib, sys
request = json.loads(pathlib.Path(sys.argv[1]).read_text())
readiness = json.loads(pathlib.Path(sys.argv[2]).read_text())
schema = json.loads(pathlib.Path(sys.argv[3]).read_text())
assert request["producer_product"] == "syno_plug_demo"
assert request["producer_host"] == "nuc-192-168-0-193"
assert request["consumer_product"] == "oikodome"
assert request["consumer_host"] == "gb10-192-168-0-48"
assert readiness["endpoint_url"] == "https://192.168.0.193:3900"
assert readiness["expected_tls_peer_certificate_sha256"] == readiness["observed_tls_peer_certificate_sha256"]
assert readiness["catalogue_mapping"]["ambiguous_unmapped_objects"] == 0
assert readiness["mapping_exclusion"] is None
assert readiness["upload_completion"]["source_sha256"] == request["source_sha256"]
assert readiness["catalogue_object"]["object_id"] == request["object_id"]
assert readiness["provider_group_status"]["settled"] is True
assert readiness["hdd_replicas"][0]["disposition"] == "hdd_verified"
encoded = json.dumps([request, readiness, schema], sort_keys=True)
for forbidden in ("/srv/dasobjectstore", "managed_path", "bucket", "secret_key", "access_key"):
    assert forbidden not in encoded
assert schema["$defs"]["request"]["properties"]["producer_host"]["const"] == "nuc-192-168-0-193"
assert schema["$defs"]["readiness"]["properties"]["endpoint_url"]["const"] == "https://192.168.0.193:3900"
assert schema["$defs"]["authenticated_readiness"]["properties"]["schema_version"]["const"] == "dasobjectstore.authenticated_synoptikon_projection_readiness.v1"
PY

if [[ $# -eq 1 ]]; then
  deb="$1"
  listing="$(dpkg-deb -c "$deb")"
  for name in schema-v1.json request-v1.json readiness-v1.json; do
    grep -Fq "./usr/share/doc/dasobjectstore/contracts/synoptikon-projection/$name" <<<"$listing"
  done
  control="$(dpkg-deb -f "$deb")"
  grep -Fq 'Version: 0.170.0' <<<"$control"
fi

printf 'synoptikon projection package contract passed\n'
