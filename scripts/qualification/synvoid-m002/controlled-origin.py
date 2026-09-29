#!/usr/bin/env python3
"""Deterministic controlled origin for real-SynVoid qualification (C002).

Harness-only scaffolding (no production semantics): serves a fixed set of
exact paths with 200 + deterministic bodies, answers 501 everywhere else
(mirroring the eggserve-origin route discipline), and appends one line per
request to a log file so the harness can prove blocked requests never
reached the origin.

Loopback-only. No filesystem reads besides the routes file. No timestamps
in responses or logs (log lines are METHOD, TARGET, STATUS only).

Usage:
  controlled-origin.py --port <port> --routes-json <path> --log-file <path>

Routes file: {"<path>": {"status": 200, "body_bytes": 1024}, ...}
Path matching uses the raw request-target path (no normalization, no query).
"""

import argparse
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

FILL = b"\x42"


class Handler(BaseHTTPRequestHandler):
    routes = {}
    log_path = ""

    def _serve(self):
        raw = self.path
        path = raw.split("?", 1)[0]
        route = self.routes.get(path)
        if route is None:
            status, body = 501, b""
        else:
            status, body = route["status"], FILL * route["body_bytes"]
        length = self.headers.get("Content-Length")
        if length:
            try:
                self.rfile.read(min(int(length), 8 * 1024 * 1024))
            except ValueError:
                pass
        self.send_response(status)
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if self.command != "HEAD" and body:
            self.wfile.write(body)
        with open(self.log_path, "a", buffering=1) as handle:
            handle.write("%s %s %d\n" % (self.command, raw, status))

    do_GET = _serve
    do_POST = _serve
    do_PUT = _serve
    do_DELETE = _serve
    do_HEAD = _serve
    do_OPTIONS = _serve
    do_PATCH = _serve

    def log_message(self, *args):  # keep stderr clean for harness parsing
        pass


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--routes-json", required=True)
    parser.add_argument("--log-file", required=True)
    args = parser.parse_args()
    routes = json.load(open(args.routes_json))
    Handler.routes = {
        path: {"status": int(spec["status"]), "body_bytes": int(spec["body_bytes"])}
        for path, spec in routes.items()
    }
    Handler.log_path = args.log_file
    open(args.log_file, "w").close()
    server = ThreadingHTTPServer(("127.0.0.1", args.port), Handler)
    server.serve_forever()


if __name__ == "__main__":
    main()
