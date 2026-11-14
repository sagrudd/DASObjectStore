import importlib.util
import os
from pathlib import Path
import sqlite3
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('nuc_claim_target', Path(__file__).resolve().parents[2] / 'tools/nuc_claim_target.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

class TargetBindingTests(unittest.TestCase):
    def test_held_path_rejects_parent_rename_and_replacement(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp); parent = root / 'original'; parent.mkdir()
            (parent / 'data').write_bytes(b'example')
            held = module.HeldPath(str(parent / 'data'))
            try:
                held.recheck()
                parent.rename(root / 'moved'); parent.mkdir()
                (parent / 'data').write_bytes(b'example')
                with self.assertRaises(RuntimeError): held.recheck()
            finally: held.close()

    def test_held_path_rejects_symlink(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp); (root/'real').mkdir(); (root/'link').symlink_to(root/'real')
            with self.assertRaises(OSError): module.HeldPath(str(root/'link'), directory=True)

    def test_sqlite_fd_and_wal_use_held_parent(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp); path=root/'live.sqlite'
            db=sqlite3.connect(path); db.execute('CREATE TABLE t (x)'); db.commit(); db.close()
            parent=module.HeldPath(str(root),directory=True)
            held=module.HeldPath(str(path))
            try:
                original=os.fstat(held.fds[-1]); before=module.HeldTarget._db_fds(original)
                db=sqlite3.connect(f'file:/proc/self/fd/{parent.fds[-1]}/live.sqlite?mode=rw',uri=True,isolation_level=None)
                try:
                    added=module.HeldTarget._db_fds(original)-before
                    self.assertEqual(len(added),1)
                    db.execute('PRAGMA journal_mode=WAL'); db.execute('BEGIN IMMEDIATE'); db.execute('INSERT INTO t VALUES (1)')
                    self.assertTrue((root/'live.sqlite-wal').is_file()); parent.recheck(); held.recheck(); db.commit()
                    self.assertEqual(db.execute('SELECT x FROM t').fetchone(),(1,))
                finally: db.close()
            finally: held.close(); parent.close()

if __name__=='__main__': unittest.main()
