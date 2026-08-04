Rustls provider ownership inventory
===================================

Status
------

This is qualification evidence for ADR-0001 and its merged transitive TLS 1.2
amendment. It is not a release, installation, deployment or activation claim.

Classification
--------------

``tools/verify-rustls-provider-ownership.sh`` emits a deterministic Cargo
metadata inventory of every workspace library, executable and integration-test
target. Jenkins retains that TSV alongside the locked feature graph. The check
fails if a new Cargo example appears without a class.

The shipped native executables ``dasobjectstore``,
``dasobjectstore-server``, ``dasobjectstore-local-auth-helper``,
``dasobjectstore-auth-migrate``, ``dasobjectstored`` and
``dasobjectstore-remote`` are provider owners. Each has the reviewed AWS-LC
installation call as the first statement of ``main``. The narrow auth helpers
are classified as owners because their shipped link closure includes the GUI
API; they are not allowed to infer a provider from a future command path.

``dasobjectstore-gui-web`` is a WebAssembly browser target with no native
Rustls dependency. ``dasobjectstore-workspace-host`` and the downstream
``dasobjectstore-reference`` fixture are native Rustls-absent classes. The
check verifies those manifests remain free of a direct Rustls dependency.

All workspace libraries are provider-neutral. Their production ``lib.rs``
modules may not install or replace a global provider. The GUI API and remote
trust unit fixtures install AWS-LC only inside ``cfg(test)`` test processes;
they do not create a released executable hook. Every integration-test target
is recorded as an isolated test process rather than a shipped provider owner.

Limitations and retained evidence
---------------------------------

The inventory is a source/metadata guard, not evidence that a released binary
has executed. It complements the locked graph check, source ordering review,
the external Jenkins conflict fixture proposed in ADR-0007, clean-process
subprocess tests, TLS corpus and package acceptance. It deliberately does not
provide a CLI flag, environment variable, configuration field, dynamic-loader
mechanism, source patch or runtime provider-selection path.
