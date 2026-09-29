# Security Qualification M002 Post-Conditional-Closure Corrective Addendum

Status: closed (C001 STOPPED with verification at `b98973f` per plan §10, preserved as historical; SynVoid asset contract CLOSED upstream 2026-09-28; C002 closed on exact-head hosted four-lane run `36640125422` and live run `36640125453` after the M003d CI corrective landed at `4703333`)

Baseline: `660ab472ebf214e1378da777b1f06412decc008a`

Historical M002 implementation/closure:

- routine implementation: `b74f8617b4cdc2599330f01dea1fa8bac496c966`;
- M002a conditional closure:
  `plans/closure/security-qualification/002a-status.md`;
- M002b umbrella conditional closure:
  `plans/closure/security-qualification/002b-status.md`.

Trigger evidence:

- hosted CI run `36262615058`:
  - Linux stable: pass;
  - Linux Rust 1.89: pass;
  - Windows stable: pass;
  - macOS stable: fail in
    `cargo test --workspace --all-targets --all-features --locked`;
- live workflow `36262614991`: green, including
  `live-synvoid-linux`, but the closure record truthfully reports the
  real-SynVoid Stage B as NOT-EXECUTED because the owner-side asset contract
  was open at that time;
- SynVoid owner-side contract:
  `dbowm91/synvoid:plans/eggbench_security_qualification_asset_contract.md`
  — **CLOSED** 2026-09-28 (implementation `ae045481`, closeout head
  `30e68af8`); C002 consumes it.

## Purpose

The conditional M002 closure is preserved as historical evidence of routine,
synthetic qualification. It is not equivalent to final M002 qualification.

This corrective closes two outstanding proof gaps:

1. restore exact-head four-lane hosted CI on macOS without weakening M002;
2. consume the closed SynVoid-owned export and execute real reverse-proxy
   correctness/performance proof before converting M002 to full closure.

## Corrective sequence

1. **C001 — macOS hosted portability restoration**
   `plans/implementation/security-qualification-m002-corrective/001-macos-hosted-portability-restoration.md`
   → closure `plans/closure/security-qualification-m002-corrective/001-status.md`
   — **STOPPED with verification** at implementation `b98973f`
   (`fix(qualification): restore macOS hosted portability via tcp-loopback
   readiness probe`). Local workspace all-feature tests + MSRV green;
   hosted CI run `36451730637` shows the C001 runner-side fix worked
   (`synvoid_m002a` 7/7 green). C001 was STOP-with-verification recorded
   on plan §10 because the first correction surfaced an independent live-host
   repeatability failure on `synvoid_m002b::perf_same_source_pair_never_fails`
   (Apple Silicon cold/warm-cache asymmetry on the same-source
   `statistical_relative` gate) that the test source itself documents as
   "M002 closure condition" and that belongs to C002.
2. **C002 — real SynVoid qualification and final M002 closure**
   `plans/implementation/security-qualification-m002-corrective/002-real-synvoid-live-qualification-and-final-closure.md`
   → status `plans/closure/security-qualification-m002-corrective/002-status.md`
   — **closed**. SynVoid asset contract closed upstream 2026-09-28
   (implementation `ae045481`, policy `synvoid.eggbench-qualification.v1`);
   the real-live implementation landed (harness-only import layer +
   `live-synvoid-linux` Stage B/C-real wiring, `synvoid_m002c` contract
   tests) with local + in-harness proof green — Stage B 15/15 positive,
   negative Fail, origin-log and teardown proofs; Stage C-real smoke
   Pass, same-source Pass/Inconclusive never Fail, oha/h2load green;
   full harness 29/0/1. The M003d CI portability corrective
   (`plans/implementation/security-qualification/003d-corrective-eggsec-load-ci-portability-and-unblock.md`,
   closure `plans/closure/security-qualification/003d-corrective-status.md`)
   landed at `4703333`, repairing the stale production
   workload/descriptor count and the Windows cfg-only `unused_mut` that had made
   exact-head four-lane run `36607614262` red. C002 then discharged its
   closure gates on that repairing source: four-lane run `36640125422`
   (all lanes green, including the `macos-stable` same-source lane) and live
   run `36640125453` (`live-synvoid-linux` pass=25 stopped=0 notexec=5 with
   real owner-exported stages executed). C001's STOPPED disposition remains
   historical and is explicitly adjudicated by C002 §8 rather than rewritten.

## Invariants

- do not rewrite the historical conditional closure records;
- do not claim the green live workflow proved real SynVoid while Stage B was
  NOT-EXECUTED;
- do not remove macOS from the required normal CI matrix;
- do not skip/ignore a failing macOS test merely to recover green CI;
- do not translate SynVoid Detect/Pass semantics inside Eggbench;
- do not weaken M001 corpus restrictions;
- no public target or internet dependency;
- correctness and performance remain separate verdict families;
- no SynVoid Rust dependency enters Eggbench.

## Final closure gate

M002 became fully closed on 2026-09-29 when all of the following held on
proof-bearing Eggbench SHA `4703333`:

- C001's historical `STOPPED with verification` disposition is preserved and
  its runner-side fix remains verified; C002 explicitly adjudicates the
  live-host condition surfaced by C001 rather than falsely rewriting C001 as
  closed;
- SynVoid owner-side asset contract is closed with a proof-bearing source SHA;
- real SynVoid positive correctness proof passes;
- deterministic negative correctness proof fails as expected;
- real benign performance/oracle scenarios execute;
- exact-head normal four-lane CI is green (run `36640125422`);
- exact-head live qualification including real SynVoid is green (run
  `36640125453`);
- the roadmap/registry are reconciled from conditional to closed.

M003c terminal telemetry qualification and M003d profile work no longer carry
an M002 dependency. Full M003d/M003 closure must still satisfy the original
M003d acceptance gates; a fully qualified M002 no longer blocks them.
