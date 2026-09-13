# DASObjectStore package-owned development validation

This directory is an inert package payload for Base Camp 0.127.0's
package-owned development-cohort extension. It contains the immutable
DASObjectStore 0.185.4 source manifest, its exact Prosopikon, Proxenos and
Thesaurophylax source closure, a policy, one approved task, an existing
digest-pinned CI recipe and the secret-free cohort selector.

The selector has separate vault and Jenkins credential identifiers, but no
credential value. Installation does not create either credential, configure a
webhook, start Jenkins, submit a build, retain a dossier, sign or publish a
package, or modify a DASObjectStore host.

The approved task verifies both mounted revisions and rewrites only Cargo's
Git URLs for those exact repositories to their read-only mounted checkouts.
The authenticated source credential is available only to Base Camp's checkout
containers; it does not enter the task container, its environment, the
manifest, the task catalogue, the selector, logs or artifacts. Public
dependency acquisition is allowed only during `cargo fetch`; Rust checks then
run with `CARGO_NET_OFFLINE=true`.

This is development build/test configuration, not a release, installation,
activation, availability or service-health claim. Before configuration, verify
the reviewed CI image is available for both declared architectures and use a
genuine supported package closure plus the normal attended authority gates.
