# Comparison policies

## Policy v1: unpaired (`eggbench.trial-bootstrap.v1`)

Policy v1 compares two immutable `.eggb` bundles without mutating either.
The candidate bundle is authoritative for metric requests, gates, and
environment policy; the baseline supplies historical observations and
provenance.

## Statistical unit

One measured trial is one observation. Only normalized trial-level scalars
(`trials/NNN/metrics.json`, state `observed`) enter the comparison; request
counts, histogram buckets, and per-request samples never increase the sample
count. Missing/invalid observations are listed with stable reasons, never
imputed.

## Estimates

- Absolute gates use the arithmetic mean of valid candidate observations.
- Relative gates use geometric means via `mean(log(value))` on each side.
  Every included value must be strictly positive, finite, and domain-valid.

## Oriented degradation

Degradation is a fraction where positive means the candidate is worse:

- lower-is-better: `candidate / baseline - 1`
- higher-is-better: `baseline / candidate - 1`

The practical threshold is `allowance / 10_000` (500 basis points = 0.05).
Threshold and uncertainty are recorded as separate receipt fields.

## Worked example

Baseline trials: `[100, 100, 100, 100, 100, 100, 100]`, candidate trials:
`[130 × 7]`, lower-is-better, allowance 500.

- candidate estimate (arithmetic mean): `130.0`
- baseline estimate (geometric mean): `100.0`
- degradation: `130 / 100 - 1 = 0.30`
- threshold: `0.05`; `0.30 > 0.05` so a relative gate **fails**.
- statistical gate: the 95% bootstrap interval is degenerate at
  `[0.30, 0.30]` (identical inputs); its lower bound exceeds the threshold,
  so the verdict is also **fail**.

A candidate of `[101 × 7]` degrades `0.01 ≤ 0.05`: **pass**.

## Bootstrap

Unpaired trial-level resampling, 10,000 resamples, 95% percentile interval:

1. sample candidate trial values with replacement to candidate length;
2. sample baseline trial values with replacement to baseline length;
3. compute each side's mean log value;
4. compute oriented degradation in log space;
5. transform to degradation space.

Quantile indexing is exact: for `n` sorted resamples the lower bound is
index `(25 * n) / 1000` and the upper bound is `((975 * n + 999) / 1000) - 1`
(250 and 9749 for `n = 10_000`).

Verdict: lower bound above threshold → fail; upper bound at or below
threshold → pass; interval crossing threshold → inconclusive. No p-value is
computed or reported.

## Determinism

The RNG is SplitMix64, owned by the policy and used only for bootstrap
indices. Without `--seed`, the base seed derives deterministically from both
manifest digests and the policy id (FNV-1a); each metric mixes in its name.
Same bundles, policy, and seed yield byte-equivalent receipts: no OS
randomness participates and no timestamp enters the receipt.

## Minimum evidence

Statistical gates require `max(plan.min_trials, 5)` valid observations per
side, else the metric is invalid. Fewer than 7 per side emits a diagnostic
warning (recommendation, not failure).

## Verdicts and aggregation

Per-metric dispositions: `pass`, `fail`, `inconclusive`, `invalid`, or
`descriptive` (baseline effects that must not masquerade as gate verdicts).
Unsupported directions (`target_range`, `informational`) and zero/negative
relative values are invalid, never silently reinterpreted.

Aggregate over gated primary metrics only: any `Invalid` beats any `Fail`
beats any `Inconclusive`; all-pass yields `Pass`; no gate-eligible primary
verdict yields no aggregate. Diagnostics never affect the aggregate.

## Policy v2: paired (`eggbench.trial-bootstrap-paired.v1`)

`eggbench compare --paired <bundle.eggb>` compares the two arms of one
paired bundle; see [`paired-experiments.md`](paired-experiments.md) for the
methodology. Pairs are the resampling unit, absolute gates are invalid over
paired evidence, and unpaired comparison of a paired bundle is invalid with
a stable reason. Receipts use schema v2 (v1 receipts are unchanged).
