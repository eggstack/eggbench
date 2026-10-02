#!/usr/bin/env python3
"""Deterministic controlled origin for the M003 real-SynVoid qualification.

Harness-only scaffolding (no production semantics): serves an exact route
table with fixed deterministic bodies, answers 501 everywhere else, accepts
and drains request bodies, appends one line per request so blocked-request
proofs stay auditable, and can inject a qualification-only throttle.

The throttle exists for the M003d performance-only regression proof. It is
applied at the origin, never at the subject, so SynVoid's security decisions
are untouched: owner security outcomes stay correct while the performance
gate fails. It is never part of a plan, so the comparison identity is
unchanged.

Loopback only, no timestamps in responses or logs.

Usage:
  controlled-origin-m003.py --port <port> --routes-json <path>
                            [--log-file <path>] [--delay-ms <ms>]
                            [--delay-signal-file <path>]

Routes file: {"<path>": {"status": 200, "body_bytes": 1024}, ...}
"""

import argparse
import json
import os
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

FILL = b"\x42"
MAX_BODY = 1 << 20


class Handler(BaseHTTPRequestHandler):
    server_version = "ControlledOriginM003/0"
    protocol_version = "HTTP/1.1"
    routes = {}
    log_path = ""
    delay_ms = 0.0
    delay_signal_file = ""

    def log_message(self, *args):  # noqa: ANN002, ANN202 - stdlib signature
        return

    def _delay(self):
        if not self.delay_signal_file:
            return self.delay_ms
        # A signal file lets the harness toggle the throttle between two runs
        # of the same plan without changing the plan or the process.
        try:
            with open(self.delay_signal_file) as handle:
                return float(handle.read().strip() or 0)
        except (OSError, ValueError):
            return 0.0

    def _serve(self):
        delay = self._delay()
        if delay:
            time.sleep(delay / 1000.0)
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
                remaining = min(int(length), MAX_BODY)
            except ValueError:
                remaining = 0
            while remaining > 0:
                chunk = self.rfile.read(min(65536, remaining))
                if not chunk:
                    break
                remaining -= len(chunk)
        if self.log_path:
            try:
                with open(self.log_path, "a") as handle:
                    handle.write("%s %s %d\n" % (self.command, self.path, status))
            except OSError:
                pass
        self.send_response(status)
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)

    do_GET = _serve
    do_HEAD = _serve
    do_POST = _serve
    do_PUT = _serve
    do_DELETE = _serve


def main():
    parser = argparse.ArgumentParser(description="Controlled M003 origin.")
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--routes-json", required=True)
    parser.add_argument("--log-file", default="")
    parser.add_argument("--delay-ms", type=float, default=0.0)
    parser.add_argument("--delay-signal-file", default="")
    args = parser.parse_args()

    if args.delay_signal_file and os.path.exists(args.delay_signal_file):
        os.remove(args.delay_signal_file)
    with open(args.routes_json) as handle:
        routes = json.load(handle)
    for path, spec in routes.items():
        if not 100 <= int(spec.get("status", 200)) <= 599:
            print("controlled origin: invalid status for %s" % path, file=sys.stderr)
            return 2
        if not 0 <= int(spec.get("body_bytes", 1024)) <= MAX_BODY:
            print("controlled origin: invalid body_bytes for %s" % path, file=sys.stderr)
            return 2
    Handler.routes = routes
    Handler.log_path = args.log_file
    Handler.delay_ms = args.delay_ms
    Handler.delay_signal_file = args.delay_signal_file
    class _Server(ThreadingHTTPServer):
        # The perf scenarios open pooled and fresh-per-request campaigns against
        # this origin, and the performance regression makes it the bottleneck.
        # The stdlib backlog of 5 turns that into refused connections, which the
        # transport-error gate then reports as a subject failure instead of the
        # intended origin degradation, so the backlog is sized above the offered
        # concurrency.
        request_queue_size = 512
        daemon_threads = True

    server = _Server(("127.0.0.1", args.port), Handler)
    sys.stderr.write("controlled origin on 127.0.0.1:%d\n" % args.port)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
