# SynVoid qualification

Eggbench keeps two SynVoid qualification workspaces under
[`qualification/synvoid/`](../qualification/synvoid/). Each README describes its
profile files, routine synthetic runs, required baselines, and live harness.

## Choose a workspace

- [v1](../qualification/synvoid/v1/README.md) covers WAF correctness and
  native workload profiles. Its checked-in WAF corpus is a synthetic fixture.
- [v2](../qualification/synvoid/v2/README.md) adds load shapes and
  SynVoid-owned telemetry mapping and contract files. Its load and correctness
  corpora remain Eggbench-authored synthetic fixtures.

Routine profiles use a synthetic subject for repeatable tests. Live
qualification uses pinned external binaries and the owner assets described by
the workspace README. A synthetic pass alone does not prove behavior of the
live SynVoid binary.

## Run a routine profile

For a bounded smoke run, use the v2 workspace:

```sh
cd qualification/synvoid/v2
eggbench qualify validate smoke.profile.json --json
eggbench qualify run smoke.profile.json --output /tmp/synvoid-smoke
eggbench qualify inspect /tmp/synvoid-smoke/qualification-receipt.json --json
```

The output directory must be new. `qualify` returns the aggregate verdict as
its exit code, so a detected failure exits `6`. Performance profiles require
their referenced baseline bundles to be prepared first. See each workspace's
README for the baseline and live-run procedures.

For the profile format and general qualification behavior, see [Security
qualification](security-qualification.md).
