# Paired experiments

Measurement M003 lets one Eggbench bundle contain a drift-controlled
paired experiment: two variant arms (baseline + candidate services, both
live for the whole run) executed under a deterministic alternating
schedule, compared under paired policy v2.

## Why interleave

Two bundles executed hours apart share nothing about intervening drift:
host temperature, noisy neighbors, caches, and background work shift both
arms together or apart in ways unpaired comparison cannot see. A paired run
executes baseline and candidate trials adjacently (trial 1 baseline, trial
2 candidate, trial 3 baseline, …), so each adjacent pair experiences nearly
the same conditions. Pair-level differences cancel shared drift; the
bootstrap resamples *pairs*, preserving that structure.

Interleaving controls drift; it does not remove it. Pair effects in
execution order plus half-split means are recorded as descriptive drift
diagnostics (see below) so drift stays visible.

## Predeclared design (plan schema v2)

Pairing is declared in the experiment plan before execution and never
inferred afterward. A paired plan uses schema version 2:

- top-level `subject` is a `label` naming the comparison (for example,
  `"waf-config-a-vs-b"`); the physical variants are services;
- `paired.baseline` / `paired.candidate` each name a declared `service`
  plus a `subject` identity for that arm's provenance;
- `trials.measured` is even and at least 2; pairs = measured / 2;
- the plan workload `target` equals the baseline arm service, so
  unpaired-minded readers see the control arm as the nominal target.

Validation fails closed: unknown or identical arm services, a non-label
top-level subject, an odd measured count, a `managed_command` arm subject,
or a workload target that is not the baseline service are all rejected
with stable categories.

Arm subjects are `label` or `external` identities only. They are
provenance declarations: the runner never launches or digests them.
Variant binaries already resolve through the normal managed-service
machinery. Managed-subject process switching per trial does not exist in
v1; arms are services, not the singular subject.

## Schedule v1: alternating, baseline first

`alternating-baseline-first`: trial `n` (1-based) measures baseline when
`n` is odd and candidate when `n` is even; pair `(n+1)/2` groups each two
consecutive trials. Warmups alternate arms round-robin starting with
baseline and carry no pair identity (an odd warmup count leaves a
deterministic, documented arm imbalance).

Both arm services stay up for the whole run. The runner switches which
service receives load per trial by overriding the workload target; the
load shape is identical across arms, and native (Eggfetch) as well as
external (oha/h2load/iperf3) drivers follow automatically because they all
derive the target from `InvocationContext.workload`. Per-trial seeds are
arm-namespaced so paired trials never share a seed stream.

Each staged `trials/NNN/result.json` (schema v2) records `arm`
(`baseline`/`candidate`) and `pair_id`. The manifest carries a `paired`
record (schedule, pair count, arm services and subjects); per-arm
declared-only subject snapshots stage as `subject-arm-baseline.json` and
`subject-arm-candidate.json` alongside the top-level `subject.json`.
Schema-v1 trial results still parse (tags default to absent).

## Paired comparison policy v2

`eggbench compare --paired <bundle.eggb>` compares the two arms of one
paired bundle under `eggbench.trial-bootstrap-paired.v1`:

- candidate trials = candidate-arm trials, baseline trials =
  baseline-arm trials, joined by runner-assigned pair identity;
- a pair is complete only when both arms measured `completed` and
  `observed` with finite domain-valid values; incomplete pairs are
  excluded whole with the reason `pair_incomplete` — pairs are never
  split and nothing is imputed;
- statistical gates require `max(plan.min_trials, 5)` *complete pairs*
  (7+ recommended, diagnostic warning below that);
- the paired bootstrap resamples pairs (not individual trials) 10,000
  times: per-pair oriented log-differences
  (`ln(candidate) − ln(baseline)`, negated for higher-is-better),
  resampled mean per draw, `exp() − 1` transform, same 95% percentile
  extraction as v1;
- verdict rules mirror v1 (lower bound above threshold → fail; upper
  bound at or below → pass; crossing → inconclusive);
- non-statistical relative gates use geometric means over complete-pair
  arm values only;
- absolute gates are invalid over paired evidence (arm-agnostic
  averaging would silently mix variants);
- the unpaired `compare` of a paired bundle is invalid per metric with
  the stable reason `paired_evidence_requires_paired_comparison`.

The receipt (schema v2) records both bundle identities (identical),
comparability (matching by construction — one plan, one testbed), and a
`paired` section: schedule, declared pairs, arm services and subjects,
per-metric complete/excluded pairs, per-pair oriented effects in execution
order, drift diagnostics, and effective seeds. Without `--seed`, the base
seed derives from the single manifest digest and the v2 policy id.

## Worked example

Six pairs, lower-is-better latency, allowance 500 (threshold 0.05),
baseline `100.0`, candidate `130.0`:

- complete pairs: `[1, 2, 3, 4, 5, 6]`;
- degradation: `exp(mean(ln(130) − ln(100))) − 1 = 0.30`;
- 95% interval degenerate at `[0.30, 0.30]`; lower bound exceeds `0.05` →
  **fail**, aggregate **fail**, exit 6.

A candidate of `100.0` everywhere degrades `0.0 ≤ 0.05`: **pass**, exit 0.

## Drift diagnostics (descriptive only)

Per-pair oriented effects (`ratio − 1`) are stored in execution order with
first-half and second-half means. For odd pair counts the second half
holds one more pair. `trend` is the exact sign of
`second_half_mean − first_half_mean` (`up`/`down`/`flat`); fewer than two
complete pairs yields `insufficient` with no means claimed. Drift evidence
never changes a gate verdict: the same verdict results with or without
the drift section.

## Limitations

- Alternating order only; no counterbalanced (`ABBA`) or adaptive
  schedules.
- Arms must be co-runnable services. Externally-switched variants with no
  runner-observable difference are rejected (identical arm services fail
  validation) rather than recorded as theater.
- No arm-specific service config overlays; no managed-subject switching.
- Environment fingerprinting is bundle-level; there is no per-trial
  fingerprint.
- Cross-bundle pairing (including pair-by-order heuristics across two
  bundles) is unsupported by design: pair identities exist only where the
  runner created them.
