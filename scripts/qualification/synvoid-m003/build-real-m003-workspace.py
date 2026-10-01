#!/usr/bin/env python3
"""Build the real-SynVoid M003 qualification workspace (live harness only).

Harness-only wiring. It moves owner-export facts into Eggbench workspace
files and never reinterprets them:

* the owner corpus is translated by the shared pinned translator (owner
  policy id and owner-declared detect/pass statuses only);
* the owner's `telemetry-mapping.json` is copied byte-for-byte and its
  owner-recorded SHA-256 plus the Eggbench workspace content identity are
  recorded in `target-config.json`;
* the controlled-origin route table is derived from the translated pass
  cases plus the M003b load paths;
* the M003c telemetry plans, the M003d load-shape plans, the Eggsec load
  plan, the oracle plan, and the direct-origin control are generated with
  the M003 policy: absolute-zero correctness gates, diagnostic performance
  metrics, and one frozen-policy gated scenario;
* content identities are resolved afterwards by
  `resolve-content-digests.py`, so no digest is invented here.

Usage:
  build-real-m003-workspace.py --export <dir> --workspace <dir>
      --listen-port <p> --origin-port <p> --metrics-port <p>
      --synvoid <binary> --config <dir> --policy <id> --git-sha <sha>
      --mapping-sha256 <hex> --poll-interval-ms <n> [--seed-note <text>]
"""

import argparse
import json
import os
import shutil
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
TRANSLATOR = os.path.join(HERE, "..", "synvoid-m002", "translate-owner-corpus.py")

MAPPING_REF = "telemetry/telemetry-mapping.json"

DETECT_SQLI_BODY = "sqli_invalid_utf8"
DETECT_SSRF_BODY = "ssrf_decoded_private_ip"


def fail(message):
    print("build-real-m003-workspace: %s" % message, file=sys.stderr)
    sys.exit(2)


def write_json(path, document):
    with open(path, "w") as handle:
        json.dump(document, handle, indent=2)
        handle.write("\n")


def http_corpus(target, schedule, concurrency, policy, corpus_ref, headers=None):
    return {
        "kind": "http_corpus",
        "target": target,
        "corpus_ref": corpus_ref,
        "corpus_sha256": "RESOLVED",
        "schedule": [{"case_id": case, "count": count} for case, count in schedule],
        "concurrency": concurrency,
        "connection_policy": policy,
        "default_headers": headers or [],
    }


def absolute_zero(name, unit="ratio"):
    return {
        "name": name,
        "unit": unit,
        "direction": {"kind": "lower_is_better"},
        "intent": "primary",
        "gate": {"kind": "absolute", "value": 0.0},
    }


def diagnostic(name, unit, direction):
    return {
        "name": name,
        "unit": unit,
        "direction": {"kind": direction},
        "intent": "diagnostic",
        "gate": None,
    }


def statistical(name, unit, direction, allowance):
    return {
        "name": name,
        "unit": unit,
        "direction": {"kind": direction},
        "intent": "primary",
        "gate": {
            "kind": "statistical_relative",
            "allowance": allowance,
            "min_trials": 5,
        },
    }


def subject_metric(name, unit):
    return {
        "name": name,
        "unit": unit,
        "direction": {"kind": "informational"},
        "intent": "diagnostic",
        "gate": None,
    }


def standard_metrics():
    return [
        absolute_zero("expected_outcome_mismatch_rate"),
        absolute_zero("transport_error_rate"),
        diagnostic("throughput", "rps", "higher_is_better"),
        diagnostic("latency_p95", "ms", "lower_is_better"),
    ]


def gated_metrics():
    return [
        absolute_zero("expected_outcome_mismatch_rate"),
        absolute_zero("transport_error_rate"),
        statistical("throughput", "rps", "higher_is_better", 1500),
        statistical("latency_p95", "ms", "lower_is_better", 2000),
    ]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--export", required=True)
    parser.add_argument("--workspace", required=True)
    parser.add_argument("--listen-port", required=True, type=int)
    parser.add_argument("--origin-port", required=True, type=int)
    parser.add_argument("--metrics-port", required=True, type=int)
    parser.add_argument("--synvoid", required=True)
    parser.add_argument("--config", required=True)
    parser.add_argument("--policy", required=True)
    parser.add_argument("--git-sha", required=True)
    parser.add_argument("--mapping-sha256", required=True)
    parser.add_argument("--poll-interval-ms", default=500, type=int)
    parser.add_argument("--telemetry-timeout-ms", default=30000, type=int)
    args = parser.parse_args()

    workspace = args.workspace
    scenarios = os.path.join(workspace, "scenarios")
    os.makedirs(scenarios, exist_ok=True)
    os.makedirs(os.path.join(workspace, "telemetry"), exist_ok=True)

    provenance = json.load(open(os.path.join(args.export, "provenance.json")))
    detect_status = provenance["detect_status"]
    pass_status = provenance["pass_status"]

    # ---- Owner telemetry mapping, byte-for-byte ----------------------------
    owner_mapping = os.path.join(args.export, "telemetry-mapping.json")
    shutil.copyfile(owner_mapping, os.path.join(workspace, MAPPING_REF))
    shutil.copyfile(
        os.path.join(args.export, "telemetry-contract.json"),
        os.path.join(workspace, "telemetry/telemetry-contract.json"),
    )
    mapping = json.load(open(os.path.join(workspace, MAPPING_REF)))
    required_fields = [f["output_name"] for f in mapping["fields"] if f.get("required")]
    optional_fields = [f["output_name"] for f in mapping["fields"] if not f.get("required")]
    # Units and trial aggregation come from the owner mapping; Eggbench never
    # invents a subject metric name or unit. Subject metrics stay diagnostic:
    # no target-resource gate is frozen from a single hosted run (M003c §8).
    subject_units = {f["output_name"]: f["unit"] for f in mapping["fields"]}

    # ---- Owner corpus, translated mechanically -----------------------------
    load_corpus_path = os.path.join(workspace, "corpus.json")
    rc = subprocess.call(
        [
            sys.executable,
            TRANSLATOR,
            os.path.join(args.export, "corpus.json"),
            os.path.join(args.export, "provenance.json"),
            args.policy,
            "synvoid-waf-owner-m003-v1",
            "SynVoid owner export %s (%s)" % (args.policy, args.git_sha),
            load_corpus_path,
        ]
    )
    if rc != 0:
        fail("owner corpus translation failed")
    correctness_corpus_path = os.path.join(workspace, "correctness-corpus.json")
    shutil.copyfile(load_corpus_path, correctness_corpus_path)
    load_corpus = json.load(open(load_corpus_path))

    pass_ids = [
        case["id"]
        for case in load_corpus["cases"]
        if case["expectation"] == {"status_exact": pass_status}
    ]
    detect_ids = [
        case["id"]
        for case in load_corpus["cases"]
        if case["expectation"] == {"status_any_of": [detect_status]}
    ]
    if not pass_ids or not detect_ids:
        fail("owner export has no pass/detect cases")
    # The owner export's body-bearing case is a pass case; its detect cases are
    # request-line cases. M003d needs body-bearing load and an exact mixed
    # schedule, not a body-bearing blocked case, so the mixed schedule pairs
    # the body pass case with owner detect cases.
    body_pass_ids = [
        case["id"]
        for case in load_corpus["cases"]
        if case["expectation"] == {"status_exact": pass_status}
        and case["request"]["method"] == "POST"
    ]
    if not body_pass_ids:
        fail("owner export has no body-bearing pass case for the M003 load shapes")
    body_case = body_pass_ids[0]
    blocked_case = detect_ids[0]

    # ---- Controlled-origin routes derived from the owner pass cases --------
    routes = {}
    for case in load_corpus["cases"]:
        if case["expectation"] != {"status_exact": pass_status}:
            continue
        path = case["request"]["path_and_query"].split("?", 1)[0]
        routes[path] = {"status": pass_status, "body_bytes": 1024}
    routes["/qualbench/small"] = {"status": pass_status, "body_bytes": 1024}
    routes["/qualbench/stream"] = {"status": pass_status, "body_bytes": 65536}
    write_json(os.path.join(workspace, "routes.json"), routes)

    target_config = {
        "contract": "synvoid-qualification-target.v1",
        "note": "Real SynVoid-owned export with the M003 telemetry contract (live harness).",
        "policy_id": args.policy,
        "expected_tree": [
            "config/main.toml",
            "config/sites/loopback.qualification.local.toml",
            MAPPING_REF,
        ],
        "listen": {"host": "127.0.0.1", "port": args.listen_port},
        "origin": {"url": "http://127.0.0.1:%d" % args.origin_port},
        "metrics": {
            "host": "127.0.0.1",
            "port": args.metrics_port,
            "scrape_url": "http://127.0.0.1:%d/metrics" % args.metrics_port,
        },
        "mapping": {
            "detect": {"status_any_of": [detect_status]},
            "pass": {"status_exact": pass_status},
        },
        "telemetry": {
            "contract_id": provenance["telemetry_contract_id"],
            "owner_mapping_sha256": args.mapping_sha256,
            "mapping_ref": MAPPING_REF,
            "required_fields": required_fields,
            "optional_fields": optional_fields,
            "poll_interval_ms": args.poll_interval_ms,
        },
        "provenance": {
            "synvoid_git_sha": provenance["synvoid_git_sha"],
            "materializer": provenance["materializer"],
            "generated_config_sha256": provenance["generated_config_sha256"],
            "generated_corpus_sha256": provenance["generated_corpus_sha256"],
            "site_config_sha256": provenance["site_config_sha256"],
            "telemetry_contract_digest": provenance["telemetry_contract_digest"],
            "telemetry_mapping_digest": provenance["telemetry_mapping_digest"],
        },
    }
    write_json(os.path.join(workspace, "target-config.json"), target_config)

    # ---- Topology ---------------------------------------------------------
    # The owner subject and its controlled origin are the single long-running
    # instances the harness already started from the owner export. Declaring
    # them external keeps every scenario on that one instance: a per-scenario
    # managed child would contend for the owner metrics port and its telemetry
    # would then describe a different process than the one under test.
    origin_service = {
        "name": "origin",
        "kind": {
            "kind": "command",
            "argv": [
                "./controlled-origin-m003.py",
                "--port",
                str(args.origin_port),
                "--routes-json",
                "routes.json",
                "--log-file",
                "origin-requests.log",
                "--delay-signal-file",
                "origin-delay-ms",
            ],
        },
        "lifecycle": "external",
        "depends_on": [],
        "config": {},
        "http_url": "http://127.0.0.1:%d/api/users" % args.origin_port,
        "readiness": None,
        "shutdown": None,
        "working_directory": None,
        "log_limit_bytes": 65536,
    }
    subject_service = {
        "name": "synvoid",
        "kind": {
            "kind": "command",
            "argv": [args.synvoid, "--foreground", "--config-path", args.config],
        },
        "lifecycle": "external",
        "depends_on": ["origin"],
        "config": {},
        "http_url": "http://127.0.0.1:%d/" % args.listen_port,
        "readiness": None,
        "shutdown": None,
        "working_directory": None,
        "log_limit_bytes": 262144,
    }
    telemetry_source = {
        "name": "subject-metrics",
        "kind": {"kind": "named", "service_type": "prometheus-http"},
        "lifecycle": "external",
        "depends_on": [],
        "config": {
            "target_service": "subject-metrics",
            "binding_key": "http_url",
            "mapping_ref": MAPPING_REF,
            "mapping_sha256": "RESOLVED",
            "poll_interval_ms": str(args.poll_interval_ms),
        },
        "http_url": "http://127.0.0.1:%d/metrics" % args.metrics_port,
        "readiness": None,
        "shutdown": None,
        "working_directory": None,
        "log_limit_bytes": 65536,
    }

    def plan(
        name,
        experiment,
        workload,
        metrics,
        seed,
        measured=3,
        warmup=1,
        measurement_ms=120000,
        with_telemetry=False,
        telemetry_fields=None,
        corpus_checks=False,
    ):
        services = [subject_service, origin_service]
        telemetry = []
        if with_telemetry:
            services.append(telemetry_source)
            telemetry = [
                {
                    "source": "prometheus-http",
                    "fields": telemetry_fields or required_fields,
                    "required": True,
                }
            ]
        document = {
            "schema_version": 10 if not corpus_checks else 9,
            "experiment": experiment,
            "subject": {"kind": "label", "label": "synvoid-real-minimal"},
            "services": services,
            "workload": workload,
            "trials": {
                "measured": measured,
                "warmup": warmup,
                "cooldown_ms": 50,
                "reset": {"kind": "none"},
                "timeouts": {
                    "measurement": measurement_ms,
                    "drain": 10000,
                    "telemetry": args.telemetry_timeout_ms,
                },
            },
            "telemetry": telemetry,
            "metrics": metrics,
            "environment_policy": {"kind": "strict_same_testbed"},
            "seed": seed,
            "bounds": {
                "artifact_count": 256,
                "artifact_bytes": 16777216,
                "total_bytes": 268435456,
            },
        }
        if corpus_checks:
            document["http_corpus_checks"] = [
                {
                    "id": "synvoid-m003-correctness",
                    "source": "eggbench-http-corpus",
                    "target": "synvoid",
                    "corpus_ref": "correctness-corpus.json",
                    "corpus_sha256": "RESOLVED",
                    "timeout_ms": 120000,
                    "case_timeout_ms": 10000,
                }
            ]
        write_json(os.path.join(scenarios, name), document)

    # Exact 80/20 benign/malicious mixture: 24 + 4 benign-body requests
    # against 4 + 2 owner-blocked requests.
    mixed = [[body_case, 24], [detect_ids[1], 4], [blocked_case, 2]]
    plan(
        "waf-correctness.json",
        "synvoid-m003-correctness-real",
        {"kind": "finite_count", "target": "synvoid", "requests": 4, "concurrency": 1},
        [],
        61,
        measured=1,
        warmup=0,
        measurement_ms=20000,
        corpus_checks=True,
    )
    plan(
        "body-pooled-c8.json",
        "synvoid-m003-body-pooled-c8-real",
        http_corpus("synvoid", [[body_case, 40]], 8, "pooled", "corpus.json"),
        standard_metrics(),
        62,
    )
    plan(
        "body-gated-c8.json",
        "synvoid-m003-body-gated-c8-real",
        http_corpus("synvoid", [[body_case, 800]], 8, "pooled", "corpus.json"),
        gated_metrics(),
        63,
        # The only gated scenario carries the whole M003d performance claim, so
        # it is measured more often than the diagnostic scenarios: a narrower
        # bootstrap interval is what makes "pass" distinguishable from host
        # noise on a shared runner.
        measured=9,
    )
    plan(
        "body-fresh-c8.json",
        "synvoid-m003-body-fresh-c8-real",
        http_corpus("synvoid", [[body_case, 40]], 8, "fresh_per_request", "corpus.json"),
        standard_metrics(),
        64,
    )
    plan(
        "mixed-80-20-pooled-c8.json",
        "synvoid-m003-mixed-80-20-pooled-c8-real",
        http_corpus("synvoid", mixed, 8, "pooled", "corpus.json"),
        standard_metrics(),
        65,
    )
    plan(
        "mixed-80-20-fresh-c8.json",
        "synvoid-m003-mixed-80-20-fresh-c8-real",
        http_corpus("synvoid", mixed, 8, "fresh_per_request", "corpus.json"),
        standard_metrics(),
        66,
    )
    plan(
        "mixed-80-20-pooled-c32.json",
        "synvoid-m003-mixed-80-20-pooled-c32-real",
        http_corpus("synvoid", [[c, n * 4] for c, n in mixed], 32, "pooled", "corpus.json"),
        standard_metrics(),
        67,
    )
    # The telemetry trials are long and moderately concurrent on purpose: they
    # must span many poll cadences (a short trial retains too few in-window
    # samples to be subject evidence) and at least one full owner refresh
    # interval, because an owner that publishes its series on a slower cadence
    # than the trial would only ever show its start-of-process values.
    plan(
        "telemetry-pressure-c8.json",
        "synvoid-m003-target-telemetry-pressure-c8-real",
        http_corpus(
            "synvoid", [[c, n * 32] for c, n in mixed], 8, "pooled", "corpus.json"
        ),
        standard_metrics()
        + [subject_metric(field, subject_units[field]) for field in required_fields],
        68,
        with_telemetry=True,
    )
    plan(
        "telemetry-optional-c8.json",
        "synvoid-m003-target-telemetry-optional-c8-real",
        http_corpus(
            "synvoid", [[c, n * 32] for c, n in mixed], 8, "pooled", "corpus.json"
        ),
        standard_metrics()
        + [
            subject_metric(field, subject_units[field])
            for field in required_fields + optional_fields
        ],
        69,
        with_telemetry=True,
        telemetry_fields=required_fields + optional_fields,
    )
    plan(
        "control-origin-body-c8.json",
        "origin-m003-body-control-c8-real",
        http_corpus("origin", [[body_case, 40]], 8, "pooled", "corpus.json"),
        standard_metrics(),
        70,
    )
    plan(
        "body-oha-c8.json",
        "synvoid-m003-body-oha-c8-real",
        http_corpus("synvoid", [[body_case, 40]], 8, "pooled", "corpus.json"),
        standard_metrics(),
        71,
    )
    plan(
        "eggsec-benign-body-c8.json",
        "eggsec-m003-benign-body-c8-real",
        http_corpus("synvoid", [[body_case, 40]], 8, "pooled", "corpus.json"),
        standard_metrics(),
        72,
    )
    plan(
        "eggsec-blocked-c8.json",
        "eggsec-m003-blocked-c8-real",
        http_corpus("synvoid", [[blocked_case, 40]], 8, "pooled", "corpus.json"),
        standard_metrics(),
        73,
    )

    smoke = {
        "schema_version": 2,
        "id": "synvoid-m003-smoke-real-v1",
        "owner": "SynVoid owner export %s (%s)" % (args.policy, args.git_sha),
        "scenarios": [
            {"id": "synvoid-m003-correctness", "plan": "scenarios/waf-correctness.json"},
            {
                "id": "synvoid-m003-body-pooled-c8",
                "plan": "scenarios/body-pooled-c8.json",
                "workload_driver": "eggfetch-http",
            },
            {
                "id": "synvoid-m003-body-fresh-c8",
                "plan": "scenarios/body-fresh-c8.json",
                "workload_driver": "eggfetch-http",
            },
            {
                "id": "synvoid-m003-mixed-80-20-pooled-c8",
                "plan": "scenarios/mixed-80-20-pooled-c8.json",
                "workload_driver": "eggfetch-http",
            },
            {
                "id": "origin-m003-body-control-c8",
                "plan": "scenarios/control-origin-body-c8.json",
                "workload_driver": "eggfetch-http",
            },
            {
                "id": "synvoid-m003-target-telemetry-pressure-c8",
                "plan": "scenarios/telemetry-pressure-c8.json",
                "workload_driver": "eggfetch-http",
            },
        ],
        "corpus": {"path": "corpus.json"},
        "target_config": {"path": "target-config.json"},
    }
    write_json(os.path.join(workspace, "smoke.profile.json"), smoke)

    perf_scenarios = [
        ("synvoid-m003-body-gated-c8", "body-gated-c8"),
        ("synvoid-m003-body-pooled-c8", "body-pooled-c8"),
        ("synvoid-m003-body-fresh-c8", "body-fresh-c8"),
        ("synvoid-m003-mixed-80-20-pooled-c8", "mixed-80-20-pooled-c8"),
        ("synvoid-m003-mixed-80-20-fresh-c8", "mixed-80-20-fresh-c8"),
        ("synvoid-m003-mixed-80-20-pooled-c32", "mixed-80-20-pooled-c32"),
        ("synvoid-m003-target-telemetry-pressure-c8", "telemetry-pressure-c8"),
        ("origin-m003-body-control-c8", "control-origin-body-c8"),
    ]
    perf = {
        "schema_version": 2,
        "id": "synvoid-m003-perf-real-v1",
        "owner": smoke["owner"],
        "scenarios": [{"id": "synvoid-m003-correctness", "plan": "scenarios/waf-correctness.json"}]
        + [
            {
                "id": scenario_id,
                "plan": "scenarios/%s.json" % plan_name,
                "baseline_bundle": "baselines/%s.eggb" % plan_name,
                "workload_driver": "eggfetch-http",
            }
            for scenario_id, plan_name in perf_scenarios
        ],
        "corpus": {"path": "corpus.json"},
        "target_config": {"path": "target-config.json"},
    }
    write_json(os.path.join(workspace, "perf.profile.json"), perf)
    os.makedirs(os.path.join(workspace, "baselines"), exist_ok=True)
    print(
        "real M003 workspace ready: %d cases, %d routes, %d required + %d optional owner samples"
        % (len(load_corpus["cases"]), len(routes), len(required_fields), len(optional_fields))
    )


if __name__ == "__main__":
    main()
