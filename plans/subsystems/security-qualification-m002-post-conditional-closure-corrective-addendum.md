# Security Qualification M002 Post-Conditional-Closure Corrective Addendum

Status: active

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
  remains open;
- SynVoid owner-side contract:
  `dbowm91/synvoid:plans/eggbench_security_qualification_asset_contract.md`,
  now ready for implementation.

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
   — ready.
2. **C002 — real SynVoid qualification and final M002 closure**
   `plans/implementation/security-qualification-m002-corrective/002-real-synvoid-live-qualification-and-final-closure.md`
   — blocked on C001 closure and SynVoid asset-contract closure.

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

M002 becomes fully closed only when all of the following hold on a
proof-bearing Eggbench SHA:

- C001 macOS corrective closed;
- SynVoid owner-side asset contract closed with a proof-bearing source SHA;
- real SynVoid positive correctness proof passes;
- deterministic negative correctness proof fails as expected;
- real benign performance/oracle scenarios execute;
- exact-head normal four-lane CI is green;
- exact-head live qualification including real SynVoid is green;
- the roadmap/registry are reconciled from conditional to closed.

M003 research may proceed in parallel, but M003 implementation should not use
"fully qualified M002" as a premise until this corrective closes.
