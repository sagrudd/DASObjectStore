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
     --acknowledge-existing-data \
     --confirm "confirm prepare das"

The default mount root is ``/srv/dasobjectstore``. The SSD root is normally
``/srv/dasobjectstore/ssd`` and HDD members are mounted under the same managed
tree.

First Install
-------------

On a new installation only, the packaged daemon has a separate bootstrap
operation because its normal start is correctly held behind the complete
storage-ready check. It accepts one blank SSD-role device and one blank HDD-role
device, each at least 1 GiB. It rejects existing signatures, partitions,
mountpoints, non-template storage manifests, and occupied managed mount roots.
The operation still destroys the selected devices, so use it only for newly
allocated blank media and supply both explicit confirmations:

.. code-block:: console

   sudo dasobjectstored bootstrap-storage \
     --ssd-device /dev/disk/by-id/<new-ssd-device> \
     --hdd-device /dev/disk/by-id/<new-hdd-device> \
     --allow-format \
     --acknowledge-blank-devices \
     --confirm "confirm bootstrap das"

This one-shot daemon mode prepares the enclosure, initializes the SSD pool
metadata, writes UUID-based persistent mounts and the managed-storage manifest,
then starts the storage-ready verifier and normal daemon. A root-owned journal
binds an interrupted operation to the original device paths, device numbers,
sizes and sysfs identities. Resume only that operation with the same device
paths and confirmations plus ``--resume`` and
``--confirm-resume-reformat "confirm resume reformat das"``. This separate
confirmation explicitly permits reformatting those same journaled devices
after an interrupted preparation. If the identity changed or the manifest is
no longer the first-install template, the command refuses to format. A
completed journal can be resumed without formatting. It cannot be used to
replace or reconfigure an existing enclosure. For development VM
surrogates, these devices remain virtual and must not be described as physical
disks.

``--allow-format`` permits the destructive preparation policy. The separate
``--acknowledge-existing-data`` flag records that the operator understands the
selected media may already contain data that will be destroyed by preparation.

Lock Down Managed Media
-----------------------

After preparation, lock down the managed roots through the Monas DASObjectStore
page so ordinary users do not write files directly onto member disks. Monas
requires a Pistis-authenticated approval and forwards the versioned verified
subject through its fixed DAS GUI/API service peer. The direct
``dasobjectstore disk lockdown-das`` CLI command is intentionally rejected,
including dry runs; root, sudo, and local group membership are not a substitute
for a human Pistis approval.

Direct writes to individual disks bypass object metadata and can corrupt the
store contract. Users should interact through DASObjectStore commands and, later,
the object service or Web UI.
