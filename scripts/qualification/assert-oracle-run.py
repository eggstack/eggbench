#!/usr/bin/env python3
"""Assert an external-oracle run really executed under the oracle.

An external load generator is the only independent corroboration the
qualification harnesses have: if the subject's own correctness outcome also
holds when a third-party client drives the same body-bearing case, the result
is not an artifact of the harness's native driver.

That claim is worth nothing unless it is actually checked. A driver that is
absent, exits zero without producing observations, or produces them under a
different producer all look identical to a harness that only checks the exit
status. So this asserts, from the evidence rather than the process status:

* the run command reported success and completed every measured trial;
* each metric the scenario gates absolutely was observed, at its expected
  value, in every measured trial;
* the observations carry a producer label, so the numbers can be attributed to
  the oracle rather than to the native workload driver.

Nothing here interprets a performance metric. Oracle scenarios in the
qualification profiles keep throughput and latency diagnostic, so this asserts
the correctness outcome the scenario actually gates and nothing more.

Usage:
  assert-oracle-run.py --run-json run.json --bundle run.eggb \\
      --expect-zero error_rate --expect-zero expected_outcome_mismatch_rate \\
      [--expect-producer oha] [--min-trials 1]
"""

import argparse
import json
import os
import sys


def fail(message):
    print("assert-oracle-run: %s" % message, file=sys.stderr)
    sys.exit(2)


def observed_metrics(bundle):
    """Per-trial observed values, keyed by trial directory name.

    Only measured trials are returned; a warmup trial is not evidence.
    """
    root = os.path.join(bundle, "trials")
    if not os.path.isdir(root):
        fail("bundle %s published no trials" % bundle)
    per_trial = {}
    for name in sorted(os.listdir(root)):
        metrics = os.path.join(root, name, "metrics.json")
        result = os.path.join(root, name, "result.json")
        if not os.path.isfile(metrics) or not os.path.isfile(result):
            continue
        staged = json.load(open(result))
        if staged.get("kind") == "warmup" or staged.get("phase") == "warmup":
            continue
        document = json.load(open(metrics))
        values, producers = {}, {}
        for observation in document.get("observations", []):
            state = observation.get("state", {})
            if state.get("state") != "observed":
                continue
            values[observation["name"]] = state.get("value")
            producers[observation["name"]] = (
                observation.get("provenance", {}) or {}
            ).get("producer")
        per_trial[name] = (values, producers)
    if not per_trial:
        fail("bundle %s retained no measured trial" % bundle)
    return per_trial


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--run-json", required=True)
    parser.add_argument("--bundle", required=True)
    parser.add_argument("--expect-zero", action="append", default=[])
    parser.add_argument("--expect-producer")
    parser.add_argument("--min-trials", type=int, default=1)
    args = parser.parse_args()

    document = json.load(open(args.run_json))
    if not document.get("ok"):
        fail("run command reported failure: %s" % json.dumps(document)[:300])
    result = document.get("result") or {}
    if result.get("execution_status") != "completed":
        fail(
            "execution_status %s (primary_failure %s)"
            % (result.get("execution_status"), result.get("primary_failure"))
        )
    measured = result.get("measured_trials") or 0
    if measured < args.min_trials:
        fail("run measured %d trials, expected at least %d" % (measured, args.min_trials))

    per_trial = observed_metrics(args.bundle)
    if len(per_trial) < args.min_trials:
        fail(
            "bundle retained %d measured trials, expected at least %d"
            % (len(per_trial), args.min_trials)
        )

    producers = set()
    for trial, (values, trial_producers) in sorted(per_trial.items()):
        for metric in args.expect_zero:
            if metric not in values:
                fail("trial %s did not observe gated metric %s" % (trial, metric))
            if values[metric] != 0:
                fail("trial %s observed %s=%r, expected 0" % (trial, metric, values[metric]))
        for producer in trial_producers.values():
            if producer:
                producers.add(producer)
    if args.expect_producer:
        if not producers:
            fail("no observation carried a producer label")
        if args.expect_producer not in producers:
            fail(
                "expected observations produced by %s, saw %s"
                % (args.expect_producer, ",".join(sorted(producers)))
            )
    print(
        "oracle run: %d measured trials, %d gated metrics at expected value, producers %s"
        % (len(per_trial), len(args.expect_zero), ",".join(sorted(producers)) or "none")
    )


if __name__ == "__main__":
    main()
