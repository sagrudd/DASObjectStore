import importlib.util
import os
from pathlib import Path
import sqlite3
import tempfile
import unittest
from unittest import mock

spec = importlib.util.spec_from_file_location('nuc_claim_target', Path(__file__).resolve().parents[2] / 'tools/nuc_claim_target.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

class TargetBindingTests(unittest.TestCase):
    def test_exact_manager_and_process_cgroup_are_required(self):
        expected='ControlGroup='+module.DAEMON_CGROUP+'\n'
        with mock.patch.object(module.subprocess,'check_output',return_value=expected), \
             mock.patch.object(module.Path,'read_text',return_value='0::'+module.DAEMON_CGROUP+'\n'):
            module.verify_daemon_cgroup()
        with mock.patch.object(module.subprocess,'check_output',return_value='ControlGroup=/system.slice/wrong.service\n'):
            with self.assertRaises(ValueError):module.verify_daemon_cgroup()
        with mock.patch.object(module.subprocess,'check_output',return_value=expected), \
             mock.patch.object(module.Path,'read_text',return_value='0::/system.slice/wrong.service\n'):
            with self.assertRaises(ValueError):module.verify_daemon_cgroup()

    def test_stopped_actual_cgroup_and_descendants_must_be_empty(self):
        with tempfile.TemporaryDirectory() as tmp:
            parent=Path(tmp);group=parent/'service';group.mkdir();(group/'cgroup.procs').write_text('')
            module.require_stopped_cgroup_empty(group)
            child=group/'child';child.mkdir();(child/'cgroup.procs').write_text('123\n')
            with self.assertRaises(RuntimeError):module.require_stopped_cgroup_empty(group)
            (child/'cgroup.procs').write_text('');module.require_stopped_cgroup_empty(group)
            (child/'cgroup.procs').unlink()
            with self.assertRaises(RuntimeError):module.require_stopped_cgroup_empty(group)
            child.rmdir();(group/'cgroup.procs').unlink();group.rmdir()
            module.require_stopped_cgroup_empty(group)

    def test_quiet_probe_sends_complete_envelope_before_peer_denial(self):
        import socket,struct,types
        events=[]
        client=mock.MagicMock()
        client.__enter__.return_value=client
        client.sendall.side_effect=lambda raw:events.append(('sent',raw))
        client.getsockopt.side_effect=lambda *args:(events.append(('peer',None)) or struct.pack('3i',1,2,3))
        target=object.__new__(module.HeldTarget);target.daemon_stopped=False
        with mock.patch.object(module.HeldTarget,'recheck'),mock.patch.object(socket,'socket',return_value=client):
            with self.assertRaisesRegex(RuntimeError,'socket peer changed'):module.assert_daemon_quiet(target)
        self.assertEqual([item[0] for item in events],['sent','peer'])
        self.assertTrue(events[0][1].endswith(b'\n'))

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
