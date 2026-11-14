"""Stopped recovery hashes must retain deadlines without per-chunk full probes."""
import hashlib
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'tools'))
import nuc_recovery_restore as restore


class RestoreHashCadenceTests(unittest.TestCase):
    def test_both_payloads_recheck_full_custody_at_boundaries_and_every_ten_seconds(self):
        now = [100_000_000_000]
        full = []
        def guard():
            full.append(now[0])
        def chunks(fd, check):
            for _ in range(5):
                now[0] += 5_000_000_000
                check()
            return 'digest-' + str(fd)
        with mock.patch('time.monotonic_ns', side_effect=lambda: now[0]), mock.patch('integrated_claim_recovery._hash_fd', side_effect=chunks):
            self.assertEqual(restore._hash_restoration_payload(1, guard, 200_000_000_000), 'digest-1')
            self.assertEqual(restore._hash_restoration_payload(2, guard, 200_000_000_000), 'digest-2')
        self.assertEqual(full[0], 100_000_000_000)
        self.assertEqual(full[-1], 150_000_000_000)
        self.assertLessEqual(max(b-a for a,b in zip(full,full[1:])), 10_000_000_000)
        self.assertLess(len(full), 12)  # not one expensive full check per chunk

    def test_original_deadline_is_checked_on_every_chunk(self):
        now = [100_000_000_000]
        reached = []
        def chunks(fd, check):
            for _ in range(5):
                now[0] += 3_000_000_000
                check()
                reached.append(now[0])
            return 'must not finish'
        with mock.patch('time.monotonic_ns', side_effect=lambda: now[0]), mock.patch('integrated_claim_recovery._hash_fd', side_effect=chunks):
            with self.assertRaisesRegex(RuntimeError, 'deadline expired'):
                restore._hash_restoration_payload(1, lambda: None, 107_000_000_000)
        self.assertEqual(reached, [103_000_000_000, 106_000_000_000])

    def test_full_custody_denial_stops_the_hash(self):
        now = [100_000_000_000]
        checks = []
        def guard():
            checks.append(now[0])
            if len(checks) == 3:
                raise RuntimeError('custody changed')
        def chunks(fd, check):
            for _ in range(6):
                now[0] += 5_000_000_000
                check()
            return 'must not finish'
        with mock.patch('time.monotonic_ns', side_effect=lambda: now[0]), mock.patch('integrated_claim_recovery._hash_fd', side_effect=chunks):
            with self.assertRaisesRegex(RuntimeError, 'custody changed'):
                restore._hash_restoration_payload(1, guard, 200_000_000_000)
        self.assertEqual(checks, [100_000_000_000,105_000_000_000,115_000_000_000])

    def test_actual_hash_bytes_are_unchanged(self):
        with tempfile.TemporaryFile() as file:
            data = b'fixed-recovery-payload' * 500
            file.write(data);file.flush()
            self.assertEqual(restore._hash_restoration_payload(file.fileno(), lambda: None, 2**63-1), hashlib.sha256(data).hexdigest())

if __name__ == '__main__':
    unittest.main()
