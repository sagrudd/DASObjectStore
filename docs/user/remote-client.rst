Remote Client CLI
=================

``dasobjectstore-remote`` is the lightweight client for computers that are not
the DAS appliance. It talks to object stores through the appliance's
S3-compatible endpoint and uses the AWS CLI for object transfer operations.
It is a DASObjectStore workspace member (``crates/dasobjectstore-remote``), not
a separately sourced project; only its DEB/RPM distribution is standalone.

The remote client is intended for workstations, sequencers, analysis servers,
and other hosts that need to list accessible object stores and upload files or
folders without mounting the DAS storage directly.

Requirements
------------

Install ``dasobjectstore-remote`` and the AWS CLI on the remote computer. The
remote client plans and invokes ``aws s3api`` and ``aws s3`` commands against
the configured DASObjectStore endpoint.

Build the remote client from a source checkout when testing locally:

.. code-block:: console

   make remote

This target builds only ``dasobjectstore-remote``. It does not build the Web
UI, appliance daemon, object-service orchestration assets, or full appliance
packages.

Build the remote-only packages when distributing the client to upload-only
hosts:

.. code-block:: console

   make remote-deb
   make remote-rpm

``make remote-deb`` requires ``dpkg-deb`` from the Debian ``dpkg`` tooling.
``make remote-rpm`` requires ``rpmbuild`` from ``rpm-build`` or the equivalent
RPM tooling for the packaging host. Both targets compile the release
``dasobjectstore-remote`` binary before assembling the package.

These package targets produce packages named ``dasobjectstore-remote`` and
install only the remote client binary and its documentation. They do not install
``dasobjectstored``, systemd service units, local appliance configuration, or
managed storage directories.

The remote package has a hard runtime dependency on system CA certificates so
it can connect to public appliance HTTPS endpoints. Monas private-root
installations instead use the package's signed, process-local Site Trust path;
they do not require a global CA-store mutation. The AWS CLI is a runtime
dependency for actual object transfer: Debian packages list it as ``Suggests:
awscli`` and RPM packages list it as ``Recommends: awscli`` because some sites
install AWS CLI v2 outside the OS package manager. Install a working ``aws``
command before running ``stores list`` or ``upload``.

Easyconnect tries to open the browser login URL automatically. On macOS this
uses the platform ``open`` command; on Linux it uses ``xdg-open``; on Windows
it uses ``cmd /C start``. Remote-only packages do not install a browser or
desktop opener. On headless sequencers, servers, containers, or SSH sessions,
run easyconnect with ``--no-browser`` and open the printed URL from a browser
that can reach the DAS appliance while the remote client keeps waiting for the
loopback callback.

The remote computer must have one of the following credential paths:

* an AWS CLI profile containing S3 access key credentials authorized for the
  object stores;
* a configured DASObjectStore credential helper that obtains temporary S3
  credentials from the site authority without receiving a password.

The remote client does not accept, prompt for, retain, or forward an appliance
password. ``local-password`` and ``--prompt-password`` are retired and fail
closed. Establish a browser-approved Pistis EasyConnect session, or use a
site-issued passwordless credential helper.

Easyconnect Contract
--------------------

``easyconnect`` is the browser-approved connection contract for users who
know the appliance host or IP address but should not paste passwords, S3 access
keys, or bucket names into the terminal. The command binds a loopback callback
listener, opens the appliance login page in a browser, and waits for a one-time
pairing callback:

.. code-block:: console

   dasobjectstore-remote easyconnect 192.168.1.192

To bind the request to one ObjectStore before browser approval:

.. code-block:: console

   dasobjectstore-remote easyconnect 192.168.1.192 \
     --object-store epic_collection

The command resolves the standalone Web application URL using HTTPS port
``8448`` by default. After authenticated approval in the browser, the remote
client receives a one-time pairing result as a form-encoded loopback ``POST``.
The exchange code never enters a URL and is neither printed nor retained after
the one-time exchange.

The Monas/Pistis product boundary separates the routes deliberately:

* pairing creation and one-time exchange are public, bounded JSON operations;
* creation accepts only an exact loopback callback on ``127.0.0.1`` or ``::1``
  with an explicit port and the fixed EasyConnect callback path;
* approval requires a live host browser session and its session-bound CSRF
  value;
* the immutable Prosopikon principal remains the audit subject while a
  separately host-verified local username is used for appliance group policy;
  and
* the daemon rejects an approval that substitutes a different ObjectStore for
  the one requested when the pairing was created.

The command requires previously enrolled appliance certificate trust. Enrol
that trust without a password before the first EasyConnect run, using exactly
one independently obtained evidence source:

.. code-block:: console

   dasobjectstore-remote trust enroll das.example \
     --ca-cert /secure/site-ca.crt

   # Or, when no private CA is available:
   dasobjectstore-remote trust enroll das.example \
     --trust-fingerprint VERIFIED_LEAF_SHA256

The CA form verifies both the presented certificate chain and the exact host
name. The fingerprint form accepts only the independently compared leaf
fingerprint. The command performs no password prompt and sends no
authentication credential. It refuses to overwrite an existing trust record;
use ``trust inspect`` and the explicit rotation or repair workflow instead.

EasyConnect uses the resulting pin for discovery, pairing creation, and
exchange; URLs returned by the appliance cannot redirect the client away from
the pinned HTTPS origin. The server returns the public S3 endpoint, region, and
addressing style alongside the approved principal, grants, and short-lived
session. The client validates the envelope and commits the complete session
generation atomically. It never guesses an S3 endpoint from the browser or
control URL.

In a Monas/Pistis deployment, the public EasyConnect router exposes discovery,
pairing creation, bounded status polling, and one-time exchange without a
browser session. Discovery is emitted only from the deployment-owned HTTPS
origin and persisted appliance identity; missing, non-HTTPS, credential-bearing,
or path-bearing origin configuration fails closed. The discovery contract
advertises ``pistis`` as the authentication provider. Pairing approval is not
part of this public router and remains behind the verified host actor boundary.
Both the public and protected routers send ceremony transitions to the
daemon-owned state machine over one trusted local Unix socket. Host composition
may inject a different absolute socket path for hermetic deployment and
conformance testing, but it must be supplied in process: HTTP input, headers,
cookies, and query parameters can never select the daemon endpoint. Normal
packaged deployments use the daemon's standard socket path.

Inspect the enrolled, non-secret certificate identity before pairing:

.. code-block:: console

   dasobjectstore-remote trust inspect das.example

The credential-free Pistis completion design is recorded as Accepted
:doc:`../adr/0003-credential-free-pistis-easyconnect-approval`.  It requires a
distinct Pistis provider and binds the Prosopikon authority, principal,
session, exact ObjectStore grant, correlation, and audit identities without an
OS-account lookup.  It also requires daemon-owned ceremony expiry and random
identifiers, durable session commit before pairing consumption, idempotent
crash recovery, and a bounded polling fallback.  These are review requirements,
not behavior available in the current command.  Acceptance fixes the security
design; activation remains gated by the immutable principal-to-ObjectStore
resolver, reviewed liveness adapter, negative and crash/replay tests, and
evidence through the real Monas/Pistis route.

For non-browser automation, consume only short-lived credentials issued by the
site authority through a passwordless credential helper. The former
``authenticate`` command is a retired local-password transport and fails
closed; it cannot establish trust, prompt for a password, issue an HTTP
request, or write a remote session. Enrol or inspect TLS identity separately,
then complete EasyConnect with an approved Pistis session.

Inspect and deliberately maintain enrolled trust with:

.. code-block:: console

   dasobjectstore-remote trust inspect 192.168.1.192
   dasobjectstore-remote trust list
   dasobjectstore-remote trust remove APPLIANCE_ID
   dasobjectstore-remote trust repair 192.168.1.192 \
     --username stephen \
     --store epic_collection \
     --set-s3-config

``authenticate --set-s3-config`` performs the repair automatically for a valid
CA-backed renewal. ``trust repair`` is the single exceptional recovery command:
it displays the enrolled identity and old/new certificate evidence, identifies
the independent appliance-local check, asks once when continuity cannot be
proved, renews the session, and configures and verifies S3. Obtain independent
evidence on the appliance with:

.. code-block:: console

   dasobjectstore trust identity --json

That report contains the authoritative appliance ID, certificate fingerprint,
subject, issuer, SANs and validity without private-key material. Self-signed or
wrong-CA replacement certificates, appliance-ID changes, SAN mismatch, invalid
validity, and unauthorized CA changes remain fail-closed. The legacy
``trust rotate`` command remains available for compatibility, but normal users
do not need to copy fingerprints or edit JSON. Removal requires interactive
confirmation or ``--yes``. ``--ca-cert`` and ``--tls-server-name`` remain
advanced administrator-controlled overrides. There is no insecure TLS bypass
or accept-any-certificate option.

Integrated resynchronization
----------------------------

Use ``resync`` when a remote workstation may have stale appliance trust,
temporary sessions, ObjectStore bindings, or AWS profile state. It discovers
the appliance's versioned capability descriptor, negotiates the remote-client
protocol, reconciles exactly one authoritative session for the ObjectStore,
optionally installs and verifies the S3 profile, and finishes with the
daemon-owned readiness check:

.. code-block:: console

   dasobjectstore-remote resync 192.168.1.192 epic_collection \
     --username stephen \
     --set-s3-config

The authenticated endpoint descriptor supplies the externally reachable
scheme, host, port, bucket, region, addressing style, TLS trust requirements,
credential expiry, and supported S3 operations. The appliance proves that its
listener matches the advertised protocol before issuing credentials. The
client independently rejects plaintext HTTP behind an advertised HTTPS URL;
it never silently downgrades the connection.

S3 verification uses private provisional AWS files to perform a signed,
profile-only ``ListObjectsV2`` and, when the bucket is not empty, a
``HeadObject`` against the returned key. Only after both checks pass are the
trust/session generation and standard AWS profile committed. An error reports
the operation, endpoint, bucket, process status, S3 error code, sanitized AWS
diagnostic, and rollback result without exposing credentials. The command
succeeds only after the committed session, profile association, and readiness
state all refer to the discovered appliance identity.

Inspect the proposed changes without writing trust, session, configuration, or
AWS files:

.. code-block:: console

   dasobjectstore-remote resync 192.168.1.192 epic_collection \
     --username stephen \
     --set-s3-config \
     --dry-run \
     --json

The JSON report uses schema ``dasobjectstore.remote_resync.v1`` and contains
only non-secret appliance identity, component version, compatibility, action,
warning, blocker, and readiness information. Passwords, tokens, renewal
material, and S3 credentials are never included.

Ordinary CA-backed certificate renewal and stale session generations are
repaired automatically. A genuine appliance-identity replacement remains
fail-closed. Verify the new identity independently with
``dasobjectstore trust identity --json`` on the appliance. Controlled
non-interactive recovery requires both the independently obtained fingerprint
and explicit replacement acceptance:

.. code-block:: console

   dasobjectstore-remote resync 192.168.1.192 epic_collection \
     --username stephen \
     --trust-fingerprint ACTUAL_SHA256_FINGERPRINT \
     --accept-verified-appliance-replacement \
     --set-s3-config

Protocol incompatibility is reported as either ``remote_client_too_old`` or
``appliance_too_old`` with the component that must be upgraded. Feature
availability is negotiated from the descriptor capability set rather than
inferred from semantic-version ordering.

After a successful resync, ordinary AWS CLI commands require only the profile
and normal S3 arguments—no manual ``aws configure set`` or endpoint override:

.. code-block:: console

   aws --profile dasobjectstore-epic_collection \
     s3api list-objects-v2 \
     --bucket dos-epic-collection \
     --max-keys 1

``--set-s3-config`` installs the temporary session into the standard AWS
credentials and config files under the deterministic profile
``dasobjectstore-epic_collection``. Use ``--s3-profile NAME`` to override it.
The API, not the client, supplies the exact endpoint URL, bucket, region and
addressing style. Profile updates preserve unrelated profiles, are locked and
atomically replaced, respect ``AWS_CONFIG_FILE`` and
``AWS_SHARED_CREDENTIALS_FILE``, and are verified with a bounded authenticated
bucket listing and an object metadata read when the bucket is non-empty. A
conflicting association requires ``--force``;
``--no-verify-s3`` is reserved for diagnostics.

All text and ``--json`` output is secret-free. Temporary credentials and
renewal material remain only in private local configuration. The password is
never stored, sent to the S3 service, or included in process arguments.

Inspect a configured association without disclosing credentials:

.. code-block:: console

   dasobjectstore-remote s3 status epic_collection \
     --profile dasobjectstore-epic_collection \
     --json

The server-side easyconnect contract is defined as stable daemon/API DTOs for
the following operations:

* discovery of appliance pairing capabilities;
* pairing challenge creation for a loopback callback URL;
* browser-authenticated pairing approval;
* exchange of the one-time pairing code for a remote upload session;
* explicit session revocation; and
* renewal of an active session during long uploads.

Monas appliances advertise ``pistis`` as the active easyconnect authentication
provider. PAM, local passwords, browser-local identity, and operating-system
identity are not accepted as human authority. A Monas-hosted approval carries the immutable
Prosopikon subject and a separately verified appliance-local policy subject;
neither GitHub display values nor email addresses are accepted as product
authorization.

Session exchange responses carry a daemon-generated access key, secret,
mandatory session token, one exact ObjectStore/bucket grant, expiry time, and
renewal metadata. They are unrelated to the persistent Garage provider
credential, which remains daemon-custodied and is never returned or copied into
the client configuration. The S3 gateway checks the signed session token,
bucket, read/write grant, expiry, and revocation on every request.

The accessible ObjectStore list is filtered by the daemon before a remote
session is issued. A remote user can only see ObjectStores that the same
authenticated local account may read through public-read, reader-group,
writer-group, or configured administrator-group policy. Write access requires
the daemon writer authorization policy, usually membership in the ObjectStore
writer group. A temporary session cannot be reused for another bucket or
ObjectStore.

Remote upload sessions default to eight hours. The appliance advertises that
default in discovery and the remote client treats renewal as an explicit
session operation rather than a password replay. For the default eight-hour
session, renewal becomes eligible one hour before expiry. Shorter test or
operator-limited sessions become renewable halfway through their lifetime so a
long upload can refresh credentials before interruption. Renewal uses a
daemon-issued renewal-only token and is accepted only after the advertised
``renew_after`` time. A successful renewal atomically rotates the access key,
secret, session token, and renewal token, immediately invalidating the previous
values. It does not require ``dasobjectstore-remote`` to keep the login password
in memory.

Use ``--contract`` to inspect the readable product contract without launching a
browser, or ``--json`` when another tool should consume the contract:

.. code-block:: console

   dasobjectstore-remote easyconnect 192.168.1.192 --contract
   dasobjectstore-remote easyconnect 192.168.1.192 --json

For normal use, create the passwordless session and verified AWS profile in one
command:

.. code-block:: console

   dasobjectstore-remote login 192.168.1.192 OBJECTSTORE \
     --username USER --set-s3-config

The canonical profile uses Monas on HTTPS port 8443 for discovery, browser
approval, and session completion. Integrated deployments keep the standalone
DAS Web/API listener on 8448 closed. ``--authority-profile legacy-standalone``
is required to select the legacy 8448 boundary explicitly.

Integrated Monas requires a one-time signed Site Trust provision before the
first login. On the Monas authority, ``domain-cert site-root public-export``
emits a short-lived public-only ``PXCE/v1`` envelope and prints its SHA-256.
Transfer the envelope and SHA-256 through the independently authenticated
host/container provisioning channel, then run:

.. code-block:: console

   dasobjectstore-remote trust provision 192.168.0.193 \
     --site-uuid SITE_UUID \
     --envelope /secure-mount/site-trust.pxce \
     --authenticated-envelope-sha256 SHA256_FROM_THE_AUTHENTICATED_CHANNEL

The command verifies the Site UUID, signed envelope, current receipt-key
registration, expiry, action, root fingerprint, and canonical CA before writing
a public Site Trust record and PEM bundle. It does not modify the OS CA store,
does not start a daemon, and does not receive a Site private key, GitHub
credential, Pistis identity, or S3 credential. A container or HPC host may
mount that generated record and PEM read-only and pass its record path with
``--site-trust-bundle`` (or set
``DASOBJECTSTORE_REMOTE_SITE_TRUST_BUNDLE``). A missing record fails before any
HTTPS request with ``site trust not provisioned``.

After successful provisioning, the normal command remains fully automated
apart from its intentional browser/Pistis approval:

.. code-block:: console

   dasobjectstore-remote login 192.168.0.193 OBJECTSTORE \
     --username USER --set-s3-config --no-browser

The generated AWS profile includes the same process-local CA bundle, so S3
verification and transfers do not depend on the host CA store either. Do not
use legacy ``trust enroll`` for an integrated Monas endpoint; it remains only
for the explicit legacy standalone profile.

Each invocation asks the appliance to mint a fresh, one-use pairing and shows
the exact browser approval URL. The exchange must return the requested
ObjectStore and exact Pistis actor. Temporary credentials are written only to
owner-private configuration and the AWS profile; terminal output is redacted.
The historical ``authenticate`` spelling remains parseable solely so retired
local-password invocations fail with explicit remediation. It is deliberately
not an alias around Pistis.

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

DGX Spark over SSH
------------------

For a DGX Spark reached over SSH, run the canonical integrated-Monas command
on the Spark and keep that terminal attached while the pairing is in progress:

.. code-block:: console

   dasobjectstore-remote login 192.168.0.193 epic_collection \
     --username stephen --set-s3-config --no-browser

The client prints an ``Open Monas approval URL (shows Pistis QR):`` line. Open
that URL in a browser that can reach the NUC. Even when that browser already
has an ordinary Monas session, Monas displays a fresh Pistis QR for this exact
remote pairing; scan it and complete Face ID. The resulting approval is bound
to the selected ObjectStore and cannot authorise a substituted or replayed
pairing. The QR is rendered only by the Monas host; it is never copied into the
SSH terminal. The client polls for the approved exchange and then prints that
the short-lived session and AWS profile were committed. ``--no-browser`` is
important on the headless Spark because the SSH session cannot launch a local
browser.

Verify the resulting profile from the Spark without exposing credentials:

.. code-block:: console

   aws --profile dasobjectstore-epic_collection s3 ls \
     --endpoint-url https://192.168.0.193:3900

Repeat ``login`` for a different ObjectStore.  Each invocation is scoped to
one store and creates a fresh, expiring session; ordinary object transfers do
not require another QR until that session expires.  Destructive or governed
operations remain behind the normal Monas/Pistis approval policy.

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
     --endpoint-url https://objects.appliance.example:3900 \
     --region garage \
     --profile dasobjectstore

The configured endpoint must be reachable from the remote computer. A Garage
or S3-compatible service bound only to ``127.0.0.1:3900`` on the DAS host is
valid for local testing but will not accept remote uploads. Render the
production object-service Compose file with the default DASObjectStore
``0.0.0.0`` binding, or set an equivalent non-loopback bind address, before
using an appliance IP such as ``192.168.1.192`` in remote upload plans.

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

For a Pistis-managed site, use the site-provided passwordless helper:

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

An EasyConnect session is scoped to one ObjectStore. Its signed S3
``ListBuckets`` response therefore contains only that ObjectStore's backing
bucket, even if the same principal has other grants. Run ``login`` for each
additional ObjectStore; the gateway never uses one session to enumerate
another grant.

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
If the paired ObjectStore has no active session, or if the stored session has
expired, the client rejects the upload before using any stored credentials and
asks the user to run ``dasobjectstore-remote easyconnect`` again.

Remote catalogue control without SSH
------------------------------------

Production clients use two deliberately separate network surfaces:

* standard S3 for LIST, HEAD, PUT, multipart completion, and object reads; and
* the authenticated appliance HTTPS API for readiness, catalogue inventory,
  payload-group settlement, and reconciliation status.

They do not require an SSH binary, an ``~/.ssh`` directory, sudo, a daemon Unix
socket, an appliance filesystem path, or Garage administrator credentials.
Authenticate once to obtain a temporary, store-scoped session. The same
rotating session token authorizes the HTTPS control calls; the S3 secret and
renewal token are never placed in an HTTP control header, command argument, or
JSON error.

The stable control command hierarchy is:

.. code-block:: console

   dasobjectstore-remote stores readiness epic_collection --json
   dasobjectstore-remote objects snapshot epic_collection \
     --prefix EPICv1/ --limit 20000 --json
   dasobjectstore-remote objects group-status epic_collection \
     --key EPICv1/GSE224365_RAW.tar --json
   dasobjectstore-remote objects reconcile-s3 epic_collection \
     --key EPICv1/GSE224365_RAW.tar \
     --expected-bytes 10705582080 \
     --expected-sha256 "$PAYLOAD_SHA256" \
     --idempotency-key epic-gse224365-v1 \
     --ack-policy after-ssd-ingest --json
   dasobjectstore-remote operations status "$OPERATION_ID" --json
   dasobjectstore-remote operations wait "$OPERATION_ID" \
     --until ssd-acknowledged --timeout 10m --json

Snapshot results are bounded to 20,000 objects per response. Follow
``next_cursor`` until ``complete`` is true; cursors are opaque and bind later
pages to the first page's catalogue high-water mark. Never decode, edit, or
persist a cursor as object identity.

``reconcile-s3`` expects the payload, ``.manifest.json``, and ``.sha256`` keys
to exist. The appliance independently verifies their identities, payload byte
count, and authoritative SHA-256 metadata before it queues daemon-owned work.
Reuse the same idempotency key after a timeout or client restart. With
``after-ssd-ingest``, a successful response means the object is immediately
catalogue-visible and HDD destage is durably queued; it does not claim HDD
settlement. Use ``operations wait --until hdd-settled`` where that stronger
boundary is required.

The HTTPS client uses the appliance certificate pinned during
``authenticate``. A changed certificate fails closed and requires deliberate
out-of-band verification. An expired session, an unauthorized store or prefix,
catalogue lock, and capacity pressure are returned as typed errors. Honour
``retry_after`` for retryable lock or backpressure responses. Catalogue locks,
daemon restarts, and storage backpressure are retryable; unsupported daemon
operations, API/daemon contract mismatch, catalogue configuration or
permission failures, and transport permission failures are not. Error JSON and
the ``x-correlation-id`` response header carry the same correlation ID for
matching a client failure to the server journal without exposing credentials.

Migrating an SSH-based harvester
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Replace appliance-shell calls one-for-one: local ``profile-readiness`` becomes
``stores readiness``; catalogue SQL or per-key SSH loops become one paginated
``objects snapshot``; settlement inspection becomes ``objects group-status``;
``store repair --reconcile-s3`` becomes the constrained, idempotent
``objects reconcile-s3``; and daemon job polling becomes ``operations
status/wait``. Keep object transfer on S3. Remove SSH identities only after the
HTTPS workflow has passed a three-object payload/manifest/checksum acceptance
test and a repeated idempotency test.

Upload a single file to a prefix. The filename is preserved:

.. code-block:: console

   dasobjectstore-remote upload zymo_fecal_2025.05 \
     --source ./report.json \
     --prefix experiments/run-001

Upload a single file with an exact object key:

.. code-block:: console

   dasobjectstore-remote upload zymo_fecal_2025.05 \
     --source ./report.json \
     --key experiments/run-001/report.json \
     --content-type application/json

For a single file, ``--content-type`` preserves an explicit bounded MIME type
on the stored object. It is rejected for folder uploads and rejects parameters,
control characters, and malformed values; use a plain ``type/subtype`` token.

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

When the local agent is running on the DAS appliance, or on a host where the
source path is readable by ``dasobjectstored``, submit the upload through the
daemon instead of executing the AWS CLI directly:

.. code-block:: console

   dasobjectstore-remote upload zymo_fecal_2025.05 \
     --source /srv/incoming/run-001 \
     --prefix experiments/run-001 \
     --submit-to-daemon

This path sends the planned AWS command, source byte count, backpressure
policy, redacted display arguments, and temporary AWS session environment to
the daemon over its local socket. The daemon owns admission control, remote S3
transfer capacity, SSD pressure gating, HDD landing queue accounting,
verification queue accounting, and final job persistence. Use
``--daemon-socket`` only when testing a non-default local daemon socket.
The remote client renders the daemon job events returned by this route using
the same job model as local ingest: running/progress/final rows include the
daemon job id, state, percent complete when the daemon has a byte total, byte
counters, unit counters, stage, and daemon message or failure text. Use
``--no-progress`` to suppress intermediate running/progress rows while still
printing the terminal daemon result.

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
remote-upload admission observes SSD staging, HDD write, and verification queue
depths from daemon ingest telemetry; scan and source-read queue depths do not
contribute to remote upload backpressure. Daemon
upload workers reserve S3 intake capacity with a transfer permit and release it
when the transfer completes or fails. The shared worker wrapper checks admission
before invoking transfer code, so blocked intake does not start partially. The
daemon job wrapper carries the remote upload job id, target ObjectStore, source
byte count, final outcome, and runtime queue snapshot back to the future job
registry/event stream. Those summaries now map onto the common daemon job
event model using the stable ``remote_upload`` job kind, with completed
transfers emitted as complete events, temporary backpressure emitted as waiting
progress events, and rejected/failed transfers emitted as failed events. The
same summaries are persisted in the daemon job registry, so remote-upload
transfer attempts can be inspected through the common job status/list path even
before the final live progress stream is attached. The daemon worker facade now
records ``running`` only after admission capacity is acquired, executes the
byte-transfer implementation, releases capacity after completion or failure,
and records the final job state. Concrete byte-transfer implementations can
also publish intermediate byte progress through the worker while the admission
permit is held; those updates are persisted as normal daemon job progress
events. The daemon exposes a typed byte-transfer adapter for concrete
S3/object-service upload engines; those engines should implement the adapter
instead of invoking raw upload code directly, so admission, progress recording,
permit release stay centralized. A completion implementation can additionally
be injected at the worker boundary; the terminal ``complete`` event is emitted
only after that implementation commits the manifest/catalogue handoff. A
handoff error leaves the job failed and releases the intake permit, so provider
success is never presented as ObjectStore acceptance. For Garage, the
EasyConnect AWS CLI submit contract can carry a single-object completion record
with provider, bucket, object identity/version, relative key, endpoint, and
SHA-256. The daemon then performs an independent ``aws s3api head-object``,
requires the admitted size and ``dasobjectstore-sha256`` metadata to match, and
atomically publishes the provider placement through shared SQLite before the
terminal event. Producers requesting this authoritative completion must set
that S3 metadata during upload. Legacy multi-object requests without the
completion record retain transfer-only semantics during migration. The paired
``dasobjectstore-remote`` daemon-submit path generates this contract
automatically for a single file: it streams SHA-256 locally, adds the metadata
to ``aws s3 cp``, and derives a content-stable logical version. Directory
``sync`` remains transfer-only until its per-key manifest producer is wired.
The daemon
also includes a concrete AWS
CLI transfer adapter for S3-compatible object-service intake. That adapter
runs the configured ``aws s3`` command through the daemon command-runner
boundary, keeps redacted display arguments separate from execution arguments,
and records completion bytes through the common remote-upload progress model.
Progress updates now have a typed telemetry payload for source scan count,
staged bytes, S3 transfer rate, SSD queue depth, HDD landing queue depth,
active per-HDD writers, verification state, and session-renewal status. Until
the source scanner, SSD stager, HDD landing, verification, and renewal executor
workers are fully wired, those fields appear only when a producer supplies
them. The easyconnect AWS CLI submit path supplies source scan count and
staged-byte totals from the client-side source inventory. The daemon
remote-upload worker derives S3 transfer-rate telemetry from byte progress and
progress timestamps when a transfer producer does not supply its own rate;
non-zero SSD stage and HDD landing queue depths are populated from the daemon
admission gate snapshot; active HDD writer counts and pending verification
state are derived from daemon ingest telemetry. The easyconnect AWS CLI submit
path also reports whether paired session renewal metadata is configured,
missing, or unavailable; active renewal execution remains future work.
For operator diagnosis of slow remote uploads, read these telemetry fields
together: low S3 rate with empty queues points to the remote host, network, or
object-service path; non-zero SSD queue depth or high SSD pressure means intake
is waiting for staging capacity; a non-zero HDD landing queue with active
per-HDD writers at the daemon limit means all safe HDD write slots are in use;
pending verification means the object has arrived but is not settled yet.
The runtime job executor constructs the remote-upload job and AWS CLI transfer
from one easyconnect job request, then runs that job through the same
admission-gated worker used by lower-level transfer adapters. The daemon API
and typed daemon client now expose that executor as an easyconnect AWS CLI
upload submission route, so paired clients can hand upload jobs to
``dasobjectstored`` instead of invoking storage mutation paths directly.
Cancelled or interrupted remote uploads use a typed daemon cleanup plan. The
plan identifies partial SSD-staged objects, incomplete S3 multipart uploads,
abandoned remote sessions, expired pairings, and interrupted browser handoffs
before cleanup workers mutate any state. Required destructive cleanup, such as
partial SSD-stage removal or multipart abort, is distinguished from resumable
session and browser-handoff cleanup so later progress views can report what is
safe to retry. The runtime cleanup worker facade records per-action completion
or failure and continues through the plan, so a failed multipart abort does not
hide session or handoff cleanup status. Remote upload transfer workers can now
run that cleanup plan after a failed transfer and return the cleanup report to
the daemon caller. The daemon cleanup runtime removes only configured managed
SSD-stage and local state-record paths, rejects path-escape identifiers, and
uses the configured AWS CLI environment to abort incomplete multipart uploads
against the object service.

Credential Helper Contract
--------------------------

A credential helper is an executable command configured with
``--credential-helper``. DASObjectStore runs it with the following environment
variables:

* ``DASOBJECTSTORE_REMOTE_AUTHORITY``: ``pistis``, ``mneion``, or
  ``synoptikon``;
* ``DASOBJECTSTORE_REMOTE_ENDPOINT_URL``: the configured appliance endpoint;
* ``DASOBJECTSTORE_REMOTE_USERNAME``: the configured username when present.

``DASOBJECTSTORE_REMOTE_PASSWORD`` is never set or forwarded. A helper that
requires a password is not compatible with the remote client and must fail
closed.

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
Authentication state
--------------------

Remote authentication is stored as immutable generations.  A small
``state.json`` pointer selects exactly one active generation, and each
ObjectStore has one canonical appliance-bound session used by both HTTPS
control requests and its temporary S3 profile. Re-authentication replaces that
binding only after both S3 verification and HTTPS readiness succeed; a failed
attempt leaves the previous coherent generation active.

Legacy ``remote.json`` files are migrated and privately archived
automatically. Operators must not rename or edit them. Inspect state without
revealing credentials with::

   dasobjectstore-remote config doctor --json
   dasobjectstore-remote config repair --dry-run --json

Apply a reported safe migration with::

   dasobjectstore-remote config repair --apply --json

Doctor, repair, and normal session lookup all apply the same rule: there may
be exactly one authoritative session and one S3 profile association for an
ObjectStore, even when an appliance identity has changed. A dry run lists the
appliance IDs, stores, profiles, and expiry times it will retain or retire; it
never renders session credentials. Repair selects the complete binding whose
endpoint trust matches the currently enrolled appliance. If none matches, it
retires the obsolete bindings so the active ``authenticate`` invocation can
establish the replacement. Apply writes a new immutable generation and moves
``state.json`` atomically; the previous generation remains the private archive.
``authenticate --set-s3-config`` performs this safe repair automatically and
continues without requiring a second authentication command.
