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
  grep -Fq 'Version: 0.172.0' <<<"$control"
fi

grep -Fq 'SYNOPTIKON_PROJECTION_FIXED_PEER_USER: &str = "dasobjectstore"' \
  "$repo_root/crates/dasobjectstore-daemon/src/api/synoptikon_projection.rs"
grep -Fq 'SYNOPTIKON_PROJECTION_MAX_BODY_BYTES: u64 = 1024 * 1024' \
  "$repo_root/crates/dasobjectstore-daemon/src/api/synoptikon_projection.rs"
for route in intent bytes readback; do
  grep -Fq "/v1/synoptikon-projection/$route" \
    "$repo_root/crates/dasobjectstore-gui-api/src/s3_gateway.rs"
done
grep -Fq '"synoptikon_projection_v1"' \
  "$repo_root/crates/dasobjectstore-gui-api/src/s3_gateway.rs"
grep -Fq 'ReadOnlyPaths=-/etc/dasobjectstore/synoptikon-projection-credential.json' \
  "$repo_root/packaging/linux/systemd/dasobjectstore-s3-gateway.service"
grep -Fq 'exact `192.168.0.193` IP SAN' "$repo_root/CHANGELOG.md"
grep -Fq 'localhost or `.192`' "$repo_root/CHANGELOG.md"
if find "$repo_root/packaging" -type f -name 'synoptikon-projection-credential.json' -print \
  | grep -q .; then
  printf 'projection service secret must not be package-owned\n' >&2
  exit 1
fi
grep -Fq '.join("projection-authority")' \
  "$repo_root/crates/dasobjectstore-daemon/src/runtime/synoptikon_projection.rs"
grep -Fq 'd /var/lib/dasobjectstore/projection-authority 0700 dasobjectstore dasobjectstore -' \
  "$repo_root/packaging/linux/tmpfiles.d/dasobjectstore.conf"
grep -Fq 'ensure_owned_dir /var/lib/dasobjectstore/projection-authority 0700' \
  "$repo_root/packaging/debian/postinst"
if find "$repo_root/packaging/linux/systemd" -type f -iname '*synoptikon*' -print \
  | grep -q .; then
  printf 'synoptikon projection contract unexpectedly activates a unit\n' >&2
  exit 1
fi

printf 'synoptikon projection package contract passed\n'
