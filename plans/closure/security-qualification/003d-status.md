# Security Qualification M003d — Status

Disposition: **CORRECTIVE REQUIRED / PARTIAL IMPLEMENTATION**. The Eggsec-load slice landed at `739f382`, but exact-head hosted CI exposed two M003d-owned portability/integration defects. M003d remains open; this is not an M003d or M003 milestone closure.

Implementation: `739f382` (`feat(security): add Eggsec strict-scope load workload`).

## Implemented slice

- Registered `eggsec-load` as an explicit non-default external workload driver using the trusted Eggsec executable resolver and version probe.
- Restricted execution to one repeated case in a digest-pinned `http_corpus` schedule, with bounded request count/concurrency/body/header inputs.
- Generated a private temporary strict-scope manifest, ran Eggsec's guarded `load-test` preflight, and checked the preflight operation, target authority, scope source, confirmation set, and override state before load traffic.
- Parsed bounded Eggsec load JSON against its current report shape and exact target URL; normalized throughput, p95 latency, transport-error rate, and expected status mismatch rate.
- Retained bounded raw JSON and method/status evidence including executable and scope digests, Eggsec version, and the fact that Eggsec's load path uses Eggfetch and is not an independent transport oracle.
- Added tests for bounded report parsing, strict request argv, and separation of expected HTTP status mismatch from transport errors.

## Verification

Initial local evidence:

- `cargo clippy -p eggbench-drivers -p eggbench-cli --all-targets --all-features --locked -- -D warnings`: passed on the implementation host;
- `cargo test -p eggbench-drivers --all-features --locked`: passed (166 tests across nine suites);
- `cargo test -p eggbench-cli --all-features --locked`: passed before the final `eggsec-load` inventory addition and therefore was not sufficient proof of the finished slice.

Stronger hosted evidence later found the portability defect:

- exact-head normal CI run `36607614262`:
  - `linux-msrv` green;
  - `linux-stable` red on
    `workload_registry::tests::production_runtime_reports_no_driver`;
  - `macos-stable` red on the same stale workload/descriptor count;
  - `windows-stable` red on `unused_mut` in
    `crates/eggbench-drivers/src/external/eggsec_load.rs` where mutation is
    Unix-only;
- live qualification run `36607614200` was green for the then-current C002
  real SynVoid stages, demonstrating that the current blocker is normal
  cross-platform CI rather than the M002 live subject path.

The initial local pass remains historical evidence but is not terminal
cross-platform qualification.

## Immediate corrective

Ready plan:

`plans/implementation/security-qualification/003d-corrective-eggsec-load-ci-portability-and-unblock.md`.

It is limited to:

- correcting stale production workload/descriptor accounting after
  `eggsec-load` registration;
- preserving Unix `0700`/`0600` scope-file hardening without Windows
  `unused_mut`;
- restoring green four-lane CI;
- revalidating exact-head live evidence so M002 C002 can close.

## Remaining M003d gates

After that corrective, the original plan still requires a checked-in SynVoid
M003 profile family, repeatability-accepted baselines,
correctness-only/performance-only/telemetry-only negative demonstrations, the
bounded real SynVoid Linux stages, and terminal C002 closure/reconciliation.

The prior upstream telemetry blocker is closed: SynVoid's
`synvoid.eggbench-telemetry.v2` implementation is qualified at
`739e7ba6f02c5e3f83fe9ff5321b09213182b193` with terminal closeout on
current head `0dc1f7fb21a5df60e72fc7f2cd60b7cb73bc9f35`. Eggbench M003c
is now closing on its consumer-side live/current-head proof.

Terminal M003d remains gated on terminal M003c plus M002 C002. The narrow CI
corrective itself is not gated by either and should execute first because it
currently blocks C002's repo-wide closure criterion.
