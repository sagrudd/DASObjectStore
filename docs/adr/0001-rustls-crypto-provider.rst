ADR-0001: Select the DASObjectStore Rustls cryptographic provider
========================================================================

:Status: Proposed
:Date: 2026-07-28
:Deciders: Project owner, security reviewer, cryptography reviewer, supply-chain reviewer, DASObjectStore maintainers
:Related issue: `DASObjectStore #7 <https://github.com/sagrudd/DASObjectStore/issues/7>`_
:Related decision: `Monas ADR-0001 at reviewed commit 432dee2e2f7b125013e3f8b516d5d4d65e0978f6 <https://github.com/sagrudd/monas/blob/432dee2e2f7b125013e3f8b516d5d4d65e0978f6/docs/adr/0001-rustls-crypto-provider.rst>`_

Context
-------

DASObjectStore currently compiles Rustls 0.23.42 with both built-in providers.
``axum-server`` selects AWS-LC through ``tls-rustls`` while Reqwest selects
Ring through ``rustls-tls``. Cargo features are additive. A final process
cannot safely let that feature union infer its global provider, and an
embedding executable such as Monas must not inherit a provider installation
from a DASObjectStore library.

The provider controls cipher suites, key-exchange groups, signature
verification, randomness, and private-key operations. Selection is therefore
a security and native supply-chain decision. This Proposed ADR records the
DAS-owned policy and acceptance work; it changes no dependency, feature, TLS
configuration, or runtime behavior.

Current executable and library inventory
----------------------------------------

The locked workspace contains these shipped executable processes:

``dasobjectstore-server``
   Terminates public HTTPS through ``axum-server`` and hosts the separate
   mutual-TLS listener from ``dasobjectstore-gui-api``. Its current ``main``
   installs Ring before entering the async runtime. If this ADR is accepted,
   this executable owns the global AWS-LC installation.

``dasobjectstored``
   Runs the storage daemon and constructs a blocking Reqwest client for
   object-service endpoint probes. It does not terminate Rustls today but is a
   final executable with an outbound TLS path and therefore owns installation.

``dasobjectstore-remote``
   Constructs blocking Reqwest clients for appliance control and
   authentication. Its capture-only enrollment probe currently uses
   ``ClientConfig::builder_with_provider`` with an explicit Ring provider.
   The implementation must migrate that explicit path to the selected
   executable provider while preserving handshake-signature validation, zero
   application data, certificate parsing and display, and explicit
   out-of-band confirmation of the full fingerprint. Capture remains
   explicitly untrusted. After enrollment, discovery, authentication, and
   control clients retain pin or CA trust together with hostname validation.

``dasobjectstore``
   The management CLI can reach daemon/server functionality. Even when a
   particular invocation does not construct TLS, it is a shipped executable
   and must install the provider before argument parsing so future command
   routing cannot silently change ownership.

``dasobjectstore-auth-migrate`` and ``dasobjectstore-local-auth-helper``
   Narrow helper processes. They currently need no TLS. They remain
   provider-free executables unless the accepted implementation proves their
   final link graphs contain Rustls; if Rustls is linked, each must install the
   provider as its first statement or remove the dependency path.

``dasobjectstore-gui-web``
   A WebAssembly executable whose browser transport does not use native
   Rustls. Its target-specific graph must prove Rustls is absent.

``dasobjectstore-workspace-host``
   A workspace-host helper with no current TLS construction. Its locked final
   graph must prove Rustls is absent or it must own installation.

All workspace libraries—especially ``dasobjectstore-core``,
``dasobjectstore-daemon``, ``dasobjectstore-gui-api``,
``dasobjectstore-mnemosyne``, ``dasobjectstore-remote``, and
``dasobjectstore-workspace-host``—must remain provider-neutral. A library may
construct TLS only after its documented executable prerequisite is satisfied;
it must never call ``install_default``. Tests, examples, benches, build
scripts, and integration-test binaries are separate processes and belong in
the normal/build/dev inventory even though they are not shipped products.

Decision
--------

If accepted, every DASObjectStore executable whose final locked graph contains
Rustls will use the non-FIPS
``rustls::crypto::aws_lc_rs::default_provider()`` from exactly pinned Rustls
0.23.42 as its one process-wide provider.

Each owning executable will call one narrowly scoped, executable-owned startup
function as the exact first statement of ``main``. Installation completes
synchronously before argument or configuration parsing, logging/runtime
initialization, TLS construction, filesystem or durable-state mutation, task
or thread spawn, subprocess launch, or socket bind. Any prior provider—even
the same AWS-LC provider—is an ownership conflict. The process exits with a
stable structured error and performs none of those actions. It must not accept
an unknown winner or retry.

Provider-neutral libraries expose the prerequisite but do not install global
state. The embedded Monas process remains owned by ``monas-server`` under its
own accepted ADR; importing ``dasobjectstore-mnemosyne`` must never mutate the
provider.

Dependency and feature policy
-----------------------------

The accepted implementation will:

* change ``axum-server`` to ``tls-rustls-no-provider``;
* change Reqwest to ``rustls-tls-webpki-roots-no-provider``;
* declare Rustls 0.23.42 with default features disabled and exactly
  ``aws_lc_rs``, ``prefer-post-quantum``, and ``std``;
* exclude ``ring``, ``custom-provider``, ``fips``, ``tls12``, and ``logging``
  from the complete normal/build/dev feature union;
* retain exactly one Rustls version and one provider feature in every
  provider-owning final graph; and
* add compile-time executable guards while retaining an independent locked
  Jenkins graph check.

This exactly matches the reviewed Monas ADR-0001 provider contract. A
DAS-specific deviation would create incompatible embedded and standalone
artifacts and therefore requires a superseding accepted ADR with an explicit
cross-project migration.

An independently reviewed standalone executable profile may differ when a
real DAS peer class requires it, but only if that profile is compile-time
distinct, has its own inventory and fingerprint, and does not flow through a
provider-neutral library into the Monas graph. The embedded Mnemosyne adapter
must remain compatible with Monas's exact profile. Runtime selection and a
single executable carrying multiple profiles remain forbidden.

This is expressly non-FIPS. Ordinary ``aws-lc-rs`` does not establish a FIPS
claim. FIPS AWS-LC, approved algorithms, ``ServerConfig::fips()``, and a
certified deployment environment require a separate accepted ADR and evidence.

Peer inventory and compatibility boundary
-----------------------------------------

The implementation review must close a versioned peer inventory before this
ADR can be Accepted:

Appliance HTTPS
   Browsers, ``dasobjectstore-remote``, Monas, Synoptikon, and operator API
   clients of the public HTTPS listener.

Application mutual TLS
   Registered applications and operator tooling that reach the dedicated
   client-certificate listener.

S3 and object services
   Garage, RustFS, AWS-compatible S3 endpoints, and daemon health/probe
   targets reached through Reqwest or externally launched AWS tooling.

Daemon and local operator paths
   CLI-to-daemon, workspace-host, package acceptance, health checks, and any
   reverse-proxy-to-DAS hop, including paths that are intentionally Unix
   socket or local HTTP rather than Rustls.

Jenkins and deployment peers
   Packaged Linux acceptance clients, supported browsers, reverse proxies,
   and every deployment image or appliance currently claimed as supported.

TLS 1.3-only is the recommended target and the Monas-integrated requirement,
but it is a compatibility-breaking boundary for any deployed TLS 1.2-only
appliance, S3/object service, reverse proxy, application certificate client,
or operator tool. No retained deployed-peer evidence currently proves that
every class above supports TLS 1.3. The project owner must confirm the complete
peer inventory and accepted compatibility loss before this ADR can become
Accepted. Until then, TLS 1.3-only remains a proposal, not a product claim.

If a required standalone peer cannot migrate, the owner may approve a
separately reviewed executable profile as described above. TLS 1.2 must never
be re-enabled by feature unification, in a provider-neutral library, or in the
Monas-integrated graph.

Algorithm and proposed inventory contract
-----------------------------------------

Subject to that peer confirmation, the proposed default is TLS 1.3 only, with
this exact ordered inventory:

* key exchange: ``X25519MLKEM768``, ``X25519``, ``secp256r1``,
  ``secp384r1``;
* cipher suites: ``TLS13_AES_256_GCM_SHA384``,
  ``TLS13_AES_128_GCM_SHA256``,
  ``TLS13_CHACHA20_POLY1305_SHA256``;
* certificate-signature verification:
  ``ECDSA_P256_SHA256``, ``ECDSA_P256_SHA384``, ``ECDSA_P256_SHA512``,
  ``ECDSA_P384_SHA256``, ``ECDSA_P384_SHA384``, ``ECDSA_P384_SHA512``,
  ``ECDSA_P521_SHA256``, ``ECDSA_P521_SHA384``, ``ECDSA_P521_SHA512``,
  ``ED25519``, ``RSA_PSS_2048_8192_SHA256_LEGACY_KEY``,
  ``RSA_PSS_2048_8192_SHA384_LEGACY_KEY``,
  ``RSA_PSS_2048_8192_SHA512_LEGACY_KEY``,
  ``RSA_PKCS1_2048_8192_SHA256``, ``RSA_PKCS1_2048_8192_SHA384``,
  ``RSA_PKCS1_2048_8192_SHA512``,
  ``RSA_PKCS1_2048_8192_SHA256_ABSENT_PARAMS``,
  ``RSA_PKCS1_2048_8192_SHA384_ABSENT_PARAMS``, and
  ``RSA_PKCS1_2048_8192_SHA512_ABSENT_PARAMS``; and
* server signing: ``ECDSA_NISTP256_SHA256``, ``ECDSA_NISTP384_SHA384``,
  ``ECDSA_NISTP521_SHA512``, ``ED25519``, ``RSA_PSS_SHA512``,
  ``RSA_PSS_SHA384``, ``RSA_PSS_SHA256``, ``RSA_PKCS1_SHA512``,
  ``RSA_PKCS1_SHA384``, ``RSA_PKCS1_SHA256``.

The key-provider identity is
``rustls-0.23.42/aws_lc_rs/AwsLcRs``. Qualification requires CA-signed ECDSA
P-256 and RSA-2048 certificate/private-key paths. Other listed algorithms are
inventory, not operator recommendations.

The normative fingerprint is SHA-256 over these UTF-8 bytes, with the shown
field and comma order, one LF after every line including the last, and no
other whitespace:

.. code-block:: text

   schema=dasobjectstore.rustls-provider.v1
   rustls=0.23.42
   aws-lc-rs=1.17.3
   aws-lc-sys=0.43.0
   rustls-webpki=0.103.13
   features=aws_lc_rs,prefer-post-quantum,std
   protocol_versions=TLS1.3
   cipher_suites=TLS13_AES_256_GCM_SHA384,TLS13_AES_128_GCM_SHA256,TLS13_CHACHA20_POLY1305_SHA256
   kx_groups=X25519MLKEM768,X25519,secp256r1,secp384r1
   verification_algorithms=ECDSA_P256_SHA256,ECDSA_P256_SHA384,ECDSA_P256_SHA512,ECDSA_P384_SHA256,ECDSA_P384_SHA384,ECDSA_P384_SHA512,ECDSA_P521_SHA256,ECDSA_P521_SHA384,ECDSA_P521_SHA512,ED25519,RSA_PSS_2048_8192_SHA256_LEGACY_KEY,RSA_PSS_2048_8192_SHA384_LEGACY_KEY,RSA_PSS_2048_8192_SHA512_LEGACY_KEY,RSA_PKCS1_2048_8192_SHA256,RSA_PKCS1_2048_8192_SHA384,RSA_PKCS1_2048_8192_SHA512,RSA_PKCS1_2048_8192_SHA256_ABSENT_PARAMS,RSA_PKCS1_2048_8192_SHA384_ABSENT_PARAMS,RSA_PKCS1_2048_8192_SHA512_ABSENT_PARAMS
   key_provider=rustls-0.23.42/aws_lc_rs/AwsLcRs
   server_signing=ECDSA_NISTP256_SHA256,ECDSA_NISTP384_SHA384,ECDSA_NISTP521_SHA512,ED25519,RSA_PSS_SHA512,RSA_PSS_SHA384,RSA_PSS_SHA256,RSA_PKCS1_SHA512,RSA_PKCS1_SHA384,RSA_PKCS1_SHA256

The expected digest is
``f7fe15d3cbe3b2531fe41cd33f8ebe9f6893ce89cc80c157d609381523c9623b``.
It was independently reconstructed from the proposed bytes above, including
the final LF. The implementation must emit the schema, inventory, and digest
from its installed provider; Jenkins reconstructs the bytes independently.
Any inventory or version change requires a superseding accepted ADR.

Hybrid key exchange is an interoperability choice, not a system-wide
post-quantum claim. Certificates, signatures, stored keys, peer support, and
operational controls remain outside such a claim.

Trust boundaries
----------------

Inbound public HTTPS and mutual TLS must validate configured certificate and
private-key material before binding. The mutual-TLS listener must validate the
client chain against its configured CA. Outbound Reqwest clients and the
remote certificate-pinning path must validate CA trust and the requested
hostname; no implementation or test may disable certificate or hostname
verification.

Capture-only enrollment is the sole exception, and it is not an authenticated
application client. The first-contact probe may complete an AWS-LC TLS 1.3
handshake using signature-valid certificate parsing while deliberately
withholding chain and hostname trust. It sends zero HTTP or other application
data, presents no client certificate, token, password, cookie, or credential,
and treats every captured identity field as explicitly untrusted. It parses
the leaf certificate only after the server proves possession of its signing
key, then displays the requested/resolved address, certificate SANs, validity,
leaf SHA-256 fingerprint, SPKI fingerprint, and whether the requested address
matches a SAN.

The probe must label mismatch and untrusted state prominently and must not
discover appliance identity, persist trust, authenticate, or send any
credential. Enrollment requires the operator to compare the full fingerprint
over a separate trusted channel and explicitly confirm that exact fingerprint.
Only after confirmation may a trust record be persisted. Every subsequent
discovery, authentication, control, renewal, appliance, or S3 credential path
uses the enrolled certificate/CA or pin *and* validates the selected hostname.
Capture-only behavior must not be reusable through a general Reqwest client.

Operator-managed reverse-proxy TLS remains a distinct deployment boundary.
The proxy becomes part of the trusted computing base, its DAS hop must be
private or authenticated, and forwarded headers are never an identity source.
Proxy evidence cannot qualify direct HTTPS.

Subprocess and interoperability acceptance
------------------------------------------

Global installation is one-shot, so ownership tests run in isolated
subprocesses. Every owning executable must prove:

* clean installation succeeds before any observable side effect;
* same-provider preinstallation is rejected as an ownership conflict;
* conflicting-provider preinstallation is rejected deterministically;
* both conflicts cause no configuration/durable mutation, task/thread spawn,
  subprocess launch, or socket bind; and
* provider-neutral libraries never install or replace the default.

Packaged Linux qualification must additionally prove:

* capture-only enrollment uses AWS-LC and TLS 1.3, verifies the handshake
  signature, sends zero application bytes and zero credentials, and records
  no trust before explicit confirmation;
* capture display contains the requested/resolved address, SANs, validity,
  full leaf/SPKI fingerprints, address-match result, and an untrusted label;
* invalid handshake signatures, missing/invalid leaf certificates, malformed
  SANs, expired/not-yet-valid certificates, unsupported protocol versions,
  and operator fingerprint mismatch fail without persistence or application
  data;
* a SAN/address mismatch may be displayed for diagnosis but cannot silently
  choose a trusted server name or proceed without out-of-band fingerprint
  confirmation;
* after enrollment, wrong pin/CA, wrong hostname, changed certificate, and
  changed endpoint fail before credentials or application requests are sent;
* hostname- and CA-validating public HTTPS health for ECDSA P-256 and RSA-2048;
* client-CA-validating mutual TLS, including wrong-CA and wrong-host negatives;
* outbound daemon and remote Reqwest paths against a local CA-authenticated
  upstream with valid and invalid hostnames;
* the remote pinned-certificate path under the selected provider without
  bypassing WebPKI checks;
* successful TLS 1.3 ``X25519MLKEM768`` and ``X25519`` negotiation, with
  version, suite, and group retained;
* TLS 1.2-only negotiation fails for the proposed Monas-compatible profile,
  with the peer-inventory decision retained as evidence;
* every executable and test binary's normal/build/dev feature graph matches
  its declared provider-owning or Rustls-absent class; and
* termination releases listeners and preserves existing rollback behavior.

DAS-first implementation and evidence
-------------------------------------

DASObjectStore owns and merges the provider-neutral dependency change first.
Its Jenkins Expedition must qualify the immutable DAS revision before Monas
updates its pin. Monas then qualifies that exact revision in its integrated
process. A Monas patch or feature fork of the DAS dependency is forbidden.

Jenkins is the sole authoritative builder. It must build the pinned source and
Sphinx/Read-the-Docs documentation hermetically in pinned containers, treat
warnings as errors, retain the pre-rendered HTML, and publish only retained
HTML from an eligible current-``main`` build. GitHub-hosted automation must
not build, test, or publish this decision.

The dossier retains exact source revisions, lockfile digest, complete
normal/build/dev feature trees, target triple, compiler and native compiler
identities and flags, container digest, SBOM, advisory/licence/source results,
AWS-LC and ``aws-lc-sys`` source/checksum provenance, ordered provider
inventory, test logs, and negotiated TLS facts. Test CA private material is
created in a private temporary directory, destroyed, and never retained.

Rollback
--------

Before release, rollback returns DASObjectStore to its last qualified
standalone artifact and leaves Monas on its last matching immutable DAS pin.
After release, operators roll back the complete matched DASObjectStore/Monas
artifact pair, lockfiles, SBOMs, and evidence. Independently changing or
rolling back the DAS pin is forbidden. If DAS loses the provider-neutral
contract, Monas direct HTTPS remains disabled until a new matched pair passes.
Existing certificates require no conversion.

Changing provider, FIPS posture, algorithm inventory, protocol versions, or
TLS termination boundary requires a superseding accepted ADR and fresh
qualification. Runtime provider selection is forbidden.

Consequences
------------

The final executable—not a transitive feature or library—owns process-global
cryptography. Standalone and embedded products receive a deterministic,
matched contract. The provider graph is smaller and independently auditable.

AWS-LC introduces native code and toolchain provenance obligations. The hybrid
group improves key-establishment posture for compatible peers but makes no
broader post-quantum claim. Both trade-offs require the named specialist
reviews before this ADR may become Accepted.

Alternatives considered
-----------------------

Ring
   Viable, and used by current DAS paths, but incompatible with the reviewed
   Monas direction and without the same direct FIPS migration option.

Keep both providers and install AWS-LC
   Avoids the immediate panic but retains unused Ring and an ambiguous supply
   chain. Rejected as a release state.

Explicit per-client providers only
   Cannot govern builders inside all framework dependencies and leaves server
   ownership inconsistent. Rejected.

Rely on transitive feature inference
   Dependency updates can silently change the winner. Rejected.

Always use a reverse proxy
   Supported operationally but changes the direct-HTTPS boundary and does not
   resolve executable ownership.

Runtime provider selection
   Makes one artifact cryptographically ambiguous. Rejected.

References
----------

* `Rustls CryptoProvider
  <https://docs.rs/rustls/0.23.42/rustls/crypto/struct.CryptoProvider.html>`_
* `Reqwest 0.12.28 features
  <https://docs.rs/crate/reqwest/0.12.28/features>`_
* `AWS-LC for Rust <https://aws.github.io/aws-lc-rs/>`_
