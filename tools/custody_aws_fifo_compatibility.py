#!/usr/bin/env python3
"""Unsigned synthetic loopback GET: actual AWS CLI FIFO compatibility only."""
import hashlib
import http.server
import json
import os
from pathlib import Path
import select
import stat
import subprocess
import sys
import tempfile
import threading
import time


def main():
    executable = Path(sys.argv[1]).resolve(strict=True)
    payload = b"custody FIFO compatibility: synthetic public bytes\n" * 129
    requests = []

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            requests.append((self.path, self.headers.get("Authorization")))
            self.send_response(200)
            self.send_header("Content-Length", str(len(payload)))
            self.send_header("Content-Type", "application/octet-stream")
            self.end_headers()
            self.wfile.write(payload)

        def log_message(self, *_args):
            pass

    with tempfile.TemporaryDirectory(prefix="custody-fifo-proof-") as temp:
        fifo = Path(temp) / "body"
        os.mkfifo(fifo, 0o600)
        before = fifo.lstat()
        descriptor = os.open(fifo, os.O_RDWR | os.O_NONBLOCK)
        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        server_thread = threading.Thread(target=server.serve_forever, daemon=True)
        server_thread.start()
        environment = {
            "PATH": "/usr/bin:/bin", "AWS_CONFIG_FILE": "/dev/null",
            "AWS_SHARED_CREDENTIALS_FILE": "/dev/null",
            "AWS_EC2_METADATA_DISABLED": "true", "AWS_MAX_ATTEMPTS": "1",
            "AWS_PAGER": "", "AWS_CLI_AUTO_PROMPT": "off",
        }
        process = subprocess.Popen([
            str(executable), "s3api", "get-object", "--bucket", "fixture",
            "--key", "public", "--endpoint-url", f"http://127.0.0.1:{server.server_port}",
            "--no-sign-request", "--region", "us-east-1", str(fifo),
        ], env=environment, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        received = bytearray()
        deadline = time.monotonic() + 10
        try:
            while time.monotonic() < deadline:
                if select.select([descriptor], [], [], 0.02)[0]:
                    received.extend(os.read(descriptor, 65536))
                if len(received) > len(payload):
                    raise AssertionError("extra bytes")
                if process.poll() is not None:
                    while select.select([descriptor], [], [], 0)[0]:
                        received.extend(os.read(descriptor, 65536))
                    break
            assert process.poll() == 0, "AWS GET failed or timed out"
            after = fifo.lstat()
            assert stat.S_ISFIFO(after.st_mode)
            assert (before.st_dev, before.st_ino) == (after.st_dev, after.st_ino)
            assert received == payload
            assert requests == [("/fixture/public", None)]
            version = subprocess.check_output([str(executable), "--version"], env=environment, text=True).strip()
            print(json.dumps({"result": "PASS", "aws_version": version,
                "executable": str(executable), "executable_sha256": hashlib.sha256(executable.read_bytes()).hexdigest(),
                "bytes": len(received), "fifo_inode_preserved": True,
                "requests": 1, "authorization_header": False,
                "scope": "local synthetic unsigned loopback, not native Garage qualification"}, sort_keys=True))
        finally:
            if process.poll() is None:
                process.kill()
            process.wait()
            os.close(descriptor)
            server.shutdown()
            server.server_close()
            server_thread.join()


if __name__ == "__main__":
    main()
