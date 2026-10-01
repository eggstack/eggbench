#!/usr/bin/env python3
"""Controlled loopback origin for the M003 synthetic routine scope.

Direct-origin control: it answers the same request shapes the subject
answers, but performs no blocking and runs no telemetry, which is what
isolates subject/harness overhead from origin overhead (M003d plan
section 5). It also records every served request so a negative proof can
assert which requests actually reached the origin.

Routes come from a JSON file:

    {"/qualbench/small": {"status": 200, "body_bytes": 1024}}

Any undeclared route answers 501 so unexpected traffic can never look like
a pass. Loopback only; never a public listener.

Usage:
    controlled_origin_m003.py --port 18090 --routes-json routes.json
                              [--log-file origin-requests.log]
"""

import argparse
import json
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlsplit

FILL = b"B"


def main():
    parser = argparse.ArgumentParser(description="Controlled M003 origin.")
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--routes-json", required=True)
    parser.add_argument("--log-file", default="")
    args = parser.parse_args()

    with open(args.routes_json) as handle:
        routes = json.load(handle)
    prepared = {}
    for path, spec in routes.items():
        status = int(spec.get("status", 200))
        body_bytes = int(spec.get("body_bytes", 1024))
        if not 100 <= status <= 599 or not 0 <= body_bytes <= 1 << 20:
            print("controlled origin: invalid route spec for %s" % path, file=sys.stderr)
            return 2
        prepared[path] = (status, (FILL * body_bytes) if body_bytes else b"")

    class Handler(BaseHTTPRequestHandler):
        server_version = "ControlledOriginM003/0"
        protocol_version = "HTTP/1.1"

        def log_message(self, *args):  # noqa: ANN002, ANN202 - stdlib signature
            return

        def _read_body(self):
            length = self.headers.get("Content-Length")
            try:
                size = int(length) if length else 0
            except ValueError:
                return 0
            if size < 0:
                return 0
            size = min(size, 1 << 20)
            if size:
                self.rfile.read(size)
            return size

        def _handle(self):
            route = urlsplit(self.path).path
            self._read_body()
            status, body = prepared.get(route, (501, b""))
            self._status = status
            if args.log_file:
                try:
                    with open(args.log_file, "a") as handle:
                        handle.write("%s %s %d\n" % (self.command, self.path, status))
                except OSError:
                    pass
            self.send_response(status)
            self.send_header("Content-Type", "text/plain")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            if self.command != "HEAD":
                self.wfile.write(body)

        do_GET = _handle
        do_HEAD = _handle
        do_POST = _handle
        do_PUT = _handle
        do_DELETE = _handle

    server = ThreadingHTTPServer(("127.0.0.1", args.port), Handler)
    sys.stderr.write("controlled origin on 127.0.0.1:%d\n" % args.port)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
