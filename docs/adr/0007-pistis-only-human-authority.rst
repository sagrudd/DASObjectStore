ADR-0007: Pistis-only human authority and retirement of local credentials
=======================================================================

:Status: Accepted
:Date: 2026-08-05
:Deciders: Project owner, DASObjectStore and Mnemosyne programme maintainers
:Supersedes: the standalone-human-authority portions of :doc:`0003-credential-free-pistis-easyconnect-approval`

Context
-------

DASObjectStore historically offered an appliance-local browser authority.  It
contains a PAM helper, local username/password login endpoints, a persisted
local session registry, OS-user/group and sudo-derived policy checks, and a
legacy Monas adapter that parses a raw ``monas_session`` cookie.  Even where a
new host-composed Pistis boundary is available, retaining those paths creates
a second human authority and a potential fallback that cannot be accepted for
the Mnemosyne product.

The programme decision is explicit: every human authentication and role
decision is made through Pistis, with Monas owning the product session.  A
customer installation must not require TPM, PKCS hardware, PAM, an appliance
password database, a local browser session issuer, or a POSIX account mapping.
The storage daemon remains the owner of storage operations and its own
non-human service boundaries; this ADR does not remove its Unix service
account, file ownership, daemon socket checks, or S3 SigV4 data-plane
validation.

Decision
--------

The sole permitted human-authority input to DASObjectStore is an already
verified ``VerifiedHostAuthenticatedContext`` supplied by an embedding Monas
or Synoptikon host.  For the Monas product, that context originates in a
current Pistis ceremony/session and must be audience-, Site-Trust-, expiry-,
CSRF-, correlation- and subject-bound before any DAS route is reached.

``DasRolePolicy`` is the only DAS role interpreter.  It derives the closed
``storage_viewer``, ``storage_operator`` and ``storage_administrator`` policy
from the verified host context.  A username, POSIX uid/gid, group membership,
``sudo``/``wheel`` status, PAM result, cookie, bearer header, password,
registration token, local session token, or local credential file grants no
human storage authority.

Legacy input must fail closed.  DASObjectStore must neither reinterpret nor
silently migrate a legacy local session, password record, raw Monas cookie, or
OS role.  A retained legacy registry may be preserved by an operator solely as
rollback evidence, but it is not read by a released authority path.  No package
script may delete credentials, create a substitute account, enable a service,
or perform an unattended recovery.  Recovery is attended: establish Site
Trust, complete the iPhone Pistis ceremony, create the Monas session, and then
authorise a new operation through the verified host context.

Ordered retirement plan
-----------------------

The implementation is intentionally ordered so that removal never leaves an
unprotected operational surface:

#. Migrate every required operational API to a dedicated preverified router.
   Each route receives the verified context, re-derives ``DasRolePolicy``,
   binds the actor subject to the requested scope, and uses only the bounded
   daemon bridge.  Missing, expired, malformed, audience-mismatched, or
   role-insufficient context returns an explicit denial before daemon work.
#. Make Monas and Synoptikon mount only those preverified product routers.
   Remove the legacy ``monas_session`` parser and the adapter constructors that
   accept ``ProsopikonAuthStore`` or ``LocalAuthStore``.  The product must not
   independently parse a host bearer or invoke an identity store.
#. Delete the standalone GUI authority as one coherent source change:
   ``LocalAuthStore``, PAM authentication, login/register/logout/session
   routes, standalone header/Bearer extraction, intrinsic-login components,
   local-user/group management views, and OS/sudo policy adapters.  Replace
   their tests with negative tests proving that the former headers, routes and
   credential files are unusable.
#. Delete the CLI local-auth helper and local-registry migration executable,
   then remove PAM from the GUI API feature set and dependency graph.
#. Remove the PAM service, setuid helper payload, PAM runtime/build
   requirements, local-authority configuration defaults, lifecycle hooks and
   documentation from DEB/RPM packages.  Package validation must reject every
   one of those artefacts rather than merely omitting a test expectation.

The package transition is manual and fail-closed.  It may retain an operator's
pre-upgrade files untouched, but it must not load them; an operator who needs
them for recovery rolls back through the documented APT/RPM procedure.  The
new package only becomes usable after the explicit Monas/Pistis configuration
and custody gates pass.

Implementation ledger (2026-08-06)
------------------------------------

This ledger is deliberately route-level.  It prevents a package-only PAM
removal or an apparently harmless read route from being mistaken for the
authority cutover.

Delivered host-composed routes
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

``host_composed_gui_api_router`` is mounted only beneath the verified host
middleware.  Its status and planning routes have no local authority input.
The following operational routes are separately migrated and re-derive
``DasRolePolicy`` from the matching ``VerifiedHostAuthenticatedContext``:

* administrator: ingest control, ingest-policy update, ObjectStore creation,
  enclosure preparation, endpoint upsert and connection test, job status and
  cancellation, portable catalogue import, and performance-report rebuild;
* operator: profile-object ``PUT`` and ``DELETE``.

Each uses the bounded daemon bridge (the priority bridge for mutations).  A
legacy local session, password, PAM result, POSIX group, sudo state, or raw
cookie cannot supply authority to these host routes.

The first profile-inspection tranche is also host-composed: profile S3 LIST,
HEAD, verify, diagnostics and health, together with profile readiness.  These
routes require a matching verified ``storage_viewer`` role *and* an exact
ObjectStore scope bound to the same verified subject, session and correlation
identifier.  A product-wide viewer role alone is insufficient.  The scope is
an in-process trusted-host extension, never a browser header, cookie or DAS
session; its absence, substitution or store mismatch fails before daemon work.

Remaining route families and safe order
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

#. Migrate the profile read and multipart family from
   ``StandaloneDashboardRouteState``: store capacity; S3 list, HEAD, GET,
   verify, diagnostics and health; profile readiness/capabilities/catalogue
   export; then multipart part, status and completion.  The first group needs
   an exact verified ``storage_viewer`` store scope.  Multipart mutations need
   a verified ``storage_operator`` store scope, subject-bound daemon requests,
   and replay/idempotency coverage before they are exposed by the host router.
#. Replace the detailed enclosure, ObjectStore and remote-upload dashboards.
   Their current aggregators resolve ``LocalUserAuthorityProvider`` and
   sudo/group state.  They must consume the verified subject and closed DAS
   role policy instead; copying an OS-derived role into the host context is not
   an acceptable bridge.
#. Migrate the object browser, object download and folder-download routes.
   ``StandaloneObjectBrowserRouteState`` currently translates a browser actor
   through ``discover_local_user`` into a daemon delegated actor.  Define a
   path-free verified-subject delegation DTO and validate store/prefix scope
   in the daemon before removing that lookup.  Downloads must retain their
   existing no-store, bounded archive-worker and provider-stream protections.
#. Remove, rather than rehost, the users/groups workspace and local-group
   mutation routes.  Their only purpose is OS account/group administration;
   Monas/Pistis and product roles replace that responsibility.
#. Keep remote control and application/S3 capability routes distinct from
   human browser authority.  They are service-capability contracts and must
   remain independently scoped, replay-protected and passwordless; they must
   not be made callable by a host browser session merely to simplify routing.
#. Once every required host route has a verified counterpart, remove the raw
   Monas-cookie/``ProsopikonAuthStore`` adapter and all
   ``federated_gui_api_router(LocalAuthStore)`` production composition.  Then
   delete the local GUI authority, CLI helper/migration binary and package PAM
   assets in the order above.

Daemon boundary caveat
~~~~~~~~~~~~~~~~~~~~~

The daemon may keep a dedicated service account and Unix-socket peer checks
for its non-human boundary.  It must not retain root, ``sudo`` or
``dasobjectstore-admin`` membership as an alternative human authorizer for a
shipped operation.  Before deleting the local GUI stack, qualify every direct
daemon/CLI administrative entrypoint so it either carries the verified host
authority through the trusted bridge or fails closed.  This is separate from
the S3 SigV4 data plane and does not change the 3900 gateway contract.

Direct daemon/CLI authority audit and bridge contract (v1)
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

The daemon's Linux socket currently derives a transport peer from
``SO_PEERCRED`` and resolves its POSIX name/groups.  The legacy
``DaemonLocalActor::is_administrator`` predicate still recognises UID 0,
``sudo`` and ``dasobjectstore-admin`` for direct administrative request
families.  This is residual authority debt, not an approved human authority
mechanism.  A future release must not silently reinterpret these values as a
Pistis subject.

The following rule applies while that migration is incomplete:

* a direct CLI/socket request that depends on the legacy administrator
  predicate is a compatibility-only operation and is **not** a valid Monas
  product authority path;
* a replacement direct operation requires a new versioned daemon request
  envelope containing the verified subject, session, correlation, Site Trust
  audience, exact operation and bounded resource scope; the envelope must be
  mutually exclusive with legacy POSIX delegation;
* the daemon must bind the envelope's fixed peer identity to the Unix peer,
  reject missing/substituted/replayed values before operation dispatch, and
  record the verified subject/correlation rather than a username; and
* no generic root/sudo/admin bypass may be added while that bridge is absent.

As an immediate bounded closure, legacy delegated browser/workspace authority
is accepted only from the fixed ``dasobjectstore`` Web/API service peer.
Root is no longer a delegation principal.  This prevents a privileged direct
socket client from manufacturing an arbitrary POSIX actor while preserving the
existing package service boundary.  It does **not** make the legacy envelope a
Pistis bridge: all legacy delegation must still be removed after the verified
successor covers the corresponding route family.

The ObjectStore ingest-policy and acknowledgement-policy mutation families
have their preverified host-route counterparts and therefore no longer retain
the compatibility exception: they accept only the fixed packaged host service
peer and a non-blank verified subject.  Direct root, ``sudo`` and
``dasobjectstore-admin`` CLI/socket peers are rejected before the policy
registry or administrative-job ledger is touched.  The service peer is a
transport boundary, not an authorizer; Monas/Pistis verification remains a
required precondition in the host route.

The direct request families that remain to be migrated are the daemon
administrative service, disk, store, workspace and local-group operations,
together with CLI commands that submit them.  The migration must be organised
by request family and include an explicit rejection regression for root,
``sudo`` and ``dasobjectstore-admin`` peer credentials.  It may not be
completed by removing package assets or by weakening socket permissions.

Acceptance criteria
-------------------

Before the retirement is marked complete, retained Jenkins evidence must show:

* every shipped DAS human-operational route is behind a verified host context;
* invalid/missing context, a raw Monas cookie, standalone headers, Bearer
  tokens, PAM success and an OS administrator identity all fail before daemon
  mutation;
* no released crate depends on the Prosopikon ``pam`` feature, Argon2, or a
  password prompt/library for human authority;
* the DEB and RPM contain no PAM service, setuid local-auth helper, local
  credential/session database, local authority configuration or lifecycle
  activation; and
* a fresh package installation reaches a human-authorised DAS operation only
  after Site Trust, Thesaurophylax custody, an iPhone-attested Pistis ceremony
  and a durable Monas session, with auditable correlation retained.

Consequences
------------

The removal is deliberately larger than a package-only cleanup.  A package
that omitted PAM while retaining callable local routes would hide an unsafe
fallback, while deleting the routes before their preverified counterparts are
complete could strand safe operations.  The ordered plan permits small,
independently reviewable migrations without weakening the final red line.
