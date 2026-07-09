Object Stores
=============

Object stores are system-managed definitions. Users should not edit
``/etc/dasobjectstore/stores.json`` directly, and they should not need to pass a
stores-file path for normal operations.

Create a Store
--------------

Create a generated-data store. Store creation and policy changes are privileged
operations:

.. code-block:: console

   sudo dasobjectstore store create generated-data \
     --class generated_data \
     --writer-group mnemosyne

The command uses built-in defaults for the class. For ``generated_data`` this
currently means two verified HDD copies, SSD-first ingest, and acknowledgement
after HDD placement.

``--writer-group`` is the Unix group whose members may ingest files into the
store without root privileges. A store without a writer group is not writable by
normal users. On Linux, ``store create`` grants that group ACL access to the
known DAS SSD and managed HDD roots when they are present, so users can ingest
through DASObjectStore without direct ad hoc disk access.

Read access is separate from write access. Use ``--reader-group`` when a store
should be browsable or downloadable by users who should not ingest into it. Use
``--public`` only for stores that every authenticated DASObjectStore user may
browse and download through the daemon-backed API. Public read does not grant
direct filesystem access to DAS media and does not make anonymous HTTP access
available.

.. code-block:: console

   sudo dasobjectstore store create generated-data \
     --class generated_data \
     --writer-group mnemosyne \
     --reader-group mnemosyne-readers

   sudo dasobjectstore store create public-reference \
     --class reproducible_cache \
     --writer-group mnemosyne \
     --public

Override the copy count only when the policy is intentional:

.. code-block:: console

   sudo dasobjectstore store create generated-data \
     --class generated_data \
     --copies 3 \
     --writer-group mnemosyne

The S3 bucket identity is derived from the store name by default. The Web
console displays the derived bucket name as an immutable outcome during store
creation so operators do not have to choose a second object-service name.

Portable Registry
-----------------

``store create`` writes the host registry and also mirrors the definition to the
DAS SSD when a known SSD root is available. By default the SSD root is:

.. code-block:: text

   /srv/dasobjectstore/ssd

Set ``DASOBJECTSTORE_SSD_ROOT`` or pass ``--ssd-root`` when using a non-default
root:

.. code-block:: console

   DASOBJECTSTORE_SSD_ROOT=/mnt/das-ssd \
     sudo dasobjectstore store create generated-data \
       --class generated_data \
       --writer-group mnemosyne

   sudo dasobjectstore store create generated-data \
     --class generated_data \
     --writer-group mnemosyne \
     --ssd-root /mnt/das-ssd

The SSD root must contain ``.dasobjectstore/device.env`` with ``role=ssd``. This
marker is created by the managed disk preparation workflow.

List Stores
-----------

List host-managed stores:

.. code-block:: console

   dasobjectstore store list

List stores as JSON:

.. code-block:: console

   dasobjectstore store list --json

List the portable registry on the DAS SSD:

.. code-block:: console

   dasobjectstore store list --portable

Drain and Delete Stores
-----------------------

Store drain and delete are administrative operations. They delete payload files
from managed HDD roots and update live metadata so object, placement, and ingest
job references are removed. Use them when a store contains data that is no
longer required, especially reproducible public data that can be redownloaded if
needed.

Always inspect the plan first:

.. code-block:: console

   sudo dasobjectstore store drain generated-data --dry-run

To drain a store without deleting the store definition:

.. code-block:: console

   sudo dasobjectstore store drain generated-data \
     --allow-store-drain \
     --confirm "confirm store drain"

Drain removes the store's object rows, placement rows, ingest-job rows, and
known payload files. It leaves the store definition in the host and portable
registries, so the store can be reused after the contents are cleared.

The command resolves live metadata from the managed SSD root, using
``DASOBJECTSTORE_SSD_ROOT`` when set and otherwise
``/srv/dasobjectstore/ssd``. Operators should not need to pass a SQLite path for
normal managed stores.

To delete a store entirely:

.. code-block:: console

   sudo dasobjectstore store delete generated-data \
     --live-sqlite-path /srv/dasobjectstore/ssd/.dasobjectstore/live.sqlite \
     --allow-store-delete \
     --confirm "confirm store delete"

Delete performs the same content cleanup as drain, removes the store row from
live metadata, removes the host registry entry, removes any SubObjects rooted in
the store, and removes portable registry entries when a known DAS SSD root is
available. Pass ``--ssd-root`` when the SSD is mounted somewhere other than the
default path.

Both commands use the default managed HDD root unless ``--hdd-root`` is passed.
They refuse non-dry-run execution unless run by an administrative user and the
matching policy allowance plus confirmation phrase are provided.

Adopt Stores on a New Host
--------------------------

When moving the DAS to another host, adopt the portable SSD registry into that
host's system registry:

.. code-block:: console

   dasobjectstore store adopt

Use ``--ssd-root`` if the SSD is mounted somewhere other than
``/srv/dasobjectstore/ssd``:

.. code-block:: console

   dasobjectstore store adopt --ssd-root /mnt/das-ssd

Inspect Policy Defaults
-----------------------

Before creating a store, inspect the class defaults:

.. code-block:: console

   dasobjectstore store defaults --class generated_data

See :doc:`store-classes` for class meanings and redundancy defaults.
