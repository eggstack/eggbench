#!/usr/bin/env python3
"""Synthetic SynVoid stand-in for Eggbench M002a routine tests.

Emulates the observable HTTP contract the closed SynVoid qualification
export is expected to provide (M002a plan sections 3-8):

* benign routes return the controlled-origin success contract
  (200 + deterministic fixed body);
* allowlisted attack shapes return the owner-authored block status (403);
* anything else returns 501 so unexpected traffic can never be mistaken
  for a passing expectation.

This stub is routine-test-only. It is NOT SynVoid, it does not proxy to
the controlled origin (a managed command subject cannot discover the
named adapter's ephemeral port; see v1 README deviation D1), and it must
never be presented as live reverse-proxy qualification. Live
qualification requires the real SynVoid minimal binary plus the
SynVoid-owned exported assets (see scripts/qualification/synvoid-m002/).

Usage:
    fake_synvoid.py --port 18080 [--delay-ms 50]
"""

import argparse
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlsplit, unquote

SMALL_BODY = b"B" * 1024
LARGE_BODY = b"B" * 65536
BLOCK_BODY = b"Blocked by synthetic WAF"

BENIGN_ROUTES = {
    "/": SMALL_BODY,
    "/search": SMALL_BODY,
    "/bench/small": SMALL_BODY,
    "/bench/large": LARGE_BODY,
}


def is_attack(raw_target: str) -> bool:
    """Match the synthetic allowlist attack shapes (documented emulation).

    Deliberately narrow: a bare encoded `<` (`%3C`, as in the benign
    `price%3C100` filter case) is NOT an attack marker on its own.
    """
    decoded = unquote(raw_target).lower()
    return (
        "script" in decoded
        or ".." in decoded
        or "etc/passwd" in decoded
    )


class Handler(BaseHTTPRequestHandler):
    server_version = "FakeSynVoid/0"
    delay_ms = 0.0
    port = 0

    @staticmethod
    def sidecar_delay_ms(port: int) -> float:
        """Plan-invisible harness throttle (M002b performance-only proof).

        The delay lives in a sidecar file keyed by listen port so the
        scenario plan -- and therefore the comparison identity -- stays
        identical between baseline and candidate runs. A plan-embedded
        delay would (correctly) compare as incomparable drift instead.
        Absent/unparseable file means no delay.
        """
        try:
            with open("/tmp/fake-synvoid-%d.delay_ms" % port) as handle:
                return float(handle.read().strip() or 0)
        except (OSError, ValueError):
            return 0.0

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
        delay = Handler.delay_ms + Handler.sidecar_delay_ms(Handler.port)
        if delay:
            time.sleep(delay / 1000.0)
        if self.command not in ("GET", "HEAD"):
            self._send(501, b"")
            return
        raw_target = self.path
        route = urlsplit(raw_target).path
        if is_attack(raw_target):
            self._send(403, BLOCK_BODY)
            return
        body = BENIGN_ROUTES.get(route)
        if body is None:
            self._send(501, b"")
            return
        self._send(200, body)

    do_GET = _handle
    do_HEAD = _handle
    do_POST = _handle
    do_PUT = _handle
    do_DELETE = _handle


def main() -> int:
    parser = argparse.ArgumentParser(description="Synthetic SynVoid stand-in.")
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument(
        "--delay-ms",
        type=float,
        default=0.0,
        help="Qualification-only controlled latency injected per request "
        "(M002b performance-only regression proof). Never changes WAF "
        "semantics: block/pass decisions are identical with any delay.",
    )
    args = parser.parse_args()
    Handler.delay_ms = args.delay_ms
    Handler.port = args.port
    server = ThreadingHTTPServer(("127.0.0.1", args.port), Handler)
    sys.stderr.write("fake-synvoid listening on 127.0.0.1:%d\n" % args.port)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
