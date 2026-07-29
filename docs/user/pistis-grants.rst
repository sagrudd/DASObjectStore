Pistis ObjectStore grants
=========================

Pistis authentication proves an immutable Prosopikon principal. It does not
grant storage access by itself. DASObjectStore keeps a separate, explicit
policy record for each ``(authority_id, principal_id, object_store_id)`` tuple.
There is no wildcard or email-domain grant.

The email address accepted by the administrator command is a provisioning
selector only. The command resolves exactly one active principal in the
canonical Prosopikon SQLite authority and persists only its authority and
principal UUIDs. An absent or ambiguous email fails without changing policy.

Inspect and provision
---------------------

Keep the authority, policy, and ObjectStore registry on the same trusted host.
All paths must be absolute. First inspect the current policy revision::

   dasobjectstore pistis-grant inspect \
     --grant-registry /var/lib/dasobjectstore/pistis-grants.json

A missing registry is reported as revision ``0``. Provision the first explicit
MVP grant only after confirming the exact ObjectStore with
``dasobjectstore store list --json``::

   dasobjectstore pistis-grant grant \
     --authority /var/lib/prosopikon/authority.sqlite3 \
     --email stephen@mnemosyne.co.uk \
     --grant-registry /var/lib/dasobjectstore/pistis-grants.json \
     --store-registry /var/lib/dasobjectstore/stores.json \
     --expected-revision 0 \
     --object-store epic_collection \
     --read --write

Every mutation requires the revision returned by the preceding inspection.
The writer holds an exclusive sidecar lock, rereads the policy after acquiring
it, and publishes a private temporary file with an atomic rename and directory
sync. A stale revision or concurrent writer fails without replacing the prior
policy. Repeating a grant updates the exact tuple; it cannot create a second
active copy. The same atomic document retains an append-only logical audit
event containing the operation, revision, immutable identity tuple, exact
ObjectStore, record ID, and a unique event ID. ``inspect`` presents both policy
and audit history without credentials.

Revoke and rollback
-------------------

Revoke the exact tuple using the newly observed revision::

   dasobjectstore pistis-grant revoke \
     --authority /var/lib/prosopikon/authority.sqlite3 \
     --email stephen@mnemosyne.co.uk \
     --grant-registry /var/lib/dasobjectstore/pistis-grants.json \
     --expected-revision 1 \
     --object-store epic_collection

Before a planned change, retain a root-readable copy of the registry and its
SHA-256 digest. Rollback means stopping the Monas host, restoring the reviewed
private file atomically with its original owner and mode, inspecting it with
the supported command, and restarting Monas. Never edit the JSON in place.

Deployment boundary
-------------------

Invoke the command as the account designated to own policy; do not use root
merely for convenience. The policy file must be mode ``0600`` and exposed
read-only to the Monas process through a narrowly scoped service permission.
Configure Monas with the same grant and ObjectStore registry paths. The browser
supplies only the requested ObjectStore ID; bucket, writer group, object class,
allowed operations, and prefixes are derived from the current server-side
registries.

Approval fails closed for a missing, inactive, read-only, duplicate,
stale-revision, wrong-authority, wrong-principal, substituted, unknown, or
non-S3-exported grant. Existing standalone PAM/OS policy remains independent.

Bare-earth evaluation
---------------------

Never reuse a grant registry created for an older Prosopikon authority. The
isolated evaluation keeps its compare-and-swap policy below the same private
root as the fresh authority:

.. code-block:: console

   install -d -m 0700 "$HOME/.mnemosyne/pistis-evaluation/das"
   /usr/local/libexec/mnemosyne/pistis-evaluation/dasobjectstore \
     pistis-grant inspect \
     --grant-registry \
       "$HOME/.mnemosyne/pistis-evaluation/das/pistis-grants.json"
   /usr/local/libexec/mnemosyne/pistis-evaluation/dasobjectstore \
     pistis-grant grant \
     --authority \
       "$HOME/.mnemosyne/pistis-evaluation/authority/prosopikon.sqlite3" \
     --email stephen@mnemosyne.co.uk \
     --grant-registry \
       "$HOME/.mnemosyne/pistis-evaluation/das/pistis-grants.json" \
     --store-registry /var/lib/dasobjectstore/stores.json \
     --expected-revision 0 \
     --object-store epic_collection \
     --read --write

The first command reports revision 0 without creating a registry. Run the
grant only after personally commissioning the fresh authority and confirming
``epic_collection`` in the live ObjectStore registry. The result must contain
the new authority and principal UUIDs, revision 1, read and write permission,
and no email selector. Keep the resulting file mode ``0600``.

The evaluation's parallel TLS endpoint uses port ``3902`` and the separately
prepared development certificate. It does not modify or replace the live
port-3900 listener. This policy preparation still grants no session: Monas
requires a completed, audience-bound Pistis enrolment and authentication.
