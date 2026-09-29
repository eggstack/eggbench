# Security Qualification M003d — Status

Disposition: partial implementation landed; M003d remains blocked and open. This is not an M003d or M003 milestone closure.

Implementation: `739f382` (`feat(security): add Eggsec strict-scope load workload`).

## Implemented slice

- Registered `eggsec-load` as an explicit non-default external workload driver using the trusted Eggsec executable resolver and version probe.
- Restricted execution to one repeated case in a digest-pinned `http_corpus` schedule, with bounded request count/concurrency/body/header inputs.
- Generated a private temporary strict-scope manifest, ran Eggsec's guarded `load-test` preflight, and checked the preflight operation, target authority, scope source, confirmation set, and override state before load traffic.
- Parsed bounded Eggsec load JSON against its current report shape and exact target URL; normalized throughput, p95 latency, transport-error rate, and expected status mismatch rate.
- Retained bounded raw JSON and method/status evidence including executable and scope digests, Eggsec version, and the fact that Eggsec's load path uses Eggfetch and is not an independent transport oracle.
- Added tests for bounded report parsing, strict request argv, and separation of expected HTTP status mismatch from transport errors.

## Verification

- `cargo clippy -p eggbench-drivers -p eggbench-cli --all-targets --all-features --locked -- -D warnings`: passed.
- `cargo test -p eggbench-drivers --all-features --locked`: passed (166 tests across nine suites).
- `cargo test -p eggbench-cli --all-features --locked`: passed earlier in this turn (88 tests across eight suites); it predates the `eggsec-load` inventory addition. The CLI and driver targets compile with all features and strict clippy after the addition.
- No Eggsec executable live load, full four-lane CI, or live SynVoid M003 qualification has been run for this slice.

## Remaining M003d gates

The plan still requires a checked-in SynVoid M003 profile family, repeatability-accepted baselines, correctness-only/performance-only/telemetry-only negative demonstrations, the bounded real SynVoid Linux stages, and terminal C002 closure/reconciliation. Those rely on a closed SynVoid M003 telemetry contract and completion of M002 C002's real subject and live-host repeatability evidence. The upstream telemetry plan remains `READY`, not closed, and the M002 C002 record still identifies C001's non-closed STOPPED disposition as a hard prerequisite. Therefore no future terminal M003 plan is unblocked by this adapter slice.
