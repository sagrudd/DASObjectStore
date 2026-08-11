Monas package access boundary
=============================

The DAS package owns the producer half of the fixed
``mnemosyne-pistis-das`` boundary consumed by Monas 0.84.0. Only
``/var/lib/dasobjectstore/stores.json`` (0640) and the live
``/run/dasobjectstore/dasobjectstored.sock`` (0660) are group-projected; their
parent directories are 0750. DAS remains the sole owner and writer.

Service startup refuses missing registries, substituted paths, unsafe metadata
or stale socket inodes. The socket is published only after it is listening and
is retired only after it is no longer listening. Package upgrades do not start
or restart the data plane.

Credential and local-auth state, storage content and private TLS material must
not use the shared group and must grant no access to other users. The package
does not delete ``auth/users.json``. Its governed retirement remains blocked on
Prosopikon issue #59 and Monas issue #273.
