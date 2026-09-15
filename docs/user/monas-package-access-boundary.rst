Monas package access boundary
=============================

The DAS package owns the producer half of the fixed
``mnemosyne-pistis-das`` boundary consumed by the integrated Monas runtime.
Only ``/var/lib/dasobjectstore/stores.json`` (0640), the stable non-secret
``/var/lib/dasobjectstore/appliance-identity.json`` (0640), and the live
``/run/dasobjectstore/dasobjectstored.sock`` (0660) are group-projected; their
parent directories are 0750. DAS remains the sole owner and writer.

Service startup refuses missing registries, substituted paths, unsafe metadata
or stale socket inodes. The stable appliance identity is published after its
create-once write and is reconciled during package upgrades. The socket is
published only after it is listening and is retired only after it is no longer
listening. A Debian package upgrade does not enable or start the data plane. It
restarts only already-running DAS application services after package
configuration, so they execute the newly installed bytes while retaining the
existing configuration, credentials and storage data.

Credential and local-auth state, storage content and private TLS material must
not use the shared group and must grant no access to other users. The package
does not delete ``auth/users.json``. Its governed retirement remains blocked on
Prosopikon issue #59 and Monas issue #273.
