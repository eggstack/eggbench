#!/usr/bin/env python3
"""Generate real-SynVoid perf/control/oracle scenarios from synthetic ones.

Harness-only adaptation (C002 production-change policy): rewrites service
wiring (origin adapter -> harness controlled origin; stub subject -> real
minimal binary) and target paths (/bench/* -> /qualbench/*). Everything
else — workload shape, trial counts, metrics, gates, seeds, bounds — is
copied verbatim so the frozen v1 policy is never weakened.

Path map (owner Workstream H contract):
  /bench/small -> /qualbench/small
  /bench/large -> /qualbench/stream

Usage:
  generate-real-scenarios.py <synthetic-scenarios-dir> <out-dir> \\
      <lport> <oport> <synvoid-binary> <export-config-dir>

Writes one <name>.json per scenario listed in REAL_SCENARIOS plus
real-smoke.profile.json / real-perf.profile.json descriptors are left to
the caller (scenario IDs are stable: <base>-real).
"""

import copy
import json
import sys

REAL_SCENARIOS = [
    "smoke-small", "smoke-large",
    "perf-small-c1", "perf-small-c8", "perf-small-c32",
    "perf-large-c1", "perf-large-c8", "perf-large-c32",
    "control-small", "control-large",
    "oracle-oha-c8", "oracle-h2load-c8",
]

PATH_MAP = {
    "/bench/small": "/qualbench/small",
    "/bench/large": "/qualbench/stream",
}


def fail(message):
    print("generate-real-scenarios: %s" % message, file=sys.stderr)
    sys.exit(2)


def map_path(url, owner):
    if owner not in PATH_MAP:
        fail("unknown synthetic path %r" % (owner,))
    return url.replace(owner, PATH_MAP[owner])


def main():
    if len(sys.argv) != 7:
        fail("expected 6 arguments, got %d" % (len(sys.argv) - 1))
    src_dir, out_dir, lport, oport, synbin, cfgdir = sys.argv[1:]
    import os
    os.makedirs(out_dir, exist_ok=True)

    for name in REAL_SCENARIOS:
        with open("%s/%s.json" % (src_dir, name)) as handle:
            plan = json.load(handle)
        plan = copy.deepcopy(plan)
        plan["experiment"] = plan["experiment"].replace("synthetic", "real")
        if plan.get("subject", {}).get("label") == "synvoid-synthetic-stand-in":
            plan["subject"]["label"] = "synvoid-real-minimal"
        target = plan["workload"]["target"]
        for service in plan["services"]:
            if service["name"] == "origin":
                service["kind"] = {"kind": "command", "argv": [
                    "./controlled-origin.py", "--port", oport,
                    "--routes-json", "routes.json", "--log-file",
                    "origin-requests-%s.log" % name,
                ]}
                service["config"] = {}
                if target == "origin":
                    # Direct-origin control: the workload hits the origin
                    # binding, so the static URL must carry the mapped
                    # control path.
                    base_url = service.get("http_url") or ""
                    service["http_url"] = "http://127.0.0.1:%s%s" % (
                        oport, mapped_control_path(src_dir, name))
                else:
                    service["http_url"] = "http://127.0.0.1:%s/" % oport
                service["readiness"] = {
                    "kind": "probe", "probe": "tcp-loopback", "timeout_ms": 20000}
            elif service["name"] == "synvoid":
                service["kind"] = {"kind": "command", "argv": [
                    synbin, "--foreground", "--config-path", cfgdir]}
                old_url = service["http_url"]
                new_url = old_url.replace("127.0.0.1:18080",
                                          "127.0.0.1:%s" % lport)
                new_url = map_path(new_url, "/bench/small")
                new_url = map_path(new_url, "/bench/large")
                if "/bench/" in new_url:
                    fail("%s: unmapped /bench/ path in %r" % (name, new_url))
                service["http_url"] = new_url
                service["readiness"] = {
                    "kind": "probe", "probe": "tcp-loopback", "timeout_ms": 20000}
        with open("%s/%s-real.json" % (out_dir, name), "w") as handle:
            json.dump(plan, handle, indent=2)
    print("generated %d real scenarios" % len(REAL_SCENARIOS))


def mapped_control_path(src_dir, name):
    with open("%s/%s.json" % (src_dir, name)) as handle:
        plan = json.load(handle)
    for service in plan["services"]:
        if service["name"] == "origin":
            config = service.get("config", {})
            if isinstance(config, dict) and "path" in config:
                path = config["path"]
                for old, new in PATH_MAP.items():
                    if old in path:
                        return path.replace(old, new)
                fail("%s: unmapped control path %r" % (name, path))
    fail("%s: no origin service with path config" % name)


if __name__ == "__main__":
    main()
