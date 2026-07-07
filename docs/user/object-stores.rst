Object Stores
=============

Object stores are system-managed definitions. Users should not edit
``/etc/dasobjectstore/stores.json`` directly, and they should not need to pass a
stores-file path for normal operations.

Create a Store
--------------

Create a generated-data store:

.. code-block:: console

   dasobjectstore store create generated-data --class generated_data

The command uses built-in defaults for the class. For ``generated_data`` this
currently means two verified HDD copies, SSD-first ingest, and acknowledgement
after HDD placement.

Override the copy count only when the policy is intentional:

.. code-block:: console

   dasobjectstore store create generated-data \
     --class generated_data \
     --copies 3

Create with an explicit S3 bucket name:

.. code-block:: console

   dasobjectstore store create generated-data \
     --class generated_data \
     --bucket generated-data

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
     dasobjectstore store create generated-data --class generated_data

   dasobjectstore store create generated-data \
     --class generated_data \
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

