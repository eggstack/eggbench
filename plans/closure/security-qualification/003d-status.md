# Security Qualification M003d — Status

Disposition: **PARTIAL IMPLEMENTATION; CI corrective closed**. The Eggsec-load slice landed at `739f382`; the two portability/integration defects it introduced were repaired at `4703333` and the repository-wide qualification gate is green again (four-lane run `36640125422`, live run `36640125453`). M003d remains open; this is not an M003d or M003 milestone closure.

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

## Immediate corrective (closed)

Plan:

`plans/implementation/security-qualification/003d-corrective-eggsec-load-ci-portability-and-unblock.md`

It is limited to:

- correcting stale production workload/descriptor accounting after
  `eggsec-load` registration;
- preserving Unix `0700`/`0600` scope-file hardening without Windows
  `unused_mut`;
- restoring green four-lane CI;
- revalidating exact-head live evidence so M002 C002 can close.

## Reconciliation update — 2026-09-29 (corrective closed)

The portability/CI repair is complete. Closure record:
`plans/closure/security-qualification/003d-corrective-status.md`.

Implementation:
`470333370e14c6a2ac31047f3f6836ce756f9a2a`
(`fix(security): repair Eggsec-load registration accounting and CI
portability`).

- `workload_registry::tests::production_runtime_reports_no_driver` now asserts
  the exact expected workload-name list and full descriptor-name list for the
  compiled feature matrix, with counts derived from those hand-maintained
  lists. `eggsec-load` is explicitly required in both.
- `create_scope_file` now builds its directory/file options through cfg-specific
  `scope_dir_builder`/`scope_file_options` helpers that are genuinely
  non-mutable off Unix. Unix mode `0700`/`0600`, `create_new(true)`, collision
  retry, and drop cleanup are unchanged and are covered by a new
  `scope_file_is_private_exclusive_and_removed_on_drop` test.
- Hosted evidence on the exact corrective head: normal CI run `36640125422`
  (linux-stable, linux-msrv, macos-stable, windows-stable all green) and live
  qualification run `36640125453` (all four live jobs green, with
  `live-synvoid-linux` executing the real SynVoid stages at pass=25
  stopped=0 notexec=5).
- M002 corrective C002 is unblocked and terminally reconciled; M003c's
  green-hosted-source closure precondition is satisfiable.

No Eggsec-load behavioral or security contract was weakened. M003d itself is
still open: the profile family, negative demonstrations, bounded real-SynVoid
M003 qualification, and `plans/closure/security-qualification/003-status.md`
remain outstanding.

## Remaining M003d gates

After that corrective, the original plan still requires a checked-in SynVoid
M003 profile family, repeatability-accepted baselines,
correctness-only/performance-only/telemetry-only negative demonstrations, the
bounded real SynVoid Linux stages, and terminal C002 closure/reconciliation.
The corrective is now closed and M002 C002 is closed; what remains is
M003d's own profile and qualification work plus terminal M003c proof.

The prior upstream telemetry blocker is closed: SynVoid's
`synvoid.eggbench-telemetry.v2` implementation is qualified at
`739e7ba6f02c5e3f83fe9ff5321b09213182b193` with terminal closeout on
current head `0dc1f7fb21a5df60e72fc7f2cd60b7cb73bc9f35`. Eggbench M003c
is now closing on its consumer-side live/current-head proof.

Terminal M003d is now gated only on terminal M003c; M002 C002 is closed. The
narrow CI corrective that previously blocked C002's repo-wide closure
criterion is closed and is no longer a blocker for M003d.
