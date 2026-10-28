"""Verify the staged Monas traversal helper is part of the RPM file list."""
from pathlib import Path
import unittest


class RpmManagedRootFileListTests(unittest.TestCase):
    def test_staged_helper_is_listed_once_in_rpm_payload(self):
        script=(Path(__file__).resolve().parents[1]/'rpm/build-rpm.sh').read_text()
        helper='/usr/libexec/dasobjectstore/prepare-monas-managed-root-traversal'
        self.assertIn('"$payload_root'+helper+'"',script)
        spec=script.split('cat >"$spec_path" <<SPEC\n',1)[1].split('\nSPEC\n',1)[0]
        files=spec.split('\n%files\n',1)[1].split('\n%changelog\n',1)[0]
        self.assertEqual(files.splitlines().count(helper),1)
        descriptor='/opt/dasobjectstore/plugin-process-descriptor.json'
        self.assertIn('"$payload_root'+descriptor+'"',script)
        self.assertEqual(files.splitlines().count(descriptor),1)
        for name in ('managed-storage.v1.json','s3-gateway.json','workspace-host.json'):
            self.assertEqual(files.splitlines().count(
                '%attr(0640,root,root) %config(noreplace) /etc/dasobjectstore/'+name),1)


if __name__=='__main__':
    unittest.main()
