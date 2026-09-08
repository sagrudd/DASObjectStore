#!/usr/bin/python3
"""Pure transaction guard regressions, not an executed RPM transaction."""
import copy
import importlib.util
import json
import pathlib
import unittest

spec = importlib.util.spec_from_file_location('guard', pathlib.Path(__file__).with_name('aws-transaction-guard.py'))
guard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guard)


class GuardTests(unittest.TestCase):
    def setUp(self):
        self.filename = 'awscli2-2.33.0-1.fc44.noarch.rpm'
        self.nevra = 'awscli2-2.33.0-1.fc44.noarch'
        self.packages = {self.filename: (self.nevra, 'awscli2\t0\t2.33.0\t1.fc44\tnoarch', 'awscli2')}
        self.item = {'nevra': self.nevra, 'action': 'Install', 'reason': 'User',
            'repo_id': '@stored_transaction(das-vm-aws)', 'package_path': './packages/' + self.filename}
        self.plan = {'version': '1.0', 'rpms': [self.item]}

    def test_new_install(self):
        self.assertEqual(len(guard.validate(self.plan, {'systemd'}, self.packages)), 1)

    def test_every_non_install_action_denied(self):
        for action in ['Upgrade', 'Downgrade', 'Remove', 'Reinstall', 'Reason Change', 'Obsoleted', 'install']:
            plan = copy.deepcopy(self.plan)
            plan['rpms'][0]['action'] = action
            with self.assertRaises(ValueError):
                guard.validate(plan, set(), self.packages)

    def test_existing_name_and_duplicate_install_denied(self):
        with self.assertRaises(ValueError):
            guard.validate(self.plan, {'awscli2'}, self.packages)
        self.plan['rpms'].append(self.item.copy())
        with self.assertRaises(ValueError):
            guard.validate(self.plan, set(), self.packages)

    def test_unknown_format_fields_and_duplicate_json_denied(self):
        for name in ['groups', 'environments', 'unexpected']:
            plan = dict(self.plan, **{name: []})
            with self.assertRaises(ValueError):
                guard.validate(plan, set(), self.packages)
        with self.assertRaises(ValueError):
            json.loads('{"version":"1.0","version":"1.0"}', object_pairs_hook=guard.closed_pairs)
        self.plan['version'] = '1.1'
        with self.assertRaises(ValueError):
            guard.validate(self.plan, set(), self.packages)

    def test_foreign_payload_repo_nevra_and_path_denied(self):
        for field, value in [('nevra', 'awscli2-99-1.fc44.noarch'), ('repo_id', 'fedora'),
            ('package_path', '/tmp/' + self.filename), ('package_path', './packages/../' + self.filename),
            ('package_path', './packages/other.rpm'), ('reason', 'Unknown')]:
            plan = copy.deepcopy(self.plan)
            plan['rpms'][0][field] = value
            with self.assertRaises(ValueError):
                guard.validate(plan, set(), self.packages)

    def test_empty_oversized_and_missing_aws_denied(self):
        for rpms in [[], [self.item] * 72]:
            with self.assertRaises(ValueError):
                guard.validate({'version': '1.0', 'rpms': rpms}, set(), self.packages)
        with self.assertRaises(ValueError):
            guard.validate(self.plan, set(), {self.filename: (self.nevra, 'other', 'other')})


if __name__ == '__main__':
    unittest.main()
