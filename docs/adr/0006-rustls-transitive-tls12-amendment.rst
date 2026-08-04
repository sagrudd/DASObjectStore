ADR-0006: Proposed amendment to ADR-0001 transitive TLS 1.2 handling
=====================================================================

:Status: Proposed
:Date: 2026-08-04
:Deciders: Project owner, security reviewer, cryptography reviewer, supply-chain reviewer, DASObjectStore maintainers
:Amends: :doc:`0001-rustls-crypto-provider`
:Related issue: `DASObjectStore #7 <https://github.com/sagrudd/DASObjectStore/issues/7>`_

Status and scope
----------------

This is review material only. It authorises no dependency update, feature
change, TLS configuration, package, deployment, installation, activation or
release claim. ADR-0001 remains Accepted unless and until this amendment is
separately accepted.

The proposal resolves one implementation contradiction in ADR-0001 only. It
does not alter the selected AWS-LC provider, the non-FIPS posture, the
provider-neutral library boundary, executable-owned first-statement
installation, the peer inventory, or the qualification corpus.

Context
-------

ADR-0001 requires Reqwest's provider-neutral
``rustls-tls-webpki-roots-no-provider`` feature and also excludes ``tls12``
from the complete normal/build/dev feature union. Current Reqwest 0.12.x
declares its optional Hyper-Rustls dependency with the ``tls12`` feature. That
feature reaches ``tokio-rustls/tls12`` and ``rustls/tls12`` even when the
provider-neutral Reqwest feature is selected and all direct DAS dependency
defaults are disabled.

Consequently, the prescribed provider-neutral Reqwest path cannot satisfy the
literal complete-graph ``tls12`` exclusion. Treating that incidental compiled
code as permission to negotiate TLS 1.2 would violate the TLS 1.3-only
Monas-compatible profile. Treating it as an unreported exception would make
the dependency graph misleading.

Proposed decision
-----------------

Replace only ADR-0001's requirement to exclude ``tls12`` from the complete
normal/build/dev feature union with the following narrow rule:

* The complete normal/build/dev graph must continue to exclude Rustls
  ``ring``, ``custom-provider``, ``fips`` and ``logging`` features. It must
  retain exactly one Rustls version and AWS-LC as the sole selected provider.
* The graph may contain transitive Rustls ``tls12`` code only where the locked
  provider-neutral Reqwest path unavoidably enables it. This is a build-time
  exception, not a runtime protocol profile, supported-peer claim or
  configuration option.
* Every DAS TLS client and server configuration, including public HTTPS,
  mutual TLS, daemon probes, remote control, pinned-certificate verification,
  capture-only enrollment and test fixtures, must explicitly offer and accept
  TLS 1.3 only. No configuration may rely on a Rustls default protocol list.
* A TLS 1.2-only peer must fail before application data, credentials, client
  certificates, trust persistence or durable mutation. No downgrade, fallback
  or retry to TLS 1.2 is permitted.
* The lockfile graph check must identify the exact Reqwest-to-Hyper-Rustls
  provenance of the transitive ``tls12`` feature and fail for any additional
  source of it. It must still fail for every prohibited provider-related
  feature and for more than one Rustls version/provider.

AWS-LC remains the only Rustls provider. Every final executable containing
Rustls still installs AWS-LC as the first statement, rejects same-provider and
conflicting-provider preinstallation without side effects, and libraries still
must not install or replace a provider. Embedded Monas remains the provider
owner of its own process.

Required qualification changes
------------------------------

The complete ADR-0001 qualification corpus remains required. In addition to
its existing graph, subprocess, provenance, TLS, capture-only, mTLS, package
and Jenkins evidence, qualification must retain all of the following:

* the full normal/build/dev feature trees and a reverse dependency path showing
  every permitted transitive ``tls12`` occurrence;
* an assertion that no direct DAS dependency enables ``tls12`` and that the
  only accepted path is the locked provider-neutral Reqwest implementation;
* retained TLS facts for each successful client/server test proving TLS 1.3;
* a TLS 1.2-only server negative for every outbound path, including daemon,
  remote and capture-only paths, proving no application bytes, credentials,
  client certificate or persisted trust are sent; and
* a TLS 1.2-only client negative for public HTTPS and mutual TLS, proving no
  listener accepts it and normal listener termination/rollback remains intact.

Jenkins remains the sole authoritative builder and must retain the amendment's
feature-graph and protocol-negotiation evidence alongside the existing
ADR-0001 dossier. A successful compilation alone is not qualification.

Alternative considered: audited Reqwest fork or replacement
------------------------------------------------------------

Maintaining an audited provider-neutral Reqwest fork or replacing Reqwest with
another HTTP client could remove the transitive ``tls12`` code. That is
deferred rather than adopted: it creates a new security-maintenance and
provenance boundary, requires a separately accepted dependency/supply-chain
decision, and does not justify an unqualified change to the currently reviewed
Monas-compatible profile.

Until such a decision is accepted and independently qualified, no fork,
replacement or local patch is permitted to claim that it restores the original
complete-graph exclusion.

Consequences if accepted
------------------------

The protocol boundary remains TLS 1.3-only while the locked dependency graph
is described truthfully. The implementation must prove both facts separately:
AWS-LC-only/provider-neutral dependency selection at build time, and explicit
TLS 1.3-only configuration and negotiated-peer behaviour at runtime. Any
provider change, FIPS posture change, protocol expansion, or new transitive
``tls12`` path requires a superseding accepted decision and fresh
qualification.
