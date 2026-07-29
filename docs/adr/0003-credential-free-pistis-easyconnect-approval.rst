ADR 0003: Credential-free Pistis EasyConnect approval
=====================================================

:Status: Proposed
:Date: 2026-07-29

Context
-------

EasyConnect must let a remote CLI obtain one short-lived, ObjectStore-scoped
session after approval by a current Pistis user.  DASObjectStore already has a
credential-free host boundary: Monas resolves a live, audience-bound
Prosopikon actor and does not pass the browser credential, Pistis message, or
an independent verifier into product code.

The approval transaction must preserve that boundary.  A GitHub username,
email address, or other federated subject is not necessarily an appliance OS
username.  Looking it up in the host account database would conflate identity
namespaces and make customer-hosted deployments depend on matching local
accounts.  Conversely, a product-wide Pistis role is not by itself authority
for a particular ObjectStore.

Pairing creation, approval, exchange, session issue, and audit are one security
transaction.  Client-supplied expiry, predictable pairing identifiers,
unimplemented polling, or consuming a pairing before its session is durable
can leave that transaction ambiguous after retries or service failure.

Proposed decision
-----------------

Pistis approval uses a typed ``EasyConnectApprovalContext`` derived from the
request-time ``AudienceBoundActorContext``.  It contains no bearer credential,
password, private key, exchange code, renewal token, or S3 credential.  Its
versioned contract binds all of the following:

* the configured Prosopikon ``authority_id``;
* the immutable ``principal_id`` and current ``session_id``;
* the exact ``dasobjectstore`` audience and a distinct ``pistis`` authentication
  provider value;
* the exact requested ObjectStore and the DAS-owned grant resolved for it,
  including read/write facts, allowed prefix, bucket, and permitted control
  operations;
* the host correlation identifier and the host authentication audit-event
  identifier; and
* bounded issue and expiry times no later than the verified host session.

The context is constructed only after the existing preverified Pistis boundary
has checked authority, audience, principal status, session/principal agreement,
session currency, and CSRF.  The product passes it to the daemon over the
authenticated local service boundary.  The daemon records a canonical digest
of the context with its own approval audit event.  Audit records link pairing,
authority, principal, session, ObjectStore, correlation, host event, daemon
event, and the resulting remote-session identifier without recording secrets.

The ``pistis`` provider is distinct from ``standalone_local_user``.  A Pistis
principal is never resolved with ``getpwnam``, PAM, sudo, or local group lookup.
Instead, a DAS-owned policy record keyed by
``(authority_id, principal_id, object_store_id)`` supplies the exact storage
grant.  Active Prosopikon product entitlement permits the adapter to attempt
that lookup but cannot create or widen a storage grant.  Missing, stale,
ambiguous, read-only-for-a-write-request, or differently scoped policy fails
closed.  Standalone PAM/OS approval remains a separate provider and policy
path.

The daemon owns ceremony time and identity:

* pairing identifiers and poll capabilities use operating-system randomness
  with at least 256 bits of entropy;
* a pairing ceremony has a server-owned maximum lifetime of five minutes,
  independent of the requested remote-session lifetime;
* approval expiry is derived from daemon pairing state and the verified host
  session, never from a query parameter or approval JSON field;
* an optional client request identifier is an idempotency key bound to the
  canonical creation request, and reuse with different input is rejected; and
* the exact requested ObjectStore cannot be changed during approval or
  exchange.

Approval and exchange use one durable transaction journal rather than
competing pairing and session state machines.  Exchange first prepares and
persists the remote session, its exact grant, credential generation, audit
linkage, and response identity.  Only then may the pairing become consumed.
The durable transition atomically links both records where the persistence
engine permits it; otherwise a journaled prepare/commit protocol makes recovery
converge to the same result.  A retry with the same valid one-time exchange
capability returns the already committed session result, subject to a short
bounded recovery window, and never issues a second session.  A wrong,
expired, or replayed capability cannot reveal or replace the committed result.
Restart recovery completes or rolls back an interrupted prepare deterministically.

Pairing creation returns a separate high-entropy poll capability.  A bounded
poll endpoint reports only pending, approved, denied, expired, or exchanged
state and releases the exchange result only to the matching poll capability.
The CLI prefers the loopback form-POST callback, never places the exchange
secret in a URL, and falls back to bounded polling when callback binding or
delivery is unavailable.  Polling uses a deadline, capped exponential
backoff, and explicit cancellation.  Callback and polling must converge on the
same durable exchange transaction.

Consequences
------------

Pistis users can approve remote CLI access without a matching appliance
account and without disclosing an upstream credential to DASObjectStore.  An
operator must configure explicit principal-to-ObjectStore policy before such
approval can succeed.  Existing standalone local-user EasyConnect remains
compatible but cannot be mislabeled as Pistis.

The provider enum, approval context, persisted transaction schema, audit
schema, policy registry, and polling contract are compatibility-sensitive.
Implementation requires specialist security/protocol review, migration and
negative fixtures, crash/replay testing, and acceptance through the real
Monas/Pistis route.  This Proposed record authorizes no implementation and
does not indicate project-owner acceptance.
