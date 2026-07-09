Remote Client CLI
=================

``dasobjectstore-remote`` is the lightweight client for computers that are not
the DAS appliance. It talks to object stores through the appliance's
S3-compatible endpoint and uses the AWS CLI for object transfer operations.

The remote client is intended for workstations, sequencers, analysis servers,
and other hosts that need to list accessible object stores and upload files or
folders without mounting the DAS storage directly.

Requirements
------------

Install ``dasobjectstore-remote`` and the AWS CLI on the remote computer. The
remote client plans and invokes ``aws s3api`` and ``aws s3`` commands against
the configured DASObjectStore endpoint.

Build the remote-only packages from a source checkout when distributing the
client to upload-only hosts:

.. code-block:: console

   make remote-deb
   make remote-rpm

These targets produce packages named ``dasobjectstore-remote`` and install only
the remote client binary and its documentation. They do not install
``dasobjectstored``, systemd service units, local appliance configuration, or
managed storage directories.

The remote computer must have one of the following credential paths:

* an AWS CLI profile containing S3 access key credentials authorized for the
  object stores;
* a configured DASObjectStore credential helper that obtains temporary S3
  credentials from Mneion, Synoptikon, or the standalone appliance's local
  password authority.

Passwords are never written to the terminal by ``dasobjectstore-remote``. When
``--prompt-password`` or a non-profile credential helper flow is used, the
password prompt disables terminal echo and passes the password only to the
credential helper through the ``DASOBJECTSTORE_REMOTE_PASSWORD`` environment
variable for that child process.

Easyconnect Contract
--------------------

``easyconnect`` is the planned browser-approved connection flow for users who
know the appliance host or IP address but should not paste passwords, S3 access
keys, or bucket names into the terminal. The command binds a loopback callback
listener, opens the appliance login page in a browser, and waits for a one-time
pairing callback:

.. code-block:: console

   dasobjectstore-remote easyconnect 192.168.1.192

The command resolves the standalone Web application URL using HTTPS port
``8448`` by default. After authenticated approval in the browser, the remote
client receives a one-time pairing result on its loopback callback listener. The
exchange code is treated as secret-bearing material and is not printed.

Server-side session exchange is the next implementation stage. Until that API
is available, successful output states that the browser-approved callback was
received and that session exchange is not yet implemented in the build.

The server-side easyconnect contract is defined as stable daemon/API DTOs for
the following operations:

* discovery of appliance pairing capabilities;
* pairing challenge creation for a loopback callback URL;
* browser-authenticated pairing approval;
* exchange of the one-time pairing code for a remote upload session;
* explicit session revocation; and
* renewal of an active session during long uploads.

Standalone appliances advertise ``standalone_local_user`` as the active
easyconnect authentication provider. The browser approval path uses the same
local-user Web session as the rest of the standalone console: the user logs in
with their appliance OS/PAM credentials, and protected easyconnect approval
routes resolve the authenticated local subject from the browser session token.
The API shape also reserves ``synoptikon`` and ``mneion`` providers for later
integrated-host deployments, but those providers are not active in standalone
mode.

Session exchange responses carry temporary S3 credentials, accessible
ObjectStore grants, expiry time, and renewal metadata. Those credentials are
intended for the paired ``dasobjectstore-remote`` process only and must not be
pasted into terminal commands or support tickets.

The accessible ObjectStore list is filtered by the daemon before a remote
session is issued. A remote user can only see ObjectStores that the same
authenticated local account may read through public-read, reader-group,
writer-group, or configured administrator-group policy. Upload permission is
stricter: the store must be writable and the account must satisfy the daemon
writer authorization policy, usually by membership in the ObjectStore writer
group. Public-read access never grants upload rights by itself.

Remote upload sessions default to eight hours. The appliance advertises that
default in discovery and the remote client treats renewal as an explicit
session operation rather than a password replay. For the default eight-hour
session, renewal becomes eligible one hour before expiry. Shorter test or
operator-limited sessions become renewable halfway through their lifetime so a
long upload can refresh credentials before interruption. Renewal uses a
daemon-issued renewal token, rotates that token after a successful renewal, and
does not require ``dasobjectstore-remote`` to keep the login password in memory
after the browser-approved pairing has completed.

Use ``--contract`` to inspect the readable product contract without launching a
browser, or ``--json`` when another tool should consume the contract:

.. code-block:: console

   dasobjectstore-remote easyconnect 192.168.1.192 --contract
   dasobjectstore-remote easyconnect 192.168.1.192 --json

Use ``--https-port`` only when a standalone appliance is intentionally deployed
on a non-default Web port. Use ``--callback-port`` when firewall policy or a
launcher requires a fixed loopback callback port; otherwise the client chooses
an ephemeral loopback port. Use ``--timeout-seconds`` to change the pairing wait
time. Use ``--no-browser`` on headless systems: the client prints the browser
URL and still waits for the callback.

The easyconnect lifecycle is:

* discover appliance pairing capabilities from the HTTPS Web API;
* start a local loopback callback listener, or use polling fallback when
  callback binding is unavailable;
* open the appliance browser login and pairing approval page;
* wait for authenticated approval without printing passwords or S3 credentials;
* exchange the approved pairing for a remote upload session and accessible
  ObjectStore list; and
* persist only non-secret appliance metadata and issued session references.

Expected failure states include unreachable discovery URL, untrusted appliance
identity, callback bind failure, browser launch failure, denied login, expired
pairing, denied session exchange, and local agent disconnection.

Browser-To-Agent Upload Handoff
-------------------------------

After easyconnect login, the appliance Web ``Remote Upload`` page can prepare a
browser-to-local-agent handoff for selected files or folders. The handoff uses
a loopback ``dasobjectstore-remote`` endpoint such as
``http://127.0.0.1:<port>/v1/dasobjectstore/remote/uploads/handoffs``. The
browser sends only the target ObjectStore, derived bucket, selected relative
display paths, byte counts, and a client handoff identifier. Absolute local
paths stay private to the remote computer and are not part of the browser
payload.

The local agent must require explicit user confirmation before it accepts
transfer authority. The confirmation phrase is derived from the ObjectStore,
for example ``confirm upload to zymo_fecal_2025.05``. If the loopback agent is
not reachable, the browser reports ``agent_unreachable`` and allows the user to
retry after restarting ``dasobjectstore-remote``. If the user cancels before
confirmation, no transfer authority or appliance credentials are handed to the
agent.

Remote easyconnect uploads are classified by the daemon as ``remote_s3``
ingress. That origin always uses ``ssd_first`` landing mode: bytes enter the
selected ObjectStore through its managed SSD path and only then move through
daemon-owned HDD settlement and verification. The remote client must not write
directly to managed HDD roots and users are never asked to choose a disk.

Configure a Remote Host
-----------------------

Configure the DASObjectStore S3 endpoint once on the remote computer:

.. code-block:: console

   dasobjectstore-remote config set \
     --endpoint-url http://192.168.1.192:3900 \
     --region garage \
     --profile dasobjectstore

The default config path is:

.. code-block:: text

   ~/.config/dasobjectstore/remote.json

Use ``--config <PATH>`` or ``DASOBJECTSTORE_REMOTE_CONFIG`` for a different
configuration file.

The remote configuration file is also the planned storage location for
easyconnect pairings. It can contain paired appliance records, issued remote
upload session credentials, session expiry time, renewal metadata, a
secret-bearing renewal token, and the selected default ObjectStore for each
appliance. The file is written with owner-only permissions on Unix systems
because active upload sessions may carry temporary S3 credentials.

Display commands redact secret-bearing fields. ``config show`` prints whether a
credential helper, upload session, and renewal path are configured. ``config
show --json`` emits a redacted JSON view suitable for support logs: session and
access-key identifiers are shortened, secret keys and session tokens are
replaced with ``<redacted>``, renewal tokens are redacted, and raw helper output
is never printed.

Updating the base endpoint with ``config set`` preserves paired appliance and
session records. Pairings are removed only by future explicit pairing/session
management commands; they are not silently discarded by normal endpoint
configuration changes.

For a standalone appliance local-password flow, configure the username and the
credential helper supplied by the site:

.. code-block:: console

   dasobjectstore-remote config set \
     --endpoint-url https://dos-appliance.example:3900 \
     --auth local-password \
     --username alice \
     --credential-helper dasobjectstore-local-s3-credentials

For Mneion or Synoptikon managed sites, use the site-provided helper:

.. code-block:: console

   dasobjectstore-remote config set \
     --endpoint-url https://dos-appliance.example:3900 \
     --auth mneion \
     --credential-helper mneion-dasobjectstore-s3-credentials

List Accessible Object Stores
-----------------------------

List the object stores visible to the configured S3 credentials:

.. code-block:: console

   dasobjectstore-remote stores list

Emit machine-readable output:

.. code-block:: console

   dasobjectstore-remote stores list --json

Inspect the AWS command without running it:

.. code-block:: console

   dasobjectstore-remote stores list --dry-run

Upload Files and Folders
------------------------

For paired easyconnect sessions, the upload argument is the ObjectStore name,
not an S3 bucket. The client resolves that ObjectStore against appliance-issued
writer grants, derives the backing bucket, and uses the stored temporary
session credentials for the AWS CLI environment. If a bucket name is passed
while a paired appliance is configured, the command is rejected and asks for a
writable ObjectStore name.

Upload a single file to a prefix. The filename is preserved:

.. code-block:: console

   dasobjectstore-remote upload zymo_fecal_2025.05 \
     --source ./report.json \
     --prefix experiments/run-001

Upload a single file with an exact object key:

.. code-block:: console

   dasobjectstore-remote upload zymo_fecal_2025.05 \
     --source ./report.json \
     --key experiments/run-001/report.json

Upload a folder recursively:

.. code-block:: console

   dasobjectstore-remote upload zymo_fecal_2025.05 \
     --source ./run-001 \
     --prefix experiments/run-001

For folders, ``dasobjectstore-remote`` uses ``aws s3 sync``. For files, it uses
``aws s3 cp``. Use ``--dry-run`` before large transfers:

.. code-block:: console

   dasobjectstore-remote upload zymo_fecal_2025.05 \
     --source ./run-001 \
     --prefix experiments/run-001 \
     --dry-run

Remote upload plans include the appliance backpressure contract. The default
contract limits remote S3 transfer concurrency to two, multipart part
concurrency to two, browser handoff metadata to 100,000 files or 8 TiB, SSD
stage queue depth to four, HDD landing queue depth to eight, and verification
queue depth to four. When SSD pressure is high, clients should pause new
transfers; when SSD pressure is critical, clients should reject new transfers
until daemon health reports capacity for more intake.

The daemon exposes the same policy as an admission decision for remote upload
intake. The decision can accept intake, pause new transfers with a retry hint
when S3 concurrency or SSD/HDD/verification queues are full, or reject new
transfers while SSD pressure is critical. Remote upload executors should call
the daemon admission API before starting additional intake rather than applying
local-only queue guesses. The daemon runtime maintains the active S3 transfer
count and queue depths used by this decision, so clients should treat
``pause_new_transfers`` and ``reject_new_transfers`` as authoritative. Daemon
upload workers reserve S3 intake capacity with a transfer permit and release it
when the transfer completes or fails. The shared worker wrapper checks admission
before invoking transfer code, so blocked intake does not start partially. The
daemon job wrapper carries the remote upload job id, target ObjectStore, source
byte count, final outcome, and runtime queue snapshot back to the future job
registry/event stream.

Credential Helper Contract
--------------------------

A credential helper is an executable command configured with
``--credential-helper``. DASObjectStore runs it with the following environment
variables:

* ``DASOBJECTSTORE_REMOTE_AUTHORITY``: ``local-password``, ``mneion``, or
  ``synoptikon``;
* ``DASOBJECTSTORE_REMOTE_ENDPOINT_URL``: the configured appliance endpoint;
* ``DASOBJECTSTORE_REMOTE_USERNAME``: the configured username when present;
* ``DASOBJECTSTORE_REMOTE_PASSWORD``: the password captured without terminal
  echo for this invocation.

The helper must print JSON to stdout:

.. code-block:: json

   {
     "access_key_id": "S3 access key",
     "secret_access_key": "S3 secret key",
     "session_token": "optional temporary session token"
   }

The remote client passes those credentials to the AWS CLI process through
standard AWS environment variables and does not write them to the config file.

Operational Notes
-----------------

``dasobjectstore-remote`` uploads through the object service surface. It does
not write into DAS member disks and does not use the local
``dasobjectstored`` Unix socket. Use object prefixes that make upload batches
easy to inspect and clean up if a transfer is interrupted or repeated.
