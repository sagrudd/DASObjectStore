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

Package Builds
--------------

``make deb`` and ``make rpm`` must include the full Trunk-built WebAssembly
operator interface. A package build should fail if the Trunk toolchain or
``wasm32-unknown-unknown`` Rust target is missing; it must not silently install
the developer placeholder page. Prepare a packaging host with:

.. code-block:: console

   sudo apt-get install dpkg clang libclang-dev libpam0g-dev
   rustup target add wasm32-unknown-unknown
   cargo install trunk

On AlmaLinux or RHEL package builders, install the native build tools with:

.. code-block:: console

   sudo dnf install rpm-build clang libclang-devel pam-devel

If the Web page says to install the Trunk WebAssembly toolchain, the installed
package contains the developer fallback page and should be rebuilt from a
toolchain-complete checkout.

The packaged standalone configuration also declares the authentication
authority. The DAS appliance default is local user authentication:

.. code-block:: json

   {
     "authentication": {
       "authority": "local_user",
       "session_ttl_seconds": 3600
     }
   }

``local_user`` enables the standalone login, session validation, and logout
routes under ``/products/dasobjectstore/api``. ``synoptikon`` and ``monas`` are
external authority modes; those deployments should mount DASObjectStore behind
the host product surface so account, entitlement, audit, and correlation context
come from that host.

Standalone login verifies the supplied username and password against the
appliance OS through PAM using the packaged ``dasobjectstore`` PAM service. The
product-local file under ``/opt/dasobjectstore/users.json`` stores only
DASObjectStore browser session tokens; users do not need to be pre-created in
that file before logging in. OS-local sudo status and daemon policy remain the
authority for administrative storage mutation.

Packaged appliances keep the Web service unprivileged and perform the PAM check
through ``/usr/libexec/dasobjectstore/dasobjectstore-local-auth-helper``. The
helper must be owned by ``root:dasobjectstore`` with mode ``4750`` so
``pam_unix`` can verify local OS passwords without running the whole Web server
as root. The packaged ``dasobjectstore-server.service`` therefore sets
``NoNewPrivileges=false``; otherwise Linux would block the helper's setuid
transition and PAM would report valid local users as failed logins.

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

The JSON output includes ``auth_host_mode`` so operators can confirm whether the
server is exposing local standalone auth routes or expecting an integrated host
authority.

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
