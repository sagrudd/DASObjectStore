Preparing DAS Disks
===================

Disk preparation is destructive. It must be performed through
``dasobjectstore disk prepare-das`` rather than ad hoc shell commands so that the
software records roles, creates the expected layout, and can later detect the
DAS on another host.

Inspect First
-------------

Start with a probe:

.. code-block:: console

   dasobjectstore probe --pretty

On Linux, prefer stable ``/dev/disk/by-id/...`` paths for preparation. Avoid
volatile names such as ``/dev/sdb`` in documentation, scripts, and runbooks.

Dry Run
-------

Build the command with the SSD and every HDD member:

.. code-block:: console

   sudo dasobjectstore disk prepare-das \
     --ssd-device /dev/disk/by-id/<ssd-device> \
     --hdd-device hdd-a=/dev/disk/by-id/<hdd-a-device> \
     --hdd-device hdd-b=/dev/disk/by-id/<hdd-b-device> \
     --mount-root /srv/dasobjectstore \
     --filesystem ext4 \
     --owner dasobjectstore \
     --dry-run

Review the plan. The dry run should show the managed operations without changing
the devices.

Apply Preparation
-----------------

Only run the destructive command once the device mapping is correct:

.. code-block:: console

   sudo dasobjectstore disk prepare-das \
     --ssd-device /dev/disk/by-id/<ssd-device> \
     --hdd-device hdd-a=/dev/disk/by-id/<hdd-a-device> \
     --hdd-device hdd-b=/dev/disk/by-id/<hdd-b-device> \
     --mount-root /srv/dasobjectstore \
     --filesystem ext4 \
     --owner dasobjectstore \
     --allow-format \
     --confirm "confirm prepare das"

The default mount root is ``/srv/dasobjectstore``. The SSD root is normally
``/srv/dasobjectstore/ssd`` and HDD members are mounted under the same managed
tree.

Lock Down Managed Media
-----------------------

After preparation, lock down the managed roots so ordinary users do not write
files directly onto member disks:

.. code-block:: console

   sudo dasobjectstore disk lockdown-das \
     --mount-root /srv/dasobjectstore \
     --service-user dasobjectstore \
     --service-group dasobjectstore \
     --create-service-user \
     --confirm "confirm lockdown das"

Direct writes to individual disks bypass object metadata and can corrupt the
store contract. Users should interact through DASObjectStore commands and, later,
the object service or Web UI.

