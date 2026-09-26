# Security Qualification M002a — Closure

Disposition: conditionally closed

Implementation commit: `b74f861` (routine scope; production, assets, tests, harness).

## What landed

Checked-in profile workspace `qualification/synvoid/v1/` (M001 profile
schema v1, HTTP corpus schema v1, plan schema 8):

- `profile.json` (`synvoid-waf-correctness-v1`), one correctness-gating scenario;
- `corpus.json` (5-case synthetic WAF corpus, owner explicitly non-SynVoid);
- `target-config.json` (synthetic target identity with policy id, listen port, mapping);
- `upstream-manifest.md` (frozen SynVoid import boundary + six harness checks);
- `materialized/provenance.json` (synthetic manifest fixture with 22 exclusion reasons);
- `scenarios/waf-correctness.json` (managed EggServe origin + managed command
  subject with static `http_url` + M001b `eggbench-http-corpus/http_observable` check);
- `stubs/fake_synvoid.py` (routine-only stand-in: 403 attack shapes, 200 + 1024x`0x42`
  benign routes, 501 otherwise);
- `README.md` (source mapping, deviations D1-D3, routine procedure).

Production change (one, generic, not SynVoid-specific): `eggfetch-http`
`compatible_service_types` gains `command`, so a managed command subject
with an M001a static `http_url` binding is a drivable workload target
(M001a roadmap intent: process subjects need no subject-specific code).
Without it no native workload can target SynVoid and the plan is
unimplementable as written.

Harness/CI: `scripts/qualification/synvoid-m002/run-live-qualification.sh`
(Stage A synthetic smoke + Stage B real-SynVoid gate that fails closed
while the upstream contract is open) and the `live-synvoid-linux` job in
`.github/workflows/live-tools.yml`.

## Requirement-to-evidence matrix

| # | Plan acceptance criterion | Evidence | Result |
|---|---|---|---|
| 1 | SynVoid upstream asset contract is closed | Contract absent at audited SHA (harness proves absence each run) | CONDITION |
| 2 | Eggbench does not parse SynVoid internal fixture semantics | No SynVoid crate import; corpus owner is Eggbench-synthetic; exclusion list carries source IDs as opaque labels only | Pass |
| 3 | SynVoid minimal binary runs as a managed command subject | Scenario declares managed command subject; routine proof uses the stand-in; real binary wiring is harness-staged | Partial (live condition) |
| 4 | Loopback controlled origin and static binding deterministic | Real `eggserve-origin` adapter in scenario (`/search`, 1024 B, 200); binding in runtime topology evidence | Pass |
| 5 | Exported corpus executes through M001b | `synvoid-waf` check via `eggbench-http-corpus/http_observable`, policy v2, ComparisonReceipt v4 | Pass |
| 6 | Live Pass/Detect mappings match owner-authored expectations | Synthetic positive run Pass (5/5 cases); owner-authored proof awaits the export | CONDITION |
| 7 | Negative expectation mutation yields qualification Fail | `mutated_expectation_yields_qualification_fail` (exit 6); harness repeats it | Pass |
| 8 | Raw payloads not copied into portable result evidence | `positive_run_passes_and_leaves_no_subject_behind` scans the finalized bundle (attack bytes only in bounded service logs, never in sanitized projections) | Pass |
| 9 | Executable/config/corpus/source provenance immutable | Expansion freezes corpus/target identities; manifest checks (policy/SHA/digest/port) proven by `manifest_verification_rejects_mismatch` (4 rejections) | Pass (routine inputs) |
| 10 | Cancellation/teardown leaves no SynVoid child behind | Post-run port rebind proof in positive test; runner drain/teardown machinery unchanged and green | Pass (routine) |
| 11 | Rust 1.89 and four-lane CI green | MSRV check + core/runner/cli tests pass; hosted four-lane run is pending push | CONDITION (hosted) |
| 12 | Linux live SynVoid qualification green | Harness Stage B reports NOT-EXECUTED (contract open); no live claim made | CONDITION |

## Tests/guards run and outcomes

- `synvoid_m002a` (6 tests): validate/expand, positive Pass + teardown +
  sanitization, mutated-expectation Fail (exit 6), unreachable-subject
  Invalid (exit 8), tampered-corpus Invalid (exit 8), manifest 1-pass/4-reject —
  all pass, stable + MSRV.
- `eggstack` descriptor test extended for the `command` compatibility — pass.
- Full matrix: `cargo fmt --check`, workspace `check`, `clippy --all-features
  -D warnings`, workspace `test --all-targets --all-features` — pass;
  `cargo +1.89.0 check` + core/runner/cli tests — pass; `git diff --check` — pass.
- Live harness (local, debug binary): Stage A 5/5 PASS; Stage B
  NOT-EXECUTED with the pinned SHA recorded; exit 0.

## Schema/migration/compatibility evidence

Profile v1, corpus v1, plan v8, expansion `eggbench.security-profile-expansion.v1`,
correctness `eggbench.security-correctness.v2`, ComparisonReceipt v4,
qualification receipt v1 — no schema change. The `eggfetch-http`
descriptor widens one compatibility set (`command` added); no existing
plan changes meaning (oha/h2load sets were already empty = universal).

## Security and lifecycle evidence

All listeners loopback; no public target/DNS/credentials; managed
lifecycle with delay readiness, 3s grace shutdown, bounded logs;
correctness Fail never becomes `WorkloadFailed` (Invalid > Fail
precedence preserved); transport failure maps to Invalid, never to a
correctness verdict.

## Documentation/operational evidence

Profile README, upstream-manifest boundary doc, baselines workflow doc
(M002b), `docs/synvoid-qualification.md` guide (M002b), live script with
PASS/STOPPED/NOT-EXECUTED verdicts and trap cleanup (children, work
dir, delay sidecars).

## Known limitations

- D1: named-adapter ephemeral ports are unconsumable by static managed-command
  configs (no runner interpolation); stand-in emulates the adapter contract;
  generic interpolation is an M003 candidate.
- D2: corpus/provenance are Eggbench-authored synthetic fixtures, not the
  SynVoid-owned export.
- D3: Detect expects 403; the SynVoid site-level `action = "block"` wire
  mapping is unverified against a real binary.

## Unresolved findings

- High: upstream asset contract open — full closure blocked until
  `dbowm91/synvoid:plans/eggbench_security_qualification_asset_contract.md` closes.
- High: live reverse-proxy Pass/negative proof and hosted four-lane + live-job
  runs pending.
- Medium: real-binary 403 wire status and dot-segment normalization behavior
  unverified (recorded live risks).

## Conditions for full close

1. Upstream contract closes with a proof-bearing source SHA.
2. Harness Stage B executes green (positive Pass + negative Fail + cleanup).
3. Hosted four-lane CI plus the `live-synvoid-linux` job are green on the
   closing commit; D3 verified against the wire.

Compilation alone was never treated as evidence: every claim above names
the test, receipt, or harness line that proves it.
