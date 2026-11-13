import hashlib
import importlib.util
import os
from pathlib import Path
import tempfile
import unittest
from unittest import mock

SCRIPT = Path(__file__).resolve().parents[2] / 'tools' / 'sticky_journal_parent.py'
spec = importlib.util.spec_from_file_location('sticky_journal_parent', SCRIPT)
g = importlib.util.module_from_spec(spec);spec.loader.exec_module(g)

@unittest.skipUnless(os.geteuid() == 0, 'requires root-owned disposable fixture')
class StickyParentTest(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.root=Path(self.tmp.name)
        self.parent=self.root/'journal';self.parent.mkdir(mode=0o750);os.chown(self.parent,994,972);os.chmod(self.parent,0o750)
        self.state=self.root/'state';self.state.mkdir(mode=0o700)
        self.journal=self.parent/'operation.json';self.journal.write_bytes(b'old');os.chown(self.journal,994,972);os.chmod(self.journal,0o600)
        self.pfd=os.open(self.parent,os.O_RDONLY|os.O_DIRECTORY);self.sfd=os.open(self.state,os.O_RDONLY|os.O_DIRECTORY)
        p=os.fstat(self.pfd);j=self.journal.stat();self.guard=g.StickyJournalParent(self.pfd,self.sfd,(p.st_dev,p.st_ino),(j.st_dev,j.st_ino),hashlib.sha256(b'old').hexdigest())
    def tearDown(self):
        self.guard.close();os.close(self.pfd);os.close(self.sfd);self.tmp.cleanup()
    def test_acquire_and_exact_restore(self):
        self.guard.acquire();s=os.fstat(self.pfd)
        self.assertEqual((s.st_uid,s.st_gid,s.st_mode&0o7777),(0,972,0o1770))
        self.assertEqual(g._acl(self.pfd),g._FENCED_ACL)
        self.guard.restore_before_exchange();s=os.fstat(self.pfd)
        self.assertEqual((s.st_uid,s.st_gid,s.st_mode&0o7777),(994,972,0o750))
        self.assertEqual(g._acl(self.pfd),g._BASE_ACL)
    def test_changed_journal_refuses_restore(self):
        self.guard.acquire();self.journal.rename(self.parent/'old-preserved');self.journal.write_bytes(b'new')
        with self.assertRaisesRegex(RuntimeError,'journal identity differs'):
            self.guard.restore_before_exchange()
        self.assertTrue((self.state/'parent-intent.json').exists())
        # Fixture-only cleanup after assertion, restoring exact original identity.
        self.journal.unlink();(self.parent/'old-preserved').rename(self.journal)
        os.fchown(self.pfd,994,972);g._set_acl(self.pfd,g._BASE_ACL);os.fchmod(self.pfd,0o750)
        (self.state/'parent-intent.json').unlink()
    def test_failure_after_chown_restores(self):
        real=g._set_acl
        def fail_once(fd,text):
            if text == g._FENCED_ACL: raise RuntimeError('injected')
            return real(fd,text)
        with mock.patch.object(g,'_set_acl',side_effect=fail_once):
            with self.assertRaisesRegex(RuntimeError,'injected'):self.guard.acquire()
        s=os.fstat(self.pfd)
        self.assertEqual((s.st_uid,s.st_gid,s.st_mode&0o7777),(994,972,0o750))
        self.assertFalse((self.state/'parent-intent.json').exists())

if __name__=='__main__':unittest.main()
