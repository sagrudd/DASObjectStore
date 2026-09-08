"""Static source regression only; never executes Garage or a guest."""
from pathlib import Path
import unittest

SOURCE = Path(__file__).with_name("garage-guest.sh").read_text()


class GarageGuestSource(unittest.TestCase):
    def test_control_and_permit_precede_installer_and_secrets_follow(self):
        directory = SOURCE.index("install -d -m 755 /run/das-systemd-vm-fixture")
        permit = SOURCE.index("install -m 644 /dev/null /run/das-systemd-vm-fixture/permit")
        installer = SOURCE.index("/bin/bash /mnt/cidata/aws-guest-install.sh")
        self.assertLess(directory, permit)
        self.assertLess(permit, installer)
        self.assertLess(installer, SOURCE.index("secrets.token_hex(32)"))

    def test_exact_binary_single_node_without_default_bucket(self):
        self.assertIn("8ced2ad3040262571de08aa600959aa51f97576d55da7946fcde6f66140705e2", SOURCE)
        self.assertIn("server --single-node", SOURCE)
        self.assertNotIn("--default-bucket", SOURCE)
        self.assertNotIn("GARAGE_DEFAULT_SECRET", SOURCE)
        self.assertIn('rpc_bind_addr = "127.0.0.1:3902"', SOURCE)
        self.assertIn('api_bind_addr = "127.0.0.1:3901"', SOURCE)
        self.assertIn('s3_region = "garage"', SOURCE)
        rust = Path(__file__).parents[2] / "crates/dasobjectstore-daemon/src/runtime/custody_garage_vm_tests.rs"
        self.assertIn('vec!["--region".into(), "garage".into()]', rust.read_text())

    def test_real_readiness_precedes_retention_and_terminal_is_retained(self):
        self.assertLess(SOURCE.index('test "$ready" = true'), SOURCE.index("phase=admission_retention"))
        self.assertIn("RemainAfterExit=yes", SOURCE)
        for field in ("ExecMainPID", "ExecMainStatus", "InvocationID", "SubState", "Result"):
            self.assertIn(field, SOURCE)
        self.assertLess(SOURCE.index("grep -qx 'VM_GARAGE_ADMISSION_BATCH_PASS"), SOURCE.index("systemctl stop das-vm-garage-test.service"))
        self.assertEqual(SOURCE.count("NoNewPrivileges=yes"), 2)
        self.assertEqual(SOURCE.count("CapabilityBoundingSet=\n"), 2)


if __name__ == "__main__":
    unittest.main()
