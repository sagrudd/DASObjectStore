#!/usr/bin/python3
"""Static fixture contract checks, never a substitute for real systemd execution."""
import configparser
import pathlib
import re
import unittest

ROOT = pathlib.Path(__file__).parent
SCRIPT = (ROOT / 'tls-guest.sh').read_text()


class JoinedScriptTests(unittest.TestCase):
    def test_retained_terminal_units_preserve_real_process_and_least_privilege(self):
        units = dict(re.findall(r'cat > /etc/systemd/system/(das-vm[^ ]+) <<UNIT\n(.*?)\nUNIT', SCRIPT, re.S))
        for name, uid in [('das-vm-tls-reader.service', '2000'), ('das-vm-tls-verifier.service', '2001')]:
            parser = configparser.ConfigParser(interpolation=None)
            parser.read_string(units[name])
            service = parser['Service']
            for key, expected in {'Type': 'exec', 'User': uid, 'Group': uid, 'RemainAfterExit': 'yes',
                'NoNewPrivileges': 'yes', 'CapabilityBoundingSet': '', 'PrivateMounts': 'yes'}.items():
                self.assertEqual(service[key], expected)
            self.assertLessEqual(int(service['RuntimeMaxSec']), 240)
            self.assertTrue(service['ExecStart'].startswith('/opt/das-vm-adapter --exact '))
            if uid == '2000':
                self.assertEqual(service['LoadCredentialEncrypted'], 'reader:$encrypted')
            else:
                self.assertNotIn('LoadCredentialEncrypted', service)

    def test_terminal_evidence_precedes_explicit_stop_and_inactive(self):
        wait = SCRIPT.split('wait_success() {', 1)[1].split('\n}\n', 1)[0]
        for required in ['test -f "$marker"', 'ActiveState', '= active', 'SubState', '= exited',
            'ExecMainStatus', 'Result', '= success', 'ExecMainPID', '-gt 0', 'InvocationID', '^[0-9a-f]{32}$']:
            self.assertIn(required, wait)
        verify = SCRIPT.index('wait_success das-vm-tls-verifier.service')
        reader = SCRIPT.index('wait_success das-vm-tls-reader.service')
        stop = SCRIPT.index('systemctl stop das-vm-tls-reader.service das-vm-tls-verifier.service')
        self.assertLess(verify, stop)
        self.assertLess(reader, stop)
        for name in ['reader', 'verifier']:
            inactive = SCRIPT.index(f'ActiveState --value das-vm-tls-{name}.service)" = inactive', stop)
            self.assertGreater(inactive, stop)

    def test_actual_backend_listen_readiness_precedes_verifier(self):
        readiness = SCRIPT.index('/run/das-systemd-vm-fixture/protocol-ready)" = "$mode"')
        start = SCRIPT.index('systemctl start das-vm-tls-verifier.service')
        self.assertLess(readiness, start)
        self.assertIn('test "$ready" = yes', SCRIPT[readiness:start])
        responder = (ROOT / 's3-responder.py').read_text()
        self.assertLess(responder.index("with http.server.HTTPServer(('127.0.0.1', 19000)"),
            responder.index("(CONTROL / 'protocol-ready').write_text"))

    def test_package_install_precedes_all_generated_identities(self):
        self.assertLess(SCRIPT.index('/bin/bash /mnt/cidata/aws-guest-install.sh'),
            SCRIPT.index('$driver::prepare_joined_tls_vm'))
        self.assertIn('journal-$mode.sqlite3', SCRIPT)


if __name__ == '__main__':
    unittest.main()
