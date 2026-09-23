# Local Runner M003 + Measurement M001 — Post-Implementation Qualification Corrective Addendum

Status: active

Repository audit baseline: `6c9a3906874618978e68a1f92e49c25f23b0ab1c`

Historical implementation/closure records:

- `plans/implementation/local-runner-lifecycle/003-environment-fingerprint-and-cli-lifecycle.md`
- `plans/closure/local-runner-lifecycle/003-status.md`
- `plans/implementation/measurement-comparison/001-metric-vocabulary-and-trial-normalization.md`
- `plans/closure/measurement-comparison/001-status.md`

Controlling references:

- `plans/003-planning-process.md#8-corrective-passes`
- `plans/000-long-term-specification.md#13-environment-and-testbed-model`
- `plans/000-long-term-specification.md#17-library-and-cli-surface`
- `plans/000-long-term-specification.md#19-portability-and-toolchain`
- `plans/000-long-term-specification.md#21-compatibility-and-versioning`
- `plans/adrs/ADR-0001-typed-core-and-driver-boundaries.md`
- `plans/adrs/ADR-0002-immutable-evidence-and-testbed-provenance.md`

## 1. Corrective trigger

The M003 and Measurement M001 implementation round landed, but post-implementation audit and hosted qualification found four classes of work that prevent clean closure of the round:

1. **The production CLI currently executes the qualification fake workload.**
   `WorkloadRegistry::with_builtin()` registers `fake-load` as the default workload descriptor and `commands/run.rs` constructs `FakeWorkload` through `BuiltinWorkloadExecutor`. This contradicts the M003 plan, which required the production binary to fail explicitly when no real workload adapter exists and limited fake execution to injected/test-only qualification plumbing.

2. **Binary exit-code routing is incorrect.**
   `ExitCode` defines codes 0/1/2/3/4/5, but:
   - JSON failure envelopes return from `present()` and `main()` subsequently returns success, making JSON failures exit 0;
   - human-mode failed envelopes exit 3 regardless of the intended failure class;
   - a finalized run with `ExecutionStatus::Failed|Cancelled|Invalid` is currently emitted as an `ok:true` envelope and does not produce code 4;
   - evidence/bundle failures represented as envelopes cannot reliably produce code 5.

3. **SIGINT/Ctrl-C is not connected to the M002 cancellation token.**
   `commands/run.rs` creates a `CancellationToken` but no OS signal listener cancels it, so the documented single-signal cancellation behavior is not delivered.

4. **Hosted CI at current HEAD is red.**
   CI run `35803742746`:
   - Linux stable — pass;
   - Linux Rust 1.89 MSRV — pass;
   - macOS stable — fail at `cargo check`;
   - Windows stable — fail at Clippy.

   Exact macOS failures:
   - `environment.rs:362`: `usize::from_str` used without `FromStr` in scope;
   - `environment.rs:444`: same;
   - cfg-specific unused `key` parameter in `read_cpuinfo_field`.

   Exact Windows Clippy failures:
   - Linux-only helpers `cpuinfo_logical_count`, `parse_cpu_range_count`, and `read_trimmed` compile as dead code;
   - `os_version_label() -> Option<String>` is unnecessarily wrapped on Windows because that cfg branch always returns `Some`, currently using an `unknown` fallback.

The hosted failure also means Measurement M001's historical closure does not satisfy its planned hosted-qualification acceptance criterion even though its local correctness evidence is strong.

## 2. Work classification

Primary class: invariant/correctness + portability corrective.

The work is intentionally one corrective because the failed hosted matrix is testing the combined M003 + Measurement M001 repository state and the remaining user-visible defects are confined to M003's CLI/environment boundary.

## 3. Invariants

- Production `eggbench` must not silently execute a synthetic benchmark driver.
- Qualification fakes remain available to tests through explicit injection, not production default registration.
- Machine output and process exit status tell the same truth.
- A valid finalized failed/cancelled/invalid run is preserved and surfaced, not discarded.
- Ctrl-C requests existing M002 cancellation; it must not bypass drain/teardown.
- Environment collection is cfg-correct and warning-free on all supported CI targets.
- Missing optional environment facts remain absent rather than becoming fabricated `unknown` values.
- Linux/macOS/Windows support claims require hosted evidence.
- Measurement M001 metric schemas and normalization semantics must not be reopened by this corrective.
- Manifest v2, TrialExecutionResult v1, TrialMetrics v1, and CLI envelope schema v1 remain unchanged unless a stop condition is reached.

## 4. Non-goals

Do not:

- add a real Eggfetch/oha/h2load/iperf3 workload adapter;
- implement Measurement M002 statistical comparison;
- change metric vocabulary/normalization semantics;
- add Windows managed process ownership;
- add a second force-kill signal path;
- add a hidden production fake-workload CLI flag;
- loosen Clippy with broad `allow` attributes instead of fixing cfg ownership;
- alter M002 measurement timing or cleanup semantics;
- rewrite historical closure records.

## 5. Corrective milestone

### C001 — CLI truthfulness, platform qualification, and production-fake separation

Implementation plan:

- `plans/implementation/post-m003-m001-qualification-corrective/001-cli-truthfulness-portability-and-hosted-qualification.md`

Status: ready.

Closing C001:

- fully closes Local Runner M003;
- supplies the missing hosted qualification for Measurement M001;
- re-enables Measurement M002 plan authoring/implementation;
- re-enables Eggstack Integration M001 planning/implementation against a truthful CLI/runner substrate;
- permits External Oracles implementation to introduce the first real workload adapter without competing with a production fake.

## 6. Dependency disposition

Until C001 closes:

- Local Runner M003 remains **conditionally closed**;
- Measurement M001 is treated as **conditionally closed for qualification purposes**, while its landed schema/API contracts remain stable;
- Measurement M002 implementation is blocked;
- Eggstack Integrations M001 implementation is blocked;
- External Oracles plan authoring may continue, but production integration should not land against the incorrect default workload registry.

## 7. Completion definition

This corrective closes when:

- the production binary has no executable fake workload path by default;
- tests can still inject deterministic fake workloads;
- exit codes 0/1/2/3/4/5 are observable and locked at binary level in both JSON and human modes;
- finalized non-success runs preserve bundle/result data while exiting 4;
- SIGINT reaches M002 cancellation and mandatory cleanup;
- macOS and Windows platform compilation/lints are clean without broad suppressions;
- a fresh four-lane hosted CI run is green;
- closure/registry records accurately reflect the resulting state.
