import importlib.util
import ctypes
import os
from pathlib import Path
import tempfile
import threading
import time
import unittest

SCRIPT = Path(__file__).resolve().parents[2] / 'tools' / 'housekeeping_generation_observer.py'
spec = importlib.util.spec_from_file_location('report_observer', SCRIPT)
module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)

class ObserverTest(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory(); self.root=Path(self.tmp.name)
        (self.root/'latest.json').write_text('before')
        (self.root/'operation.json').write_text('old')
        self.private=self.root/'private';self.private.mkdir();(self.private/'marker').write_text('marker')
        self.obs=module.ReportGenerationObserver(self.root)
    def tearDown(self): self.obs.close(); self.tmp.cleanup()
    def replace(self, text):
        p=self.root/'temporary';p.write_text(text);os.replace(p,self.root/'latest.json')
    def exchange(self):
        self.obs.begin_exchange()
        lib=ctypes.CDLL(None,use_errno=True)
        a=os.open(self.root,os.O_RDONLY|os.O_DIRECTORY);b=os.open(self.private,os.O_RDONLY|os.O_DIRECTORY)
        try:
            if lib.renameat2(a,b'operation.json',b,b'marker',2): raise OSError(ctypes.get_errno(),'exchange')
        finally: os.close(a);os.close(b)
        self.obs.finish_exchange()
    def test_discards_prebarrier_and_observes_two_distinct_inodes(self):
        self.replace('prebarrier')
        self.exchange()
        def writer():
            time.sleep(.1);self.replace('generation one')
            time.sleep(.2);self.replace('generation two')
        thread=threading.Thread(target=writer);thread.start()
        found=self.obs.wait_two(3);thread.join()
        self.assertEqual(len(found),2)
        self.assertNotEqual(found[0]['identity'],found[1]['identity'])
    def test_prebarrier_event_cannot_count(self):
        self.replace('old');self.exchange()
        with self.assertRaises(TimeoutError):self.obs.wait_two(.05)
    def test_single_event_cannot_count_twice(self):
        self.exchange()
        thread=threading.Thread(target=lambda:(time.sleep(.05),self.replace('only one')))
        thread.start()
        with self.assertRaises(TimeoutError):self.obs.wait_two(.2)
        thread.join()
    def test_post_exchange_journal_mutation_denied(self):
        self.exchange()
        (self.root/'operation.json').write_text('changed')
        with self.assertRaisesRegex(RuntimeError,'journal changed'):
            self.obs.assert_intact()
    def test_symlink_report_denied(self):
        self.exchange()
        def writer():
            time.sleep(.05); (self.root/'bad').symlink_to('/etc/passwd');os.replace(self.root/'bad',self.root/'latest.json')
        thread=threading.Thread(target=writer);thread.start()
        with self.assertRaises(OSError):self.obs.wait_two(1)
        thread.join()

if __name__=='__main__':unittest.main()
