Programmatic ObjectStore Access
===============================

This page describes the supported pattern for applications, analysis
pipelines, and workstation tools that access an S3-exported DASObjectStore.
The appliance remains the storage authority: clients use a scoped S3 grant and
never write to managed DAS filesystem roots.

Choose an ObjectStore and credential authority
-----------------------------------------------

Use the ObjectStore name and an appliance-issued grant. Do not infer bucket
names, reuse a provider administrator key, or copy the daemon's managed
credential registry. A store-specific grant is restricted by the appliance's
read/write policy and can be rotated without changing application code.

For a configured AWS CLI profile, the remote client can list stores or render
an upload plan without exposing secret values:

.. code-block:: console

   dasobjectstore-remote config set \
     --endpoint-url http://192.168.1.192:3900 \
     --region garage \
     --profile dasobjectstore-epic_collection
   dasobjectstore-remote stores list

For Mneion, Synoptikon, or standalone local-password deployments, use the
site-provided credential helper. The helper is an explicit process boundary;
it receives the authority, endpoint, username, and (only when prompted)
password through environment variables and emits one JSON object on stdout:

.. code-block:: json

   {"access_key_id":"...","secret_access_key":"...","session_token":"..."}

``access_key_id`` and ``secret_access_key`` are required; ``session_token`` is
optional. The remote client validates this shape, rejects blank values, and
does not print the helper output. A helper must never write credentials to a
log, shell history, manifest, or temporary file.

Configure a helper without embedding credentials in application arguments:

.. code-block:: console

   dasobjectstore-remote config set \
     --endpoint-url https://dos-appliance.example:3900 \
     --auth mneion \
     --credential-helper /usr/local/bin/site-das-s3-credentials

The helper command is site-owned. Keep its executable path and non-secret
configuration in the remote client config; keep password prompts and secret
material inside the helper boundary.

Provision the ObjectStore before requesting remote access
----------------------------------------------------------

After creating or updating an ObjectStore registry entry, provision the
configured object-service provider from that registry. Inspect the plan before
applying it:

.. code-block:: console

   sudo dasobjectstore service provision --provider garage --dry-run
   sudo dasobjectstore service provision --provider garage

Provisioning creates the derived bucket and the store-scoped credential
managed by DASObjectStore. It must be repeated after adding or changing stores;
do not create buckets manually or use the Garage provider administrator key.

Render the remote upload plan using the ObjectStore identifier, not a guessed
bucket name:

.. code-block:: console

   sudo dasobjectstore store s3-upload alleleanchor_mvp \
     --endpoint-url http://192.168.1.192:3900 \
     --auth mneion \
     --json

The plan identifies the derived bucket, AWS profile, credential reference, and
safe command shape. The credential authority named by ``--auth`` supplies the
actual short-lived grant: use ``mneion`` or ``synoptikon`` site integration
where available, or ``local-password`` with ``--username`` for standalone
appliance authentication. Treat the plan as configuration metadata; never
copy secret values into source files, shell history, or chat.

Standalone password exchange
-----------------------------

When the appliance is running in standalone local-auth mode, the remote client
can exchange the same local account used by the Web console for a temporary
ObjectStore-scoped context:

.. code-block:: console

   dasobjectstore-remote authenticate 192.168.1.192 alleleanchor_mvp \
     --username stephen

The command prompts for the password without echoing it and prints a redacted
summary by default. Use ``--json`` only when a trusted process must consume
the temporary context; redirect it to a mode-0600 local file and never paste
the output into a terminal transcript, issue, or chat. A browser login alone
does not copy its session cookie into the remote client.

S3 client settings
------------------

Every S3 client must use the endpoint, the ``garage`` signing region, and
path-style addressing. Virtual-host addressing commonly fails when the
appliance exposes one shared non-TLS endpoint:

.. code-block:: toml

   endpoint_url = "http://192.168.1.192:3900"
   region = "garage"
   addressing_style = "path"

For AWS CLI and SDKs, set the endpoint explicitly rather than relying on a
global default:

.. code-block:: console

   aws --profile dasobjectstore-epic_collection \
     --endpoint-url http://192.168.1.192:3900 \
     s3api head-bucket --bucket dos-epic-collection

The command above is a reachability and authorization check; it does not
upload data. Use the remote client's upload command for normal file/folder
transfers, or use an SDK only when the application needs streaming, custom
retry, or object metadata control.

Safe upload contract
--------------------

Applications should follow the same settlement pattern as
``epic_collection``:

* stage downloads locally in a controlled directory;
* stream or copy the payload with bounded concurrency;
* calculate SHA-256 and record byte count before upload;
* upload the payload and its JSON provenance/checksum sidecars under one
  deterministic, ObjectStore-scoped key prefix;
* treat a payload plus valid manifest as the restart marker, so retries skip
  already verified objects; and
* inspect the DASObjectStore job or service status before treating an upload as
  settled and available to downstream workflows.

Use stable keys such as
``<project>/<assembly>/<sample-or-fixture>/<artifact>/<file>``. Never place
passwords, S3 secrets, signed URLs, raw credential-helper output, or protected
read/profile data in object keys or manifests. For human genomic data, keep
the key and manifest free of sample-identifying details unless the governing
data-use policy explicitly permits them.

Python SDK shape
----------------

The following schematic uses credentials supplied by the process environment;
it intentionally does not show how credentials are obtained. Prefer a
short-lived helper/session and pass values directly to the SDK rather than
writing a credentials file:

.. code-block:: python

   import os
   import boto3
   from botocore.config import Config

   s3 = boto3.client(
       "s3",
       endpoint_url="http://192.168.1.192:3900",
       region_name="garage",
       aws_access_key_id=os.environ["AWS_ACCESS_KEY_ID"],
       aws_secret_access_key=os.environ["AWS_SECRET_ACCESS_KEY"],
       aws_session_token=os.environ.get("AWS_SESSION_TOKEN"),
       config=Config(s3={"addressing_style": "path"}),
   )
   s3.upload_file("artifact.bin", "dos-epic-collection", "project/artifact.bin")

Validate the target ObjectStore grant and upload a checksum/provenance
manifest alongside the payload. Do not log the client object or exception
strings if they may contain signed request details.

Failure handling and verification
----------------------------------

Handle ``403`` as an authorization or grant problem, not as permission to
retry indefinitely. Check the endpoint, region, path-style setting, ObjectStore
grant, and credential expiry. Retry transient transport/5xx failures with
bounded exponential backoff and an idempotent key; never retry a non-idempotent
operation without a client token or equivalent deduplication rule.

After upload, verify with a metadata-only ``head-object`` or SDK equivalent,
then let the appliance daemon perform its normal SSD-first landing,
verification, and HDD settlement. A successful S3 PUT means bytes reached the
object-service ingress; it is not by itself proof that durable DAS settlement
is complete.

Related pages
-------------

* :doc:`remote-client` — configure and use ``dasobjectstore-remote``.
* :doc:`remote-s3-uploads` — operator upload plans and ObjectStore grants.
* :doc:`service-boundary` — daemon ownership and storage mutation boundaries.
