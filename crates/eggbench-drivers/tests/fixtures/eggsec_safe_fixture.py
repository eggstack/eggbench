"""Safe local WAF qualification fixture (Eggstack M004a only).

NOT scanner logic: a deliberately non-reflecting HTTP server. Every request
receives the same fixed body with status 200, so no Eggsec payload can be
reflected and no Eggsec-declared bypass is observable. The Eggbench
correctness expectation is Pass (zero successful bypasses).

Authorized loopback qualification only. Binds 127.0.0.1 on the given port.
"""

from http.server import BaseHTTPRequestHandler, HTTPServer
import sys

BODY = b"eggbench-m004a-safe-fixture-response"


class Handler(BaseHTTPRequestHandler):
    def _handle(self):
        self.send_response(200)
        self.send_header("Content-Length", str(len(BODY)))
        self.end_headers()
        self.wfile.write(BODY)

    do_GET = _handle
    do_POST = _handle

    def log_message(self, *args):
        pass


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 18080
    HTTPServer(("127.0.0.1", port), Handler).serve_forever()
