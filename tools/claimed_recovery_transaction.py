"""Catalogue transition for one authenticated DAS repair claim.

The caller supplies the transaction and owns its commit, the durable filesystem
hold, the authenticated intent and verification of journal presence.
"""
COLUMNS = ('claim_id', 'claim_kind', 'owner_id', 'request_id', 'request_digest',
           'disk_id', 'reserved_bytes', 'consumed_bytes', 'created_at_utc')


def validate_exact_claim(conn, intent, *, journal_present):
    """Validate catalogue facts without writing. Return the exact claim state."""
    if not conn.in_transaction:
        raise ValueError("claim release requires an explicit transaction")
    if set(intent['claim']) != set(COLUMNS):
        raise ValueError('claim binding incomplete')
    claim = intent['claim']
    if any(type(claim[k]) is not str or not claim[k] for k in COLUMNS if k not in ('reserved_bytes', 'consumed_bytes')):
        raise ValueError('invalid claim text')
    if type(claim['reserved_bytes']) is not int or claim['reserved_bytes'] <= 0 or type(claim['consumed_bytes']) is not int or claim['consumed_bytes'] != 0 or claim['claim_kind'] != 'repair':
        raise ValueError('invalid claim quantities')
    claim = intent['claim']
    candidate = intent['candidate']
    keys = {'placement_id','object_id','store_id','disk_id','disk_state','relative_path','size_bytes','content_hash','verified_at_utc','existing_disk_ids'}
    if (set(candidate) != keys or type(candidate['size_bytes']) is not int or candidate['size_bytes'] <= 0
            or any(type(candidate[k]) is not str or not candidate[k] for k in ('placement_id','object_id','store_id','disk_id','disk_state','relative_path','content_hash'))
            or len(candidate['content_hash']) != 64 or any(ch not in '0123456789abcdef' for ch in candidate['content_hash'])
            or (candidate['verified_at_utc'] is not None and type(candidate['verified_at_utc']) is not str)
            or type(candidate['existing_disk_ids']) is not list
            or any(type(x) is not str or not x for x in candidate['existing_disk_ids'])
            or len(set(candidate['existing_disk_ids'])) != len(candidate['existing_disk_ids'])
            or candidate['disk_id'] not in candidate['existing_disk_ids']
            or claim['disk_id'] in candidate['existing_disk_ids']):
        raise ValueError('candidate binding differs')
    if claim['reserved_bytes'] != candidate['size_bytes'] or candidate['content_hash'] != intent['source_sha256']:
        raise ValueError('claim or content size differs')
    obj = conn.execute('SELECT store_id,state,size_bytes,content_hash FROM objects WHERE object_id=?', (candidate['object_id'],)).fetchall()
    if obj != [(candidate['store_id'],'HddCopyVerified',candidate['size_bytes'],candidate['content_hash'])]:
        raise ValueError('catalogue object differs')
    source = conn.execute('SELECT object_id,disk_id,relative_path,content_hash FROM placements WHERE placement_id=?', (candidate['placement_id'],)).fetchall()
    if source != [(candidate['object_id'],candidate['disk_id'],candidate['relative_path'],candidate['content_hash'])]:
        raise ValueError('source placement differs')
    if conn.execute('SELECT COUNT(*) FROM placements WHERE disk_id=? AND relative_path=?', (claim['disk_id'],candidate['relative_path'])).fetchone()[0] != 0:
        raise ValueError('destination placement exists')
    if conn.execute('SELECT COUNT(*) FROM destage_queue WHERE object_id=?', (candidate['object_id'],)).fetchone()[0] != 0:
        raise ValueError('object has destage work')
    # The immediate transaction acquired by the caller keeps this row set
    # stable until that caller commits or rolls back.
    rows = conn.execute('SELECT ' + ','.join(COLUMNS) + ',state,released_at_utc,lease_owner,lease_expires_at_utc,updated_at_utc FROM disk_capacity_claims WHERE claim_kind=? AND owner_id=?', (claim['claim_kind'], claim['owner_id'])).fetchall()
    exact = [row for row in rows if tuple(row[:len(COLUMNS)]) == tuple(claim[k] for k in COLUMNS)]
    if any((row[-5] == 'active' and row[-4] is not None) or (row[-5] == 'released' and row[-4] is None) for row in rows):
        raise ValueError('inconsistent claim state')
    active = [row for row in rows if row[-5] == 'active' and row[-4] is None]
    if len(exact) != 1 or any(row is not exact[0] for row in active):
        raise ValueError('ambiguous claim')
    row = exact[0]
    if row[-5:-3] == ('active', None):
        if row[-1] != intent['original_updated_at']:
            raise ValueError('claim timestamp changed')
        if list(row[-3:-1]) != intent['original_lease']:
            raise ValueError('lease changed')
        if not journal_present or len(active) != 1:
            raise ValueError('active claim without exact journal')
        return 'active'
    elif row[-5:] != ('released', intent['release_time'], None, None, intent['release_time']) or active:
        raise ValueError('unexpected released claim')
    return 'released'


def release_exact_claim(conn, intent, *, journal_present):
    """Revalidate immediately before the caller commits the bound transition."""
    if validate_exact_claim(conn, intent, journal_present=journal_present) == 'active':
        claim = intent['claim']
        # Match the entire identity, not owner alone.  The fixed transition
        # timestamp is part of the immutable intent, making retries exact.
        conn.execute("UPDATE disk_capacity_claims SET state='released', released_at_utc=?, updated_at_utc=?, lease_owner=NULL, lease_expires_at_utc=NULL WHERE claim_id=? AND state='active' AND released_at_utc IS NULL", (intent['release_time'], intent['release_time'], claim['claim_id']))
        if conn.execute('SELECT changes()').fetchone()[0] != 1:
            raise ValueError('claim changed')
