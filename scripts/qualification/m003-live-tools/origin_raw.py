"""Deterministic raw-socket loopback origin for M003 live qualification.

Emits no Date/Server/timestamp headers: every response carries exactly
Content-Length (deterministic), Content-Type, Connection: close, then closes.

  GET /bench -> <STATUS> + exactly <BODY_LEN> bytes of 0x42
  anything else -> 404 + b"not found"

Usage: origin_raw.py [status] [body_len]
Prints the bound port on stdout, then serves forever (threaded).
Qualification-only helper: never imported by production Eggbench code.
"""
import socket
import sys
import threading

STATUS = int(sys.argv[1]) if len(sys.argv) > 1 else 200
BODY_LEN = int(sys.argv[2]) if len(sys.argv) > 2 else 64

REASON = {200: "OK", 201: "Created", 404: "Not Found"}


def handle(conn: socket.socket) -> None:
    try:
        data = b""
        conn.settimeout(5)
        while b"\r\n\r\n" not in data:
            chunk = conn.recv(4096)
            if not chunk:
                break
            data += chunk
        line = data.split(b"\r\n", 1)[0].decode("latin1")
        parts = line.split()
        path = parts[1] if len(parts) >= 2 else "/"
        if path == "/bench":
            body = b"B" * BODY_LEN
            status = STATUS
        else:
            body = b"not found"
            status = 404
        head = (
            f"HTTP/1.1 {status} {REASON.get(status, 'OK')}\r\n"
            f"Content-Length: {len(body)}\r\n"
            "Content-Type: application/octet-stream\r\n"
            "Connection: close\r\n"
            "\r\n"
        ).encode("latin1")
        conn.sendall(head + body)
    except OSError:
        pass
    finally:
        try:
            conn.close()
        except OSError:
            pass


def main() -> None:
    srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind(("127.0.0.1", 0))
    srv.listen(64)
    print(srv.getsockname()[1], flush=True)
    while True:
        conn, _ = srv.accept()
        threading.Thread(target=handle, args=(conn,), daemon=True).start()


if __name__ == "__main__":
    main()
