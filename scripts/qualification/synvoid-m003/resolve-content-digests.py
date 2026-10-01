#!/usr/bin/env python3
"""Resolve Eggbench content identities inside a qualification workspace.

The runner pins corpus and telemetry-mapping inputs by Eggbench's own
workspace content identity (SHA-256 over the sorted `path`, byte length, and
file digest of every input), not by a bare file digest. A workspace that is
authored (checked-in plans) or generated (live harness translation) must
therefore resolve those identities after its inputs are final, and a
telemetry contract that changes must change every pinned digest with it.

This tool is mechanical wiring only. It copies identity facts out of the
files themselves; it never derives, relaxes, or reinterprets one. A
mismatch against an already-pinned digest is a hard error unless
`--repin` is passed, which is how an intentional contract change is made
visible in review.

Usage:
    resolve-content-digests.py --workspace <dir> [--mapping <ref>]
                               [--repin]
"""

import argparse
import hashlib
import json
import os
import sys

MAX_FILES = 4096


def fail(message):
    print("resolve-content-digests: %s" % message, file=sys.stderr)
    sys.exit(2)


def file_identity(path, relative):
    raw = open(path, "rb").read()
    return {
        "path": relative,
        "length": len(raw),
        "sha256": hashlib.sha256(raw).hexdigest(),
    }


def content_identity(workspace, requested):
    target = os.path.join(workspace, requested)
    if os.path.islink(target):
        fail("%s is a symlink" % requested)
    if os.path.isfile(target):
        records = [file_identity(target, os.path.basename(target))]
    elif os.path.isdir(target):
        records = []
        for root, _dirs, files in os.walk(target):
            for name in files:
                full = os.path.join(root, name)
                if os.path.islink(full):
                    fail("tree contains a symlink: %s" % full)
                records.append(
                    file_identity(full, os.path.relpath(full, target).replace(os.sep, "/"))
                )
        if len(records) > MAX_FILES:
            fail("input tree exceeds the file bound")
    else:
        fail("%s is missing" % requested)
    hasher = hashlib.sha256()
    for record in sorted(records, key=lambda item: item["path"]):
        hasher.update(record["path"].encode())
        hasher.update(b"\x00")
        hasher.update(record["length"].to_bytes(8, "big"))
        hasher.update(b"\x00")
        hasher.update(record["sha256"].encode())
        hasher.update(b"\x00")
    return hasher.hexdigest(), hashlib.sha256(open(target, "rb").read()).hexdigest()


# A freshly generated workspace carries this placeholder; it is a request to
# resolve, not a pin that could silently disagree with its input.
UNRESOLVED = ("COMPUTE", "RESOLVED")


def pin(container, key, value, repin, label):
    existing = container.get(key)
    if existing in UNRESOLVED:
        container[key] = value
        return
    if existing and existing != value and not repin:
        fail("%s pins %s=%s but the resolved identity is %s" % (label, key, existing, value))
    container[key] = value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--workspace", required=True)
    parser.add_argument("--mapping", default="telemetry/telemetry-mapping.json")
    parser.add_argument("--repin", action="store_true")
    args = parser.parse_args()

    mapping_identity, mapping_file_digest = content_identity(args.workspace, args.mapping)
    # One workspace may carry more than one corpus input: the correctness
    # family and the load-shape family are separate evidence, so each pinned
    # `corpus_ref` resolves its own identity.
    corpus_identities = {}
    corpus_digests = {}
    scenarios = os.path.join(args.workspace, "scenarios")
    if os.path.isdir(scenarios):
        for name in sorted(os.listdir(scenarios)):
            if not name.endswith(".json"):
                continue
            path = os.path.join(scenarios, name)
            with open(path) as handle:
                plan = json.load(handle)
            references = []
            workload = plan.get("workload", {})
            if workload.get("kind") == "http_corpus":
                references.append((workload, workload.get("corpus_ref")))
            for check in plan.get("http_corpus_checks", []):
                references.append((check, check.get("corpus_ref")))
            for container, reference in references:
                if not reference:
                    fail("%s pins a corpus without corpus_ref" % name)
                if reference not in corpus_identities:
                    corpus_identities[reference], corpus_digests[reference] = content_identity(
                        args.workspace, reference
                    )
                pin(container, "corpus_sha256", corpus_identities[reference], args.repin, name)
            for service in plan.get("services", []):
                config = service.get("config", {})
                if service.get("kind", {}).get("service_type") == "prometheus-http":
                    pin(
                        config,
                        "mapping_sha256",
                        mapping_identity,
                        args.repin,
                        "%s/%s" % (name, service["name"]),
                    )
            with open(path, "w") as handle:
                json.dump(plan, handle, indent=2)
                handle.write("\n")

    for reference in sorted(corpus_identities):
        print(
            "corpus %s identity %s (file digest %s)"
            % (reference, corpus_identities[reference], corpus_digests[reference])
        )
    print("mapping identity %s (file digest %s)" % (mapping_identity, mapping_file_digest))


if __name__ == "__main__":
    main()
