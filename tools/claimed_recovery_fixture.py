#!/usr/bin/env python3
"""Disposable-fixture model of cb68 claimed recovery. Never operates on an appliance."""
import argparse
from contextlib import ExitStack
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import stat

from claimed_recovery_transaction import COLUMNS, release_exact_claim


def digest(path):
    h = hashlib.sha256()
    with path.open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            h.update(chunk)
    return h.hexdigest()


def checked_regular(path):
    st = path.lstat()
    if not stat.S_ISREG(st.st_mode) or st.st_nlink != 1:
        raise ValueError('expected single-link regular file')
    return st


def pin_inputs(root, stack):
    pinned = {}
    for name in ('backup-source', 'backup-partial', 'quarantine'):
        fd = os.open(root / name, os.O_RDONLY | os.O_NOFOLLOW)
        stack.callback(os.close, fd)
        before = os.fstat(fd)
        if not stat.S_ISREG(before.st_mode) or before.st_nlink != 1:
            raise ValueError('invalid preserved file')
        h = hashlib.sha256()
        while True:
            data = os.read(fd, 1024 * 1024)
            if not data:
                break
            h.update(data)
        after = os.fstat(fd)
        if fingerprint(before) != fingerprint(after):
            raise ValueError('file changed during verification')
        pinned[name] = (fd, fingerprint(after), h.hexdigest())
    return pinned


def fingerprint(st):
    return (st.st_dev, st.st_ino, st.st_size, st.st_mode, st.st_nlink,
            st.st_uid, st.st_gid, st.st_mtime_ns, st.st_ctime_ns)


def check_inputs(root, intent, pinned):
    if root.is_symlink() or not root.is_dir() or (root / '.disposable-recovery-fixture').read_text() != 'fixture-only\n':
        raise ValueError('not a disposable fixture')
    # Fixed child names only. No caller-supplied appliance paths are accepted.
    for name in ('db.sqlite', 'intent.json', 'receipt.json', 'backup-source', 'backup-partial', 'quarantine'):
        checked_regular(root / name)
    for name, (fd, expected, _) in pinned.items():
        if fingerprint(os.fstat(fd)) != expected or fingerprint(checked_regular(root / name)) != expected:
            raise ValueError('pinned input changed')
    if digest(root / 'receipt.json') != intent['receipt_sha256']:
        raise ValueError('receipt mismatch')
    receipt = json.loads((root / 'receipt.json').read_text())
    if receipt != intent['receipt'] or len(receipt) != 2 or {r['name'] for r in receipt} != {'source', 'partial'}:
        raise ValueError('receipt binding mismatch')
    for row in receipt:
        if set(row) != {'name', 'size', 'sha256', 'device', 'inode'} or type(row['size']) is not int or row['size'] < 0 or type(row['device']) is not int or type(row['inode']) is not int:
            raise ValueError('invalid receipt row')
        backup = root / ('backup-' + row['name'])
        if backup.stat().st_size != row['size'] or pinned['backup-' + row['name']][2] != row['sha256']:
            raise ValueError('backup differs from receipt')
    source = next(row for row in receipt if row['name'] == 'source')
    if source['sha256'] != intent['source_sha256']:
        raise ValueError('source receipt hash mismatch')
    partial = next(row for row in receipt if row['name'] == 'partial')
    if [partial['device'], partial['inode']] != intent['quarantine_identity'] or partial['sha256'] != intent['partial_sha256']:
        raise ValueError('partial receipt identity mismatch')
    if (root / 'destination').exists() or (root / 'destination').is_symlink():
        raise ValueError('destination exists')
    for name, key in (('backup-source', 'source_sha256'), ('backup-partial', 'partial_sha256'), ('quarantine', 'partial_sha256')):
        if pinned[name][2] != intent[key]:
            raise ValueError('preservation mismatch')
    q = (root / 'quarantine').stat()
    if (q.st_dev, q.st_ino) != tuple(intent['quarantine_identity']):
        raise ValueError('quarantine identity mismatch')
    journal = root / 'operation.json'
    if journal.exists() or journal.is_symlink():
        st = checked_regular(journal)
        if (st.st_dev, st.st_ino) != tuple(intent['journal_identity']) or digest(journal) != intent['journal_sha256']:
            raise ValueError('journal mismatch')
        operation = json.loads(journal.read_text())
        if operation.get('schema') != 'dasobjectstore.storage_assurance.operation.v1' or operation.get('phase') != 'claimed' or operation.get('action') != 'rotate' or operation.get('operation_id') != intent['operation_id'] or operation.get('claim_owner') != intent['claim']['owner_id'] or operation.get('destination_disk_id') != intent['claim']['disk_id'] or operation.get('destination_relative_path') != intent['candidate']['relative_path'] or operation.get('candidate') != intent['candidate']:
            raise ValueError('journal operation mismatch')
        return True
    return False


def run(root, stop_after=None):
    if os.geteuid() == 0:
        raise ValueError('fixture runner refuses root')
    root = Path(root).resolve(strict=True)
    with ExitStack() as stack:
        pinned = pin_inputs(root, stack)
        return run_pinned(root, pinned, stop_after)


def run_pinned(root, pinned, stop_after):
    intent = json.loads((root / 'intent.json').read_text())
    journal_present = check_inputs(root, intent, pinned)
    conn = sqlite3.connect(root / 'db.sqlite', timeout=5, isolation_level=None)
    try:
        conn.execute('BEGIN IMMEDIATE')
        release_exact_claim(conn, intent, journal_present=journal_present)
        if stop_after == 'before_commit':
            os._exit(89)
        conn.commit()
    except BaseException:
        conn.rollback()
        raise
    finally:
        conn.close()
    if stop_after == 'claim':
        os._exit(90)
    if journal_present:
        # Recheck exact inode and bytes after committing; a changed name is denied.
        check_inputs(root, intent, pinned)
        os.unlink(root / 'operation.json')
    if stop_after == 'unlink':
        os._exit(91)
    fd = os.open(root, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)
    return 'complete'


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('fixture_directory')
    parser.add_argument('--test-interrupt', choices=('before_commit', 'claim', 'unlink'))
    args = parser.parse_args()
    print(run(args.fixture_directory, args.test_interrupt))
