Remote Client CLI
=================

``dasobjectstore-remote`` is the lightweight client for computers that are not
the DAS appliance. It talks to object stores through the appliance's
S3-compatible endpoint and uses the AWS CLI for object transfer operations.

The remote client is intended for workstations, sequencers, analysis servers,
and other hosts that need to list accessible object stores and upload files or
folders without mounting the DAS storage directly.

Requirements
------------

Install ``dasobjectstore-remote`` and the AWS CLI on the remote computer. The
remote client plans and invokes ``aws s3api`` and ``aws s3`` commands against
the configured DASObjectStore endpoint.

The remote computer must have one of the following credential paths:

* an AWS CLI profile containing S3 access key credentials authorized for the
  object stores;
* a configured DASObjectStore credential helper that obtains temporary S3
  credentials from Mneion, Synoptikon, or the standalone appliance's local
  password authority.

Passwords are never written to the terminal by ``dasobjectstore-remote``. When
``--prompt-password`` or a non-profile credential helper flow is used, the
password prompt disables terminal echo and passes the password only to the
credential helper through the ``DASOBJECTSTORE_REMOTE_PASSWORD`` environment
variable for that child process.

Configure a Remote Host
-----------------------

Configure the DASObjectStore S3 endpoint once on the remote computer:

.. code-block:: console

   dasobjectstore-remote config set \
     --endpoint-url http://192.168.1.192:3900 \
     --region garage \
     --profile dasobjectstore

The default config path is:

.. code-block:: text

   ~/.config/dasobjectstore/remote.json

Use ``--config <PATH>`` or ``DASOBJECTSTORE_REMOTE_CONFIG`` for a different
configuration file.

For a standalone appliance local-password flow, configure the username and the
credential helper supplied by the site:

.. code-block:: console

   dasobjectstore-remote config set \
     --endpoint-url https://dos-appliance.example:3900 \
     --auth local-password \
     --username alice \
     --credential-helper dasobjectstore-local-s3-credentials

For Mneion or Synoptikon managed sites, use the site-provided helper:

.. code-block:: console

   dasobjectstore-remote config set \
     --endpoint-url https://dos-appliance.example:3900 \
     --auth mneion \
     --credential-helper mneion-dasobjectstore-s3-credentials

List Accessible Object Stores
-----------------------------

List the object stores visible to the configured S3 credentials:

.. code-block:: console

   dasobjectstore-remote stores list

Emit machine-readable output:

.. code-block:: console

   dasobjectstore-remote stores list --json

Inspect the AWS command without running it:

.. code-block:: console

   dasobjectstore-remote stores list --dry-run

Upload Files and Folders
------------------------

Upload a single file to a prefix. The filename is preserved:

.. code-block:: console

   dasobjectstore-remote upload dos-generated-data \
     --source ./report.json \
     --prefix experiments/run-001

Upload a single file with an exact object key:

.. code-block:: console

   dasobjectstore-remote upload dos-generated-data \
     --source ./report.json \
     --key experiments/run-001/report.json

Upload a folder recursively:

.. code-block:: console

   dasobjectstore-remote upload dos-generated-data \
     --source ./run-001 \
     --prefix experiments/run-001

For folders, ``dasobjectstore-remote`` uses ``aws s3 sync``. For files, it uses
``aws s3 cp``. Use ``--dry-run`` before large transfers:

.. code-block:: console

   dasobjectstore-remote upload dos-generated-data \
     --source ./run-001 \
     --prefix experiments/run-001 \
     --dry-run

Credential Helper Contract
--------------------------

A credential helper is an executable command configured with
``--credential-helper``. DASObjectStore runs it with the following environment
variables:

* ``DASOBJECTSTORE_REMOTE_AUTHORITY``: ``local-password``, ``mneion``, or
  ``synoptikon``;
* ``DASOBJECTSTORE_REMOTE_ENDPOINT_URL``: the configured appliance endpoint;
* ``DASOBJECTSTORE_REMOTE_USERNAME``: the configured username when present;
* ``DASOBJECTSTORE_REMOTE_PASSWORD``: the password captured without terminal
  echo for this invocation.

The helper must print JSON to stdout:

.. code-block:: json

   {
     "access_key_id": "S3 access key",
     "secret_access_key": "S3 secret key",
     "session_token": "optional temporary session token"
   }

The remote client passes those credentials to the AWS CLI process through
standard AWS environment variables and does not write them to the config file.

Operational Notes
-----------------

``dasobjectstore-remote`` uploads through the object service surface. It does
not write into DAS member disks and does not use the local
``dasobjectstored`` Unix socket. Use object prefixes that make upload batches
easy to inspect and clean up if a transfer is interrupted or repeated.
