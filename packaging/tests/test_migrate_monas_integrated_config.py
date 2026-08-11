import json
import os
import pathlib
import subprocess
import tempfile
import unittest


SCRIPT = pathlib.Path(__file__).parents[1] / "linux/usr/libexec/dasobjectstore/migrate-monas-integrated-config"


class MigrationTest(unittest.TestCase):
    def test_preserves_unrelated_configuration_and_sets_integrated_boundary(self):
        with tempfile.TemporaryDirectory() as directory:
            config_path = pathlib.Path(directory) / "config.json"
            original = {
                "bind_address": "0.0.0.0",
                "authentication": {"authority": "local_user", "session_ttl_seconds": 3600},
                "tls": {"certificate_path": "/protected/server.crt"},
                "s3_ingress": {"mode": "direct_gateway", "max_concurrent_uploads": 17},
            }
            config_path.write_text(json.dumps(original), encoding="utf-8")
            os.chmod(config_path, 0o640)

            subprocess.run(
                [
                    str(SCRIPT), "--config", str(config_path),
                    "--public-endpoint-url", "https://192.168.1.192:3900",
                    "--region", "mnemosyne-local",
                ],
                check=True,
            )
            migrated = json.loads(config_path.read_text(encoding="utf-8"))
            self.assertEqual(migrated["authentication"]["authority"], "monas")
            self.assertEqual(migrated["s3_ingress"]["mode"], "external_gateway")
            self.assertEqual(migrated["s3_ingress"]["max_concurrent_uploads"], 17)
            self.assertEqual(migrated["tls"], original["tls"])
            self.assertEqual(config_path.stat().st_mode & 0o777, 0o640)

    def test_rejects_non_https_endpoint_without_modifying_file(self):
        with tempfile.TemporaryDirectory() as directory:
            config_path = pathlib.Path(directory) / "config.json"
            original = '{"authentication":{"authority":"local_user"}}\n'
            config_path.write_text(original, encoding="utf-8")
            result = subprocess.run(
                [str(SCRIPT), "--config", str(config_path),
                 "--public-endpoint-url", "http://192.168.1.192:3900", "--region", "local"],
                check=False,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(config_path.read_text(encoding="utf-8"), original)


if __name__ == "__main__":
    unittest.main()
