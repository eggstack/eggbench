# Eggstack Integration M004b — Security Correctness Gate Family — Closure

Disposition: **closed**

This record is also the umbrella Eggstack M004 closure: M004a (strict
Eggsec WAF correctness adapter, implementation `273e5b1`, closure
`plans/closure/eggstack-integration/004a-status.md`) plus M004b
(independent correctness gate family and combined verdicts) together
close M004.

Closed: 2026-09-25

Implementation commit: `b2de53e` (`feat(eggstack): implement M004b
security correctness gate family`), on top of planning baseline
`aeed8f7` (M004b plan `5b5ef97`) with M004a closure as predecessor.

Hosted qualification: four-lane CI run `36200879518` plus live
qualification run `36200879546` (M003 live-tool, M004a live-Eggsec, and
M004b live-combined jobs) on the exact implementation candidate
`b2de53e` — all green (see §3).

M004b closure unblocks Security Qualification M001 implementation
planning/handoff and closes Eggstack M004.

## 1. Requirement-to-evidence matrix

Acceptance criteria are
`plans/implementation/eggstack-integration/004b-security-correctness-gate-and-m004-closure.md`
§30.

| Plan requirement | Evidence | Outcome |
|---|---|---|
| Security correctness remains separate from metrics | No `MetricRequest` is synthesized from bypass counts (grep-clean); `TrialMetrics` carry workload observations only; no-contamination test proves opposite correctness outcomes leave metric bytes and the performance verdict identical | Pass |
| ComparisonReceipt v3 additive/read-compatible with v1/v2 | `COMPARISON_RECEIPT_SCHEMA_VERSION = 3` with v1/v2 consts retained; `performance_verdict`/`correctness` are `Option` with skip-None (performance-only serialization gains only the explicit verdict); saved `receipt-v1.json`/`receipt-v2-paired.json` fixtures parse with historical semantics; `parse_comparison_receipt` gates versions 1/2/3 and rejects unknown versions/fields | Pass |
| Performance-only verdict retained explicitly in v3 | `performance_verdict` is the conservative metric-only aggregate; goldens show it mirroring the legacy aggregate for security-free receipts; paired v3 receipts behave exactly like v2 policy plus the explicit field | Pass |
| Correctness section has an immutable policy identifier | `eggbench.security-correctness.v1` in every section; live Case A-F receipts assert it; any semantic change requires a new identifier by contract | Pass |
| Correctness evidence recomputed/validated from bundle artifacts | `load_correctness_records`: index + per-check digest verification, contract validation, config/producer/scope matching, stored-versus-recomputed disposition rejection; valid live bundles load as Pass/Fail | Pass |
| Security Pass/Fail/Invalid aggregation deterministic | `aggregate_correctness` truth-table test (Invalid > Fail > Pass; empty → Invalid) | Pass |
| Final precedence Invalid > Fail > Inconclusive > Pass | `combine_verdicts` truth-table test over all performance × correctness combinations | Pass |
| Performance Pass cannot override security Fail | Live Case B (real binary, real lifecycle): performance Pass + correctness Fail → final Fail; CLI exit 6 with security-specific detail and typed v3 sections | Pass |
| Security Pass cannot override performance Fail | Live Case C (tight absolute budget): performance Fail + correctness Pass → final Fail, exit 6 | Pass |
| Security invalidity cannot be hidden by performance Pass | Live Case D (tampered bytes, consistent digest chain): Invalid record with `disposition_mismatch` → final Invalid, exit 8, despite performance Pass | Pass |
| Security Fail leaves execution Completed when tool execution was valid | Case B bundle: `execution_status completed` with 2/2 trials; M004a lifecycle tests (Fail continues) unchanged and green | Pass |
| Candidate-only security comparison works | Live Cases A/F via `compare --absolute-only` (performance-gated and correctness-only); exit 0 with valid v3 receipts | Pass |
| Security Fail returns existing comparison-fail exit 6 | Case B CLI assertion (`ComparisonFail`, code 6); no new exit code added | Pass |
| Baseline comparison never derives expectations from baseline security outcomes | Records are built from the candidate bundle only; baseline participates solely through existing performance policy plus `compare_security` identity; Case E uses a baseline for the statistical gate while correctness stays candidate-evaluated | Pass |
| Security configuration is comparison-critical | M004a `compare_security` (presence/order/IDs/source/family/target/test-type/allowance/concurrency/timeout/producer/scope/adapter) plus mismatch matrix tests; StrictSameTestbed mismatch → critical mismatch | Pass |
| Security timing never enters TrialMetrics/bootstrap | Structural (no timing field in the sanitized result) plus the no-contamination regression test with extreme-evidence variants | Pass |
| Paired/path invalid combinations fail closed | Plan validation rejects both compositions; the loader fails closed on paired/path bundles carrying security evidence despite validation | Pass |
| Legacy v1/v2 receipts remain unchanged and readable | Saved pre-v3 golden bytes as fixtures parse with historical aggregates and no v3 sections; no historical receipt rewritten; golden deltas limited to schema 3 + explicit `performance_verdict` | Pass |
| Real Eggsec Pass/Fail bundles exercise combined comparison | Cases A-F all use the pinned binary (A/C/D/E/F via production runs, B via the live echo-adapter run); unit/golden coverage additionally proves the matrix without invoking Eggsec | Pass |
| Rust 1.89 is green | `cargo +1.89.0` check default/all-features plus core (161), runner (118), drivers (147) all-feature tests green | Pass |
| Live Eggsec hosted qualification is green | `live-eggsec-linux` green in run `36200879546` (plus `36197530332` on the M004a candidate) | Pass |
| Normal four-lane hosted CI is green | Run `36200879518`: linux-stable, linux-msrv, macos-stable, windows-stable all green | Pass |
| Umbrella M004 closure/reconciliation is committed | This file plus plan-status/roadmap/registry reconciliation (M004 closed; Security Qualification M001 ready; no historical evidence rewritten) | Pass |

## 2. Sibling seam audit — implementation time

- No new sibling surface: M004b consumes only M004a's sanitized
  bundle evidence (`security-checks.json` + `security/<id>.json`) from
  the pinned Eggsec contract (`0509ac66`, workspace 0.1.0; provenance
  in the M004a closure §2, re-verified by the hosted live jobs on this
  candidate). No Eggsec process is spawned by comparison; no payload,
  severity, timing, or WAF-product semantics are interpreted.
- The M004b production delta outside tests/harness/docs is confined to
  `comparison.rs` (receipt v3, loader, aggregation, combination,
  receipt parser), the CLI human detail line, goldens/fixtures, and
  behavior-preserving Clippy refactors in `eggsec.rs`. M004a execution
  and evidence semantics are unchanged (M004a suites green unmodified
  except the shared golden suffix rule).
- Case B's reflecting service is a qualification-only test adapter
  (`m004b_live.rs`, never production): it echoes the request target
  with 200 except `/` (404) and publishes a loopback `http_url`
  binding. It contains no scanner logic and is registered only inside
  the live test.

## 3. Verification record

Local (implementation host, `EGGBENCH_PARENT_SENTINEL_26CE=set-in-parent-process`):

- `cargo fmt --all -- --check` — pass.
- `cargo check --workspace --all-targets --locked` — pass.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — pass.
- `cargo test --workspace --all-targets --locked` — pass: **440 tests across 22 suites**.
- `cargo check --workspace --all-targets --all-features --locked` — pass.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` — pass.
- `EGGBENCH_PARENT_SENTINEL_26CE=set-in-parent-process cargo test --workspace --all-targets --all-features --locked` — pass: **499 tests across 22 suites**.
- `cargo +1.89.0 check --workspace --all-targets --locked` — pass.
- `cargo +1.89.0 check --workspace --all-targets --all-features --locked` — pass.
- `cargo +1.89.0 test -p eggbench-core --all-features --locked` — pass (161).
- `cargo +1.89.0 test -p eggbench-runner --all-features --locked` — pass (118).
- `cargo +1.89.0 test -p eggbench-drivers --all-features --locked` — pass (147).
- `cargo tree --locked` — pass; no `eggsec-*` crate in any graph.
- `git diff --check` — pass.
- M004b harness `scripts/qualification/m004b-combined/run-live-qualification.sh` — **14/14 PASS** (Cases A-F with exits 0/6/7/0/8/0, typed v3 proofs, policy identifier, no-contamination, rendering separation, human family naming).
- M004a harness re-run at final state — **8/8 PASS**.
- Post-M003 live-tool harness re-run at final state — **20/20 PASS** after one timing flake: the first run stopped only on criterion-17 (the 25-trial cancel smoke finished before SIGINT on a fast host — a documented harness race, "run finished before SIGINT (timing)"); the rerun passed fully. All sibling-process machinery (record/replay/validate, probe plans, combined runs, absence paths, cancellation, child cleanup) is unregressed; the M004a/M004b code path is a no-op for security-free plans by construction (empty check list reserves nothing and executes nothing).

Hosted (on the exact implementation candidate `b2de53e`):

- CI run `36200879518`: linux-stable, linux-msrv (1.89.0), macos-stable, windows-stable — all green.
- Live run `36200879546`: `live-tools-linux`, `live-eggsec-linux`, `live-m004b-linux` — all green.

## 4. Schema, compatibility, and provenance

- Receipt v3 is additive: new receipts carry `performance_verdict`,
  `correctness`, and the combined `aggregate_verdict`. Legacy v1/v2
  bytes parse with historical metric-only aggregates and no v3
  sections; `parse_comparison_receipt` rejects unknown versions and
  unknown fields. No historical receipt, bundle, or M003/M004a closure
  was rewritten.
- Comparison goldens changed by exactly the v3 delta per file
  (`schema_version` 3 plus the explicit `performance_verdict`
  mirroring the legacy aggregate); paired v3 receipts behave exactly
  like v2 policy plus the explicit field. Pre-v3 golden bytes are
  preserved as `receipt-v1.json` / `receipt-v2-paired.json` fixtures
  with parse tests.
- `ResolvedPlan` stays v4; correctness resolution and evidence
  contracts are unchanged from M004a. Producer/scope provenance flows
  from evidence identity into per-check records; ephemeral timestamps,
  durations, and observed bypass counts remain result evidence, never
  configuration identity.
- Case E methodology note: the statistical gate is evaluated for real
  (bootstrap interval over six real observations per side) against the
  bundle itself, giving perfect comparability while the
  zero-allowance interval deterministically straddles zero. This
  qualifies the Inconclusive combination live without depending on
  cross-run timing noise.

## 5. Security and lifecycle evidence

- Stored dispositions are never trusted: the loader recomputes the
  threshold relation and rejects disagreement (`disposition_mismatch`);
  tampered digests, missing artifacts, contract violations, and
  config/producer/scope mismatches each yield stable-reason `Invalid`
  records. Invalid correctness can never become a pass, and a
  performance pass can never override a correctness failure — in either
  direction.
- Paired/path compositions fail closed at plan validation and again at
  evidence load; baseline security outcomes never redefine candidate
  expectations; correctness-only candidates produce valid receipts;
  exit codes reuse the locked matrix (Fail 6, Inconclusive 7,
  Invalid 8) with no new security-specific code.
- Human stderr names the failing family ("security correctness gate"
  vs "primary gate"); machine JSON uses the typed v3 fields. Never any
  payload strings: the loader never opens raw Eggsec output (it does
  not exist in bundles), and per-check records carry counts plus
  digests only.
- A completed run whose observation exceeds its allowance stays
  `ExecutionStatus::Completed`; operational inability to produce
  trustworthy evidence remains an execution/validity problem under
  M004a. The distinction is covered live (Case B) and by unit tests.

## 6. Known limitations and follow-up boundaries

- Correctness-only Fail is proven live at the adapter level (M004a
  permissive fixture) and by the deterministic combination truth
  table; the live combined Fail with a correctness-only plan is not
  separately run (Case F runs Pass; Case B runs the combined Fail with
  a performance gate present).
- The v1 correctness policy covers only the `waf_bypass` family with
  `MaxSuccessfulBypasses` expectations and no `Inconclusive`
  per-check state. Baseline-derived expectations, statistical
  inference over cases, severity weighting, risk scores, timing
  metrics, tradeoff scoring, paired/path correctness, and broader
  profile/corpus semantics are explicitly non-goals and belong to
  Security Qualification M001, which now owns them on this substrate.
- `eggbench inspect` continues to show M004a bundle evidence; there is
  no standalone receipt-inspection surface (the typed v3 receipt JSON
  is the machine surface, validated by `parse_comparison_receipt`).

## 7. Disposition

**Closed.** M004b delivers the versioned security-correctness
comparison policy, ComparisonReceipt v3 with an explicit
performance-only verdict and a typed correctness section, validated
candidate evidence loading with recomputed dispositions, deterministic
correctness aggregation, conservative performance-plus-correctness
combination, candidate-only and baseline comparison semantics with
comparison-critical security identity, stable CLI output/exit behavior,
receipt compatibility proofs, and real combined qualification for
Cases A-F. Together with M004a, Eggstack M004 is closed and
hosted-qualified. Security Qualification M001 is unblocked.
