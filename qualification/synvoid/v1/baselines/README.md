# Baseline bundles (materialized locally, never committed)

The `synvoid-perf-v1` profile compares every performance scenario against
an explicit baseline bundle. No automatic baseline discovery exists: the
operator materializes baseline bundles from the accepted SynVoid revision
first, then runs the candidate profile against those immutable bundles.

## Stage A — accepted revision baseline

For the accepted SynVoid revision, from this directory:

```sh
for scenario in perf-small-c1 perf-small-c8 perf-small-c32 \
    perf-large-c1 perf-large-c8 perf-large-c32 \
    control-small control-large; do
  eggbench run "scenarios/${scenario}.json" "baselines/${scenario}.eggb" --json
done
```

Record the manifest digests of the finalized bundles.

## Stage B — candidate qualification

For the candidate revision (same profile, same corpus/config contract):

```sh
eggbench qualify run perf.profile.json --output /tmp/synvoid-perf-candidate
eggbench qualify inspect /tmp/synvoid-perf-candidate/qualification-receipt.json --json
```

If a candidate intentionally changes the WAF corpus/config contract, the
existing profile becomes Invalid/incomparable rather than a performance
result. New expected security behavior requires a new profile/config
input and a reviewed baseline; expectations are never derived from the
candidate.

Routine tests and the live harness perform the same two stages with the
synthetic stand-in (same-source pair proves orchestration mechanics; it
is not a claim about any release's performance).
