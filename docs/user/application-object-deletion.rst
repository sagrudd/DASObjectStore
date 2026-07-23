Application-authorized object deletion
=======================================

DASObjectStore provides a narrow deletion boundary for applications such as
Pinakotheke. The application may request removal of one exact object version;
it never receives Garage credentials and must not delete provider data
directly.

Authority and evidence
----------------------

The daemon admits a request only when all of these agree:

* an active paired session grants the named ObjectStore and Garage bucket;
* the registered application identity includes the ``delete`` operation and
  contains the exact ObjectStore, logical key prefix, and object size;
* object ID, positive version, size, SHA-256, provider, bucket, and key match
  the authoritative ``provider:garage`` catalogue row; and
* the configured Garage endpoint and daemon-managed credential registry match
  the request.

The daemon checks catalogue evidence before provider mutation. It then verifies
the current Garage object size and ``dasobjectstore-sha256`` metadata, removes
the exact key, verifies that the key is absent, withdraws only the matching
catalogue row, and writes a redacted application audit event. Changed evidence
fails before deletion. A provider object that exists without matching
catalogue evidence is rejected rather than adopted or removed.

Idempotency and recovery
------------------------

An exact retry after both provider and catalogue removal returns
``already_absent``. If the provider object is absent but the matching catalogue
row remains after an interrupted operation, the retry withdraws that stale row
and returns ``already_absent``. Pinakotheke may remove its gallery projection
only after receiving ``deleted`` or ``already_absent`` from this authority.

The initial daemon contract is
``dasobjectstore.application_object_delete.v1`` at
``/api/v1/application-auth/object-deletions``. It contains no filesystem path,
provider credential, media bytes, source URL, browser cookie, or free-form
audit reason. The application-facing helper and live synthetic deployment are
separate integration gates; until they are configured, deletion remains
unavailable and the application must keep its record visible and retryable.

Do not use ``aws s3 rm``, Garage administrator credentials, or manual SQLite
edits as a substitute. Those paths can leave provider data, capacity state,
catalogue availability, and application projections inconsistent.
