Programmatic ObjectStore Access
===============================

This page describes the supported pattern for applications, analysis
pipelines, and workstation tools that access an S3-exported DASObjectStore.
The appliance remains the storage authority: clients use a scoped S3 grant and
never write to managed DAS filesystem roots.

Application identity exchange
-----------------------------

Unattended integrations use a daemon-registered application identity and an
active asymmetric public key. The application signs a bounded exchange request
with its private key; ``dasobjectstored`` verifies the proof against its
daemon-owned key registry, checks the ObjectStore/prefix/operation scope, and
returns a short-lived access-token claim. Bearer access tokens are not persisted
by the daemon and are never accepted as a substitute for the signed exchange
proof. Long-lived identity metadata is therefore distinct from short-lived
storage authority.

The standalone Web API also exposes the canonical proof-bearing route
``/api/v1/application-auth/access-token`` and forwards it to the same daemon
authority. The deployment listener must still provide the configured TLS/mTLS
boundary before exposing this route beyond a trusted local integration. Do not
place private keys, proofs, or access-token claims in manifests, logs, shell
history, or support tickets.

For local integrations, the CLI can submit a path-free request document to the
same daemon boundary. The request document contains public identity metadata or
an externally generated proof; it never contains a private key. Registration
and revocation still require the daemon's local administrator authorization and
the confirmation marker required by the typed request:

.. code-block:: console

   sudo dasobjectstore application-auth register-identity \
     --request ./identity-registration.json --json
   sudo dasobjectstore application-auth register-key \
     --request ./key-registration.json --json
   sudo dasobjectstore application-auth revoke \
     --request ./credential-revocation.json --json

An application signs the proof-free exchange payload using its private key and
submits the resulting request without exposing that key to DASObjectStore. The
same JSON request can be posted to the standalone HTTPS route when its listener
is configured for the deployment's TLS/mTLS policy:

.. code-block:: console

   dasobjectstore application-auth exchange \
     --request ./access-token-exchange.json --json

   curl --cert ./client.crt --key ./client.key \
     --data @./access-token-exchange.json \
     https://localhost:8443/api/v1/application-auth/access-token

The daemon verifies identity/key membership, proof, scope, and lifetime before
returning the typed access-token claims. The CLI does not sign requests, store
private keys, accept private/bearer secret fields, or bypass daemon
authorization. Treat the JSON response as credential material and keep it in a
trusted process boundary.

Profile readiness
-----------------

Before a product starts a storage workflow, inspect the daemon-owned readiness
projection rather than probing managed filesystem paths. It reports profile
root state, folder drift, and capacity admission state without exposing backend
locations or pretending that hardware-only health is available locally:

.. code-block:: console

   dasobjectstore store profile-readiness generated-data --json

The command is read-only. A missing or unreadable root, unmanaged/unsafe folder
entries, or unavailable/blocked capacity appears as an explicit not-ready
reason and must be resolved through the daemon's managed workflow.

The same projection is available to authenticated Web clients at
``/api/v1/profile-readiness/stores/{store_id}``; it uses the daemon bridge and
does not expose managed paths or credentials.

Products can discover the versioned profile catalogue through the authenticated
Web route ``/api/v1/profile-capabilities``. This is static capability
discovery only; use profile readiness and daemon provisioning workflows for
runtime decisions.

For a catalogue-authoritative content check, ask the daemon to verify the
recorded size and checksum against the profile payload. The CLI and
authenticated Web projection return only logical identity and verification
metadata; backend paths are never returned:

.. code-block:: console

   dasobjectstore store profile-verify generated-data reads/sample.fastq --json

Choose an ObjectStore and credential authority
-----------------------------------------------

Use the ObjectStore name and an appliance-issued grant. Do not infer bucket
names, reuse a provider administrator key, or copy the daemon's managed
credential registry. A store-specific grant is restricted by the appliance's
read/write policy and can be rotated without changing application code.

For direct workstation automation, authenticate one store without placing a
password in shell history or a credentials file:

.. code-block:: console

   dasobjectstore-remote authenticate 192.168.1.192 porkchop \
     --username stephen --ca-cert /etc/dasobjectstore/appliance-ca.pem \
     --tls-server-name localhost --json

The command prompts for the password with terminal echo disabled and sends it
only over verified HTTPS to the standalone appliance API. The response is a
store-scoped JSON connection context with an eight-hour session, Garage
endpoint/region/path-style settings, derived bucket, temporary S3 credentials,
expiry, and renewal metadata. Do not log or persist the JSON unless the
calling process has an explicit secret-storage policy. Read-only grants are
not issued by this command until Garage read-only credential provisioning is
available; this prevents a managed read/write key from being escalated.

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
