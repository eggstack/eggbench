# Comparison policies

Comparison is offline, deterministic, and never mutates either input bundle. One measured trial is the statistical unit; request counts, physical dials, histogram buckets, and per-request samples never increase the comparison sample count.

## Policy v1: unpaired (`eggbench.trial-bootstrap.v1`)

Policy v1 compares two immutable `.eggb` bundles. Comparisons involving a schema-v3 network path use the separately identified path-aware policy `eggbench.trial-bootstrap-network-path.v1`; path-free comparisons retain `eggbench.trial-bootstrap.v1`. The candidate bundle is authoritative for metric requests, gates, and environment policy; the baseline supplies historical observations and provenance. Both policies use 10,000 trial-level bootstrap resamples and a 95% percentile interval. Relative observations must be finite, strictly positive, and domain-valid; missing or invalid observations are listed with stable reasons and are never imputed.

Absolute gates use the arithmetic mean of valid candidate observations. Relative gates use geometric means via `mean(log(value))` on each side. For a lower-is-better metric, degradation is `candidate / baseline - 1`; for a higher-is-better metric it is `baseline / candidate - 1`. Positive means the candidate is worse. The practical threshold is `allowance / 10_000` (500 basis points = 0.05). Threshold and uncertainty remain separate receipt fields.

The exact quantile convention is stable: for `n` sorted resamples, the lower index is `(25 * n) / 1000` and the upper index is `((975 * n + 999) / 1000) - 1` (250 and 9749 for `n = 10,000`). A lower bound above the threshold is `fail`; an upper bound at or below it is `pass`; an interval crossing it is `inconclusive`. No p-value is computed.

The RNG is SplitMix64, owned by the policy and used only for bootstrap indices. Without the comparison CLI `--seed`, the base seed derives deterministically from both manifest digests and the policy id; each metric mixes in its name. The fault-plan seed is a separate plan field and is not supplied by this flag. Same bundles, policy, and seed yield byte-equivalent receipts: no OS randomness or timestamp participates.

Statistical gates require `max(plan.min_trials, 5)` valid observations per side. Fewer than seven per side emits a diagnostic warning, not a failure. Per-metric dispositions are `pass`, `fail`, `inconclusive`, `invalid`, or `descriptive`; aggregate verdicts consider gated primary metrics only, with `Invalid` taking precedence over `Fail`, then `Inconclusive`.

## Network-path identity

A schema-v3 `network_path` is comparison-critical configuration, not runtime telemetry. A baseline-relative comparison treats the following as one identity:

- presence or absence of a path;
- route driver identity, adapter version, exact `eggress-outbound` and `eggress-uri` versions, capabilities, and selected route mode;
- the canonical credential-free route-chain identity/configuration digest;
- fault driver identity, adapter version, upstream identity/version, and capabilities;
- the ordered upstream and downstream typed fault plans;
- the Eggchaos RNG version, explicit experiment fault seed, and `route-first-fault-second-v1` ordering semantics.

A change to any of these dimensions sets the path mismatch as a critical comparability mismatch. It does not invent a new verdict system: existing environment policy decides how the mismatch affects gates. Ephemeral ports, socket addresses, physical dial counts, observed hop counts, and other bounded runtime diagnostics are informational and do not make otherwise identical configurations incomparable.

The expected policy effects are:

- `strict_same_testbed`: a critical path mismatch suppresses baseline-relative primary gating and yields an invalid gate disposition;
- `warn_on_mismatch`: observed effects remain descriptive, but relative/statistical gate verdicts are suppressed;
- `cross_testbed_descriptive`: relative/statistical effects remain descriptive;
- candidate-only absolute gates remain eligible according to the existing absolute-gate rules.

Thus a route mode, chain, driver/upstream version, seed, fault presence, or ordered upstream/downstream plan must match before a baseline-relative result can be treated as gated evidence. Comparison receipts carry the typed comparability report and its path identity detail; they do not rewrite source or resolved plans.

## Worked example

Baseline trials: `[100, 100, 100, 100, 100, 100, 100]`, candidate trials: `[130 × 7]`, lower-is-better, allowance 500.

- candidate estimate: `130.0`;
- baseline estimate: `100.0`;
- degradation: `130 / 100 - 1 = 0.30`;
- threshold: `0.05`; the relative gate fails;
- the 95% interval is `[0.30, 0.30]`, so the statistical gate also fails.

A candidate of `[101 × 7]` degrades `0.01 ≤ 0.05` and passes. This example assumes matching workload, driver, topology, and network-path identity; a path mismatch is handled by the policy above.

## Policy v2: paired (`eggbench.trial-bootstrap-paired.v1`)

`eggbench compare --paired <bundle.eggb>` compares the two arms of one paired bundle; see [`paired-experiments.md`](paired-experiments.md) for the schedule and drift methodology. Pairs are the resampling unit, absolute gates are invalid over paired evidence, and unpaired comparison of a paired bundle is invalid with a stable reason. Receipts use schema v2; schema-v1 unpaired receipts remain unchanged.

Paired experiments cannot declare `network_path`: the plan is rejected before startup because both arms remain live while one run-scoped Eggfetch client/pool can reuse physical connections. Use an explicit schema-v3 path only for an unpaired native Eggfetch experiment; compare only bundles whose path identity matches under the policy above.

## Exit and output behavior

`eggbench compare` writes a standalone versioned receipt, optionally to `--output <comparison.json>`. Aggregate `Fail`, `Inconclusive`, and `Invalid` retain the compare result and use exit codes 6, 7, and 8 respectively. See [the CLI reference](cli.md) and [evidence bundles](evidence-bundle.md).
