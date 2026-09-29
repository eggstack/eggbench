#!/usr/bin/env python3
"""Assert the blocked-request proof from a harness-origin request log.

Reads the translated Eggbench corpus (owner expectations) and the
controlled-origin request log (METHOD TARGET STATUS lines) and fails
closed unless:

- every pass case's path was served 200 by the origin (the request
  provably traversed the real proxy to the controlled origin);
- no detect case's path appears in the log at all (blocked requests
  provably never reached the origin).

Usage:
  assert-origin-log.py <translated-corpus.json> <detect-status> <origin-log>
"""

import json
import sys


def fail(message):
    print("assert-origin-log: %s" % message, file=sys.stderr)
    sys.exit(2)


def main():
    if len(sys.argv) != 4:
        fail("expected 3 arguments, got %d" % (len(sys.argv) - 1))
    corpus_path, detect_status, log_path = sys.argv[1:]
    detect_status = int(detect_status)
    corpus = json.load(open(corpus_path))
    try:
        lines = open(log_path).read().splitlines()
    except OSError as exc:
        fail("cannot read origin log: %s" % exc)

    # Keyed by full request-target: a detect case may share a path prefix
    # (or path) with a pass case while carrying a different query, so
    # path-only matching would false-positive. Pass cases are matched on
    # path with a 200; detect cases must be absent as full targets.
    served_paths = {}
    served_targets = set()
    for line in lines:
        parts = line.split(" ")
        if len(parts) != 3:
            fail("malformed log line %r" % (line,))
        method, target, status = parts[0], parts[1], int(parts[2])
        served_paths.setdefault(target.split("?", 1)[0], []).append(status)
        served_targets.add("%s %s" % (method, target))

    pass_paths, detect_targets = [], []
    for case in corpus["cases"]:
        target = case["request"]["path_and_query"]
        expectation = case["expectation"]
        if expectation == {"status_any_of": [detect_status]}:
            detect_targets.append((case["id"], target))
        else:
            pass_paths.append((case["id"], target.split("?", 1)[0]))

    for case_id, path in pass_paths:
        hits = [status for status in served_paths.get(path, [])
                if status == 200]
        if not hits:
            fail("pass case %r path %r never served 200 by origin" % (case_id, path))
    violations = []
    for case_id, target in detect_targets:
        method = next(c["request"]["method"] for c in corpus["cases"]
                      if c["id"] == case_id)
        if "%s %s" % (method, target) in served_targets:
            violations.append(case_id)
    if violations:
        fail("blocked requests reached origin: %s" % "; ".join(violations))
    print("origin-log proof ok: %d pass paths served 200, %d detect targets absent"
          % (len(pass_paths), len(detect_targets)))


if __name__ == "__main__":
    main()
