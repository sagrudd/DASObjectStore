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
