# Comparison (core): offline comparison of immutable evidence bundles

## Purpose

`crates/eggbench-core/src/comparison.rs` answers one question offline: given
two finalized, verified `.eggb` bundles, did the candidate regress against the
baseline, and can that answer be trusted as same-testbed evidence?

It owns four concerns, in this order of the data flow:

1. **Identity** — a bundle's immutable identity, and a digest-pinned way to
   name one (`BundleIdentity`, `BaselineAliasFile`).
2. **Comparability** — whether two bundles are comparable at all, as four
   explicit dimensions rather than one equality test.
3. **Statistical policy** — a deterministic, versioned, unpaired or paired
   trial-level bootstrap with no p-value.
4. **Verdicts** — per-metric dispositions, a conservative aggregate, and a
   standalone receipt.

The module is deliberately infeasible to widen: `compare` and `compare_paired`
return a `ComparisonReceipt` and never a `Result` (comparison.rs:2313,
comparison.rs:2488). Every input problem is encoded as a disposition on a
metric record, so a comparison always produces a receipt that states what
could not be decided. Failure to *load* a bundle is the only `Result` path.

Two absences are structural, not omissions: no p-value is computed or stored
anywhere in the receipt schema (comparison.rs:22), and no code path in the
module can write to a bundle — bundles are opened read-only and the only
`fs::write` in the file lives in two `#[cfg(test)]` golden-refresh blocks
(comparison.rs:5703, comparison.rs:6770).

## Source layout

6,780 lines, of which the implementation is 1–4170 and `#[cfg(test)] mod
tests` is 4171–6780. The implementation is eight sub-areas, in file order:

| Lines | Sub-area | What it owns |
|---|---|---|
| 1–135 | Module contract | Version/policy constants, `ComparisonError` |
| 136–760 | Receipt and request types | `BundleIdentity`, `BaselineReference`, `ComparisonReceipt`, paired/correctness sections, `ComparisonInput`, `ComparisonRequest` |
| 761–1693 | Bundle evidence identity | `network-path.json`, `semantic-replay.json`, `diagnostics.json`, `security-checks.json` loaders and their stored DTOs |
| 1694–2062 | Per-check correctness | HTTP-corpus records, correctness aggregation, security identity |
| 2063–2248 | Load and reference entry points | `parse_comparison_receipt`, `load_comparison_input`, `load_candidate_bundle`, `load_baseline_bundle`, `load_baseline_alias` |
| 2250–2810 | Unpaired and paired policy passes | `unpaired_policy_id`, `compare`, `compare_paired`, `evaluate_metric`, `evaluate_relative`, `evaluate_paired_metric` |
| 2812–3000 | Pair join and drift | `select_pairs`, `oriented_log_difference`, `drift_diagnostics` |
| 3002–4169 | Comparability, estimators, RNG, aggregation | `evaluate_comparability` and the four `compare_*` dimension checks, means, `allowance_fraction`, bootstrap, `SplitMix64`, `aggregate`, `read_manifest_bytes` |

The policy passes (2250–2810) are intentionally one long function each: the
gate preconditions are written as a sequential early-return ladder per metric
(`#[allow(clippy::too_many_lines)] // One auditable policy pass over all
metrics`, comparison.rs:2276) so the fail-closed order is readable top to
bottom rather than spread across helpers.

## Key types

### Identity and input

| Type | Role |
|---|---|
| `BundleIdentity` (comparison.rs:139) | `manifest_schema_version`, `run_id`, `manifest_sha256`, plus `subject_revision`/`subject_digest` marked presentation-only |
| `BaselineAliasFile` (comparison.rs:157) | Human-managed `*.eggbaseline.json`: schema 1, alias, path, **required** manifest digest, optional note |
| `BaselineReference` (comparison.rs:174) | Tagged `bundle` vs `alias`; an alias pins `resolved_identity` **and** `resolved_path` so the receipt records what the name stood for |
| `ComparisonInput` (comparison.rs:619) | One verified bundle: identity, `ResolvedPlan`, `EnvironmentFingerprint`, trials, plus five `Option<…EvidenceIdentity>` slots and the validated `security_records` |
| `InputTrial` (comparison.rs:730) | `id`, terminal status, optional `arm`/`pair_id`, optional `TrialMetrics` |
| `ComparisonRequest` / `BaselineSide` (comparison.rs:754, comparison.rs:745) | Candidate is authoritative; the baseline side carries its reference for the receipt |
| `ComparisonOptions` (comparison.rs:611) | Only an optional explicit seed |

The evidence-identity slots are all the same shape in spirit: a
comparison-critical *configuration* identity, never a result. `SemanticReplayEvidenceIdentity`
(comparison.rs:655), `DiagnosticsEvidenceIdentity` (comparison.rs:676),
`SecurityEvidenceIdentity` (comparison.rs:696), `NetworkPathEvidenceIdentity`
(comparison.rs:712). Each type documents that observed statuses, timings,
counts, and dispositions are result evidence and never participate.

### Receipt and verdict vocabulary

| Type | Meaning |
|---|---|
| `ComparisonReceipt` (comparison.rs:464) | The standalone receipt; `deny_unknown_fields` |
| `MetricComparison` (comparison.rs:294) | Per-metric record: unit/direction/intent/gate, included+excluded trial ids **with reasons**, estimates, degradation, interval, threshold, method, resamples, effective seed, disposition, reason |
| `GateDisposition` (comparison.rs:198) | `Pass`/`Fail`/`Inconclusive`/`Invalid`/`Descriptive` |
| `AggregateVerdict` (comparison.rs:214) | `Invalid`/`Fail`/`Inconclusive`/`Pass` |
| `ComparabilityReport` / `FieldComparison` / `FieldOutcome` (comparison.rs:262, 228, 244) | Four dimensions + per-field outcomes |
| `PairedComparisonSection` (comparison.rs:517) | Schedule, declared pairs, both arm services/subjects, per-metric pair evidence, method, resamples, base seed |
| `DriftDiagnostics` / `DriftTrend` (comparison.rs:584, 598) | Half-split means and an exact sign; never gates |
| `CorrectnessComparisonSection` (comparison.rs:445) | Policy id, per-check records, conservative aggregate |
| `ComparisonWarning` (comparison.rs:352) | Bounded `{category, detail}`; never a verdict |

Receipt schema versions accepted on read: v1–v4
(comparison.rs:2076–2080), with `COMPARISON_RECEIPT_SCHEMA_VERSION = 4`
current (comparison.rs:45). v3/v4 split `performance_verdict` (metric gates
only) from `correctness` and reserve `aggregate_verdict` for the combined
verdict; on legacy v1/v2 the historical `aggregate_verdict` keeps its
metric-only meaning and correctness is never projected backwards
(comparison.rs:454–461).

## How it works

### 1. Verified load, then identity

`load_comparison_input` (comparison.rs:2096) calls `reader.verify()` *before*
deriving anything (comparison.rs:2097), then reads the exact `manifest.json`
bytes through a 8 MiB-bounded read (comparison.rs:4089–4107) and hashes them
(comparison.rs:2103). Identity therefore exists only for a bundle that
verified, and it is a content hash of the authoritative manifest rather than a
path, a run id, or a directory name.

`subject_revision`/`subject_digest` are extracted only for `Subject::External`
(comparison.rs:3496–3508) and exist for presentation; they are never inputs to
comparability. `ResolvedPlan` is re-validated (`validate_resolved_plan_bytes`,
comparison.rs:2108) and the environment fingerprint is validated
(comparison.rs:2114) — comparison does not trust the writer that staged them.

Trials are read in manifest order; a trial result must have schema v1 or v2 and
a matching trial id (comparison.rs:4160–4167), otherwise the load fails. The
paired summary is copied from the manifest (comparison.rs:2127).

### 2. Baseline references

Direct references record the identity and the path as supplied
(comparison.rs:2171). Aliases are read by explicit path with a 64 KiB bound
(comparison.rs:2197), must be schema 1 (comparison.rs:2208), must have
non-empty alias and path (comparison.rs:2214), resolve relative paths against
the alias file's parent (comparison.rs:2220–2228), and then require the
verified bundle's computed digest to equal the declared digest
(comparison.rs:2231) or fail closed as `baseline_alias_digest_mismatch`. There
is no registry and no "latest successful run".

### 3. Comparability: four dimensions

`evaluate_comparability` (comparison.rs:3014) never returns a single
equal/unequal answer. It fills a `ComparabilityReport` with three booleans
plus a per-field list, and folds them into one `critical_mismatch` flag.

**Testbed fields.** The field set is the union of both fingerprints, iterated
in `BTreeSet` order (comparison.rs:3020–3027), so the report is sorted by
field name. The class is taken from the candidate field, falling back to the
baseline field when the candidate lacks it (comparison.rs:3030–3032).

| Fingerprint relation | Class | `FieldOutcome` | `critical_mismatch` |
|---|---|---|---|
| Values equal | any | `Equal` | no |
| Values differ | `comparison_critical` | `Unequal` | **yes** |
| Values differ | `warning_only` | `WarningMismatch` | no (warning emitted) |
| Values differ | `informational` | *field omitted from the report* | no |
| Present on one side only | `comparison_critical` | `MissingCandidate` / `MissingBaseline` | **yes** |
| Present on one side only | `warning_only` | `WarningMismatch` | no |
| Present on one side only | `informational` | *field omitted* | no |

A comparison-critical field that exists on only one side is a critical
mismatch, not a tolerated absence: the fingerprint's own contract
(`docs/environment-fingerprint.md`) treats a missing `cpu_model` on an ARM host
as a benign collector limitation, and that benign case is only representable
because such fields are classed accordingly. Note the asymmetry: an
`informational` field that differs produces *no row at all*, so a receipt
cannot demonstrate that the field was present on one side.

**Workload.** `workload_summary` (comparison.rs:3087) canonicalizes each
workload kind to a shape string and deliberately excludes trial counts
(`termination_summary`, comparison.rs:3161). `Workload::SemanticReplay` keeps
only kind + target (comparison.rs:3141) so the workstation-local fixture path
can never enter a comparison; the replay's identity is checked separately
against `semantic-replay.json` (`compare_semantic_replay`,
comparison.rs:3213) using fixture digest, session/envelope/report schemas, and
tool version + executable digest (comparison.rs:3195–3211). A missing evidence
file is the literal `"missing-evidence"`, so two bundles that both failed to
stage replay evidence compare *equal* on that dimension.

**Driver.** `compare_driver` (comparison.rs:3234) is the conjunction of four
sub-checks, all reported in one detail string:

| Sub-check | Compared | Code |
|---|---|---|
| Workload driver | `descriptor_summary` — name, adapter version, upstream name+version, **sorted** capabilities | comparison.rs:3361, 3402 |
| Diagnostics | requested configuration + producer provenance | comparison.rs:3283, 3267 |
| Security checks | requested check configuration + producer/scope provenance | comparison.rs:3348, 3296 |
| Network path | only when either side declares or stages a path | comparison.rs:3248–3257 |

**Topology.** `compare_topology` (comparison.rs:3463) requires equal resolved
topology and then equal subject *kind* summary (comparison.rs:3488), which
deliberately excludes revision and digest: candidate and baseline binaries are
expected to differ (pinned by `subject_digest_difference_alone_does_not_invalidate`,
comparison.rs:5509).

Any false among workload/driver/topology sets `critical_mismatch`
(comparison.rs:3072–3074). Warning-only field mismatches become
`environment_warning_mismatch` warnings (comparison.rs:2338–2348) and never
change a verdict.

### 4. The three policy identities

Selection is `unpaired_policy_id` (comparison.rs:2250): if the candidate **or**
the baseline has network-path evidence or a resolved `network_path`, the
policy is `…-network-path.v1`; otherwise `…-v1`. The paired policy is not
reachable from `compare` at all — it belongs to `compare_paired`.

| | `eggbench.trial-bootstrap.v1` | `eggbench.trial-bootstrap-network-path.v1` | `eggbench.trial-bootstrap-paired.v1` |
|---|---|---|---|
| Const | comparison.rs:70 | comparison.rs:73 | comparison.rs:78 |
| Entry point | `compare` (comparison.rs:2313) | `compare` (same fn) | `compare_paired` (comparison.rs:2488) |
| Applies when | neither side has a network path | either side has a path | one paired bundle, two arms |
| Estimator | unpaired percentile bootstrap | **identical** | bootstrap over per-pair log-differences |
| Resampling unit | trial, per side | **identical** | pair |
| Absolute gates | supported | **identical** | invalid (`absolute_gate_unsupported_for_paired_evidence`, comparison.rs:2713) |
| Degradation | geomeans per side, then oriented ratio (comparison.rs:3634) | **identical** | `exp(mean(per-pair oriented log-diff)) - 1` (comparison.rs:2733–2745) |
| Base seed | FNV-1a over `cand_sha \| base_sha-or-"absolute-only" \| policy_id` (comparison.rs:4029) | same formula; policy id differs, so the seed differs | FNV-1a over `cand_sha \| policy_id` — one digest only (comparison.rs:3004) |
| Comparability | `evaluate_comparability` | same, **plus** the path dimension | `evaluate_comparability(input, input)` — matches by construction (comparison.rs:2501) |
| Extra section | none | none | `paired` (comparison.rs:2575) |

The honest summary: **the network-path policy differs from v1 in exactly two
places** — the policy identifier (which changes the derived base seed and
nothing else) and the fact that `compare_driver` adds the
`compare_network_path` sub-check to the driver dimension. The estimator,
thresholds, sample-size rules, quantile convention, disposition rules, and
aggregate are shared code. It is an identity/label change, not a second
statistical method, and it is named that way in the receipt.

The paired policy is a genuinely different inference. Its arms come from the
runner-assigned `arm`/`pair_id` in each trial result, not from two bundles;
`baseline_reference` and `baseline_identity` are both filled with the
candidate's own identity (comparison.rs:2595–2599) because the single bundle
supplies both sides.

### 5. Why network-path *configuration* is critical but runtime facts are not

`network_path_summary` (comparison.rs:3424) is the whole of path identity:

| Participates | Does not participate |
|---|---|
| presence/absence (`"absent"`, comparison.rs:3426) | `physical_dial_attempts`, `successful_dials` |
| route chain identity — evidence `chain_config_digest` when available, else a recomputed digest of the resolved route (comparison.rs:3429–3433) | `hop_count_distribution`, `max_observed_hop_count` |
| route-driver descriptor summary (comparison.rs:3434) | `connection_ordinal_min` / `_max` / `_count` |
| `eggress_uri_version`, or the literal `"unverified"` when no evidence was staged (comparison.rs:3435) | `route_failures` buckets, `route_failure_buckets_dropped` |
| `semantics_version` | `fault_wrapped_connections`, `fault_wrapper_construction_failures` |
| fault request digest, fault-driver descriptor, `rng_version`, and the seed **only when faults are active** (else `0`, comparison.rs:3439–3456) | transient socket/port identity (never staged) |

The runtime counters are not merely ignored — they are validated for internal
consistency at load and then discarded as identity
(`stored_diagnostics_match`, comparison.rs:966–1009: hop-count histogram sums
to `successful_dials`, per-bucket counts are positive and bounded by the
configured hop count, fault-wrapped + construction-failure counts equal
`successful_dials` when faults are active and 0 otherwise, ordinal bounds are
consistent, failure labels are bounded and free of secret/password/token
substrings, comparison.rs:955). A tampered counter fails the load; an honest
counter changes nothing. The same split applies to diagnostics (request
configuration is identity at comparison.rs:1463–1485; per-execution
`report_status`, artifact digests, and warnings are shape-validated only at
comparison.rs:1295–1307) and to security (identity is rebuilt from the resolved
plan at comparison.rs:2043–2052; the persisted per-check disposition/count rows
are explicitly excluded, comparison.rs:2004–2007).

Staged path evidence is bound back to the resolved plan by five independent
checks, each its own failure: request contract (comparison.rs:1025), route
provenance (comparison.rs:1030, `stored_route_matches` comparison.rs:890),
canonical route text/digest/hop count (comparison.rs:1036,
`stored_canonical_route_matches` comparison.rs:912), fault provenance
including the active-seed rule (comparison.rs:1042, `stored_faults_match`
comparison.rs:927), and diagnostics consistency (comparison.rs:1048).
`canonical_evidence_identity_controls_equivalent_route_spellings`
(comparison.rs:4978) and `stored_route_evidence_must_be_bound_to_the_resolved_chain`
(comparison.rs:5013) pin the two directions of that binding. The fault seed is
the plan's `resolved.seed`, not the comparison `--seed`
(comparison.rs:3444–3448, 1042); `seed_matters_only_when_faults_are_active`
(comparison.rs:5062) pins that it enters identity only when the fault plan is
non-empty.

### 6. Sample selection

One trial contributes at most one scalar. `select_trials`
(comparison.rs:3782) walks trials in manifest order and assigns exactly one
stable reason per excluded trial, in this precedence: no normalized metrics →
`no_normalized_metrics`; observation absent → `metric_not_requested`; terminal
status not `Completed` → `trial_not_completed`; then the observation's own
`Missing`/`Invalid` reason mapped to a `snake_case` string
(comparison.rs:3834–3851). Nothing is imputed and no reason is inferred from
values. Request counts, histogram buckets, and per-request samples never enter
this count (`request_and_histogram_volume_never_change_sample_count`,
comparison.rs:5540).

### 7. Estimates, orientation, thresholds

| Quantity | Definition | Code |
|---|---|---|
| `candidate_estimate` | arithmetic mean of valid candidate observations | comparison.rs:3542, 3858 |
| `baseline_estimate` | geometric mean via `mean(ln v)`, `None` unless all strictly positive | comparison.rs:3634, 3874 |
| `degradation` | `LowerIsBetter`: `cand/base - 1`; `HigherIsBetter`: `base/cand - 1`; else `None` | comparison.rs:3890–3907 |
| `threshold` | `allowance / 10_000`; `None` for absolute gates | comparison.rs:3911–3918 |
| required observations | `max(plan.min_trials, 5)` | comparison.rs:3920 |
| recommended | 7 per side — warning only | comparison.rs:94, 3669 |

Positive degradation always means "the candidate is worse" under the declared
direction. `TargetRange` and `Informational` directions have no relative
representation at all: `oriented_degradation` returns `None`
(comparison.rs:3904) and a gated such metric is `Invalid`
(comparison.rs:3626, comparison.rs:3741).

### 8. Gate dispatch and the disposition vocabulary

`evaluate_metric` (comparison.rs:3531) order of decisions:

1. No gate → `disposition: None`. The metric stays descriptive by construction
   and is excluded from the aggregate (comparison.rs:4073).
2. Gated but `intent != Primary` → `Invalid` / `diagnostic_metric_cannot_gate`
   (comparison.rs:3570). Plan validation already forbids this; the branch is
   fail-closed redundancy.
3. `Gate::Absolute` → `evaluate_absolute` (comparison.rs:3715).
4. `Gate::RelativeRegression` or `Gate::StatisticalRelative` →
   `evaluate_relative` (comparison.rs:3597).

Absolute gate outcomes: no valid observations → `Invalid` /
`no_valid_candidate_observations`; any non-finite → `Invalid` /
`nonfinite_candidate_value`; `TargetRange` → `Invalid` /
`target_range_absolute_unsupported` (one scalar threshold cannot stand in for
two bounds, and the code refuses to reinterpret, comparison.rs:3734);
`Informational` → `Invalid` / `informational_metric_cannot_gate`; otherwise
`estimate <= value` (lower-is-better) or `>= value` (higher-is-better) is
`Pass`, else `Fail`. Absolute gates never consult the baseline, so they remain
eligible under every environment policy and under a critical mismatch
(comparison.rs:5134).

Relative gate outcomes, in order:

| Condition | Disposition | `reason` |
|---|---|---|
| No baseline | `Invalid` | `baseline_required` |
| Baseline unit/direction disagree, or no baseline trial carries the metric | `Invalid` | `metric_semantics_mismatch` / `baseline_metric_absent` |
| Any value non-finite or ≤ 0 on either side | `Invalid` | `nonpositive_relative_value` |
| Direction is `TargetRange`/`Informational` | `Invalid` | `unsupported_direction_for_relative_gate` |
| `DescriptiveOnly` + `StrictSameTestbed` | `Invalid` | `comparability_mismatch` |
| `DescriptiveOnly` otherwise | `Descriptive` | `cross_testbed_descriptive` / `comparability_mismatch_descriptive` |
| `RelativeRegression`, `degradation <= threshold` | `Pass` | — |
| `RelativeRegression`, `degradation > threshold` | `Fail` | — |
| `StatisticalRelative`, fewer than `max(min_trials, 5)` per side | `Invalid` | `insufficient_trials` |
| `StatisticalRelative`, `low > threshold` | `Fail` | — |
| `StatisticalRelative`, `high <= threshold` | `Pass` | — |
| `StatisticalRelative`, interval crosses the threshold | `Inconclusive` | — |

`DescriptiveOnly` is decided by `baseline_use` (comparison.rs:3518): the
candidate's own `EnvironmentPolicy` of `CrossTestbedDescriptive`, or a
`critical_mismatch`. Under `WarnOnMismatch` a critical mismatch therefore
yields `Descriptive`; under `StrictSameTestbed` it yields `Invalid`. Under
`CrossTestbedDescriptive` the same evidence yields `Descriptive` regardless of
whether anything mismatched. Note the asymmetry this creates: a mismatch under
`CrossTestbedDescriptive` and a match under `WarnOnMismatch` produce the same
disposition, distinguishable only by the `reason` string and the comparability
report.

`Inconclusive` here means exactly one thing: a bootstrap interval that
straddles the practical threshold. It is **not** the same token as the legacy
manifest-v1 run status. `LegacyRunStatus::Inconclusive`
(`crates/eggbench-core/src/evidence.rs:177`) is a run-level marker documented
as "ambiguous between no comparison and an inconclusive comparison"
(evidence.rs:176), it is normalized to `ExecutionStatus::Completed` on read
(evidence.rs:461) and is only distinguishable through
`BundleReader::legacy_status` (evidence.rs:1236). The manifest's
`comparison_verdict` field is reserved and stays unset
(evidence.rs:197–206; `docs/baselines.md`). Two unrelated vocabularies share a
word; the receipt never projects into the manifest and the manifest status
never influences a disposition.

### 9. The deterministic bootstrap

| Element | Choice | Code |
|---|---|---|
| RNG | `SplitMix64`, owned by the policy, used **only** for bootstrap indices | comparison.rs:4000–4016 |
| Base seed (unpaired) | FNV-1a 64 over `cand_sha \| base_sha-or-"absolute-only" \| policy_id` | comparison.rs:4029–4044, 4019 |
| Base seed (paired) | FNV-1a 64 over `cand_sha \| policy_id` | comparison.rs:3004–3010 |
| Per-metric seed | FNV-1a 64 over `base_seed.to_le_bytes() \| metric_name` | comparison.rs:4047–4053 |
| Resamples | `BOOTSTRAP_RESAMPLES = 10_000`, never reduced in production | comparison.rs:86 |
| Unpaired draw | per iteration: `cand.len()` candidate indices, then `base.len()` baseline indices, from one stream; mean of logs per side; oriented log difference; `exp()-1` | comparison.rs:3943–3967 |
| Paired draw | per iteration: `differences.len()` indices; mean log-difference; `exp()-1` | comparison.rs:3975–3984 |
| Index draw | `rng.next() % len`, clamped | comparison.rs:3989–3998 |
| Quantiles | low `(25n)/1000`, high `((975n+999)/1000) - 1`, both clamped — indices 250 and 9749 at `n = 10_000` | comparison.rs:3934–3942 |

Both sides of an unpaired comparison draw from the *same* stream, so the
interval is a function of `(seed, candidate values, baseline values, direction)`
only. Determinism is total: metrics iterate in `BTreeSet` name order
(comparison.rs:2355), nothing consults the clock or OS randomness, and the
receipt has no timestamp field. Each statistical record carries
`statistical_method`, `resamples`, and `effective_seed` so a reader can
reproduce the exact draw. The paired policy records the same three fields in
both its `MetricComparison` rows and its `PairedMetricRecord`
(comparison.rs:2697, 2791–2793). `docs/comparison.md:13` correctly notes the
fault-plan seed is a separate plan field and is not supplied by `--seed`.

### 10. Aggregation

`aggregate` (comparison.rs:4059) walks primary-intent metrics only and applies
precedence `Invalid > Fail > Inconclusive > Pass`, returning `None` when no
metric is gate-eligible. `Descriptive` and `disposition: None` are ignored
(comparison.rs:4073) — a run of purely descriptive metrics yields no aggregate
rather than a false `Pass`. `combine_verdicts` (comparison.rs:1896) then merges
performance with correctness under the same precedence and returns `None` only
when neither side gates. `aggregate_correctness` (comparison.rs:1871) is
stricter: an empty record list is `Invalid`, so correctness can never be
absent-and-passing.

### 11. Paired comparison

`compare_paired` (comparison.rs:2488) takes one bundle. `select_pairs`
(comparison.rs:2818) is the arm extraction:

- A trial without both `arm` and `pair_id` is excluded on **both** sides with
  `trial_missing_pair_identity` (comparison.rs:2826–2834) — it cannot be
  attributed to an arm.
- A repeated `(arm, pair_id)` is excluded on its own side as
  `duplicate_pair_identity` (comparison.rs:2835).
- Remaining trials are classified exactly as `select_trials` does
  (comparison.rs:2850–2896), then indexed by `pair_id` per arm.
- A pair is complete only when both arms are present; otherwise it is excluded
  **whole** as `ExcludedPair { reason: "pair_incomplete" }`
  (comparison.rs:2925). Pairs are never split and nothing is imputed
  (`paired_split_pairs_are_excluded_never_imputed`, comparison.rs:5957).
- `candidate_included`/`baseline_included` follow **pair execution order**
  (ascending pair id), not manifest order (comparison.rs:2931).

Per-pair oriented effects (`exp(oriented_log_difference) - 1`) are recorded in
execution order as descriptive evidence (comparison.rs:2673–2686).
`drift_diagnostics` (comparison.rs:2970) splits at `len/2` — the second half
holds the extra pair for odd counts — and reports the exact sign of the
half-mean difference; fewer than two complete pairs yields `Insufficient` with
no means claimed. Drift is consumed by nothing: the same verdict is produced
with or without it (`paired_drift_trend_sign_is_correct_and_never_gates`,
comparison.rs:6107).

Gate rules mirror v1 with three differences: absolute gates are invalid
(comparison.rs:2713); the required count is complete *pairs*
(`insufficient_pairs`, comparison.rs:2768); and the below-recommended warning
counts pairs (`below_recommended_pair_count`, comparison.rs:2773).

Where paired inference lives: **only** in `compare_paired` /
`evaluate_paired_metric`. Policy v1 has none. Unpaired comparison of a paired
bundle is refused per metric — `paired_rejection_record`
(comparison.rs:2426) emits empty sample lists, no estimates, and
`Invalid` / `paired_evidence_requires_paired_comparison`, with a matching
`paired_bundle_requires_paired_comparison` warning (comparison.rs:2362). The
refusal exists because unpaired inference over paired evidence would either mix
arms or invent a pairing.

### 12. The standalone receipt

`compare` and `compare_paired` build a `ComparisonReceipt`
(comparison.rs:2403, 2590) and nothing else. Contents: schema version, policy
id, `created_by_version` (the building crate version, comparison.rs:2406),
candidate identity, baseline reference *and* identity, the candidate's
`environment_policy`, the full comparability report, base seed, per-metric
records in name order, and up to three optional sections — combined
`aggregate_verdict`, `performance_verdict`, `correctness`, `paired` — plus
bounded warnings.

Immutability is structural, not asserted. The only filesystem operations in
the module are `BundleReader::open`/`open_artifact`, `File::open` for the
manifest (comparison.rs:4093), and `std::fs::read` for the alias file
(comparison.rs:2190). There is no writer, no rename, no delete, and no call
into the evidence writer.

Reading is fail-closed: `parse_comparison_receipt` (comparison.rs:2073) uses
`deny_unknown_fields` on the struct and on every nested receipt type, and
rejects any schema version outside v1–v4. Legacy v1/v2 receipts are accepted
with their historical metric-only aggregate meaning preserved
(comparison.rs:454–461, `legacy_receipts_parse_with_historical_semantics`
comparison.rs:6626).

## Invariants

1. **One trial is one observation.** Request counts, physical dials, histogram
   buckets, and per-request samples never increase the sample count
   (comparison.rs:8–11).
2. **Identity is content, derived after verification** — manifest schema
   version + run id + SHA-256 of the exact finalized manifest bytes
   (comparison.rs:139, 2097–2106). Paths are never identity.
3. **The candidate is authoritative.** Metric requests, gates, and
   `environment_policy` come from the candidate plan only
   (comparison.rs:2370, 2388, 2413); a baseline's gate declarations can never
   override candidate policy.
4. **Nothing is imputed.** Every excluded trial or pair carries a stable
   `snake_case` reason, and pairs are excluded whole
   (comparison.rs:3782, 2925).
5. **Threshold and uncertainty stay separate.** A pass requires
   `high <= threshold`; nothing collapses the interval into the threshold
   (comparison.rs:329–340, 3695–3701).
6. **Descriptive is never a gate.** `Descriptive` and ungated metrics cannot
   reach an aggregate (comparison.rs:4073).
7. **No p-value**, anywhere in the module contract, the receipt schema, or any
   nested type; the only mention is the prohibition itself
   (comparison.rs:22).
8. **No bundle mutation** — read-only APIs only (see §12).
9. **Determinism.** Same bundles, policy, and seed → byte-equivalent receipt
   (`same_inputs_yield_byte_equivalent_receipts`, comparison.rs:4507; paired
   equivalent, comparison.rs:6145).
10. **Configuration identity excludes results.** Observed statuses, timings,
    counts, and dispositions never participate in comparability, for replay,
    diagnostics, security, and network path alike (comparison.rs:650–653,
    670–674, 689–694, 3296–3346, 3424–3461).
11. **Fail closed structurally, report per check.** A bundle-structural
    contradiction aborts the load; a per-check evidence problem becomes an
    `Invalid` record that stays visible (comparison.rs:1494–1506).

## Failure model

`ComparisonError` (comparison.rs:107) is the only error type, and `category()`
(comparison.rs:124) gives the CLI a stable machine-readable string: `bundle` for
any `BundleError`, the alias category verbatim (`baseline_alias_unreadable`,
`baseline_alias_invalid`, `baseline_alias_unsupported`,
`baseline_alias_digest_mismatch`), and `unsupported_comparison` for
`Unsupported`.

| Failure | Where | Result |
|---|---|---|
| Bundle fails verification | comparison.rs:2097 | `Error` — no identity, no receipt |
| Missing/unreadable/unparsable manifest or required role artifact | comparison.rs:4091, 4123 | `Error` |
| Trial result schema/id mismatch | comparison.rs:4160 | `Error` |
| Evidence staged without a matching plan declaration, or a declaration without evidence | comparison.rs:845, 1100, 1350, 1935 | `Error` |
| Evidence media type / role / size out of bounds | comparison.rs:853, 1109, 1368, 1953 | `Error` |
| Path provenance contradicts the resolved plan | comparison.rs:1030–1052 | `Error` |
| Security paired/path composition | comparison.rs:1520–1528 | `Error` |
| Per-check evidence missing/tampered/malformed | comparison.rs:1600–1701 | `Invalid` record: `config_mismatch`, `missing_artifact`, `digest_mismatch`, `unparsable_result`, `disposition_mismatch`, `contract_violation`, `producer_mismatch`, `scope_mismatch` |
| HTTP-corpus per-check problem | comparison.rs:1735–1802 | `Invalid` record: `missing_artifact`, `config_mismatch`, `artifact_bound`, `unparsable_result`, `contract_violation`, `unsupported_composition` |
| Persisted correctness disposition disagrees with recomputation | comparison.rs:1650 | `Invalid` / `disposition_mismatch` — the stored string is never trusted |
| Insufficient sample count | comparison.rs:3664, 2766 | `Invalid` / `insufficient_trials` / `insufficient_pairs` |
| Comparability mismatch | comparison.rs:3639–3650 | `Invalid` under strict, `Descriptive` otherwise |
| Interval crosses the threshold | comparison.rs:3695, 2794 | `Inconclusive` |
| Unpaired compare of a paired bundle | comparison.rs:2426 | `Invalid` / `paired_evidence_requires_paired_comparison` |
| `compare_paired` with no paired design | comparison.rs:2613 | `Invalid` / `paired_design_absent[_from_resolved_plan]` |

Every row is a receipt rather than an abort except the load-time rows — the
distinction that lets `compare` stay infallible. Hard bounds: manifest 8 MiB
(comparison.rs:4089), each evidence index 128 KiB (comparison.rs:1137, 1431,
1536, 1979), per-check artifact 128 KiB (comparison.rs:1585), HTTP-corpus
artifact 512 KiB (comparison.rs:1765), alias file 64 KiB
(comparison.rs:2197).

## Boundaries

- **No evidence writing.** The module depends on `BundleReader`, not
  `BundleWriter`; the manifest `comparison_verdict` field is untouched
  (`docs/baselines.md`).
- **No exit codes.** 6/7/8 for fail/inconclusive/invalid are a CLI concern
  (`docs/comparison.md:67`); core returns verdicts only.
- **No environment collection.** Fingerprints arrive already staged; core
  validates but does not produce them.
- **No plan interpretation beyond the fields it consumes.** Metric requests,
  gates, workload, drivers, topology, and subject kind are read from the
  resolved plan; the re-validation call at comparison.rs:2108 delegates to
  `plan.rs`.
- **No gate-kind invention.** `Gate` and `MetricDirection` come from
  `resolved.rs`; unsupported combinations become `Invalid` with a reason
  rather than a reinterpretation.
- **No security expectation inference.** Correctness expectations come from
  the candidate plan's declared checks; baseline outcomes never redefine them
  (comparison.rs:2278–2282).
- **No paired inference in v1, and no unpaired inference over paired
  evidence** — the two exclusions are the module's hardest boundaries and both
  are enforced with fail-closed dispositions rather than error returns.
- **No claim about drift causality.** Drift is descriptive, computed from
  first/second half means, and structurally cannot reach a disposition.

## Tests and qualification

All tests are in-file `#[cfg(test)] mod tests` (comparison.rs:4171–6780);
there is no `tests/` integration module specific to comparison except the
golden receipt files. Roughly 70 tests, grouped:

| Group | Examples |
|---|---|
| Determinism | `same_inputs_yield_byte_equivalent_receipts` (4507), `derived_seed_is_stable_and_metric_scoped` (4519), `paired_comparison_is_deterministic_with_stable_seeds` (6145) |
| Orientation monotonicity | `worsening_candidate_cannot_improve_degradation` (4653), `increasing_allowance_cannot_turn_pass_into_fail` (4669), `higher_is_better_orientation_is_correct` (5570) |
| Verdict vocabulary | `clear_regression_fails_and_non_regression_passes` (4530), `threshold_crossing_is_inconclusive` (4548), `insufficient_trials_are_invalid` (4562), `zero_relative_values_are_invalid` (4574) |
| Sample discipline | `missing_and_invalid_trials_are_counted_not_imputed` (5519), `request_and_histogram_volume_never_change_sample_count` (5540) |
| Environment policy | `warning_only_mismatch_warns_but_gates` (4693), `critical_mismatch_invalidates_strict_gates` (4720), `warn_policy_suppresses_verdicts_to_descriptive` (4746), `cross_testbed_is_always_descriptive` (4802) |
| Comparability dimensions | `driver_version_mismatch_invalidates` (4828), `workload_shape_mismatch_invalidates` (5158), `semantic_replay_digest_mismatch_invalidates` (5188), `semantic_vs_non_semantic_workloads_are_incomparable` (5231), `subject_digest_difference_alone_does_not_invalidate` (5509), `absolute_gate_remains_eligible_when_path_identity_differs` (5134) |
| Network path | `network_path_identity_is_comparison_critical` (4839), `every_network_path_identity_dimension_is_comparison_critical` (4878), `canonical_evidence_identity_controls_equivalent_route_spellings` (4978), `stored_route_evidence_must_be_bound_to_the_resolved_chain` (5013), `seed_matters_only_when_faults_are_active` (5062) |
| Diagnostics / security | `identical_diagnostics_compare_equal` (5274), `diagnostic_config_or_provenance_mismatch_invalidates` (5283), `diagnostics_vs_absent_runs_are_incomparable` (5346), `identical_security_checks_compare_equal` (5379), `security_config_or_provenance_mismatch_invalidates` (5441), `security_vs_absent_runs_are_incomparable` (5500) |
| Paired | `paired_clear_regression_fails` (5825), `paired_threshold_crossing_is_inconclusive` (5920), `paired_insufficient_pairs_are_invalid` (5943), `paired_split_pairs_are_excluded_never_imputed` (5957), `paired_untagged_trials_are_excluded_on_both_sides` (5988), `paired_absolute_gate_is_invalid` (6016), `unpaired_compare_of_paired_bundle_is_invalid` (6053), `paired_comparison_without_design_is_invalid` (6085), `paired_drift_trend_sign_is_correct_and_never_gates` (6107), `paired_relative_regression_gate_uses_complete_pairs_only` (6179) |
| Correctness | `correctness_aggregation_prefers_invalid_then_fail` (6250), `combined_verdict_precedence_is_invalid_fail_inconclusive_pass` (6272), `valid_security_evidence_loads_as_pass` (6499), `tampered_or_missing_evidence_is_invalid_never_pass` (6517), `correctness_only_receipt_combines_without_metrics` (6586), `invalid_correctness_cannot_hide_behind_metrics` (6607), `security_evidence_never_enters_trial_metrics` (6695) |
| Receipt I/O | `legacy_receipts_parse_with_historical_semantics` (6626), `receipt_parser_rejects_unknown_versions_and_fields` (6666) |
| Golden receipts | `golden_receipts_are_stable` (5688) over six kinds — `pass`, `fail`, `inconclusive`, `invalid-comparability`, `descriptive-cross-testbed`, `absolute-only`; `paired_golden_receipts_are_stable` (6762) over `paired-pass`, `paired-fail`, `paired-inconclusive` |

Goldens live at `crates/eggbench-core/tests/golden/comparison-*.json` and are
regenerated only with `EGGBENCH_UPDATE_GOLDEN` set
(comparison.rs:5703, 6770) — the same env var is the only write path in the
file, and it is test-only.

## Review focus

1. **Silent truncation at 256 metrics.** The metric loop breaks at
   `MAX_RECEIPT_METRICS` (comparison.rs:103, 2378, 2393, 2561) with no warning
   and no receipt field saying metrics were dropped; a plan with more than 256
   metrics produces a receipt that looks complete.
2. **`check_metric_semantics` is first-wins** (comparison.rs:3755–3777): it
   returns `Ok(())` on the first baseline trial carrying the metric, so a
   baseline whose later trials carry a different unit or direction is only
   partially checked.
3. **Class comes from the candidate when both sides have the field**
   (comparison.rs:3030–3032). A baseline that reclassifies a field from
   comparison-critical to informational is not detected.
4. **Informational differences leave no trace** (comparison.rs:3037, 3045,
   3053): a receipt cannot show that an informational field existed on only
   one side.
5. **Both-missing evidence compares equal** (`"missing-evidence"`,
   comparison.rs:3200, 3272, 3303): two bundles that both failed to stage the
   same replay/diagnostics/security evidence are comparable on it.
6. **`created_by_version` is `env!("CARGO_PKG_VERSION")`** (comparison.rs:2406,
   2593), so byte-equivalence holds for a fixed build, not across core
   versions.
7. **The network-path policy is an identity change, not a method change.** The
   estimator, thresholds, quantiles, and dispositions are shared code; a
   reviewer adding path awareness should confirm that stays true.
8. **Modulo index draw** `rng.next() % len` (comparison.rs:3994) is biased for
   non-power-of-two lengths — bounded and too small to move a 95% percentile
   interval over ≥5 observations, but a real property of the interval.
9. **Stale schema references in the module's own docs.**
   `parse_comparison_receipt` says "Accepts schema v1, v2, and v3"
   (comparison.rs:2065) but accepts v4 (comparison.rs:2076–2080); `compare`
   says "Emits a schema-v3 receipt" (comparison.rs:2302) but emits v4
   (comparison.rs:2404); `PairedComparisonSection` still says schema v2
   (comparison.rs:514).
10. **Orphaned doc block.** `compare`'s doc comment, `#[must_use]`, and
    `# Panics` (comparison.rs:2266–2277) sit above `correctness_section`
    (comparison.rs:2283), which already has its own doc comment; rustdoc
    attaches both blocks to the wrong function and `compare`'s panic contract
    is undocumented.
11. **Unreachable `StrictSameTestbed` arm** in `descriptive_reason`
    (comparison.rs:3711) — handled earlier at comparison.rs:3643.
12. **Descriptive-value collision.** A mismatch under
    `CrossTestbedDescriptive` and a mismatch under `WarnOnMismatch` both yield
    `Descriptive`, separated only by `reason` text.
13. **Correctness records are cloned into the receipt** (comparison.rs:2284,
    2135), so per-check evidence is rebuilt on every comparison; for a large
    `security_checks` list this is the pass's main non-obvious cost.

## Related

- [overview.md](overview.md) — module map and the artifact chain;
  `core-comparison.md` sits after bundle finalization and never feeds back.
- [core.md](core.md) — the M002 section: core stays free of Tokio, process,
  and network dependencies; bounded synchronous reads only. Agrees with the
  code (no async or socket types in this module).
- [core-evidence-bundle.md](core-evidence-bundle.md) — `BundleReader`,
  manifest contract, and the legacy v1 status ambiguity this module must not
  confuse with `GateDisposition::Inconclusive`.
- [core-metrics.md](core-metrics.md) — `TrialMetrics` and
  `ObservationState`, the only per-trial input this module consumes.
- [core-plan-schema.md](core-plan-schema.md) — the declarations that reach
  comparison as gates, workload, and the four evidence requests.
- [core-driver-resolution.md](core-driver-resolution.md) — where
  `descriptor_summary` and the resolved topology originate.
- [../docs/comparison.md](../docs/comparison.md) — canonical policy contract.
- [../docs/baselines.md](../docs/baselines.md) — identity, alias resolution,
  environment policy effects.
- [../docs/paired-experiments.md](../docs/paired-experiments.md) — the paired
  schedule and why pairs are the resampling unit.
- [../docs/environment-fingerprint.md](../docs/environment-fingerprint.md) —
  field classes and the canonical field table.
- [../docs/metrics.md](../docs/metrics.md) — metric vocabulary, aggregation,
  and why one trial is one observation.
