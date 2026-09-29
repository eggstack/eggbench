# Security Qualification M003c — Status

Disposition: **CLOSING** — generic collector implementation landed and the former SynVoid owner-contract blocker is closed under `synvoid.eggbench-telemetry.v2`. The green-hosted-source closure precondition is satisfied by runs `36640125422` / `36640125453` on `4703333`. This is not terminal M003c closure; Eggbench-owned live/current-head evidence remains.

Implementation commit: `2742e0e` (`feat(security): add generic Prometheus subject telemetry`).

## Implemented

- Added the feature-gated `prometheus-http` telemetry source with workspace-confined, digest-pinned mapping files, exact metric/label selection, and private/loopback endpoint validation.
- Added bounded scalar exposition parsing, trial-window polling, gauge aggregation, counter deltas/reset detection, required/optional field behavior, cancellation/drain handling, normalized `subject_*` observations, and bounded provenance evidence.
- Added CLI source construction from a declared target service binding and metric mapping, plus a core resolve test for dynamic mapped descriptor outputs.
- Documented the source and the separation between `subject_*` and Gregg `host_*` metrics.

## Verification

- `cargo fmt --all -- --check`: passed.
- `cargo check --workspace --all-targets --all-features --locked`: passed.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`: passed.
- `cargo test -p eggbench-core --all-features --locked`: passed (176 tests across three suites).
- `cargo test -p eggbench-drivers --all-features --locked`: passed (163 tests across nine suites).
- `cargo test -p eggbench-cli --all-features --locked`: passed (88 tests across eight suites, including the existing `synvoid_m002b` integration suite).
- No hosted four-lane run or live SynVoid M003 qualification has been performed for this commit.

## Reconciliation update — 2026-09-29

The prior upstream-blocked finding is superseded.

SynVoid closed the corrected owner handoff:

- owner contract: `synvoid.eggbench-telemetry.v2`;
- proof-bearing implementation:
  `739e7ba6f02c5e3f83fe9ff5321b09213182b193`;
- current closeout head observed:
  `0dc1f7fb21a5df60e72fc7f2cd60b7cb73bc9f35`;
- terminal authority:
  `architecture/eggbench_security_qualification_m003_telemetry_corrective_closeout.md`;
- mapping SHA-256:
  `622f6a13c4353cc7465cce39a57ed86fa0db2fe4114258e6f06226c1748d2d99`.

The SynVoid closeout exercised Eggbench's real parser/validator and
`PrometheusHttpCollector` against the exact v2 mapping and a live minimal
SynVoid endpoint. This satisfies the upstream interface dependency. It does not
replace Eggbench's consumer-side terminal evidence.

## Remaining closure gates

M003c now requires:

1. materialize the closed SynVoid v2 assets on an exact pinned revision;
2. run a real minimal SynVoid subject through Eggbench's normal runner;
3. execute the M003b pooled/pressure/body scenario set with subject telemetry
   active during measured windows;
4. prove required gauge observation and required counter delta through the
   normal collector path;
5. prove missing/name/TYPE drift fails closed and optional absence is not
   fabricated;
6. prove polling/task drain on trial/runner teardown;
7. retain exact mapping/contract/binary/scenario/collector identities;
8. close on a source revision whose normal hosted CI is green.

Gate 8's precondition is now satisfied. The M003d CI portability corrective
closed at `4703333` (`plans/closure/security-qualification/003d-corrective-status.md`),
and the exact-head hosted evidence on that source is green: four-lane normal CI
run `36640125422` and live qualification run `36640125453`. M003c terminal
closure must cite that evidence or a later green descendant; gates 1–7 remain
M003c's own work.

M002 corrective C002 is also closed, so M003c no longer carries an M002
dependency.

M003d remains gated for overall M003 closure by terminal M003c proof. Its
remaining profile, baseline, live, negative-demonstration, and closure work is
otherwise unblocked.
