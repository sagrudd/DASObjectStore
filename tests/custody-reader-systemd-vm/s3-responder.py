#!/usr/bin/python3
"""Guest-loopback AWS protocol fixture. No Garage authentication/retention claim."""
import hashlib
import http.server
import pathlib
import socket
import time

CONTROL = pathlib.Path('/run/das-systemd-vm-fixture')
PUBLIC = pathlib.Path('/run/das-vm-tls-public')
BODY = b'actual synthetic manager receipt'


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'

    def log_message(self, *_):
        pass  # Never expose credential-derived request headers/signatures.

    def do_GET(self):
        self.connection.settimeout(5)
        counter = CONTROL / 'get-count'
        count = int(counter.read_text()) + 1
        counter.write_text(str(count))
        # Real CLI retries must be absent, not hidden by an idempotent responder.
        if count != 1:
            self.send_error(409)
            return
        expected = (PUBLIC / 'object-path').read_text()
        if self.path != expected or self.headers.get('Host') != '127.0.0.1:19000':
            self.send_error(404)
            return
        mode = (CONTROL / 'mode').read_text().strip()
        if mode == 'disconnect':
            self.connection.shutdown(socket.SHUT_RDWR)
            self.connection.close()
            return
        body = BODY if mode == 'positive' else bytes([BODY[0] ^ 1]) + BODY[1:]
        self.send_response(200)
        self.send_header('Content-Length', str(len(body)))
        self.send_header('Content-Type', 'application/octet-stream')
        self.send_header('ETag', '"' + hashlib.md5(body, usedforsecurity=False).hexdigest() + '"')
        self.send_header('Connection', 'close')
        self.end_headers()
        self.wfile.write(body)
        self.wfile.flush()
        self.close_connection = True


if __name__ == '__main__':
    assert pathlib.Path('/proc/1/comm').read_text().strip() == 'systemd'
    assert (CONTROL / 'permit').is_file()
    # Independent systemd runtime limit also terminates the owned responder.
    end = time.monotonic() + 240
    with http.server.HTTPServer(('127.0.0.1', 19000), Handler) as server:
        server.timeout = 0.2
        # HTTPServer has bound/listened successfully here. Type=exec alone
        # cannot establish this readiness for the real first AWS GET.
        (CONTROL / 'protocol-ready').write_text((CONTROL / 'mode').read_text().strip())
        while time.monotonic() < end:
            server.handle_request()
