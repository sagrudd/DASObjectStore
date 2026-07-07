Managed Service Boundary
========================

DASObjectStore is a client/server storage appliance. Normal users should submit
jobs through ``dasobjectstore`` or the Web/API surface; they should not write
directly into managed DAS mountpoints.

Linux packages define a managed daemon named ``dasobjectstored``. The package
assets create a dedicated service identity:

.. code-block:: text

   user:  dasobjectstore
   group: dasobjectstore

The daemon owns runtime state, persistent state, logs, and managed storage
mutation. Local users are authorized by store writer/admin policy, such as
membership in a store's writer group, rather than by direct filesystem write
permission to HDD members.

Packaged Paths
--------------

The Linux package assets reserve these paths:

.. code-block:: text

   /etc/dasobjectstore/daemon.json
   /run/dasobjectstore/dasobjectstored.sock
   /var/lib/dasobjectstore
   /var/log/dasobjectstore
   /srv/dasobjectstore

The Unix-domain socket is the local client transport. The daemon will use peer
credentials on Linux to identify the submitting local actor before accepting
storage-mutating jobs.

Packaged installations restrict the socket directory and socket file to the
``dasobjectstore`` group. A local user must be in this transport group before
the CLI can connect to ``dasobjectstored``:

.. code-block:: console

   sudo usermod -aG dasobjectstore "$USER"

Start a new login session after changing group membership, then verify it with
``id -nG``. Store writer groups such as ``mnemosyne`` are still checked
separately by store policy after the client has connected to the daemon.

Permission Model
----------------

Managed DAS roots should be owned by the daemon service identity. Ingest users
should be members of the daemon transport group and the relevant store writer
group, for example ``mnemosyne``. The writer group authorizes daemon job
submission for that store. It should not be used to grant broad write access to
individual HDD filesystems.

The Debian package configuration checks the managed root at
``/srv/dasobjectstore``. If that path already exists and is owned by an ordinary
user or group, package configuration stops and asks the operator to repair the
ownership through the formal disk lockdown workflow before continuing.

The hidden ``--local-direct`` ingest mode is a developer/test fallback while the
daemon implementation is being completed. It is not the normal production
storage path.
