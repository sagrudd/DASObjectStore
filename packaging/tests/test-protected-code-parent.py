#!/usr/bin/env python3
"""Controlled package-source contracts; never installs DASObjectStore."""
import errno
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
RPM = ROOT / "packaging/rpm/build-rpm.sh"
DEB = ROOT / "packaging/debian/postinst"
TMPFILES = ROOT / "packaging/linux/tmpfiles.d/dasobjectstore.conf"


class ProtectedParent(unittest.TestCase):
    def test_shared_tmpfiles_preserves_separate_tls_owner(self):
        rows = TMPFILES.read_text().splitlines()
        self.assertIn("d /opt/dasobjectstore 0750 root dasobjectstore -", rows)
        self.assertIn("d /opt/dasobjectstore/tls 0750 dasobjectstore dasobjectstore -", rows)
        self.assertNotIn("d /opt/dasobjectstore 0750 dasobjectstore dasobjectstore -", rows)

    def test_install_hooks_defer_report_provider_execution(self):
        for source in (RPM, DEB):
            text = source.read_text()
            self.assertNotIn("prewarm-report-provider", text)
            self.assertNotIn("ensure_report_provider", text)
            self.assertNotIn("grammateus_report_provider install", text)
        wrapper = (ROOT / "packaging/reporting/gnostikon-workflow-control").read_text()
        self.assertIn("run_prewarm_report_provider()", wrapper)
        self.assertIn("ensure_report_provider_image", wrapper)

    def assay(self, source, expected):
        self.assertEqual(os.geteuid(), 0, "actual ownership assay requires the bounded root fixture")
        matches = [row for row in source.read_text().splitlines() if row == expected]
        self.assertEqual(matches, [expected])
        with tempfile.TemporaryDirectory(prefix="das-code-parent-") as name:
            root = Path(name)
            root.chmod(0o755)
            parent = root / "product"
            parent.mkdir()
            os.chown(parent, 65534, 65534)
            tls = parent / "tls"
            tls.mkdir(mode=0o750)
            os.chown(tls, 65534, 65534)
            key = tls / "existing-key"
            key.write_bytes(b"controlled-key-fixture")
            os.chown(key, 65534, 65534)
            key.chmod(0o600)
            before = (tls.stat(), key.stat(), key.read_bytes())
            command = expected.replace(r'\"', '"')
            command = command.replace(r"\$service_group", "65534").replace("$service_group", "65534")
            command = command.replace('"/opt/dasobjectstore"', shlex.quote(str(parent)))
            command = command.replace("/opt/dasobjectstore", shlex.quote(str(parent)))
            command = command.replace('"$product_root"', shlex.quote(str(parent)))
            subprocess.run(["/bin/sh", "-eu", "-c", command], check=True)
            self.assertEqual((parent.stat().st_uid, parent.stat().st_gid, parent.stat().st_mode & 0o777), (0, 65534, 0o750))
            web = parent / "web"
            web.mkdir(mode=0o755)
            (web / "app.js").write_bytes(b"controlled-code")
            probe = r'''
import errno, os, pathlib, sys
root = pathlib.Path(sys.argv[1])
try:
    (root / "web").rename(root / "replaced")
except OSError as e:
    assert e.errno in (errno.EACCES, errno.EPERM), e
else:
    raise AssertionError("service identity replaced packaged code parent")
(root / "tls" / "new-fixture").write_bytes(b"allowed mutable TLS fixture")
'''
            def service():
                os.setgroups([])
                os.setgid(65534)
                os.setuid(65534)
            subprocess.run([sys.executable, "-B", "-c", probe, str(parent)], preexec_fn=service, check=True)
            after = (tls.stat(), key.stat(), key.read_bytes())
            for left, right in zip(before[:2], after[:2]):
                self.assertEqual((left.st_ino, left.st_uid, left.st_gid, left.st_mode), (right.st_ino, right.st_uid, right.st_gid, right.st_mode))
            self.assertEqual(before[2], after[2])
            self.assertEqual((web / "app.js").read_bytes(), b"controlled-code")

    def test_rpm_parent_protects_code_and_preserves_tls(self):
        self.assay(RPM, r'install -d -o root -g "\$service_group" -m 0750 /opt/dasobjectstore')

    def test_debian_parent_protects_code_and_preserves_tls(self):
        self.assay(DEB, 'install -d -o root -g "$service_group" -m 0750 "$product_root"')


if __name__ == "__main__":
    unittest.main()
