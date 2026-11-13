import hashlib
import importlib.util
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[2] / 'tools' / 'claimed_recovery_fixture.py'
spec = importlib.util.spec_from_file_location('claimed_recovery_fixture', SCRIPT)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

class RecoveryFixtureTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        (self.root / '.disposable-recovery-fixture').write_text('fixture-only\n')
        for name, data in [('backup-source', b'complete'), ('backup-partial', b'partial'), ('quarantine', b'partial'), ('operation.json', json.dumps({'schema': 'dasobjectstore.storage_assurance.operation.v1', 'phase': 'claimed', 'action': 'rotate', 'operation_id': 'op-1', 'claim_owner': 'owner', 'destination_disk_id': 'disk'}).encode())]:
            (self.root / name).write_bytes(data)
        q, j = (self.root / 'quarantine').stat(), (self.root / 'operation.json').stat()
        self.claim = dict(zip(module.COLUMNS, ('claim-1', 'repair', 'owner', 'request', 'digest', 'disk', 100, 0, '2026-01-01T00:00:00Z')))
        receipt = [{'name': 'source', 'size': 8, 'sha256': hashlib.sha256(b'complete').hexdigest(), 'device': 1, 'inode': 2}, {'name': 'partial', 'size': 7, 'sha256': hashlib.sha256(b'partial').hexdigest(), 'device': q.st_dev, 'inode': q.st_ino}]
        (self.root / 'receipt.json').write_text(json.dumps(receipt))
        self.intent = {'receipt': receipt, 'receipt_sha256': module.digest(self.root / 'receipt.json'), 'claim': self.claim, 'source_sha256': hashlib.sha256(b'complete').hexdigest(), 'partial_sha256': hashlib.sha256(b'partial').hexdigest(), 'operation_id': 'op-1', 'journal_sha256': module.digest(self.root / 'operation.json'), 'journal_identity': [j.st_dev, j.st_ino], 'quarantine_identity': [q.st_dev, q.st_ino], 'original_updated_at': '2026-01-01T00:00:00Z', 'original_lease': [None, None], 'release_time': '2026-01-02T00:00:00Z'}
        (self.root / 'intent.json').write_text(json.dumps(self.intent))
        with sqlite3.connect(self.root / 'db.sqlite') as db:
            schema = (SCRIPT.parents[1] / 'crates/dasobjectstore-metadata/src/schema.rs').read_text().split('pub const LIVE_SCHEMA_SQL: &str = r#"', 1)[1].split('"#;', 1)[0]
            db.executescript(schema)
            db.execute('PRAGMA foreign_keys=OFF')
            db.execute('INSERT INTO disk_capacity_claims (' + ','.join(module.COLUMNS) + ',state,released_at_utc,updated_at_utc,lease_owner,lease_expires_at_utc) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?)', tuple(self.claim[k] for k in module.COLUMNS) + ('active', None, '2026-01-01T00:00:00Z', None, None))
    def tearDown(self): self.tmp.cleanup()
    def state(self):
        with sqlite3.connect(self.root / 'db.sqlite') as db:
            return db.execute('SELECT state,released_at_utc FROM disk_capacity_claims WHERE claim_id=?', ('claim-1',)).fetchone()
    def test_complete_and_retry(self):
        self.assertEqual(module.run(self.root), 'complete')
        self.assertEqual(module.run(self.root), 'complete')
        self.assertEqual(self.state(), ('released', self.intent['release_time']))
        self.assertFalse((self.root / 'operation.json').exists())
    def test_crash_after_claim_and_after_unlink(self):
        for point in ('before_commit', 'claim', 'unlink'):
            with self.subTest(point=point):
                if point != 'before_commit':
                    self.tearDown(); self.setUp()
                p = subprocess.run([sys.executable, str(SCRIPT), str(self.root), '--test-interrupt', point])
                self.assertEqual(p.returncode, {'before_commit': 89, 'claim': 90, 'unlink': 91}[point])
                self.assertEqual(module.run(self.root), 'complete')
                self.assertEqual(self.state(), ('released', self.intent['release_time']))
    def test_extra_active_or_wrong_journal_denied(self):
        with sqlite3.connect(self.root / 'db.sqlite') as db:
            db.execute("INSERT INTO disk_capacity_claims (claim_id,claim_kind,owner_id,request_id,request_digest,disk_id,reserved_bytes,consumed_bytes,created_at_utc,state,released_at_utc,updated_at_utc,lease_owner,lease_expires_at_utc) SELECT 'claim-2',claim_kind,owner_id,request_id,request_digest,'other-disk',reserved_bytes,consumed_bytes,created_at_utc,state,released_at_utc,updated_at_utc,lease_owner,lease_expires_at_utc FROM disk_capacity_claims")
        with self.assertRaises(ValueError): module.run(self.root)
        self.assertEqual(self.state(), ('active', None))
    def test_wrong_journal_denied(self):
        (self.root / 'operation.json').write_text('{}')
        with self.assertRaises(ValueError): module.run(self.root)
        self.assertEqual(self.state(), ('active', None))
    def test_inconsistent_other_owner_row_denied(self):
        with sqlite3.connect(self.root / 'db.sqlite') as db:
            db.execute("INSERT INTO disk_capacity_claims (claim_id,claim_kind,owner_id,request_id,request_digest,disk_id,reserved_bytes,consumed_bytes,created_at_utc,state,released_at_utc,updated_at_utc) SELECT 'claim-2',claim_kind,owner_id,request_id,request_digest,'other-disk',reserved_bytes,consumed_bytes,created_at_utc,'active','unexpected',updated_at_utc FROM disk_capacity_claims")
        with self.assertRaises(ValueError): module.run(self.root)
        self.assertEqual(self.state(), ('active', None))
    def test_changed_active_timestamp_denied(self):
        with sqlite3.connect(self.root / 'db.sqlite') as db:
            db.execute("UPDATE disk_capacity_claims SET updated_at_utc='changed'")
        with self.assertRaises(ValueError): module.run(self.root)
        self.assertEqual(self.state(), ('active', None))
    def test_released_timestamp_mismatch_denied(self):
        with sqlite3.connect(self.root / 'db.sqlite') as db:
            db.execute("UPDATE disk_capacity_claims SET state='released',released_at_utc='other',updated_at_utc='other'")
        with self.assertRaises(ValueError): module.run(self.root)
        self.assertTrue((self.root / 'operation.json').exists())
    def test_absent_journal_and_changed_quarantine_denied(self):
        (self.root / 'operation.json').unlink()
        with self.assertRaises(ValueError): module.run(self.root)
        self.assertEqual(self.state(), ('active', None))
        (self.root / 'quarantine').write_bytes(b'changed')
        with self.assertRaises(ValueError): module.run(self.root)

if __name__ == '__main__': unittest.main()
