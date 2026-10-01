#!/usr/bin/env python3
"""Synthetic M003 subject stand-in: HTTP load surface plus owner telemetry.

Routine-test-only. This is NOT SynVoid and must never be presented as live
reverse-proxy qualification. It exists so the generic `prometheus-http`
collector, the M003b load shapes, and the M003d negative demonstrations can
be exercised deterministically inside ordinary CI without a real subject
checkout. Live qualification runs the real minimal SynVoid binary against
the SynVoid-owned export (scripts/qualification/synvoid-m003/).

Two listeners, both loopback only:

* `--port` serves the request contract: benign routes return deterministic
  fixed bodies, allowlisted attack shapes return the owner block status
  (403), everything else returns 501 so unexpected traffic can never look
  like a pass. Request bodies are accepted and accounted (M003d requires a
  body-bearing case); a body-bearing attack shape is decided from the same
  synthetic rules, never from SynVoid semantics.
* `--metrics-port` serves a Prometheus exposition under the owner-authored
  `synvoid_subject_*` names published by the closed SynVoid telemetry
  contract `synvoid.eggbench-telemetry.v2`. Metric names, units, and
  aggregation here exist only to satisfy that owner contract; the mapping
  that binds them is the owner's own `telemetry-mapping.json` fixture.

Fault injection is qualification-only, plan-invisible, and file-driven so a
plan under test (and therefore its comparison identity) never changes:

* `mode=omit_required`     drop one required gauge from the exposition;
* `mode=type_drift`        declare a required counter as a gauge;
* `mode=counter_reset`     report a decreasing required counter;
* `mode=optional_present`  export the owner's optional CPU-worker RSS;
* `after_scrapes=N`        activate the mode only after N served scrapes,
  which is how a mid-trial disappearance is produced honestly;
* `serve_ms=X`             hold each scrape for X milliseconds, so polling
  cost is observable instead of assumed.

Usage:
    fake_synvoid_m003.py --port 18080 --metrics-port 19090
                         [--delay-ms 50] [--fault-file faults.json]
                         [--metrics-log scrapes.log]
"""

import argparse
import json
import os
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import unquote, urlsplit

SMALL_BODY = b"B" * 1024
STREAM_BODY = b"B" * 65536
BODY_ACK = b'{"accepted":true}'
BLOCK_BODY = b"Blocked by synthetic WAF"

BENIGN_ROUTES = {
    "/": SMALL_BODY,
    "/qualbench/small": SMALL_BODY,
    "/qualbench/stream": STREAM_BODY,
}

BODY_ROUTES = ("/api/users",)

# Owner-published sample names from the closed SynVoid v2 contract. They are
# asserted against telemetry-mapping.json; they are never used to derive
# Eggbench metric names.
GAUGE_EVENT_LOOP_LAG = "synvoid_subject_event_loop_lag_ms"
GAUGE_QUEUE_P95 = "synvoid_subject_request_queue_p95_ms"
GAUGE_ACTIVE_CONNECTIONS = "synvoid_subject_active_connections"
GAUGE_WORKER_MEMORY = "synvoid_subject_worker_memory_bytes"
GAUGE_WORKER_CPU = "synvoid_subject_worker_cpu_percent"
GAUGE_CPU_WORKER_RSS = "synvoid_subject_cpu_worker_rss_bytes"
COUNTER_BODY_BUFFERING = "synvoid_subject_body_buffering_bytes_total"
COUNTER_OFFLOAD_SUBMISSIONS = "synvoid_subject_offload_submissions_total"
COUNTER_OFFLOAD_TIMEOUTS = "synvoid_subject_offload_timeouts_total"
COUNTER_OFFLOAD_REJECTIONS = "synvoid_subject_offload_rejections_total"
COUNTER_OFFLOAD_FALLBACKS = "synvoid_subject_offload_fallbacks_total"
COUNTER_WORKER_METRIC_RESETS = "synvoid_subject_worker_metric_resets_total"

MAX_LOG_LINES = 4096


class State:
    """Shared, thread-safe synthetic subject state."""

    def __init__(self, port, metrics_port, delay_ms, fault_file, metrics_log):
        self.lock = threading.Lock()
        self.port = port
        self.metrics_port = metrics_port
        self.delay_ms = delay_ms
        self.fault_file = fault_file
        self.metrics_log = metrics_log
        self.inflight = 0
        self.completed = 0
        self.blocked = 0
        self.body_bytes = 0
        self.scrapes = 0
        self.served_ms_total = 0.0

    def sidecar_delay_ms(self):
        """Plan-invisible harness throttle (M003d performance-only proof).

        Keyed by listen port so the scenario plan -- and therefore the
        comparison identity -- is identical between the paired runs. A
        plan-embedded delay would (correctly) compare as incomparable drift.
        """
        try:
            with open("/tmp/fake-synvoid-%d.delay_ms" % self.port) as handle:
                return float(handle.read().strip() or 0)
        except (OSError, ValueError):
            return 0.0

    def request_delay_ms(self):
        return self.delay_ms + self.sidecar_delay_ms()

    def faults(self):
        if not self.fault_file:
            return {}
        try:
            with open(self.fault_file) as handle:
                document = json.load(handle)
        except (OSError, ValueError):
            return {}
        return document if isinstance(document, dict) else {}

    def record_scrape(self, served_ms):
        if not self.metrics_log:
            return
        line = "%d %.3f\n" % (self.scrapes, served_ms)
        try:
            if os.path.exists(self.metrics_log) and os.path.getsize(self.metrics_log) > 262144:
                os.truncate(self.metrics_log, 0)
            with open(self.metrics_log, "a") as handle:
                handle.write(line)
        except OSError:
            pass


def is_attack(raw_target, body):
    """Match the documented synthetic allowlist attack shapes.

    Deliberately narrow and fully synthetic: a bare encoded `<` (as in the
    benign `price%3C100` filter) is not an attack marker on its own. This
    function encodes no SynVoid rule; the real WAF verdict comes from the
    real subject during live qualification.
    """
    decoded = (unquote(raw_target) + " " + body.decode("utf-8", "replace")).lower()
    return (
        "script" in decoded
        or ".." in decoded
        or "etc/passwd" in decoded
        or "union select" in decoded
        or "or 1=1" in decoded
        or "169.254.169.254" in decoded
    )


class RequestHandler(BaseHTTPRequestHandler):
    server_version = "FakeSynVoidM003/0"
    protocol_version = "HTTP/1.1"
    state = None

    def log_message(self, *args):  # noqa: ANN002, ANN202 - stdlib signature
        sys.stderr.write(
            "%s %s -> %s\n"
            % (self.command, self.path, getattr(self, "_status", "?"))
        )

    def _send(self, status, body):
        self._status = status
        self.send_response(status)
        self.send_header("Content-Type", "text/plain")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)

    def _read_body(self):
        length = self.headers.get("Content-Length")
        try:
            size = int(length) if length else 0
        except ValueError:
            return b""
        if size <= 0:
            return b""
        if size > 65536:
            size = 65536
        return self.rfile.read(size)

    def _handle(self):
        state = RequestHandler.state
        body = self._read_body()
        delay = state.request_delay_ms()
        if delay:
            time.sleep(delay / 1000.0)
        raw_target = self.path
        route = urlsplit(raw_target).path
        with state.lock:
            state.inflight += 1
        try:
            if is_attack(raw_target, body):
                with state.lock:
                    state.blocked += 1
                self._send(403, BLOCK_BODY)
                return
            if self.command in ("POST", "PUT") and route in BODY_ROUTES:
                with state.lock:
                    state.body_bytes += len(body)
                self._send(200, BODY_ACK)
                return
            if self.command in ("GET", "HEAD") and route in BENIGN_ROUTES:
                self._send(200, BENIGN_ROUTES[route])
                return
            self._send(501, b"")
        finally:
            with state.lock:
                state.inflight -= 1
                state.completed += 1

    do_GET = _handle
    do_HEAD = _handle
    do_POST = _handle
    do_PUT = _handle
    do_DELETE = _handle


class MetricsHandler(BaseHTTPRequestHandler):
    server_version = "FakeSynVoidM003Metrics/0"
    protocol_version = "HTTP/1.1"
    state = None

    def log_message(self, *args):  # noqa: ANN002, ANN202 - stdlib signature
        return

    def exposition(self):
        state = MetricsHandler.state
        started = time.perf_counter()
        with state.lock:
            state.scrapes += 1
            scrapes = state.scrapes
            inflight = state.inflight
            completed = state.completed
            blocked = state.blocked
            body_bytes = state.body_bytes
        faults = state.faults()
        if faults.get("after_scrapes", 0) > scrapes:
            faults = {}
        mode = faults.get("mode", "none")

        # Plausible load coupling: lag and queue depth grow with the number
        # of requests already served in this process, connections track the
        # live in-flight count, memory tracks buffered request bytes.
        lag_ms = round(0.05 + completed * 0.002, 3)
        queue_p95 = round(0.02 + completed * 0.0015, 3)
        memory_bytes = 1048576 + body_bytes
        cpu_percent = round(min(99.0, 1.0 + completed * 0.01), 3)
        body_total = float(body_bytes)
        submissions = float(completed // 8)
        rejections = float(blocked)
        if mode == "counter_reset":
            # A worker restart: the bridged counter restarts from zero, which
            # the consumer must treat as an invalid observation rather than a
            # negative or zeroed trial delta.
            body_total = 0.0

        lines = [
            "# HELP %s Maximum event_loop_lag_ms across Unified Server "
            "workers (ms)." % GAUGE_EVENT_LOOP_LAG,
            "# TYPE %s gauge" % GAUGE_EVENT_LOOP_LAG,
        ]
        if mode != "omit_required":
            lines += [
                "%s %s" % (GAUGE_EVENT_LOOP_LAG, lag_ms),
                "# TYPE %s gauge" % GAUGE_QUEUE_P95,
                "%s %s" % (GAUGE_QUEUE_P95, queue_p95),
                "# TYPE %s gauge" % GAUGE_ACTIVE_CONNECTIONS,
                "%s %d" % (GAUGE_ACTIVE_CONNECTIONS, inflight),
                "# TYPE %s gauge" % GAUGE_WORKER_MEMORY,
                "%s %d" % (GAUGE_WORKER_MEMORY, memory_bytes),
                "# TYPE %s gauge" % GAUGE_WORKER_CPU,
                "%s %s" % (GAUGE_WORKER_CPU, cpu_percent),
            ]
        if mode == "optional_present":
            lines += [
                "# TYPE %s gauge" % GAUGE_CPU_WORKER_RSS,
                "%s 20971520" % GAUGE_CPU_WORKER_RSS,
            ]
        if mode == "type_drift":
            # The owner declares a counter; the subject now says gauge.
            lines += ["# TYPE %s gauge" % COUNTER_BODY_BUFFERING]
        else:
            lines += ["# TYPE %s counter" % COUNTER_BODY_BUFFERING]
        lines += [
            "%s %s" % (COUNTER_BODY_BUFFERING, body_total),
            "# TYPE %s counter" % COUNTER_OFFLOAD_SUBMISSIONS,
            "%s %s" % (COUNTER_OFFLOAD_SUBMISSIONS, submissions),
            "# TYPE %s counter" % COUNTER_OFFLOAD_TIMEOUTS,
            "%s 0" % COUNTER_OFFLOAD_TIMEOUTS,
            "# TYPE %s counter" % COUNTER_OFFLOAD_REJECTIONS,
            "%s %s" % (COUNTER_OFFLOAD_REJECTIONS, rejections),
            "# TYPE %s counter" % COUNTER_OFFLOAD_FALLBACKS,
            "%s 0" % COUNTER_OFFLOAD_FALLBACKS,
            "# TYPE %s counter" % COUNTER_WORKER_METRIC_RESETS,
            "%s 0" % COUNTER_WORKER_METRIC_RESETS,
        ]
        body_text = "\n".join(lines) + "\n"
        serve_ms = float(faults.get("serve_ms", 0.0))
        if serve_ms > 0:
            time.sleep(serve_ms / 1000.0)
        with state.lock:
            state.served_ms_total += (time.perf_counter() - started) * 1000.0
        state.record_scrape((time.perf_counter() - started) * 1000.0)
        return body_text.encode()

    def do_GET(self):
        if urlsplit(self.path).path != "/metrics":
            self.send_response(404)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        body = self.exposition()
        self.send_response(200)
        self.send_header("Content-Type", "text/plain; version=0.0.4")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def main():
    parser = argparse.ArgumentParser(description="Synthetic M003 subject stand-in.")
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--metrics-port", type=int, required=True)
    parser.add_argument("--delay-ms", type=float, default=0.0)
    parser.add_argument(
        "--fault-file",
        default="",
        help="Qualification-only telemetry fault descriptor (see module docstring).",
    )
    parser.add_argument(
        "--metrics-log",
        default="",
        help="Per-scrape accounting file used to measure polling overhead.",
    )
    args = parser.parse_args()

    state = State(
        args.port,
        args.metrics_port,
        args.delay_ms,
        args.fault_file,
        args.metrics_log,
    )
    RequestHandler.state = state
    MetricsHandler.state = state

    request_server = ThreadingHTTPServer(("127.0.0.1", args.port), RequestHandler)
    metrics_server = ThreadingHTTPServer(("127.0.0.1", args.metrics_port), MetricsHandler)
    metrics_thread = threading.Thread(
        target=metrics_server.serve_forever, name="metrics", daemon=True
    )
    metrics_thread.start()
    sys.stderr.write(
        "fake-synvoid-m003 request=127.0.0.1:%d metrics=127.0.0.1:%d\n"
        % (args.port, args.metrics_port)
    )
    try:
        request_server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        metrics_server.shutdown()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
