#!/usr/bin/env python3
"""Fixed-port deterministic origin for SynVoid live wiring.

The named `eggserve-origin` adapter always binds an ephemeral port, which
a managed command subject with a static config cannot discover (see v1
README deviation D1). This stub provides the same deterministic contract
(exact route -> status + fixed body, anything else -> 501) on a FIXED
loopback port so a static SynVoid upstream config can point at it during
live qualification.

Routine-test-only alongside `fake_synvoid.py`; never a production
dependency.

Usage:
    fake_origin.py --port 18081 --path /search --body-bytes 1024 --status 200
"""

import argparse
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlsplit


class Handler(BaseHTTPRequestHandler):
    server_version = "FakeOrigin/0"
    route = "/"
    body = b""
    status = 200
    delay_ms = 0.0

    def log_message(self, *args):  # noqa: ANN002, ANN202 - stdlib signature
        sys.stderr.write(
            "%s %s -> %s\n" % (self.command, self.path, getattr(self, "_status", "?"))
        )

    def _send(self, status: int, body: bytes) -> None:
        self._status = status
        self.send_response(status)
        self.send_header("Content-Type", "text/plain")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)

    def _handle(self) -> None:
        if Handler.delay_ms:
            time.sleep(Handler.delay_ms / 1000.0)
        if self.command not in ("GET", "HEAD"):
            self._send(501, b"")
            return
        if urlsplit(self.path).path == Handler.route:
            self._send(Handler.status, Handler.body)
        else:
            self._send(501, b"")

    do_GET = _handle
    do_HEAD = _handle
    do_POST = _handle
    do_PUT = _handle
    do_DELETE = _handle


def main() -> int:
    parser = argparse.ArgumentParser(description="Fixed-port deterministic origin.")
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--path", default="/")
    parser.add_argument("--body-bytes", type=int, default=1024)
    parser.add_argument("--status", type=int, default=200)
    parser.add_argument("--delay-ms", type=float, default=0.0)
    args = parser.parse_args()
    Handler.route = args.path
    Handler.body = b"B" * args.body_bytes
    Handler.status = args.status
    Handler.delay_ms = args.delay_ms
    server = ThreadingHTTPServer(("127.0.0.1", args.port), Handler)
    sys.stderr.write("fake-origin listening on 127.0.0.1:%d%s\n" % (args.port, args.path))
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
