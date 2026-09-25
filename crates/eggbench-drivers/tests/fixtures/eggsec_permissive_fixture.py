"""Permissive local WAF qualification fixture (Eggstack M004a only).

NOT scanner logic: a deliberately permissive HTTP server. The bare path
returns 404; every other path returns 200 with the request target echoed in
the body, so Eggsec-declared bypasses (2xx + changed status + reflected
payload) are observable. The Eggbench correctness expectation is Fail
(at least one successful bypass against a zero allowance).

Authorized loopback qualification only. Binds 127.0.0.1 on the given port.
"""

from http.server import BaseHTTPRequestHandler, HTTPServer
import sys


class Handler(BaseHTTPRequestHandler):
    def _handle(self):
        if self.path == "/":
            body = b"not found"
            self.send_response(404)
        else:
            body = ("echo:" + self.path).encode("utf-8", "replace")
            self.send_response(200)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    do_GET = _handle
    do_POST = _handle

    def log_message(self, *args):
        pass


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 18081
    HTTPServer(("127.0.0.1", port), Handler).serve_forever()
