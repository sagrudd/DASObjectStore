NAS and NFS Endpoints
=====================

DASObjectStore can model an external NAS/NFS export as a managed endpoint for
Mneion/Synoptikon integration. This keeps tenant-facing access object-style:
products should consume object endpoints and credential references, not raw NFS
paths.

Current Scope
-------------

The current CLI validates NAS/NFS endpoint definition files and exports a
DASObjectStore-managed endpoint shape. It does not yet perform full NAS
lifecycle management or permanent mounting.

Endpoint Definition
-------------------

Create a JSON file such as ``nas-endpoint.json``:

.. code-block:: json

   {
     "schema_version": "dasobjectstore.nas_nfs_endpoint.v1",
     "identifier": "ad255a8f-0058-4790-a640-758c573f2db1",
     "display_name": "Shared NAS",
     "nfs_server": "nas-01.local",
     "nfs_export_path": "/exports/bioinformatics",
     "object_service_endpoint": "https://nas-gateway.local:3900",
     "credential_reference": "secret://dasobjectstore/nas/shared",
     "tls_ca_reference": "secret://dasobjectstore/ca/nas",
     "tls_server_name": "nas-gateway.local",
     "status": "pending_validation"
   }

Validate it:

.. code-block:: console

   dasobjectstore mnemosyne validate-nas-nfs-endpoint \
     --definition-file nas-endpoint.json

Use JSON output when another tool will consume the result:

.. code-block:: console

   dasobjectstore mnemosyne validate-nas-nfs-endpoint \
     --definition-file nas-endpoint.json \
     --json

Validation Rules
----------------

The validator checks that:

* ``schema_version`` is ``dasobjectstore.nas_nfs_endpoint.v1``.
* ``identifier`` is UUID-like.
* ``display_name`` is not blank.
* ``nfs_server`` is a host name or address, not a path.
* ``nfs_export_path`` is absolute and does not contain parent traversal.
* ``object_service_endpoint`` starts with ``http://`` or ``https://``.
* credential and CA references are reference URIs, not inline secrets.
* ``status`` is not ``rejected``.

The pretty output deliberately avoids exposing the raw NFS export path as a
tenant-facing contract.

