import importlib
import pathlib
import sys
import types
import unittest
from unittest import mock

sys.path.insert(0,str(pathlib.Path(__file__).resolve().parents[2]/'tools'))
sys.modules.setdefault('das_unit_start_fence',types.SimpleNamespace(DasClientStartFence=type('DasClientStartFence',(),{})))
runner=importlib.import_module('nuc_claim_run')

class RecoveryDeadlineTest(unittest.TestCase):
    def test_full_authorization_is_throttled_and_deadline_is_always_checked(self):
        clock=[1_000_000_000];calls=[]
        with mock.patch.object(runner.time,'monotonic_ns',side_effect=lambda:clock[0]):
            check=runner._authorization_guard(60_000_000_000,lambda:calls.append(clock[0]))
            check(force=True);check();clock[0]=10_999_999_999;check()
            self.assertEqual(len(calls),1)
            clock[0]=11_000_000_000;check();self.assertEqual(len(calls),2)
            check(force=True);self.assertEqual(len(calls),3)
            clock[0]=60_000_000_000
            with self.assertRaisesRegex(RuntimeError,'deadline expired'):check()

    def test_budget_never_outlives_signed_deadline_or_reserve(self):
        with mock.patch.object(runner.time,'monotonic_ns',return_value=1_000_000_000):
            self.assertEqual(runner._observation_budget(3_601_000_000_000),2700)
            self.assertEqual(runner._observation_budget(1_201_000_000_000),300)
            with self.assertRaisesRegex(RuntimeError,'insufficient signed time'):
                runner._observation_budget(1_000_000_000_000)

if __name__=='__main__':unittest.main()
