"""Static fixture guard regressions only, never guest/backend execution."""
from pathlib import Path
import subprocess
import unittest

HERE = Path(__file__).parent
JOIN = (HERE / "garage-tls-guest.sh").read_text()
TLS = (HERE / "tls-guest.sh").read_text()
RUST = (HERE.parents[1] / "crates/dasobjectstore-daemon/src/runtime/custody_reader/tls_vm_tests.rs").read_text()


class GarageJoinedGuards(unittest.TestCase):
    def test_shell_error_diagnostic_is_line_only(self):
        self.assertIn("trap 'failure_line=$LINENO' ERR", TLS)
        self.assertIn('*) failure_line=$LINENO; exit 1;; esac', TLS)
        self.assertNotIn("BASH_COMMAND", TLS)
        result = subprocess.run(["bash", "-c", "set -e\nfailure_line=0\ntrap 'failure_line=$LINENO' ERR\ntrap 'printf \"line=%s\\n\" \"$failure_line\"' EXIT\nsecret_like=synthetic_private_payload\nfalse\n"], capture_output=True, text=True)
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout, "line=6\n")
        self.assertNotIn("synthetic_private_payload", result.stdout + result.stderr)

    def test_bootstrap_stops_before_new_key_and_protected_publication(self):
        self.assertLess(JOIN.index("garage-guest.sh --retain-for-tls"), JOIN.index("phase=continuation_provision"))
        self.assertLess(JOIN.index("provision_garage_continuation_vm"), JOIN.index("phase=protected_tls"))
        self.assertIn('test "$ready" = yes', JOIN)
        self.assertIn("RemainAfterExit=yes", JOIN)
        for field in ("ExecMainPID", "ExecMainStatus", "InvocationID", "SubState", "Result"):
            self.assertIn(field, JOIN)

    def test_existing_publication_survives_restart_and_binding_denial(self):
        self.assertIn("lifecycles=(initial restart binding)", TLS)
        self.assertIn('test "$after" = "$publication"', TLS)
        self.assertIn('test "$reader_pid" != "$previous_reader_pid"', TLS)
        self.assertIn('systemctl stop das-vm-garage.service', TLS)
        self.assertIn('test "$garage_existing" = no; then systemctl start das-vm-protocol.service', TLS)
        self.assertIn("VM_GARAGE_TLS_ALL_PASS_NOT_FORMAL_CUSTODY", TLS)
        self.assertIn('if test "$garage_existing" = no; then poweroff; fi', TLS)
        self.assertLess(JOIN.index("VM_GARAGE_JOIN_COMPLETE"), JOIN.rindex("poweroff"))

    def test_actual_bytes_do_not_fabricate_attestation_or_reset_journal(self):
        self.assertIn("exact bytes acquired; fixture does not produce formal attestation", RUST)
        self.assertIn("CustodyOffNucJournal::open_existing(&journal_path)", RUST)
        self.assertIn('("terminal", "incomplete")', RUST)
        self.assertIn("make_client().read(&journal, old_raw, limits()).is_err()", RUST)
        self.assertNotIn("DELETE FROM", RUST)

    def test_no_provider_metrics_or_private_console_dump(self):
        self.assertNotIn("admin_token", JOIN)
        self.assertNotIn("metrics_token", JOIN)
        self.assertNotIn("cat /var/lib", JOIN)
        self.assertIn("StandardError=append:/var/lib/das-garage-fixture/continuation-result.private", JOIN)
        self.assertIn('if test "$garage_existing" = no; then\n        test "$(cat /run/das-systemd-vm-fixture/get-count)"', TLS)


if __name__ == "__main__":
    unittest.main()
