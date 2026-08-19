Pistis Host Authentication
==========================

DASObjectStore does not authenticate human operators locally. The retired
``/etc/pam.d/dasobjectstore`` service, local password helper, product-local
browser login, and intrinsic browser session issuer are not part of the
package or runtime authority surface.

On a standalone product appliance, Monas is the host authentication authority.
On an integrated deployment, Synoptikon is the host authentication authority.
Both authorities use Pistis and Prosopikon to establish the operator session;
DASObjectStore receives only the verified, credential-free host context.

The packaged configuration must select Monas (or Synoptikon when the appliance
is embedded):

.. code-block:: json

   {
     "authentication": {
       "authority": "monas",
       "session_ttl_seconds": 3600
     }
   }

``local_user`` is retained only as a decode-only migration marker. Server
configuration validation rejects it with an explicit retirement error, so an
old configuration cannot silently re-enable a local authority. Re-run the
package's configuration migration or reset the configuration through the
supported Monas setup path before starting the service.

The host adapter validates the live Pistis session and revocation state on
every request before constructing the typed context accepted by the GUI API.
DASObjectStore then checks the schema, authority, issuer, audience, lifetime,
CSRF binding, principal state, and session freshness. A missing, expired,
revoked, malformed, or wrongly scoped context is rejected before a product
handler runs.

The context supplies subject, roles, expiry, correlation, and the CSRF binding.
It does not grant storage access by itself. ``dasobjectstored`` remains the
final authority for storage policy and mutations, including ObjectStore
entitlements, writer groups, destructive actions, and application grants.
Pistis roles are mapped only to the explicitly scoped DASObjectStore
entitlements.

Local Unix users and groups may still appear in daemon policy and device
ownership operations. They are not a password, PAM, administrator, or browser
authentication mechanism. In particular, a local group membership cannot
create a Pistis session or elevate a host-authenticated actor.

The canonical first-install flow is therefore:

1. install the Monas/Synoptikon-compatible package and its dependencies;
2. let the package services start and publish their readiness gates;
3. complete the attended Pistis onboarding ceremony in the host product; and
4. open the DASObjectStore surface through the authenticated host route.

No manual PAM configuration, local password registration, setuid helper, or
second DASObjectStore login is required.

Operational checks
------------------

Inspect the resolved configuration without starting a long-running process::

   dasobjectstore-server --config /opt/dasobjectstore/config.json --check-config
   dasobjectstore-server --config /opt/dasobjectstore/config.json --check-config --json

The package readiness preflight verifies that the selected authority is Monas
or Synoptikon and that no PAM package payload is required. It does not perform
an attended authentication ceremony or manufacture a host session.
