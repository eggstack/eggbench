#!/usr/bin/env python3
"""Translate a SynVoid owner qualification export into an Eggbench corpus.

Mechanical import-layer adaptation (C002 production-change policy: harness
only). Every semantic fact comes from the owner export + provenance:

- case identity/category/method/path/query/headers/body are copied losslessly;
- per-case expected status must equal the owner-declared detect or pass wire
  status from provenance; mapping shape mirrors the synthetic M002a contract
  (detect -> status_any_of[403], pass -> status_exact 200) using the
  owner-declared statuses, never invented ones;
- raw ASCII SP inside a request-target is %20-encoded on the wire per the
  owner transport rule (policy_v1.json `transport_rules`); any other byte
  <= 0x20 in the target fails closed because Eggbench forbids it;
- file bodies fail closed (v1 exports inline bodies only).

Usage:
  translate-owner-corpus.py <owner-corpus.json> <provenance.json> \\
      <expected-policy-id> <corpus-id> <owner-label> <out-corpus.json>

Exit 0 on success; exit 2 with an error on stderr on any mismatch.
"""

import json
import sys


def fail(message):
    print("translate-owner-corpus: %s" % message, file=sys.stderr)
    sys.exit(2)


def main():
    if len(sys.argv) != 7:
        fail("expected 6 arguments, got %d" % (len(sys.argv) - 1))
    corpus_path, provenance_path, expected_policy, corpus_id, owner_label, out_path = sys.argv[1:]

    try:
        corpus = json.load(open(corpus_path))
        provenance = json.load(open(provenance_path))
    except (OSError, ValueError) as exc:
        fail("unparseable input: %s" % exc)

    for doc, name in ((corpus, "corpus"), (provenance, "provenance")):
        if doc.get("policy_id") != expected_policy:
            fail("%s policy_id %r != pinned %r" % (name, doc.get("policy_id"), expected_policy))
    if corpus.get("policy_id") != provenance.get("policy_id"):
        fail("corpus/provenance policy_id disagree")

    detect_status = provenance.get("detect_status")
    pass_status = provenance.get("pass_status")
    if not isinstance(detect_status, int) or not isinstance(pass_status, int):
        fail("provenance detect/pass statuses are not integers")

    cases = corpus.get("cases")
    if not isinstance(cases, list) or not cases:
        fail("owner corpus has no cases")

    out_cases = []
    encoded_targets = 0
    for case in cases:
        case_id = case.get("id")
        method = case.get("method")
        path = case.get("path")
        expected = case.get("expected_status")
        if not case_id or not method or not path or not isinstance(expected, int):
            fail("owner case missing id/method/path/expected_status: %r" % (case,))
        if expected == detect_status:
            expectation = {"status_any_of": [detect_status]}
        elif expected == pass_status:
            expectation = {"status_exact": pass_status}
        else:
            fail("case %r expected_status %r is neither detect (%r) nor pass (%r)"
                 % (case_id, expected, detect_status, pass_status))

        target = path
        query = case.get("query_string")
        if query is not None:
            target += "?" + query
        if any(ord(ch) < 0x20 or ord(ch) == 0x7F for ch in target):
            fail("case %r target carries control bytes (only SP may be encoded)" % (case_id,))
        if " " in target:
            # Owner transport rule: ASCII SP must be %20-encoded on the wire.
            encoded_targets += target.count(" ")
            target = target.replace(" ", "%20")

        headers = case.get("headers") or []
        out_headers = []
        for header in headers:
            if (not isinstance(header, (list, tuple))) or len(header) != 2:
                fail("case %r has malformed header entry" % (case_id,))
            out_headers.append([str(header[0]), str(header[1])])

        body = case.get("body")
        if body is None:
            out_body = {"kind": "none"}
        elif isinstance(body, dict) and set(body.keys()) == {"inline"}:
            out_body = {"kind": "inline_utf8", "value": str(body["inline"])}
        else:
            fail("case %r has unsupported body form (v1 is inline-only)" % (case_id,))

        out_cases.append({
            "id": case_id,
            "category": "synvoid-owner:%s/%s" % (
                case.get("category", "unknown"),
                "detect" if expected == detect_status else "pass",
            ),
            "request": {
                "method": method,
                "path_and_query": target,
                "headers": out_headers,
                "body": out_body,
            },
            "expectation": expectation,
        })

    out = {
        "schema_version": 1,
        "owner": owner_label,
        "corpus_id": corpus_id,
        "cases": out_cases,
    }
    with open(out_path, "w") as handle:
        json.dump(out, handle, indent=2)
        handle.write("\n")
    print("translated %d cases (%d SP targets encoded)"
          % (len(out_cases), encoded_targets))


if __name__ == "__main__":
    main()
