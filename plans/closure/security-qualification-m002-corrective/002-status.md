# Security Qualification M002 Corrective C002 — Execution Status (BLOCKED)

Status: **BLOCKED** — execution attempted at Eggbench `b52e2a5`; all three
hard prerequisites fail; no production semantic change made; no closure
claimed.

Plan:

- `plans/implementation/security-qualification-m002-corrective/002-real-synvoid-live-qualification-and-final-closure.md`

Corrective authority:

- `plans/subsystems/security-qualification-m002-post-conditional-closure-corrective-addendum.md`

## 1. Disposition

**BLOCKED, per plan §14 and planning-process §4 (hard dependencies).**
C002 was executed as far as the repository boundary permits:

- §2 re-audit recorded (section 2);
- Stage B gate verified to fail closed with NOT-EXECUTED (section 3);
- synthetic contract tests verified green locally (section 4);
- same-source repeatability verified green on this Linux host (section 5).

Real SynVoid proof (§4 Stage B, §5 Stage C on the real subject, §8
proof-bearing `live-synvoid-linux`, §9 exact-head qualification, §11
reconciliation, §12 terminal closure) is not executable until the
prerequisites close. This record claims no M002 closure.

## 2. Re-audit at handoff (§2)

| Item | Value |
|---|---|
| Eggbench HEAD | `b52e2a5399754cd3da3e4818ae0038b9f0c8b4df` (`b52e2a5`, `main`, tree clean) |
| C001 implementation SHA | `b98973f` (`fix(qualification): restore macOS hosted portability via tcp-loopback readiness probe`) |
| C001 closure | `plans/closure/security-qualification-m002-corrective/001-status.md`, disposition **STOPPED with verification** (not closed) |
| SynVoid asset-contract closure SHA | ABSENT — `plans/eggbench_security_qualification_asset_contract.md` does not exist in the observed SynVoid checkout |
| SynVoid source SHA (observed local checkout `/home/sugarwookie/projects/synvoid`) | `7f1b79452a683e758e0b4ea1e70f6c0f2463f0d1` (matches harness `SYNVOID_PIN` default) |
| SynVoid package version | `1.1.0` (`synvoid/Cargo.toml:3`) |
| SynVoid `Cargo.lock` SHA-256 (observed) | `2b1daf913a3f3fff…` (full digest in local checkout; NOT a proof-bearing contract pin — recorded only as observation) |
| Materializer / export policy identifier | ABSENT — no closed contract names one |
| Exported corpus/config/provenance schema versions | No owner export exists. Expected import boundary remains frozen at `qualification/synvoid/v1/upstream-manifest.md` (policy `synvoid-qualification-assets.v1`, corpus schema 1, plan schema 8, profile schema v1). The checked-in `materialized/provenance.json` is explicitly synthetic (`policy_version: 0-synthetic`, `materializer: eggbench-m002a-synthetic-fixture/0`, zeroed `cargo_lock_sha256`). |
| Minimal SynVoid binary SHA-256 | NOT AVAILABLE — no canonical minimal build is named by a closed contract; none was built during this execution (building SynVoid is outside the Eggbench boundary and no contract defines the canonical feature profile to hash) |

Planning baselines (`49b4624b…` observed 2026-09-26, `7f1b7945…` routine
scope) were used only to locate the checkout, not as live proof.

## 3. Hard-prerequisite check (§9 items 1–3, §13 criteria 1–2)

| Prerequisite | Result |
|---|---|
| 1. C001 closed (`plans/closure/security-qualification-m002-corrective/001-status.md`) | FAIL — disposition is STOPPED with verification; `synvoid_m002a` verified green on hosted run `36451730637` but `synvoid_m002b::perf_same_source_pair_never_fails` exposes Apple Silicon live-host repeatability owned by C002 |
| 2. SynVoid owner-side plan closed (`dbowm91/synvoid:plans/eggbench_security_qualification_asset_contract.md`) | FAIL — file absent at the pinned SHA `7f1b79452a683e758e0b4ea1e70f6c0f2463f0d1` (verified `ls` exit 2 on the local checkout at that exact SHA) |
| 3. SynVoid closeout with proof-bearing SHA + live reverse-proxy semantic proof | FAIL — no closeout exists to record one |

Per plan §10, no Eggbench production semantic change was made to
compensate: Eggbench must not parse SynVoid internal fixture format,
derive Detect/Pass mapping itself, or normalize excluded cases into a
weaker form.

## 4. Stage B gate verification (§8)

The harness gate at
`scripts/qualification/synvoid-m002/run-live-qualification.sh:227-233`
checks for the contract file at the pinned revision and reports
NOT-EXECUTED when absent. Verified against the local checkout at the
exact pinned SHA: contract absent → Stage B is NOT-EXECUTED by
construction. The `live-synvoid-linux` job therefore remains green-in-form
without claiming live qualification — the truthful pre-C002 posture.
Per §8, it MUST be flipped to fail on NOT-EXECUTED only when the real
sequence lands; that flip was deliberately NOT made here (it would turn
CI permanently red with no contract to satisfy it).

## 5. Synthetic verification evidence (§7, §13 criterion 10)

All on `--locked`, Eggbench HEAD `b52e2a5`, tree clean:

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | clean |
| `git diff --check` | clean |
| `cargo test -p eggbench-cli --test synvoid_m002a --all-features --locked` | 7 / 7 passed in ~20 s (correctness family incl. `slow_synvoid_startup_still_passes`) |
| `cargo test -p eggbench-runner --test lifecycle --all-features --locked tcp_loopback` | 2 / 2 passed (probe waits-for-port + rejects non-loopback) |
| `cargo test -p eggbench-cli --test synvoid_m002b --all-features --locked perf_same_source_pair_never_fails` | 1 / 1 passed in ~107 s on this Linux host (aggregate Pass, never Fail) |

The last row is the C002 §5 baseline datum on this host: the frozen v1
policy (7 measured trials, 200/800/2000 requests at c1/c8/c32) holds
Pass locally while macOS-14 hosted run `36451730637` trips the same-source
`statistical_relative` gate. That divergence is the live-host
repeatability gap C002 §5/§14 owns. No threshold, policy, or gate was
widened during this execution (README §8 evidence and plan §14 forbid
silent weakening; a policy revision with evidence belongs to M003 if the
real-subject proof requires it).

## 6. Acceptance criteria (§13)

| # | Criterion | Status |
|---|---|---|
| 1 | C001 closed | FAIL (STOPPED with verification) |
| 2 | SynVoid asset contract closed | FAIL (absent) |
| 3 | Real owner export consumed without reinterpretation | NOT-EXECUTED (no export) |
| 4 | Real minimal binary built + config-tested | NOT-EXECUTED |
| 5 | Real Pass/Detect correctness green | NOT-EXECUTED |
| 6 | Negative mutation yields Fail on real subject | NOT-EXECUTED |
| 7 | Real benign native performance path executes | NOT-EXECUTED |
| 8 | Real independent-oracle path executes | NOT-EXECUTED |
| 9 | Same-source repeatability, no false Fail (real subject) | NOT-EXECUTED (synthetic same-source holds Pass on this Linux host; macOS synthetic gap recorded as C002-owned) |
| 10 | Synthetic cross-platform contract tests green | PASS locally (m002a 7/7, probe 2/2); hosted macOS m002a 7/7 green per run `36451730637`; hosted macOS m002b 5/6 (perf gap) |
| 11 | Exact-head four-lane normal CI green | FAIL (macOS m002b perf lane red on `36451730637`) |
| 12 | Exact-head live workflow green | NOT-PROOF-BEARING (`live-synvoid-linux` green-in-form with Stage B NOT-EXECUTED) |
| 13 | `live-synvoid-linux` executed real stages | FAIL (NOT-EXECUTED by design until contract lands) |
| 14 | Closure/roadmap/registry reconciliation committed | NOT APPLICABLE (nothing to close) |

## 7. Stop statement (§14)

Stop with upstream corrective required — the closed SynVoid export
violates nothing (it does not exist), the real subject cannot run under
any lifecycle (nothing to run), and no Eggbench production defect was
exposed (nothing in §10's defect list triggered because no real execution
was possible). No C003 is authored here: there is no Eggbench production
defect to correct, only absent upstream assets. The unblock sequence is:

1. SynVoid closes `plans/eggbench_security_qualification_asset_contract.md` with proof-bearing SHA + semantic proof;
2. C001's STOPPED disposition resolves (live-host repeatability gap addressed under C002 §5/§14 against the real subject);
3. C002 re-executes §§3–9 against the closed export.

## 8. Production change record (§10)

None. Zero files under `crates/`, `qualification/`, or `scripts/`
modified. This execution touched planning artifacts only (this status
record + registry/addendum pointers).
