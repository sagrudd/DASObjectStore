Local custody review assets
===========================

The 0.184.0 source line adds an in-process finite-inventory retention call,
not a new operator command or custody activation route. It uses each sealed
writer/reader handoff once for the selected finite batch. Failure preserves
partial objects and consumed handoffs; it does not authorize retry or cleanup.
No persistent reader, service lifecycle or execution companion is supplied.
See :doc:`../adr/0009-finite-inventory-custody-session` for the bounded source
contract. The review-only assets below remain inert and unchanged.

DASObjectStore 0.181.0 packages three **review-only** local trusted-
administrator custody templates below
``/usr/share/doc/dasobjectstore/custody-review/``:

- ``dasobjectstore-custody-garage.service.template``;
- ``custody-garage.compose.yml.template``; and
- ``dasobjectstored-custody-credentials.conf.template``.

They are ordinary documentation files, mode ``0644``.  The package never
copies them to ``/etc``, either systemd unit directory, a systemd drop-in
directory, a Garage directory, or a DAS state directory.  It never writes a
credential, custody activation marker, Garage configuration, catalog, bucket,
or ledger; it does not enable, start, restart, or invoke the custody service or
Docker Compose.

The templates contain placeholders only.  A custodian must not add access keys,
secrets, credential paths, or an activation marker to package configuration.
Their use is reserved for a separate, target-bound, attended transaction with
the required S4--S8 evidence and approval.  The package itself is not that
transaction and cannot establish a custody plane.

The supported assurance label remains
``local_trusted_administrator_overlay``.  The templates do not claim Garage
Object Lock, WORM, provider-enforced retention, independently administered
storage, or a regulatory custody result.  They remain subject to the local
trusted-administrator limitation described in the custody source contract.
