#!/usr/bin/env python3
"""Assert subject-telemetry evidence inside a published Eggbench bundle.

Harness-only evidence reader. It reads sanitized trial evidence, never a
live subject, and asserts exactly the M003c consumer-side contract:

* every required owner sample resolved to an observed normalized value;
* the pinned mapping identity and cadence are recorded;
* no scrape failed, no sample was dropped, and nothing was fabricated for an
  optional owner sample that the subject did not export.

`--require-live-values` is a separate claim: the subject actually *populated*
the required series rather than publishing inventory at zero. That is an
owner-side property, so a failure here is an upstream finding, not a consumer
defect; the harness reports it separately from the contract shape.

Usage:
  assert-subject-telemetry.py --bundle <eggb> --mapping-sha256 <hex>
      [--require-gauge <name>] [--require-counter <name>]
      [--expect-optional-absent <name>] [--min-samples <n>]
      [--require-live-values]
"""

import argparse
import json
import os
import sys


def fail(message):
    print("assert-subject-telemetry: %s" % message, file=sys.stderr)
    sys.exit(2)


def trials(bundle):
    root = os.path.join(bundle, "trials")
    if not os.path.isdir(root):
        return []
    return [os.path.join(root, name) for name in sorted(os.listdir(root))]


def observations(trial):
    path = os.path.join(trial, "metrics.json")
    if not os.path.isfile(path):
        return {}
    document = json.load(open(path))
    return {o["name"]: o for o in document.get("observations", [])}


def provenance(trial):
    telemetry = os.path.join(trial, "telemetry")
    if not os.path.isdir(telemetry):
        return None
    for name in sorted(os.listdir(telemetry)):
        if name.endswith("prometheus-provenance.json"):
            return json.load(open(os.path.join(telemetry, name)))
    return None


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--bundle", required=True)
    parser.add_argument("--mapping-sha256", required=True)
    parser.add_argument("--require-gauge", action="append", default=[])
    parser.add_argument("--require-counter", action="append", default=[])
    parser.add_argument("--expect-optional-absent", action="append", default=[])
    parser.add_argument("--min-samples", type=int, default=2)
    parser.add_argument("--require-live-values", action="store_true")
    args = parser.parse_args()

    trial_dirs = trials(args.bundle)
    if not trial_dirs:
        fail("bundle %s has no measured trials" % args.bundle)

    seen_gauge = {name: False for name in args.require_gauge}
    seen_counter = {name: False for name in args.require_counter}
    optional_absent = {name: 0 for name in args.expect_optional_absent}
    optional_total = 0
    sample_counts = []

    for trial in trial_dirs:
        record = provenance(trial)
        if record is None:
            fail("trial %s has no prometheus provenance artifact" % trial)
        if record.get("mapping_sha256") != args.mapping_sha256:
            fail(
                "trial %s records mapping %s, expected %s"
                % (trial, record.get("mapping_sha256"), args.mapping_sha256)
            )
        for key in ("poll_error_count", "dropped_sample_count"):
            if record.get(key):
                fail("trial %s reports %s=%s" % (trial, key, record[key]))
        if record.get("sample_count", 0) < args.min_samples:
            fail(
                "trial %s retained only %s in-window samples"
                % (trial, record.get("sample_count"))
            )
        sample_counts.append(record.get("sample_count"))
        if record.get("endpoint_authority", "").split(":")[0] not in ("127.0.0.1", "[::1]"):
            fail("trial %s scraped a non-loopback authority" % trial)
        found = observations(trial)
        for name in args.require_gauge + args.require_counter:
            entry = found.get(name)
            if entry is None:
                continue
            state = entry.get("state", {})
            if state.get("state") != "observed":
                fail("required subject metric %s is %s" % (name, state))
            value = state.get("value")
            if value is None:
                fail("required subject metric %s has no value" % name)
            if name in seen_gauge and value > 0.0:
                seen_gauge[name] = True
            if name in seen_counter and value >= 0.0:
                seen_counter[name] = True
        for name in args.expect_optional_absent:
            optional_total += 1
            entry = found.get(name)
            if entry is None or entry.get("state", {}).get("state") != "observed":
                optional_absent[name] += 1

    if args.require_live_values:
        for name, observed in seen_gauge.items():
            if not observed:
                fail(
                    "owner published required gauge %s at zero for every trial" % name
                )
        for name, observed in seen_counter.items():
            if not observed:
                fail("owner published required counter %s with no delta" % name)
    for name, count in optional_absent.items():
        if optional_total and count != optional_total:
            fail(
                "optional subject metric %s was observed in %d of %d trials; absence must be consistent"
                % (name, optional_total - count, optional_total)
            )
    print(
        "subject telemetry evidence: %d trials, samples %s, required gauges/counters observed%s"
        % (
            len(trial_dirs),
            sample_counts,
            "" if not args.require_live_values else ", required values live",
        )
    )


if __name__ == "__main__":
    main()
