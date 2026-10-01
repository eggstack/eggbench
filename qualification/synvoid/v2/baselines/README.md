# Baseline bundles (materialized locally, never committed)

The M003 performance profile compares every performance scenario against an
explicit baseline bundle. There is no automatic baseline discovery: the
operator materializes baseline bundles from the accepted revision first, then
runs the candidate profile against those immutable bundles. The live harness
performs exactly this two-stage sequence in `m003d-1`/`m003d-2`; the routine
tests perform it against the synthetic stand-in.

## Stage A — accepted revision baselines

From `qualification/synvoid/v2`:

```sh
for scenario in body-pooled-c8 body-pooled-c32 body-gated-c8 \
    mixed-80-20-pooled-c8 mixed-80-20-fresh-c8 \
    control-origin-body-c8 telemetry-pressure-c32; do
  eggbench run "scenarios/${scenario}.json" "baselines/${scenario}.eggb" --json
done
```

Those seven scenarios are exactly the `baseline_bundle` entries declared by
`perf.profile.json`; `waf-correctness` declares none because its verdict comes
from the run's own expected outcomes. Record the manifest digests of the
finalized bundles. Only `body-gated-c8` carries the frozen
statistical-relative performance gates (`throughput` and `latency_p95`, five
measured trials); every other performance observation in this workspace is
recorded as diagnostic.

## Stage B — candidate qualification

For the candidate revision (same profile, same corpus/config contract):

```sh
eggbench qualify run perf.profile.json --output /tmp/synvoid-m003-candidate
eggbench qualify inspect /tmp/synvoid-m003-candidate/qualification-receipt.json --json
```

An intentional change to the WAF corpus/config contract makes the existing
profile Invalid/incomparable rather than a performance result. New expected
security behavior requires a new profile/config input and a reviewed
baseline; expectations are never derived from the candidate.

Baselines are never committed: a checked-in bundle would let a candidate
compare against evidence from an unreviewed run.