Remote S3 Uploads
=================

DASObjectStore can expose an S3-compatible endpoint for object stores whose
policy exports S3. Remote computers can then upload with the AWS CLI using
``s3api put-object``, ``s3 cp``, or ``s3 sync`` against the DASObjectStore
endpoint URL.

For routine remote-computer use, prefer the standalone
``dasobjectstore-remote`` client described in :doc:`remote-client`. It wraps the
same S3 path with remote configuration, secure password prompting for
credential helpers, object-store listing, and file/folder upload commands.

Prepare the remote upload plan on the appliance or an administrative host that
can read the DASObjectStore store registry:

.. code-block:: console

   dasobjectstore store s3-upload generated-data \
     --endpoint-url http://192.168.1.192:3900 \
     --auth mneion

The command resolves the store's S3 bucket, credential reference, AWS CLI
profile name, endpoint URL, upload commands, and ingress classification. Remote
S3 uploads are reported as ``remote_s3`` ingress with ``ssd_first`` landing so
the appliance can stage incoming bytes on the selected ObjectStore SSD before
daemon-owned HDD settlement. Use ``--json`` when another tool needs to consume
the plan.

Endpoint reachability
---------------------

The endpoint URL in the upload plan must be reachable from the remote computer.
``http://127.0.0.1:3900`` only verifies that the object service is listening on
the DAS host itself; it is not a valid endpoint for a different workstation.

When rendering Docker Compose for the appliance object service, the CLI defaults
to a remote-capable host binding:

.. code-block:: console

   dasobjectstore service render-compose \
     --project-name dasobjectstore \
     --ssd-metadata-path /srv/dasobjectstore/ssd/garage \
     --hdd-data-path /srv/dasobjectstore/hdd/garage \
     --provider garage \
     --service-name garage \
     --image dxflrs/garage:v2.3.0 \
     --api-port 3900

This renders host port bindings on ``0.0.0.0``. For local-only testing, pass
``--bind-address 127.0.0.1`` explicitly. ``dasobjectstore status`` reports when
the detected Docker listener is loopback-only so operators do not mistake a
local health check for a remote-upload-ready endpoint. Use
``dasobjectstore status --json`` as the healthcheck surface and hand remote
clients ``object_service.remote_url`` only when
``object_service.remote_ready`` is ``true``.

Bucket provisioning and service restarts
----------------------------------------

Garage buckets and S3 keys are live service state, not Docker Compose
configuration. Adding a new DASObjectStore ObjectStore must not require
rerendering or restarting the Garage container. The Compose file should define
the Garage process, ports, volumes, and ``garage.toml`` only. After an
S3-exported ObjectStore is created, an administrator applies the ObjectStore
registry to the running service with:

.. code-block:: console

   dasobjectstore service provision --provider garage

The command asks the DASObjectStore daemon to create the required Garage bucket
and grant the appropriate per-store S3 key. Use ``--dry-run`` to inspect how
many stores, buckets, and Garage admin commands would be applied. Avoid manual
``docker compose exec garage /garage ...`` flows for routine ObjectStore
creation; they bypass DASObjectStore's registry and credential custody model.

Provisioning stores Garage credentials in a daemon-owned managed registry at
``/var/lib/dasobjectstore/object-service/garage-credentials.json`` on Linux.
The directory is installed with mode ``0700`` and the registry file is written
with mode ``0600`` because it contains S3 secret keys. Re-running
``dasobjectstore service provision`` reuses the persisted credential for each
ObjectStore and records an audit event instead of minting a new key.

Rotate the persisted store-scoped Garage credentials only when there is a clear
operational reason:

.. code-block:: console

   dasobjectstore service provision --provider garage --rotate-credentials

Rotation persists the newly issued access key and secret, records the previous
access key in the audit trail, and reapplies Garage bucket/key grants. The CLI
prints credential counts for issued, reused, and rotated credentials, but it
never prints S3 secret material.

Credential handling
-------------------

Do not distribute the Garage default key as a shared appliance credential.
Remote users should receive store-scoped S3 credentials from the DASObjectStore
remote/easyconnect flow, Mneion/Synoptikon integration, or an administrator-run
DASObjectStore credential issuance path. Local shell exports such as
``GARAGE_DEFAULT_ACCESS_KEY`` and ``GARAGE_DEFAULT_SECRET_KEY`` are suitable
only for break-glass provider administration by trusted appliance operators.

On a remote computer that does not have the DASObjectStore store registry, pass
the bucket name explicitly:

.. code-block:: console

   dasobjectstore store s3-upload generated-data \
     --endpoint-url http://192.168.1.192:3900 \
     --bucket dos-generated-data \
     --auth mneion

Authentication authority
------------------------

The AWS CLI does not send a Mneion password or a local Unix password to the S3
endpoint. It sends an S3 access key and secret key. DASObjectStore therefore
keeps authentication and S3 credentials as two distinct steps:

* ``--auth mneion`` means Mneion manages authorization, credential custody, and
  credential issuance for the displayed credential reference. The remote user
  signs in to Mneion and receives or configures the S3 access key and secret key
  that Mneion authorizes for that store.
* ``--auth local-password`` means the monolithic DASObjectStore appliance
  authenticates the remote user with the appliance's local user database or OS
  users and their passwords, then issues or rotates the S3 access key and secret
  key for that store. Include ``--username`` so the plan records the intended
  local user:

  .. code-block:: console

     dasobjectstore store s3-upload generated-data \
       --endpoint-url https://dos-appliance.example:3900 \
       --auth local-password \
       --username alice

Only the credential authority should reveal the generated S3 secret. Do not copy
Garage or provider-internal secret files to remote computers.

Verified application completion
--------------------------------

Application integrations finish a single-object provider upload through two
public HTTPS operations. ``POST
/api/v1/application-auth/upload-completions/capabilities`` exchanges the
paired EasyConnect session ID and renewal token for a one-time capability. The
request identifies the registered application, upload, ObjectStore, object
key, exact size and SHA-256 digest, bucket, and provider endpoint. Issuance
fails unless the session has a writable grant and the active application
identity permits ``complete_upload`` for that ObjectStore namespace and size.
This supports identities whose recorded ingress origin is ``remote_s3`` or a
named integration such as Synoptikon. Capabilities expire after at most 15 minutes and never contain provider
credentials.

After the provider PUT succeeds, ``POST
/api/v1/application-auth/upload-completions/complete`` submits only that
capability. The daemon requires an exact match to its durable issuance,
resolves the store-scoped Garage credential from daemon custody, independently
checks provider size and SHA-256 metadata, and atomically commits the provider
placement to the shared catalogue. A retry after a successful commit returns
``already_committed``. Provider verification or catalogue failure does not
consume the capability, and replay state is persisted only after the durable,
idempotent catalogue transaction so interruption remains safe to retry.

Clients must treat the renewal token and one-time capability as bearer
credentials, keep them out of logs, and never choose a provider endpoint that
differs from the endpoint advertised by the appliance.

Configure AWS CLI on the remote computer
----------------------------------------

After the credential authority has returned the S3 access key and secret key,
set them in the shell and run the profile commands printed by
``dasobjectstore store s3-upload``:

.. code-block:: console

   export DASOBJECTSTORE_S3_ACCESS_KEY_ID='access-key-from-authority'
   export DASOBJECTSTORE_S3_SECRET_ACCESS_KEY='secret-key-from-authority'

   aws configure set profile.dasobjectstore-generated-data.region garage
   aws configure set profile.dasobjectstore-generated-data.s3.addressing_style path
   aws configure set profile.dasobjectstore-generated-data.aws_access_key_id "$DASOBJECTSTORE_S3_ACCESS_KEY_ID"
   aws configure set profile.dasobjectstore-generated-data.aws_secret_access_key "$DASOBJECTSTORE_S3_SECRET_ACCESS_KEY"

Upload a single file with an explicit S3 PUT operation:

.. code-block:: console

   aws --profile dasobjectstore-generated-data \
     --endpoint-url http://192.168.1.192:3900 \
     s3api put-object \
     --bucket dos-generated-data \
     --key experiments/run-001/report.json \
     --body ./report.json

For ordinary file copies, ``aws s3 cp`` is usually more ergonomic:

.. code-block:: console

   aws --profile dasobjectstore-generated-data \
     --endpoint-url http://192.168.1.192:3900 \
     s3 cp ./report.json s3://dos-generated-data/experiments/run-001/report.json

For a directory tree:

.. code-block:: console

   aws --profile dasobjectstore-generated-data \
     --endpoint-url http://192.168.1.192:3900 \
     s3 sync ./run-001/ s3://dos-generated-data/experiments/run-001/

Operational notes
-----------------

Remote S3 upload bypasses the local ``dasobjectstore ingest files`` client path,
so operators should use it when object-service upload is the intended ingress
surface. Use unique object prefixes for independent upload runs. If the same
S3 key is written again, provider overwrite or versioning behaviour applies at
the S3 layer and must be reconciled with the store's metadata policy before the
data is treated as settled DASObjectStore-managed content.

When uploads are complete, inspect the provider or DASObjectStore service
health before handing the data to downstream workflows:

.. code-block:: console

   dasobjectstore service status --json
