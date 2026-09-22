# ADR-0003: Trial-Level Comparison and Regression Verdicts

Status: accepted

Date: 2026-09-22

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#45-trials-are-the-default-experimental-unit`
- `plans/000-long-term-specification.md#46-practical-significance-and-uncertainty-are-separate`
- `plans/000-long-term-specification.md#10-statistical-comparison`
- `plans/000-long-term-specification.md#11-baselines`

Affected subsystem roadmaps:

- `plans/subsystems/measurement-comparison-roadmap.md`

## Context

Network benchmarks often generate millions of request observations but only a handful of independent experimental repetitions. Treating every request as an independent sample produces falsely narrow uncertainty estimates because requests share the same process, runtime, machine, network queues, caches, and load-generator state.

Eggbench also needs to distinguish operationally meaningful regressions from tiny effects and from noisy evidence.

## Decision drivers

- statistically honest comparison;
- transparent implementation in Rust;
- small-sample behavior;
- support for throughput, latency, errors, and resource metrics;
- no dependence on opaque statistical services;
- deterministic reproducibility.

## Considered options

### Option A — Compare medians/percentages with no uncertainty

Simple but cannot distinguish noise from a durable regression.

Rejected as the only comparison mode.

### Option B — Treat individual requests as samples

Produces pseudo-replication for version comparison and exaggerates confidence.

Rejected as the default.

### Option C — Trial-level effect estimates with practical thresholds and resampling

Selected.

## Decision

The default comparison unit SHALL be one completed measured trial.

Per-request latency distributions remain descriptive within each trial and MAY feed one trial-level summary such as p99.

For paired same-testbed candidate/baseline experiments, comparison SHOULD operate on paired trial effects when pairing information exists.

Relative effects SHALL use a ratio representation. For strictly positive metrics, the implementation SHOULD compute in log-ratio space and transform back to percentage/ratio for presentation.

The initial statistical policy SHALL use deterministic bootstrap resampling of trial-level observations/effects with a recorded seed and policy version.

Initial policy defaults:

- minimum measured trials for statistical gating: 5 per side, or 5 valid pairs for paired mode;
- recommended normal qualification: 7 or more;
- confidence level: 95%;
- bootstrap resamples: 10,000 for final comparison, with lower configurable counts permitted for tests/development;
- no automatic extension of trials in the first release.

A gate has an independently declared practical regression threshold.

For a lower-is-better metric, define degradation as candidate/baseline - 1. Higher-is-better metrics use the inverse orientation so positive degradation always means worse.

Relative-gate verdict:

- **fail** when the uncertainty interval is entirely beyond the allowed regression threshold;
- **pass** when the uncertainty interval is entirely at or below the allowed regression threshold;
- **inconclusive** when the interval crosses the threshold;
- **invalid** when required observations, comparability, positivity/domain rules, or minimum trial count fail.

Absolute gates MAY use direct deterministic thresholding unless an explicit statistical absolute policy is selected.

The implementation SHALL NOT report a p-value merely because a library makes one available. Any future hypothesis-test policy requires a new comparison-policy identifier and planning.

Primary metrics capable of failing a run MUST be declared before candidate evidence is interpreted.

## Consequences

### Positive

- avoids request-level pseudo-replication;
- separates noise from operational budget;
- supports inconclusive results honestly;
- deterministic seeded bootstrap is auditable;
- paired designs can control host drift.

### Negative

- requires repeated trials;
- small experiments often become inconclusive;
- bootstrap intervals are not a substitute for good experimental control;
- p99 comparisons summarize each trial rather than modeling every request.

## Edge cases

Metrics containing zero or negative values cannot blindly use log ratios. The metric definition must select a compatible comparison method or reject statistical relative gating.

Missing trials are not silently imputed.

Outlier removal is not part of the initial default policy. A future robust policy must be explicit and versioned.

Cross-testbed comparisons are descriptive unless a policy explicitly permits gating.

## Compatibility

Every comparison result stores the comparison-policy identifier, seed, trial identities, practical threshold, effect estimate, interval, and verdict.

Changing defaults requires a new policy version for new comparisons; old evidence remains interpretable.

## Verification

Required tests include:

- clear regression;
- clear non-regression;
- threshold-crossing inconclusive;
- insufficient trials;
- paired and unpaired paths;
- higher/lower directionality;
- zero/domain errors;
- deterministic seed reproducibility;
- missing/invalid trial exclusion rules;
- proof that request count does not alter statistical sample count.

## Supersession

None.
