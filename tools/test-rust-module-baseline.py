#!/usr/bin/env python3
"""Exercise actual baseline decisions against isolated synthetic module bytes."""
import copy
import datetime
import hashlib
import importlib.util
import json
import os
import tempfile
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location('baseline', Path(__file__).with_name('check-rust-module-baseline.py'))
baseline = importlib.util.module_from_spec(spec)
spec.loader.exec_module(baseline)


class BaselineTests(unittest.TestCase):
    def test_exact_and_denials(self):
        before = Path.cwd()
        with tempfile.TemporaryDirectory() as directory:
            try:
                os.chdir(directory)
                path = Path('crates/example/src/lib.rs')
                path.parent.mkdir(parents=True)
                path.write_text('inherited\n')
                record = dict(path=str(path), sha256=hashlib.sha256(path.read_bytes()).hexdigest(),
                              max_production_lines=1001, expires_before_utc='2026-10-31',
                              owner='existing owner', remediation='split responsibility')
                document = dict(schema_version=1, records=[record])
                today = datetime.date(2026, 10, 24)
                self.assertTrue(baseline.accepted(str(path), 1001, document, today))
                self.assertFalse(baseline.accepted(str(path), 1002, document, today))
                self.assertFalse(baseline.accepted(str(path), 1001, document, datetime.date(2026, 10, 31)))
                self.assertFalse(baseline.accepted('crates/new/src/lib.rs', 1001, document, today))
                path.write_text('changed!!\n')
                self.assertFalse(baseline.accepted(str(path), 1001, document, today))
                path.unlink()
                with self.assertRaises(OSError):
                    baseline.accepted(str(path), 1001, document, today)
                for key, value in [('sha256', 'bad'), ('max_production_lines', 2000.0),
                                   ('expires_before_utc', 'bad'), ('owner', '')]:
                    changed = copy.deepcopy(document)
                    changed['records'][0][key] = value
                    try:
                        result = baseline.accepted(str(path), 1001, changed, today)
                    except ValueError:
                        result = False
                    self.assertFalse(result)
                for malformed in [[], None, dict(schema_version=1, records='bad'),
                                  dict(schema_version=1, records=[None])]:
                    self.assertFalse(baseline.accepted(str(path), 1001, malformed, today))
                self.assertFalse(baseline.accepted(str(path), 1001,
                                                   dict(schema_version=True, records=[record]), today))
                for raw in ['{"schema_version":1,"schema_version":1}',
                            '{"records":[{"sha256":"a","sha256":"b"}]}']:
                    with self.assertRaises(ValueError):
                        json.loads(raw, object_pairs_hook=baseline.unique_object)
                duplicate = copy.deepcopy(document)
                duplicate['records'].append(record)
                self.assertFalse(baseline.accepted(str(path), 1001, duplicate, today))
            finally:
                os.chdir(before)


if __name__ == '__main__':
    unittest.main()
