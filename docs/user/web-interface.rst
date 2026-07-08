Web Interface
=============

DASObjectStore has two network-facing surfaces that are easy to confuse:

* the standalone Web UI/API surface; and
* the S3-compatible object-service endpoint used for remote uploads.

The standalone Web UI/API default is HTTPS on port ``8448``:

.. code-block:: text

   https://127.0.0.1:8448

The default bind address is loopback-only. On a Linux appliance, the server may
be exposed on the host network only when the operator explicitly starts it with
an external bind address:

.. code-block:: console

   dasobjectstore-server \
     --bind-address 0.0.0.0 \
     --https-port 8448 \
     --public-base-url https://<das-host>:8448

The S3-compatible upload endpoint is separate. Its default local endpoint is
``http://127.0.0.1:3900`` and it is not the Web UI.

Checking the Web Server
-----------------------

The managed storage daemon, ``dasobjectstored``, does not by itself imply that
the standalone Web UI is listening. Check the listener explicitly:

.. code-block:: console

   ss -ltnp | grep ':8448'

Validate the resolved standalone server configuration without starting a long
running listener:

.. code-block:: console

   dasobjectstore-server --check-config
   dasobjectstore-server --check-config --json

Self-signed TLS assets may be generated for standalone bootstrap when both the
certificate and private key are missing:

.. code-block:: console

   sudo dasobjectstore-server --check-config --generate-missing-tls

Synoptikon-Integrated Mode
--------------------------

Synoptikon-integrated deployments must not expose ``8448`` as the public product
listener. In that mode, DASObjectStore is mounted behind Synoptikon's HTTPS
surface under:

.. code-block:: text

   /products/dasobjectstore
   /products/dasobjectstore/api
