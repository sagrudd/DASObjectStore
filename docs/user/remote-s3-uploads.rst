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
