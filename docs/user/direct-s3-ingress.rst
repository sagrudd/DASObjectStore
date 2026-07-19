Direct S3 Ingress
=================

Direct S3 ingress is an opt-in appliance mode that accepts AWS CLI-compatible
uploads directly into an ObjectStore's managed SSD. It removes the legacy
Garage-to-SSD full-copy step while preserving Garage for existing objects and
``store repair --reconcile-s3`` recovery.

The mode is feature-gated. ``garage_legacy`` remains the default and the safe
rollback path. Do not enable direct ingress merely because a package contains
the gateway: first preserve the current configuration, move Garage to its
private listener, validate credentials and profiles, and pass the acceptance
sequence below.

Architecture and acknowledgement
--------------------------------

The public endpoint stays at ``http://192.168.1.192:3900``. In direct mode the
DASObjectStore standalone server owns that listener; Garage moves to
``http://127.0.0.1:3901`` and remains available only for legacy/recovery
operations.

The gateway authenticates AWS Signature Version 4 against the daemon-managed,
store-scoped credential registry. The access key and bucket select the target
ObjectStore. A client cannot supply a managed path, profile, enclosure, or
placement target.

``AfterSsdIngest`` returns success only after the complete file is synchronized
on managed SSD and catalogue visibility plus a durable HDD destage job are
committed. It does not wait for HDD settlement. ``AfterHddPlacement`` remains
stricter and does not return success until its required verified HDD placement
exists. After a success the client may safely remove its local source; before
success it must treat the transfer as incomplete or uncertain and retry with
the same key, size, and digest.

The architectural decision is recorded in
``docs/direct-s3-ingress-adr.md``. The threat and failure review is in
``docs/direct-s3-ingress-threat-model.md``.

Supported and legacy surfaces
-----------------------------

Direct mode preserves existing path-style AWS CLI credentials and bucket
names. Catalogue-authoritative PUT, GET, HEAD, and bounded list are part of the
gateway acceptance surface. Multipart must pass initiate, part, resume,
complete, duplicate-complete, and abort tests before an operator enables this
mode for clients whose AWS CLI automatically selects multipart.

Legacy Garage payloads are not deleted or silently migrated. They remain
recoverable with:

.. code-block:: console

   sudo dasobjectstore store repair STORE_ID --reconcile-s3 --apply \
     --confirm "confirm store repair"

Run the same command without ``--apply`` first for a read-only plan.
Reconciliation must recognize a catalogue-visible direct object and avoid
copying it again.

Configuration
-------------

The standalone server configuration at
``/opt/dasobjectstore/config.json`` accepts this additive object:

.. code-block:: json

   {
     "s3_ingress": {
       "mode": "direct_gateway",
       "bind_address": "0.0.0.0",
       "port": 3900,
       "legacy_upstream_endpoint": "http://127.0.0.1:3901",
       "max_concurrent_uploads": 8
     }
   }

Omitting ``s3_ingress`` is backward compatible and resolves to:

.. code-block:: json

   {
     "s3_ingress": {
       "mode": "garage_legacy",
       "bind_address": "0.0.0.0",
       "port": 3900,
       "legacy_upstream_endpoint": "http://127.0.0.1:3901",
       "max_concurrent_uploads": 8
     }
   }

The concurrency value is admission, not a throughput promise. Start with the
packaged value, observe SSD latency, capacity waits, daemon queueing, Web health,
and HDD destage throughput, and change it only through a reviewed configuration
update. A value from 1 through 256 is valid; production limits should normally
be much lower than the schema maximum.

Before migration
----------------

Use a quiescent change window. Do not restart services during an active ingest,
repair, drain, migration, or user upload. These inspection commands do not
mutate data:

.. code-block:: console

   systemctl is-active dasobjectstored dasobjectstore-server
   systemctl --no-pager --full status dasobjectstored dasobjectstore-server
   ss -ltnp | grep -E ':(3900|3901|8448)\\b'
   dasobjectstore status --json
   dasobjectstore service status --json
   dasobjectstore store list --json
   sudo journalctl -u dasobjectstored -u dasobjectstore-server -n 200 --no-pager

Confirm that every enabled store has one unambiguous managed credential and
bucket, its intended profile is ready, the SSD has sufficient admitted
capacity, and its acknowledgement policy is correct. Preserve configuration
and service evidence without copying secret material into a report:

.. code-block:: console

   release="$(date -u +%Y%m%dT%H%M%SZ)"
   sudo install -d -m 0700 "/var/lib/dasobjectstore/upgrade-$release"
   sudo cp -a /opt/dasobjectstore/config.json \
     "/var/lib/dasobjectstore/upgrade-$release/config.json"
   sudo cp -a /etc/dasobjectstore/daemon.json \
     "/var/lib/dasobjectstore/upgrade-$release/daemon.json"
   if sudo test -f /etc/dasobjectstore/garage.compose.yml; then
     sudo cp -a /etc/dasobjectstore/garage.compose.yml \
       "/var/lib/dasobjectstore/upgrade-$release/garage.compose.yml"
   fi
   sudo systemctl cat dasobjectstored dasobjectstore-server \
     >"/tmp/dasobjectstore-units-$release.txt"
   sudo install -m 0600 "/tmp/dasobjectstore-units-$release.txt" \
     "/var/lib/dasobjectstore/upgrade-$release/units.txt"
   rm -f "/tmp/dasobjectstore-units-$release.txt"

Do not manually delete, move, or deduplicate Garage or managed SSD data.

Package deployment
------------------

Build on the Linux appliance and install through APT so package state remains
authoritative:

.. code-block:: console

   cd /home/stephen/src/DASObjectStore
   git fetch origin
   git switch main
   git pull --ff-only origin main
   make deb
   package="$(find "$PWD/target/deb" -maxdepth 1 -type f \
     -name 'dasobjectstore_*_amd64.deb' -print | sort -V | tail -n 1)"
   test -n "$package"
   package_dir="$(dirname "$package")"
   package_name="$(basename "$package")"
   cd "$package_dir"
   sudo apt-get install --reinstall "./$package_name"

The package install may restart DASObjectStore services. Re-check that the
host is quiescent immediately before the APT command. Never use ``dpkg -i`` for
an appliance deployment.

Listener migration
------------------

Garage and the gateway must never bind ``:3900`` at the same time. Preserve the
retained Garage container listener and ``garage.toml`` on ``3900`` while
publishing that container listener on loopback port ``3901``:

.. code-block:: console

   dasobjectstore service render-compose \
     --project-name dasobjectstore \
     --ssd-metadata-path /srv/dasobjectstore/ssd/garage \
     --hdd-data-path /srv/dasobjectstore/hdd/garage \
     --provider garage --service-name garage \
     --image dxflrs/garage:v2.3.0 \
     --bind-address 127.0.0.1 \
     --api-port 3900 --published-api-port 3901 \
     > /etc/dasobjectstore/garage.compose.yml

The resulting mapping is ``127.0.0.1:3901:3900``. This avoids rewriting the
retained provider configuration and preserves existing metadata/data volumes,
buckets, and keys. Review the generated Compose diff before applying it. Do
not use ``--api-port 3901``: that would also change the container-side ports
and would disagree with the retained ``garage.toml``.

With Garage healthy on the private listener, update only the ``s3_ingress``
object in ``/opt/dasobjectstore/config.json`` to the direct configuration above,
then validate before restart:

.. code-block:: console

   sudo dasobjectstore-server \
     --config /opt/dasobjectstore/config.json --check-config --json
   curl --silent --show-error --output /dev/null \
     --write-out 'Garage loopback HTTP status: %{http_code}\n' \
     http://127.0.0.1:3901/
   sudo systemctl restart dasobjectstored
   sudo systemctl restart dasobjectstore-server
   systemctl is-active --quiet dasobjectstored
   systemctl is-active --quiet dasobjectstore-server
   ss -ltnp | grep -E ':(3900|3901|8448)\\b'

An HTTP response code from Garage may be an S3 error and still prove that the
listener is reachable; use the provider's formal health command where
available. The required topology is one public gateway on ``:3900``, one
loopback-only Garage listener on ``127.0.0.1:3901``, and the existing Web UI on
``:8448``.

Acceptance tests
----------------

Use a dedicated generated-data ObjectStore and random fixtures only. Never use
project or customer data. Keep all automated acceptance payloads below 1 TiB.
The following examples assume a test bucket and store-scoped credential have
already been provisioned through DASObjectStore:

.. code-block:: console

   export AWS_ACCESS_KEY_ID='acceptance-store-access-key'
   export AWS_SECRET_ACCESS_KEY='acceptance-store-secret-key'
   export AWS_DEFAULT_REGION='garage'
   endpoint='http://192.168.1.192:3900'
   bucket='dos-codex-direct-s3'
   root="$HOME/.dasobjectstore-codex-validation/direct-s3"
   mkdir -p "$root"
   dd if=/dev/urandom of="$root/payload.bin" bs=1M count=64 status=progress
   sha256sum "$root/payload.bin" >"$root/payload.bin.sha256"

Single PUT, immediate HEAD/read, and catalogue visibility:

.. code-block:: console

   aws --endpoint-url "$endpoint" s3api put-object \
     --bucket "$bucket" --key acceptance/payload.bin \
     --body "$root/payload.bin"
   aws --endpoint-url "$endpoint" s3api head-object \
     --bucket "$bucket" --key acceptance/payload.bin
   aws --endpoint-url "$endpoint" s3api get-object \
     --bucket "$bucket" --key acceptance/payload.bin \
     "$root/download.bin"
   sha256sum --check "$root/payload.bin.sha256"
   cmp "$root/payload.bin" "$root/download.bin"
   dasobjectstore store profile-browser CODEX_DIRECT_S3 --json

Repeat the same PUT and verify that object identity, used capacity, and destage
job count do not increase. Upload different bytes to the same key and require a
conflict unless the store has an explicit replacement/versioning policy.

Force AWS CLI multipart with a generated payload larger than its configured
threshold, interrupt one upload, resume it, complete it, repeat completion, and
explicitly abort another upload. Verify that only complete objects become
visible and that abandoned retained bytes are explained by resumable state.
Also exercise zero-byte, sidecar (payload, ``.manifest.json``, ``.sha256``),
length/checksum mismatch, capacity exhaustion, daemon restart during receive,
restart during publication, and both acknowledgement policies.

For every successful ``AfterSsdIngest`` response confirm:

* immediate catalogue list/HEAD/GET and checksum equality;
* one verified SSD placement and one durable HDD destage identity;
* no complete Garage payload for the newly direct-ingested key;
* eventual required HDD copies and a settled state;
* no second reconciliation copy;
* Web/API liveness within the control-plane SLO while the upload budget is full.

Performance evidence
--------------------

Run the legacy and direct paths with the same generated payload, host, network,
store policy, and idle appliance conditions. Record actual values; do not infer
them from logical object size:

.. list-table:: Required comparison
   :header-rows: 1

   * - Measurement
     - Garage then reconcile
     - Direct gateway
   * - Payload bytes
     - record
     - same value
   * - Client elapsed time to policy acknowledgement
     - record
     - record
   * - Bytes written before HDD destage
     - record
     - record
   * - Managed SSD used delta
     - record
     - record
   * - Garage data used delta
     - record
     - record
   * - Source-to-SSD average/p95 throughput
     - record
     - record
   * - Complete-payload write amplification before HDD destage
     - expected near 2x
     - required near 1x

Archive ``iostat``/diskstats evidence, service status, object placement, journal
state, and exact Git/package versions with the report. A performance claim is
not complete until these before/after measurements are captured on a quiescent
appliance.

Rollback
--------

Rollback does not delete direct objects. Stop new uploads, allow active
requests to finish or reach a recorded terminal/resumable state, and verify the
daemon journal explains all staging data. Then:

1. restore the saved standalone configuration or set
   ``s3_ingress.mode`` to ``garage_legacy``;
2. validate it with ``dasobjectstore-server --check-config --json``;
3. stop the standalone server so it releases ``:3900``;
4. restore the saved ``garage.compose.yml`` with mode ``0640`` and apply it
   with ``dasobjectstore service up`` so Garage regains public ``:3900``
   without changing its data volumes, buckets, or keys;
5. start Garage, then ``dasobjectstored`` and ``dasobjectstore-server``;
6. confirm only Garage owns ``:3900`` and run HEAD/GET against a known legacy
   generated test object;
7. keep direct objects catalogue-visible through their managed SSD/HDD
   placements. Do not ask Garage reconciliation to recreate them.

If a package rollback is also required, install the retained prior DEB through
``apt-get install --reinstall /path/to/prior.deb``. Keep the new journals and
managed data until the older version's compatibility has been reviewed; never
remove metadata merely to make an older binary start.

Limitations and production gates
--------------------------------

* Direct mode is opt-in; omission means ``garage_legacy``.
* Public ``http://`` provides no transport confidentiality. Keep it on a
  trusted network or put a reviewed TLS terminator in front before wider
  exposure.
* Header-signed SigV4 with a fixed hexadecimal payload digest is the required
  write form. Presigned-query authentication, streaming SigV4 chunks, and
  ``UNSIGNED-PAYLOAD`` writes are not release claims unless their tests pass.
* Multipart is a release gate for ``aws s3 cp`` and ``sync`` workloads because
  AWS CLI may choose it automatically.
* Garage remains necessary for legacy objects and recovery. Direct mode is not
  permission to remove its data.
* Duplicate provider/managed representations may be reclaimed only by a
  daemon-owned, dry-run-first, catalogue-and-placement-proven process.
* The direct path does not make folder/drive/appliance protection equivalent;
  the selected profile and required HDD copy policy remain authoritative.
* Before/after write-amplification and control-plane soak evidence must be
  captured on the physical DASServer before declaring production acceptance.
