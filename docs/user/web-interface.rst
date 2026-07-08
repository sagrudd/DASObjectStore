Web Interface
=============

DASObjectStore has two network-facing surfaces that are easy to confuse:

* the standalone Web UI/API surface; and
* the S3-compatible object-service endpoint used for remote uploads.

Packaged Linux appliances enable the standalone Web UI/API service by default.
The packaged listener is HTTPS on port ``8448``:

.. code-block:: text

   https://<das-host>:8448

The packaged appliance configuration lives at:

.. code-block:: text

   /opt/dasobjectstore/config.json

The default packaged bind address is ``0.0.0.0`` so the Web UI is reachable from
other hosts on the appliance network. Local development without the package may
still use the compiled fallback of ``127.0.0.1``.

The server can also be started manually with explicit overrides:

.. code-block:: console

   dasobjectstore-server \
     --bind-address 0.0.0.0 \
     --https-port 8448 \
     --public-base-url https://<das-host>:8448

The S3-compatible upload endpoint is separate. Its default local endpoint is
``http://127.0.0.1:3900`` and it is not the Web UI.

Checking the Web Server
-----------------------

Use the top-level status command to inspect the daemon, Web UI, and object
service endpoints together:

.. code-block:: console

   dasobjectstore status
   dasobjectstore status --json

The managed storage daemon, ``dasobjectstored``, is separate from the standalone
Web UI service. Check the web service and listener explicitly when diagnosing
access issues:

.. code-block:: console

   systemctl status dasobjectstore-server
   ss -ltnp | grep ':8448'

The Debian and RPM packages install and enable these systemd units:

.. code-block:: console

   dasobjectstored.service
   dasobjectstore-server.service

Validate the resolved standalone server configuration without starting a long
running listener:

.. code-block:: console

   dasobjectstore-server --config /opt/dasobjectstore/config.json --check-config
   dasobjectstore-server --config /opt/dasobjectstore/config.json --check-config --json

Self-signed TLS assets may be generated for standalone bootstrap when both the
certificate and private key are missing:

.. code-block:: console

   sudo dasobjectstore-server \
     --config /opt/dasobjectstore/config.json \
     --check-config \
     --generate-missing-tls

Synoptikon-Integrated Mode
--------------------------

Synoptikon-integrated deployments must not expose ``8448`` as the public product
listener. In that mode, DASObjectStore is mounted behind Synoptikon's HTTPS
surface under:

.. code-block:: text

   /products/dasobjectstore
   /products/dasobjectstore/api
