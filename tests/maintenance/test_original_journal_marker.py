import errno,hashlib,importlib.util,os,pathlib,tempfile,unittest
from unittest import mock
import sys
sys.path.insert(0,str(pathlib.Path(__file__).resolve().parents[2]/'tools'))
import sticky_journal_parent as parent
import housekeeping_generation_observer as observer
import original_journal_marker as marker

@unittest.skipUnless(os.geteuid()==0, 'requires root disposable fixture')
class OriginalMarkerTest(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.root=pathlib.Path(self.tmp.name)
        self.p=self.root/'owner';self.p.mkdir();os.chown(self.p,994,972);os.chmod(self.p,0o750)
        self.q=self.root/'state';self.q.mkdir(mode=0o700)
        self.j=self.p/'operation.json';self.j.write_bytes(b'old journal');os.chown(self.j,994,972);os.chmod(self.j,0o600)
        self.pfd=os.open(self.p,os.O_RDONLY|os.O_DIRECTORY);self.qfd=os.open(self.q,os.O_RDONLY|os.O_DIRECTORY);p=os.fstat(self.pfd);j=self.j.stat()
        self.guard=parent.StickyJournalParent(self.pfd,self.qfd,(p.st_dev,p.st_ino),(j.st_dev,j.st_ino),hashlib.sha256(b'old journal').hexdigest());self.guard.acquire()
        self.obs=observer.ReportGenerationObserver(self.p)
    def tearDown(self):
        self.obs.close();self.guard.close();os.close(self.pfd);os.close(self.qfd);self.tmp.cleanup()
    def test_cross_process_lease_break_denies_before_promotion(self):
        hold=marker.OriginalJournalMarker(self.guard,self.obs)
        real=marker.fcntl.fcntl
        def trigger(fd,cmd,arg=0):
            result=real(fd,cmd,arg)
            if cmd==marker.fcntl.F_SETLEASE and arg==marker.fcntl.F_RDLCK:
                child=os.fork()
                if child==0:
                    try:os.open(self.j,os.O_WRONLY|os.O_NONBLOCK);os._exit(2)
                    except OSError as exc:os._exit(0 if exc.errno in (errno.EAGAIN,errno.EACCES) else 3)
                _,status=os.waitpid(child,0)
                if os.waitstatus_to_exitcode(status)!=0:raise RuntimeError('writer did not conflict')
            return result
        try:
            with mock.patch.object(marker.fcntl,'fcntl',side_effect=trigger):
                with self.assertRaisesRegex(RuntimeError,'lease lost'):
                    hold.install()
        finally:hold.close()
        self.assertEqual(self.j.read_bytes(),b'old journal')
        self.assertEqual(self.j.stat().st_uid,994)
        (self.q/'original-marker-intent.json').unlink()
        self.guard.restore_before_exchange()

if __name__=='__main__':unittest.main()
