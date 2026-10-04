# Security Qualification M003d — Status

Disposition: **closed — M003d scope complete and verified, re-verified with the
corrected owner `1338ce7b` in the same terminal harness run
(`pass=30 stopped=0 notexec=0` locally; `pass=29 stopped=0 notexec=1` hosted, the
one `NOT-EXECUTED` being `m003d-4` on a runner without `oha`)**. The Eggsec-load slice landed at `739f382`, its two portability/integration defects were repaired at `4703333` (four-lane run `36640125422`, live run `36640125453`), and the M003 profile/qualification work plus three further real-Eggsec adapter defects are closed below. The inherited M003c gate is resolved and the exact-head hosted evidence is recorded: four-lane CI run `37143714313` and live run `37143714261`, both green on closing revision `30a3825` (see `003c-status.md`).

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

## M003d profile and qualification work — closed

Implementation: the M003d slice at `739f382`, its CI portability corrective at
`4703333`, and the M003 profile/qualification work on this branch.

### Adapter defects found and repaired against the real Eggsec

The live harness executed the adapter against the real pinned Eggsec
(`0509ac668adfd78e9899cd3428a807d0b3c9f27b`, `eggsec 0.1.0`) and found three
contract defects that no unit test could have found, because the tests asserted
the adapter's own intent rather than the tool's interface:

1. **Wrong flag.** The adapter passed `--header`; Eggsec accepts `--headers`
   (`Vec<String>`, format `Key:Value`). Fixed; the strict-argv test now asserts
   repeated `--headers` and rejects the old spelling.
2. **Missing execution policy.** Eggsec denies `load-test` under *every*
   enforcement profile (`manual`, `ci`, `mcp`, `agent`, `guarded`) with
   `risk-policy-denied` until an execution policy authorizes it. The adapter now
   generates a private policy manifest that enables load testing only, keeps
   `require_explicit_scope = true`, and denies intrusive fuzzing, stress testing,
   raw packets, credential testing, and remote execution. Both the scope and the
   policy digest are recorded as security-configuration identity in
   `eggsec-load-method.json`; both files live in one `0700` directory as `0600`
   files and are removed on drop.
3. **`--quiet` suppresses the report.** With `--quiet` Eggsec prints only its
   log lines and no machine document, so report parsing could never succeed. The
   flag is gone, with a comment recording why it must not return.
4. **Preflight document extraction.** Eggsec writes structured log lines to
   stdout before its JSON document. The load report already tolerated that; the
   preflight parser required the whole stream to be one JSON value and so failed
   with `ParseFailed`. Both now share one `machine_document` extractor that takes
   the last standalone document and rejects a truncated one, with a unit test for
   each shape.

### Implemented

- Live real-SynVoid M003 profile family: correctness, `body-pooled`,
  `body-fresh`, `body-gated`, `mixed-80-20-pooled`, `mixed-80-20-fresh`,
  `mixed-80-20-pooled-c32`, origin-only control, and the telemetry-pressure and
  optional-absence trials, materialized from the owner export.
- Explicit baseline bundles for every performance scenario; no baseline is
  auto-discovered, and the perf profile validates only once its baselines exist.
- One owner instance for the whole live qualification, with the subject and its
  controlled origin declared as external services in the plans. A per-scenario
  managed subject contended for the owner metrics port, and a child that lost
  that race published no series at all, so the telemetry evidence described a
  different process than the subject under test; that defect also made an
  unrelated correctness-regression suite report `Invalid`.
- Fail-closed preconditions discovered by the harness itself: the harness
  verifies its Eggbench binary advertises `eggfetch-http`, `eggserve-origin`,
  `eggsec-load`, `oha`, and `prometheus-http` (a default-features build fails
  closed here instead of later with an opaque "missing required Service
  driver"), and it records the actual ports/pids it owns.

### Verification

Live local run (`pass=29 stopped=1 notexec=0`, the single stop being M003c's
upstream gate):

- `m003d-1` eight accepted-revision baseline bundles materialized and the perf
  profile validates against them.
- `m003d-2` same-source performance pair: `pass`.
- `m003d-3` the controlled origin logged 24,594 requests and **no** owner-blocked
  case reached it (checked against the corpus, not just a non-empty log).
- `m003d-4` `oha` body scenario compared against its own baseline.
- `m003d-5` Eggsec build and version probe at the pin.
- `m003d-6` `eggsec-load` executes a benign body case and an owner-blocked case
  under strict scope with the generated policy: expected status outcomes, no
  transport errors.
- `m003d-7` correctness-only regression (a mutated expected status) fails the
  suite with exit 6 while performance stays acceptable.
- `m003d-8` performance-only regression (a 100 ms controlled-origin throttle)
  fails the suite with exit 6, `body-gated-c8` performance `fail`, and the
  correctness scenario `pass`.
- `m003d-9` telemetry-only regression is covered by `m003c-16`/`m003c-17`.
- Teardown leaves no listener and reaps the owner process tree.

The frozen performance claim rests on one gated scenario, `body-gated-c8`
(statistical-relative throughput and p95 latency); every other performance
observation is diagnostic. The gated scenario is measured more often than the
diagnostic ones so that a bootstrap interval can distinguish a real regression
from host noise.

### Hosted evidence on the pushed head

Live qualification run `36989900894` (`live-m003-linux`, `2c25bfd`) executed the
whole M003 stage group against the real pinned SynVoid and Eggsec binaries on a
dedicated hosted runner: `pass=28 stopped=1 notexec=1`. Every M003d stage passed
there — `m003d-1` materialized 8 baseline bundles and validated the perf
profile, `m003d-2` returned `pass` for the nine-trial gated scenario, `m003d-3`
proved 30,994 logged requests with no owner-blocked case reaching the origin,
`m003d-6` ran real `eggsec-load` under strict scope (`eggsec 0.1.0`, sha256
`12a3843df00eb5dfa77850cc495dec2f6b4f22b67c31db9d0a3d9df1b916f253`), and
`m003d-7`/`m003d-8`/`m003d-9` plus teardown all passed. The single stop was
`m003c-13b`, the upstream telemetry gate; `m003d-4` was not executed because
`oha` is absent from that runner.

Reaching that result took two fixture repairs that only the hosted runner
exposed, both recorded in `plans/closure/security-qualification/003-status.md`:
the routine campaigns are sized for the evidence they must carry, and the
stand-in listen backlogs are sized for the declared fresh-per-request rate. The
live performance regression additionally needed its origin backlog raised for the
same reason — with it, `m003d-8` passes; without it the throttled run lost
evidence in an unrelated scenario and the suite came back `Invalid`.

The pinned SynVoid binary's hosted digest was
`sha256 07766ca02e129c9abe1c1b32e978e6de0cb710ed6e916b6cb5ae77bd615f8b1f`.

### Known limitation

The unthrottled perf pair can return `Inconclusive` on a heavily contended
shared host rather than `Pass`; the harness accepts 0 or 7 for that stage and
requires the correctness family to pass. On the two-core hosted runner the M002a
same-source pair returned `Invalid` (insufficient evidence is never softened to
`Inconclusive`), which is the same load-induced failure class and reproduces on
pristine `1b79f62`. Hosted runs on dedicated runners are the repeatability
authority.

## Remaining condition

M003d's own work is complete. It was re-verified with the corrected owner in
the terminal local harness run (`pass=30 stopped=0 notexec=0` on owner
`1338ce7b`) and again on hosted runners, where `m003d-1`, `m003d-2`, `m003d-3`,
`m003d-5`, `m003d-6`, `m003d-7`, `m003d-8`, and `m003d-9` all pass in
`live-m003-linux` run `37143714261` on closing revision `30a3825`. The four-lane
CI run `37143714313` is green on the same revision. `m003d-4` is `NOT-EXECUTED`
because `oha` is not installed on the hosted runner, which is an external-tool
availability condition and not a verdict; it remains executed in the local
harness. Nothing in M003d is outstanding on the Eggbench side.

## Addendum — 2026-10-04 (m003d-4 executes hosted; assertion from evidence)

The `NOT-EXECUTED` above was an external-tool availability condition, and it
is now removed rather than re-explained. `live-tools.yml` provisions `oha`
from a pinned release asset (`v1.16.0`, digest-verified, fail-closed) via
`scripts/qualification/provision-external-oracles.sh`, so `m003d-4` executes
on the hosted runners instead of reporting `NOT-EXECUTED` there.

The stage's claim is also tightened at the same time. The previous wording
said "base + candidate compared", but `body-oha-c8` declares only absolute
correctness gates (`expected_outcome_mismatch_rate`,
`transport_error_rate`, both 0) with throughput/latency diagnostic, so there
was no relative comparison to make. The harness now asserts each arm from its
run JSON and bundle via `scripts/qualification/assert-oracle-run.py`
(completed, both absolute gates observed at zero in every measured trial,
observations producer-attributed to `oha`) and records the oracle version and
binary digest in the verdict. Run and proof JSONs are retained past `$WORK`.
Source: `973451f`. Hosted confirmation is pending the in-flight live runs.
