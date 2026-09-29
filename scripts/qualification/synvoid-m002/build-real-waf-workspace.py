#!/usr/bin/env python3
"""Build the real-SynVoid correctness workspace (C002 Stage B).

Harness-only wiring: translates the owner export into an Eggbench corpus,
derives the harness-origin routes from the translated pass cases, records
the owner export identity in target-config.json, and emits the real
correctness scenario + profile with the Eggbench content identity the
scenario's corpus_sha256 must carry.

Nothing here reinterprets owner semantics: the translator enforces the
pinned policy_id and the owner-declared detect/pass statuses; this script
only moves those facts into Eggbench workspace files.

Usage:
  build-real-waf-workspace.py <export-dir> <workspace-dir> <lport> <oport> \\
      <synvoid-binary> <export-config-dir> <policy-id> <git-sha> \\
      <translator.py>

Workspace layout out:
  <workspace-dir>/corpus.json target-config.json routes.json
  <workspace-dir>/scenarios/real-waf-correctness.json real-profile.json
"""

import hashlib
import json
import shutil
import subprocess
import sys


def fail(message):
    print("build-real-waf-workspace: %s" % message, file=sys.stderr)
    sys.exit(2)


def main():
    if len(sys.argv) != 10:
        fail("expected 9 arguments, got %d" % (len(sys.argv) - 1))
    (export_dir, ws, lport, oport, synbin, cfgdir,
     policy, sha, translator) = sys.argv[1:]
    import os
    os.makedirs(ws + "/scenarios", exist_ok=True)

    corpus_id = "synvoid-waf-owner-v1"
    owner_label = "SynVoid owner export %s (%s)" % (policy, sha)
    rc = subprocess.call([sys.executable, translator,
                          export_dir + "/corpus.json",
                          export_dir + "/provenance.json",
                          policy, corpus_id, owner_label,
                          ws + "/corpus.json"])
    if rc != 0:
        fail("translator failed")

    corpus = json.load(open(ws + "/corpus.json"))
    provenance = json.load(open(export_dir + "/provenance.json"))

    routes = {}
    for case in corpus["cases"]:
        if case["expectation"] == {"status_exact": provenance["pass_status"]}:
            path = case["request"]["path_and_query"].split("?", 1)[0]
            routes[path] = {"status": 200, "body_bytes": 1024}
    routes["/qualbench/small"] = {"status": 200, "body_bytes": 1024}
    routes["/qualbench/stream"] = {"status": 200, "body_bytes": 65536}
    json.dump(routes, open(ws + "/routes.json", "w"), indent=2)

    target_config = {
        "contract": "synvoid-qualification-target.v1",
        "note": "Real SynVoid-owned export (C002 Stage B).",
        "policy_id": policy,
        "expected_tree": ["config/main.toml",
                          "config/sites/loopback.qualification.local.toml"],
        "listen": {"host": "127.0.0.1", "port": int(lport)},
        "origin": {"url": "http://127.0.0.1:%s" % oport},
        "mapping": {
            "detect": {"status_any_of": [provenance["detect_status"]]},
            "pass": {"status_exact": provenance["pass_status"]},
        },
        "provenance": {
            "synvoid_git_sha": provenance["synvoid_git_sha"],
            "materializer": provenance["materializer"],
            "generated_config_sha256": provenance["generated_config_sha256"],
            "generated_corpus_sha256": provenance["generated_corpus_sha256"],
            "site_config_sha256": provenance["site_config_sha256"],
        },
    }
    json.dump(target_config, open(ws + "/target-config.json", "w"), indent=2)

    scenario = {
        "schema_version": 8,
        "experiment": "synvoid-waf-correctness-real",
        "subject": {"kind": "label", "label": "synvoid-real-minimal"},
        "services": [
            {"name": "origin",
             "kind": {"kind": "command", "argv": [
                 "./controlled-origin.py", "--port", oport,
                 "--routes-json", "routes.json",
                 "--log-file", "origin-requests.log"]},
             "lifecycle": "managed", "depends_on": [], "config": {},
             "http_url": "http://127.0.0.1:%s/" % oport,
             "readiness": {"kind": "probe", "probe": "tcp-loopback",
                           "timeout_ms": 20000},
             "shutdown": {"grace_ms": 3000, "method": None},
             "working_directory": None, "log_limit_bytes": 65536},
            {"name": "synvoid",
             "kind": {"kind": "command", "argv": [
                 synbin, "--foreground", "--config-path", cfgdir]},
             "lifecycle": "managed", "depends_on": ["origin"], "config": {},
             "http_url": "http://127.0.0.1:%s/" % lport,
             "readiness": {"kind": "probe", "probe": "tcp-loopback",
                           "timeout_ms": 20000},
             "shutdown": {"grace_ms": 3000, "method": None},
             "working_directory": None, "log_limit_bytes": 65536},
        ],
        "workload": {"kind": "finite_count", "target": "synvoid",
                     "requests": 4, "concurrency": 1},
        "trials": {"measured": 1, "warmup": 0, "cooldown_ms": 1,
                   "reset": {"kind": "none"},
                   "timeouts": {"measurement": 10000, "drain": 5000}},
        "telemetry": [], "metrics": [],
        "environment_policy": {"kind": "strict_same_testbed"}, "seed": 11,
        "http_corpus_checks": [
            {"id": "synvoid-waf", "source": "eggbench-http-corpus",
             "target": "synvoid", "corpus_ref": "corpus.json",
             "corpus_sha256": "COMPUTE",
             "timeout_ms": 60000, "case_timeout_ms": 5000}
        ],
        "bounds": {"artifact_count": 256, "artifact_bytes": 16777216,
                   "total_bytes": 268435456},
    }
    raw = open(ws + "/corpus.json", "rb").read()
    filesha = hashlib.sha256(raw).hexdigest()
    digest = hashlib.sha256()
    digest.update(b"corpus.json")
    digest.update(b"\x00")
    digest.update(len(raw).to_bytes(8, "big"))
    digest.update(b"\x00")
    digest.update(filesha.encode())
    digest.update(b"\x00")
    scenario["http_corpus_checks"][0]["corpus_sha256"] = digest.hexdigest()
    json.dump(scenario,
              open(ws + "/scenarios/real-waf-correctness.json", "w"), indent=2)

    profile = {
        "schema_version": 1,
        "id": "synvoid-waf-correctness-real-v1",
        "owner": owner_label,
        "scenarios": [{"id": "synvoid-waf-correctness-real",
                       "plan": "scenarios/real-waf-correctness.json"}],
        "corpus": {"path": "corpus.json"},
        "target_config": {"path": "target-config.json"},
    }
    json.dump(profile, open(ws + "/real-profile.json", "w"), indent=2)
    print("real workspace ready: %d cases, %d routes"
          % (len(corpus["cases"]), len(routes)))


if __name__ == "__main__":
    main()
