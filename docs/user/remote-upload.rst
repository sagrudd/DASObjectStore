Remote Upload
=============

Remote upload is the browser-approved path for moving data from a workstation,
sequencer, or analysis host into a DASObjectStore appliance without mounting
managed DAS storage. The appliance remains the storage authority: remote
clients authenticate, receive a scoped session, and submit uploads through the
daemon/object-service boundary instead of writing directly to DAS roots.

Use this page for the interactive easyconnect and Web console workflow. Use
:doc:`remote-client` for the command-line client contract and
:doc:`remote-s3-uploads` for raw AWS CLI upload plans.

Setup
-----

Install ``dasobjectstore-remote`` on the computer that holds the source files.
The remote client must be able to reach:

* the standalone DASObjectStore Web application, normally
  ``https://<appliance>:8448``; and
* the S3-compatible object-service endpoint used for transfer, normally a
  hostname-valid appliance address such as
  ``https://objects.appliance.example:3900``.

Start the canonical login from the remote computer:

.. code-block:: console

   dasobjectstore-remote login 192.168.1.192 OBJECTSTORE \
     --username USER --set-s3-config

Canonical login discovers and completes through the Monas authority boundary
at ``https://HOST:8443``. Port 8448 is not opened by an integrated installation.
Only a deliberately standalone legacy host may select
``--authority-profile legacy-standalone``; that explicit compatibility profile
uses port 8448 and does not weaken the integrated default.

The ObjectStore and Pistis username are fixed before approval. EasyConnect
requires an already enrolled appliance certificate pin. The lower-level
``easyconnect`` command remains available for contract diagnostics; the old
``authenticate`` command is retained only to reject retired local-password
invocations and is not an authority fallback.

The client starts a loopback callback listener, opens the appliance login page
in a browser, and waits for authenticated approval. Use ``--no-browser`` on a
headless remote host; the command prints the browser URL and still waits for
approval. Use ``--contract`` to inspect the easyconnect contract without
starting a pairing.

Browser Authentication
----------------------

Monas projects the approved Pistis actor into DASObjectStore. DASObjectStore
does not accept PAM, a local password, browser-local identity, or operating
system identity as human authority. The daemon filters the issued remote
session to the ObjectStores that actor may read or write. Public-read access is
not enough for upload; the actor must have a Monas-projected writer role.

After approval, the remote client performs a one-time exchange and atomically
stores the issued session, temporary S3 credentials, expiry time, renewal
metadata, accessible ObjectStore grants, and the server-owned public S3
connection descriptor. It does not infer the S3 endpoint from the appliance
address. Config generations and their compatibility mirror use owner-only
permissions on Unix systems. Do not paste credentials into support tickets or
shell history.

ObjectStore Selection
---------------------

Remote Upload is not a global primary workspace. Enter it from an explicit
ObjectStore target (the Web workspace request carries ``store_id``); the
browser displays that ObjectStore name rather than internal S3 bucket names.
The server filters the response to the requested target and rejects a missing,
read-only, locked, non-S3, missing-writer-group, or unauthorized target. The
browser does not render a file dropzone until the target is present, and it
never silently selects the first writable store. The daemon derives the bucket
from the paired session grant. Before file selection, the target pane repeats
the display name, writer group, object type, used/free capacity, and the
paired-agent ingress origin and landing mode. This makes the storage and
placement contract visible at the point where the user authorizes a handoff;
the browser still cannot choose disks or bypass daemon admission.

The same rule applies to ``dasobjectstore-remote upload``:

.. code-block:: console

   dasobjectstore-remote upload zymo_fecal_2025.05 \
     --source ./run-001 \
     --prefix experiments/run-001

Use the ObjectStore name as the first argument. If an internal bucket name is
provided while a paired appliance session is active, the client rejects it and
asks for a writable ObjectStore name.

Drag-And-Drop Uploads
---------------------

The Web console remote-upload surface accepts files or folders. Dropping a
folder expands it in the browser into a relative file list and byte counts. The
browser handoff sends only:

* the paired session id;
* the target ObjectStore and derived bucket;
* relative display paths;
* file byte counts and total bytes; and
* a browser handoff id.

Absolute local source paths stay on the remote computer. The local
``dasobjectstore-remote`` agent receives the final transfer authority through
its loopback endpoint, not through hidden browser access to local files.

Before transfer begins, the local agent requires explicit confirmation. The
confirmation phrase names the target ObjectStore, for example:

.. code-block:: text

   confirm upload to zymo_fecal_2025.05

Remote uploads are classified as ``remote_s3`` ingress. The appliance stages
incoming bytes to the managed SSD path first, then moves them through
daemon-owned HDD settlement and verification. Users do not choose disks.

Remote production hosts need only the S3 endpoint and authenticated HTTPS on
the appliance. They must not be provisioned with SSH, sudo, daemon-socket, or
appliance filesystem access merely to inspect or finalize an upload. After the
payload and its manifest/checksum sidecars are visible through S3, use
``dasobjectstore-remote objects reconcile-s3`` and
``dasobjectstore-remote operations wait`` as documented in
:doc:`remote-client`.

Operator Throughput and Backpressure
------------------------------------

Remote upload speed is controlled by three separate parts of the appliance:
S3 intake to SSD, HDD landing, and verification. A slow remote upload is not
always a network problem.

The daemon limits remote S3 intake before it accepts more work. By default, the
remote-upload contract allows two active remote S3 transfers, two multipart
parts per transfer, an SSD staging queue depth of four, an HDD landing queue
depth of eight, and a verification queue depth of four. When SSD pressure is
``High``, new remote transfers may be paused while destage catches up. When
SSD pressure is ``Critical``, new remote transfers should be rejected until the
daemon reports that SSD capacity has recovered.

HDD settlement uses the same appliance landing rule as local ingest. The
default worker count is ``max(number_of_hdds_in_enclosure - 2, 2)``, capped by
the number of eligible HDDs. One-HDD degraded or test systems therefore use one
worker, two to four HDDs use two workers, five HDDs use three workers, and an
eight-HDD DAS uses six workers. The daemon also enforces one active writer per
physical HDD and never places redundant copies of the same object on the same
disk. This keeps disks from thrashing, but it also means that a store needing
multiple verified copies can be limited by the number of idle eligible HDDs.

To diagnose a slow remote upload, inspect the remote upload job in the Web
``Activity`` view or the daemon job status path before restarting clients. The
useful fields are:

``S3 transfer rate``
   Low rate with empty SSD/HDD queues usually points to the remote computer,
   network path, AWS CLI process, or object-service endpoint reachability.

``SSD queue depth`` and ``SSD pressure``
   Non-zero queue depth or ``High``/``Critical`` pressure means intake is
   waiting for SSD space or staging workers. Let HDD landing and verification
   catch up before starting more remote uploads.

``HDD landing queue depth`` and ``active per-HDD writers``
   A non-zero landing queue with active writers at the default limit means the
   appliance is using all safe HDD write slots. Adding remote clients will not
   make this faster; it will increase waiting work.

``Verification state``
   Pending verification after S3 transfer completion means bytes have arrived
   but the object is not protected yet. Do not treat the upload as settled until
   the daemon reports the job complete.

``Session renewal status``
   Missing or unavailable renewal metadata can interrupt long uploads when the
   easyconnect session expires. Keep ``dasobjectstore-remote`` running during
   long transfers and rerun easyconnect when renewal is not available.

If the Web Activity view reports ``waiting`` rather than ``running``, follow
the retry hint instead of killing the job. Waiting usually means the daemon is
protecting SSD capacity, S3 intake concurrency, HDD writer slots, or
verification capacity.

Session Renewal
---------------

Remote upload sessions default to eight hours. For the default session length,
renewal becomes available one hour before expiry. Shorter operator-limited
sessions become renewable halfway through their lifetime. Renewal uses a
daemon-issued renewal token and rotates that token after a successful renewal;
the remote client does not need to retain the browser login password after
pairing completes.

Long uploads should keep ``dasobjectstore-remote`` running so the client can
report renewal status and refresh the session when the daemon allows it. If a
session expires before transfer starts, the client rejects the upload before
using stored credentials and asks the user to run easyconnect again.

Cancellation
------------

Cancelling before the confirmation phrase is accepted stops the handoff before
transfer authority or object-service credentials are given to the local agent.
The browser records the cancellation as a pre-transfer state and the user can
start a new handoff.

Cancelling or interrupting an active transfer is daemon-owned cleanup. The
cleanup plan can remove partial SSD-stage payloads, abort incomplete S3
multipart uploads, and clear abandoned session, pairing, or browser-handoff
state. Required cleanup is reported separately from resumable session cleanup
so operators can see whether retry is safe.

Recovery
--------

If the browser reports ``agent_unreachable``, restart or foreground
``dasobjectstore-remote`` on the remote computer and retry the handoff. This
state is retryable and does not mean that bytes were accepted by the
appliance.

If the upload fails after bytes have started transferring, inspect the daemon
job status in the Web Activity view or with the daemon job commands. Remote
upload jobs use the common ``remote_upload`` job kind and report running,
progress, failed, waiting, or complete states with byte counters, transfer
stage, queue/backpressure details, and cleanup results where available.

When recovery requires a new session, run:

.. code-block:: console

   dasobjectstore-remote easyconnect 192.168.1.192

Then repeat ObjectStore selection and upload. Do not reuse old browser handoff
URLs or stale temporary S3 credentials.
