ADR 0002: Stable appliance identity and remote TLS renewal
==========================================================

:Status: Accepted
:Date: 2026-07-28

Context
-------

Remote clients need to distinguish two properties that have different
lifecycles:

* the identity of the DASObjectStore appliance; and
* the certificate currently used by its HTTPS endpoint.

Treating a leaf-certificate fingerprint as both properties made ordinary
certificate renewal look like appliance replacement. Earlier discovery also
advertised a shared constant while the client persisted an endpoint-derived
variant, so an authenticated response could conflict with the client's own
enrollment.

Decision
--------

Each appliance owns one random, stable identifier in daemon state. Package
reinstallation, service restart and certificate renewal retain that identifier.
EasyConnect discovery and authenticated session exchange return the same
identifier.

Remote enrollment binds an endpoint to that identifier and records the accepted
leaf/SPKI plus, when configured, its domain-cert CA. A changed leaf is accepted
automatically only after all of these independent checks pass:

* the complete presented chain validates to the enrolled CA;
* the certificate is currently valid and has server-auth usage;
* the requested DNS name or IP address matches a SAN; and
* authenticated session exchange returns the enrolled appliance identifier.

The client publishes the renewed trust and session generation with private,
fsync-backed atomic file replacement. A failed transaction restores the prior
trust and AWS profile. An interrupted operation remains fail-closed because
the session binding carries the leaf and SPKI generation; repeating
``authenticate`` safely converges it.

An exceptional replacement uses one ``trust repair`` operation. It displays
the real old and presented identities and certificate evidence, points to
``dasobjectstore trust identity --json`` as the independent appliance-local
source, and asks once if continuity cannot be proved automatically. It never
promotes self-signed, wrong-CA, wrong-SAN, expired or identity-changing evidence
without that independent confirmation.

Consequences
------------

Ordinary CA-backed leaf renewal is transparent to users. Appliance replacement
continues to require an explicit trust decision. Existing fingerprint-only
records remain supported but cannot gain automatic renewal until a trusted CA
is deliberately enrolled. Private keys and persistent provider credentials are
never included in identity or repair output.
