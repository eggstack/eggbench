# Security Qualification M002 Corrective C002 — Execution Status (CLOSING)

Status: **CLOSING** — implementation landed; real SynVoid proof green
locally and end-to-end in the live harness; exact-head hosted
qualification (four-lane normal CI + `live-synvoid-linux` with real
stages) pending the next push. No M002 closure claimed yet.

Plan:

- `plans/implementation/security-qualification-m002-corrective/002-real-synvoid-live-qualification-and-final-closure.md`

Corrective authority:

- `plans/subsystems/security-qualification-m002-post-conditional-closure-corrective-addendum.md`

Supersedes: the earlier BLOCKED revision of this record (prerequisites
1–3 have since closed on the SynVoid side; see section 3).

## 1. Disposition

**CLOSING, per registry vocabulary (implementation landed, closure
evidence being gathered).** C002 executed end-to-end:

- §2 re-audit recorded with real values (section 2);
- owner export consumed mechanically, never reinterpreted (section 4);
- Stage B real correctness proof green: positive Pass 15/15, negative
  mutation Fails closed, blocked requests provably never reach the
  origin, teardown leaves nothing behind (section 5);
- Stage C real performance path green: smoke Pass, same-source
  repeatability Pass/Inconclusive never Fail, oha + h2load oracles
  execute (section 6);
- live harness Stage B/C-real wired proof-bearing; full harness run
  29 PASS / 0 STOPPED locally (section 7);
- fast deterministic import-layer tests added (`synvoid_m002c`, 3/3);
- no production semantic change (section 9).

Remaining for terminal closure (§13 criteria 1, 11–14): exact-head
hosted runs on the implementation SHA plus the C001-disposition
reconciliation recorded in section 8.

## 2. Re-audit at handoff (§2)

| Item | Value |
|---|---|
| Eggbench implementation HEAD | (this commit; tree clean; see §9 for the file list) |
| C001 implementation SHA | `b98973f` (`fix(qualification): restore macOS hosted portability via tcp-loopback readiness probe`) |
| C001 closure | `plans/closure/security-qualification-m002-corrective/001-status.md`, disposition **STOPPED with verification** (runner-side scope verified green on hosted run `36451730637`; live-host gap owned by C002 — see section 8) |
| SynVoid asset-contract closure | **CLOSED** 2026-09-28: `dbowm91/synvoid:plans/eggbench_security_qualification_asset_contract.md` at `origin/main`; implementation `ae045481752b8f750d6e6079b185c526a09c91d5`; closeout `architecture/eggbench_security_qualification_asset_contract_closeout.md` with live reverse-proxy proof 15/15 + 2/2 perf paths |
| SynVoid source SHA (consumed) | `ae045481752b8f750d6e6079b185c526a09c91d5` (harness `SYNVOID_PIN`) |
| SynVoid package version | `1.1.0` |
| SynVoid `Cargo.lock` SHA-256 (at consumed SHA) | `426c6124dc450b9e54b16d3698f36911047f56f1f1f9fe0564e064ff57a06451` (matches closeout prefix `426c6124…`) |
| Materializer / export policy identifier | `synvoid-eggbench-qualification-materializer@1.0.0` / `synvoid.eggbench-qualification.v1` (provenance-asserted by the harness) |
| Exported corpus/config/provenance schema versions | `eggbench.security_qualification.corpus.v1` / `eggbench.security_qualification.config.v1` / `eggbench.security_qualification.provenance.v1`; 15 cases, 12 exclusions; `detect_status` 403, `pass_status` 200 |
| Minimal SynVoid binary SHA-256 | release `--locked --no-default-features` (harness CI-context build): `23f3df64e27d46e80f88ffa9815e549df9649007e0299b60dd083e8ccdf3ea8a`; local debug build: `af2f4791ede976b2529a0d6915523fae3e61569682f7ce3718da5b2bf18a160a` (profiles differ by env; both recorded) |

## 3. Hard-prerequisite check (§9 items 1–3, §13 criteria 1–2)

| Prerequisite | Result |
|---|---|
| 1. C001 closed | EXCEPTION — disposition is STOPPED with verification, not closed (see section 8 for the recorded rationale; no silent rewrite) |
| 2. SynVoid owner-side plan closed | PASS — closed 2026-09-28, implementation `ae045481…`, closeout with live proof |
| 3. SynVoid closeout with proof-bearing SHA + live reverse-proxy semantic proof | PASS — 15/15 corpus + 2/2 perf paths on pinned minimal binary |

Per plan §10, no Eggbench production semantic change compensates for
anything: Eggbench does not parse SynVoid internal fixture format (it
consumes the owner-normalized export), does not derive Detect/Pass
mapping (per-case `expected_status` must equal the owner-declared wire
status), and does not normalize excluded cases into weaker forms.

## 4. Asset import boundary (§3)

Consumed only the owner export. Deltas from the frozen
`qualification/synvoid/v1/upstream-manifest.md` expectation, all
owner-driven and harness-contained:

- `policy_id` is `synvoid.eggbench-qualification.v1` (not the pre-close
  guess `synvoid-qualification-assets.v1`); manifest doc updated; the
  harness pins and asserts equality on both corpus and provenance;
- owner `CorpusCase{method, path, query_string, headers, body, expected_status}`
  is translated mechanically to Eggbench `HttpSecurityCaseV1` by
  `scripts/qualification/synvoid-m002/translate-owner-corpus.py`
  (pinned policy; `expected_status` ∈ {owner 403, owner 200};
  detect → `status_any_of[403]`, pass → `status_exact 200`;
  raw SP → `%20` per the owner transport rule; file bodies fail closed);
- `provenance.json` carries per-fixture paths + SHA-256, verbatim
  exclusions (12), config/corpus/site digests, ports, mapping, and
  materializer version; it carries no `cargo_lock_sha256` aggregate —
  recorded here instead (section 2) via `git show <pin>:Cargo.lock`;
- the five owner pass paths plus `/qualbench/small` + `/qualbench/stream`
  are served by the harness `controlled-origin.py` (200 + deterministic
  bodies, 501 elsewhere, per-request log): the single-route
  `eggserve-origin` adapter cannot serve five distinct pass paths, so
  the harness origin is used for the real proof only (README D2
  records this); routine synthetic runs keep the real adapter;
- no Eggbench request header is injected: `HttpCorpusExecutor` sends
  per-case headers only and eggfetch sets no default UA, so the owner
  bot-stage constraint (neutral/non-bot UA) holds without any driver
  change — verified by the 15/15 live agreement itself.

## 5. Harness Stage B — real correctness proof (§4)

Local + in-harness evidence (release Eggbench binary; real minimal
binary; fresh free ports per run):

- owner export materialized (15 cases), `check` + `--configtest` green;
- provenance pins asserted (policy, SHA, 1.1.0, 403/200, 15/12);
- `qualify validate` + positive `qualify run`: **RC 0, aggregate Pass**;
- per-case evidence (`security/synvoid-waf.json`): **15/15 Pass**
  (5 × 200 reach origin; 10 × 403 blocked);
- origin-log proof (`assert-origin-log.py`): 5 pass paths served 200,
  10 detect targets absent — blocked requests never reached the origin;
- negative proof (one expectation mutated to 404 + identity recomputed):
  **RC 6, aggregate Fail** — a security-expectation mismatch is never
  reclassified as workload/process failure;
- teardown proof: both ports free, no `synvoid --foreground` or
  `controlled-origin.py` child survives.

## 6. Harness Stage C — real performance path (§5)

Same work dir / export / ports as Stage B:

- real smoke profile (correctness + small/large c1 proxy + absolute
  gates): **Pass 3/3**;
- 8 explicit same-source baseline bundles materialized;
- real perf profile (correctness + small/large × c1/c8/c32 + 2
  direct-origin controls, frozen v1 trial/metric/gate policy):
  **RC 7, aggregate inconclusive** — 7 Pass + 2 inconclusive
  (large-c8/c32), zero Fail across 9 scenarios: the §5 same-source
  repeatability proof (Pass/Inconclusive, never Fail) on the real
  subject on this Linux host;
- oha base/candidate/compare procedure: green;
- h2load run procedure: green.

## 7. Live-harness wiring (§8) and full-run evidence

`scripts/qualification/synvoid-m002/run-live-qualification.sh` Stage B
stub replaced with the proof-bearing Stage B + Stage C-real sequence
(§8 flip applied: clone/checkout/contract/build/export/check/provenance
failures are STOPPED, never NOT-EXECUTED; NOT-EXECUTED remains only for
absent optional tools). New harness files (all harness-only, §10):

- `translate-owner-corpus.py`, `controlled-origin.py`,
  `build-real-waf-workspace.py`, `assert-origin-log.py`,
  `generate-real-scenarios.py`.

Full local harness run: **pass=29 stopped=0 notexec=1** (the single
NOT-EXECUTED is the optional Gregg-daemon probe, D5). `SYNVOID_PIN`
default is now `ae045481…`; `POLICY_ID` pins
`synvoid.eggbench-qualification.v1`.

## 8. C001-disposition reconciliation (§13 criterion 1)

Criterion 1 (`C001 is closed`) cannot be marked PASS without rewriting
history: C001's record stands at STOPPED with verification per its §10.
The exception is recorded here explicitly instead:

- C001's narrow runner-side scope (managed-command readiness race) is
  verified green: macOS hosted run `36451730637` shows `synvoid_m002a`
  7/7 including the new routine regression;
- the surfacing failure (`synvoid_m002b::perf_same_source_pair_never_fails`
  on Apple Silicon) is the live-host repeatability condition the test
  source and the corrective architecture both assign to C002;
- C002 discharges it on the real subject (§6: same-source
  Pass/Inconclusive, never Fail, frozen policy, no threshold widened)
  locally; the hosted four-lane verdict on the implementation SHA
  (criterion 11) is the remaining signal and is awaited, not assumed.

## 9. Production change record (§10)

Harness, test, and documentation artifacts only:

- `scripts/qualification/synvoid-m002/`: 5 new helper scripts (all `+x`),
  `run-live-qualification.sh` Stage B/C-real wiring + pin updates;
- `crates/eggbench-cli/tests/synvoid_m002c_real_import.rs`: 3 fast
  deterministic import-layer tests (translation rules, fail-closed
  rejections, origin-log proof rule);
- `qualification/synvoid/v1/upstream-manifest.md` + `README.md`:
  contract-closure facts, real-proof origin notes (D2 resolved as
  documented).

Zero files under `crates/*/src`, zero scenario/profile/corpus fixture
changes, zero threshold/policy/gate changes. No M002 correctness rule,
threshold, or schema weakened. `cargo fmt --check`, `cargo check`,
`cargo clippy --all-targets --all-features -- -D warnings`,
`cargo +1.89.0 check`, and `git diff --check` are clean; `synvoid_m002a`
7/7, `lifecycle` 30/30 (with the CI sentinel env), `binary_exit_codes`
10/10, `synvoid_m002c` 3/3.

## 10. Remaining for terminal closure (§13 criteria 1, 11–14)

1. Push the implementation SHA; record the exact-head four-lane normal
   CI run (criterion 11) — the macOS `synvoid_m002b` synthetic lane is
   the known risk and must be read, not assumed;
2. record the exact-head live workflow incl. `live-synvoid-linux` with
   real stages executed (criteria 12–13);
3. reconcile roadmap/registry from conditional to closed (criterion 14)
   only after 1–2 are green.
