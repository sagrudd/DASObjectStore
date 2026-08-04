ADR-0007: Proposed external provider-guard qualification interface
===================================================================

:Status: Proposed
:Date: 2026-08-04
:Deciders: Project owner, security/API reviewer, DASObjectStore maintainers, Jenkins maintainers
:Related: `DASObjectStore #59 <https://github.com/sagrudd/DASObjectStore/pull/59>`_, `Jenkins #160 <https://github.com/sagrudd/jenkins/pull/160>`_

Status and scope
----------------

This is review material only. It authorises no implementation, dependency,
feature, package, executable, test target, release, deployment, installation
or activation change. In particular, it does not add an in-product way to
preinstall, select, replace, inspect or reset a Rustls cryptographic provider.

Context
-------

ADR-0001 requires each DAS executable that owns Rustls to install AWS-LC as
the first statement of its process entry point and to fail closed if a
different provider is already installed. Jenkins #160 needs a narrow,
reproducible way to qualify that conflict path with Ring preinstalled. A
separate executable cannot preinstall a provider in the released DAS process;
attempts to make that possible with a command-line option, environment value,
configuration file, dynamic loader or source replacement would create an
unacceptable production control surface.

Proposed decision
-----------------

Any future conflict qualification interface must be an immutable, external
fixture closure. It is not a DASObjectStore executable, package, Cargo feature,
``cfg(test)`` helper, development dependency, test binary, normal/build/dev
product graph member, CLI command, configuration field, environment variable,
dynamic-loader mechanism, source patch or runtime provider-selection API.

The fixture's only permitted operation is to install Ring in its own process
and immediately call the separately reviewed, narrow provider-guard entry
point before any DAS application initialisation. "Before application
initialisation" means before argument parsing, configuration loading, logging
or runtime setup, filesystem access, task/thread creation, subprocess launch,
socket creation, network access, credential access or durable mutation. It
must assert the stable ownership-conflict outcome and capture pre/post
sentinels proving no side effect. It must not invoke operational DAS code.

The fixture cannot prove provider injection into a released binary process;
that capability is explicitly forbidden. Qualification of released binaries
therefore remains two-part: source/AST or equivalent reviewed ordering evidence
that the provider guard is the first executable statement, plus clean-process
package/executable evidence. The external fixture supplies only the guarded
same-process conflict evidence.

The Jenkins fixture must pin and retain all of the following immutable facts:

* the exact DAS source revision and source-tree digest containing the reviewed
  guard interface;
* the expected guard symbol/interface digest and stable conflict result;
* the fixture's Ring-containing lockfile and immutable build-image digest; and
* the assertion and sentinel evidence produced from that exact closure.

It must fail closed if any pinned revision, source digest, interface digest,
lockfile, image digest, ownership result or sentinel differs. The fixture may
not silently substitute a later DAS checkout, a revised dependency resolution
or an unpinned builder.

Rejected alternatives
---------------------

The following are rejected because they add a production test seam, weaken
ownership, or permit an unaudited execution path: a ``--test-provider`` or
similar CLI switch; environment or configuration selection; ``LD_PRELOAD`` or
another dynamic-loader injection; a DAS Ring development dependency; source
patching/replacement; provider reset/reflection; or a hidden runtime feature.

Acceptance criteria
-------------------

Before implementation, the project owner and the named security/API reviewers
must accept this ADR and Jenkins #160's external-fixture contract. Any later
implementation requires separate review of the exact immutable fixture,
source/interface digest, lockfile, builder image, conflict code, sentinel
schema and retained Jenkins evidence. No acceptance of this ADR alone changes
the released DASObjectStore graph or relaxes ADR-0001.
