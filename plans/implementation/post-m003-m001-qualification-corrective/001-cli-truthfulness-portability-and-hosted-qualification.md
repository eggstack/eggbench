# Post-M003/M001 Qualification Corrective C001 — CLI Truthfulness, Portability, and Hosted Qualification

Status: closed (implementation `e924f2a` + `a5f8260`; closure `plans/closure/post-m003-m001-qualification-corrective/001-status.md`; hosted qualification CI run `35808371805`, four lanes green; re-verified at current HEAD 2026-09-25)

Repository baseline: `c73c00a5530d1e0b188528152f68ef855e516135`

Source corrective:

- `plans/subsystems/post-m003-m001-qualification-corrective-addendum.md`

Historical predecessor plans/closures:

- `plans/implementation/local-runner-lifecycle/003-environment-fingerprint-and-cli-lifecycle.md`
- `plans/closure/local-runner-lifecycle/003-status.md`
- `plans/implementation/measurement-comparison/001-metric-vocabulary-and-trial-normalization.md`
- `plans/closure/measurement-comparison/001-status.md`

Controlling references:

- `plans/003-planning-process.md#8-corrective-passes`
- `plans/000-long-term-specification.md#17-library-and-cli-surface`
- `plans/000-long-term-specification.md#19-portability-and-toolchain`
- `plans/adrs/ADR-0001-typed-core-and-driver-boundaries.md`
- `plans/adrs/ADR-0002-immutable-evidence-and-testbed-provenance.md`

Primary class: invariant/correctness + portability corrective.

## 1. Objective

Close the remaining correctness and qualification gaps in the M003 + Measurement M001 round without expanding product scope.

The corrective must:

1. remove the production executable `fake-load` path while retaining explicit test injection;
2. make binary exit status exactly match the documented CLI result class in JSON and human modes;
3. route SIGINT/Ctrl-C into the existing M002 `CancellationToken`;
4. fix the exact macOS and Windows environment-collector portability failures from CI run `35803742746`;
5. rerun the full hosted matrix and update closure/registry status only after all required lanes are green.

## 2. Current evidence

At baseline:

- `WorkloadRegistry::with_builtin()` registers `fake-load` as default;
- `commands/run.rs` constructs `FakeWorkload` and executes it in the production code path;
- comments claim the fake is qualification-only, but the type/control flow does not enforce that;
- `CliEnvelope` contains `ok/result/error/warnings` but does not carry process-exit metadata;
- `ExitCode` already defines:
  - 0 success;
  - 1 internal;
  - 2 parse/validation;
  - 3 capability/preflight;
  - 4 finalized run with non-success execution status;
  - 5 evidence/bundle failure;
- `main.rs::present()`:
  - returns without a nonzero status in JSON mode;
  - exits code 3 for all human-mode failed envelopes;
- `commands/run.rs` emits `CliEnvelope::ok` for every finalized `RunOutcome`, including `Failed|Cancelled|Invalid`;
- no signal listener cancels the run token;
- macOS hosted failure is two missing-trait `usize::from_str` compile errors plus a cfg-specific unused parameter;
- Windows hosted failure is cfg-leaked dead code plus `clippy::unnecessary_wraps` in `os_version_label`;
- Linux stable and Linux Rust 1.89 already pass at the same HEAD.

## 3. Required invariants

1. Production registry state contains no synthetic workload driver unless explicitly supplied by a non-production/test runtime.
2. `eggbench run` with no production workload adapter fails before managed startup.
3. Qualification tests may inject `FakeWorkload` without adding a public production flag.
4. JSON stdout remains exactly one CLI envelope document.
5. Exit status is independent of presentation mode: JSON and human mode return the same numeric status for the same command outcome.
6. Finalized non-success runs retain bundle path and run summary while returning code 4.
7. Evidence/bundle failures return code 5.
8. Parse/validation and capability/preflight retain codes 2 and 3.
9. Ctrl-C only requests cancellation; existing M002 drain/teardown remains authoritative.
10. Signal-listener cleanup does not leave detached background tasks after command completion.
11. Platform-specific environment helpers compile only where they are used.
12. Optional unavailable facts remain absent, not fabricated as `unknown`.
13. No broad lint suppression is used to hide target-specific dead code.
14. Metric normalization and schemas remain unchanged.

## 4. Non-goals

Do not:

- implement a real workload adapter;
- expose `--fake-workload`, `--qualification`, or another public synthetic-run switch;
- change CLI envelope schema v1 solely to route process exit status;
- add a general plugin registry;
- alter M002 lifecycle semantics;
- add second-signal hard termination;
- implement Windows Job Objects;
- change Measurement M001 types or vocabulary;
- change manifest v2 or trial schemas.

## 5. Work package A — Separate production and qualification runtimes

Refactor the CLI execution boundary so production command dispatch does not construct test support directly.

Preferred shape:

~~~text
CommandRuntime / WorkloadRuntime
  driver_descriptors()
  workload_executor(...)
  reset_registry(...)
~~~

Production runtime:

- returns only real compiled/registered production adapters;
- at this milestone that set is empty;
- therefore `doctor` reports no production workload adapter;
- `run` returns a stable capability failure before startup.

Qualification/test runtime:

- registers the deterministic fake descriptor;
- creates `FakeWorkload`;
- can be injected into command functions/library tests;
- is not used by `main.rs`;
- does not appear in production help or default driver inventory.

A smaller injection seam is acceptable if it provides the same invariant.

### Static guard

Add a test/guard proving the production runtime inventory does not contain `fake-load` and production `run` cannot reach `BuiltinWorkloadExecutor`.

If practical, cfg-gate CLI fake adapter wrappers under `#[cfg(test)]` or place them in test support so a normal release binary does not need to link them.

Do not remove runner-level `test_support::FakeWorkload`; it remains useful for runner/measurement tests.

## 6. Work package B — Production unsupported-driver behavior

With an empty production workload registry:

- `validate` remains unaffected;
- `doctor` should complete truthfully and report `has_workload_driver=false` or an equivalent stable capability field;
- `run` must fail with stable category `unsupported_workload` or `missing_driver`;
- failure occurs before:
  - managed startup;
  - workload invocation;
  - final bundle publication.

Prefer failing before expensive environment/bundle preparation if resolution already proves no workload adapter exists.

Add a binary-level regression proving no service process is started when production run lacks a driver.

## 7. Work package C — Internal command result carries exit status

Do not force process-exit semantics into the JSON envelope schema.

Introduce an internal result type equivalent to:

~~~text
PresentedCommandResult {
  envelope: CliEnvelope,
  exit_code: ExitCode
}
~~~

or return `(CliEnvelope, ExitCode)` from dispatch.

Requirements:

- envelope remains the machine compatibility surface;
- exit code remains process metadata;
- presentation receives both and never guesses code from `ok` alone;
- direct `CliError` conversion also yields the same structure.

Refactor `present()` to write output and return `StdExitCode` (or internal `ExitCode`) instead of calling `std::process::exit`.

`main()` should be the only place that returns the final process status.

This makes binary behavior testable and prevents control-flow divergence between JSON/human modes.

## 8. Work package D — Finalized non-success run representation

A run may finalize valid evidence with:

- `ExecutionStatus::Failed`;
- `ExecutionStatus::Cancelled`;
- `ExecutionStatus::Invalid`.

Those cases must:

- retain the `CliOutput::Run` result including bundle path;
- return exit code 4;
- not be described as an ordinary success.

Preferred envelope semantics:

~~~text
ok = false
result = Some(CliOutput::Run { ... })
error = Some({
  category: "run_non_success",
  detail: ...
})
~~~

This is representable by the existing envelope schema because `result` and `error` are independently optional.

Add a constructor/helper rather than hand-building inconsistent envelopes.

`ExecutionStatus::Completed` returns code 0.

Do not map comparison verdicts in this corrective; Measurement M002 will own comparison semantics.

## 9. Work package E — Exit-code matrix

Lock this exact binary behavior unless implementation evidence requires a documented narrow revision:

| Outcome | Exit |
|---|---:|
| successful validate/doctor/run/inspect | 0 |
| internal/presentation serialization failure | 1 |
| plan I/O/format/schema/semantic validation | 2 |
| missing/unsupported driver, platform, preflight, subject digest mismatch | 3 |
| finalized run status Failed/Cancelled/Invalid | 4 |
| evidence staging/finalization/open/verify failure | 5 |

Requirements:

- same code in `--json` and human modes;
- JSON failure output is still exactly one document;
- human diagnostics remain on stderr;
- quiet suppresses optional prose but never changes exit status;
- code 4 result still includes the finalized bundle.

## 10. Work package F — Binary-level exit-code tests

Add subprocess-level tests against the actual `eggbench` binary, not only enum/unit tests.

At minimum:

- invalid plan -> 2;
- production run with no adapter -> 3;
- synthetic injected qualification harness yielding failed finalized run -> 4;
- invalid/corrupted bundle inspect -> 5;
- successful validate -> 0;
- internal serialization/path presentation error -> 1 if a deterministic safe test seam exists.

For code 4, if the production binary correctly cannot execute a fake, use a test-only harness binary or injected command-runtime integration test that exercises the same presentation/main return path without exposing a production flag.

JSON and human modes must each be covered for codes 2/3/4/5.

## 11. Work package G — SIGINT/Ctrl-C cancellation wiring

Create the cancellation token before M002 execution and connect one OS Ctrl-C signal to it.

Preferred implementation:

- use `tokio::signal::ctrl_c()`, already available through the workspace Tokio `signal` feature;
- run signal observation concurrently with the active experiment;
- on signal:
  - call `CancellationToken::cancel()`;
  - allow `execute_run` to perform normal cancellation/drain/teardown;
- after run completion:
  - cancel/abort/join the listener so no detached task remains.

Do not call `process::exit` from the signal handler.

Do not bypass M002 with direct child killing.

### Testability seam

Factor signal waiting behind a tiny future/factory or helper so tests can deterministically trigger cancellation without depending only on wall-clock OS delivery.

Required regression:

- a long-running fake measured invocation starts;
- cancellation signal/future fires;
- `execute_run` returns a finalized `Cancelled` bundle;
- drain runs;
- managed teardown runs;
- CLI result retains bundle;
- exit code is 4.

Where practical on Unix/macOS CI, add one true subprocess SIGINT test against a dedicated test harness. Do not make the suite depend exclusively on platform-fragile timing.

## 12. Work package H — Remove masked platform fallback

Current `commands/run.rs` does:

~~~text
Name::new(platform_label).unwrap_or_else(|_| Name::new("unknown").unwrap())
~~~

Replace this with typed error propagation.

A future invalid platform label must be visible as an internal/capability defect, not silently converted to `unknown`.

This is separate from optional environment-field absence.

## 13. Work package I — macOS environment collector compile fix

Fix CI run `35803742746` exactly.

At `environment.rs:362` and `:444`, prefer:

~~~text
raw.trim().parse::<usize>()
~~~

or a cfg-scoped `FromStr` import.

Avoid globally importing traits solely to satisfy one target if direct `parse` is clearer.

For `read_cpuinfo_field(key)`:

- split target-specific helpers or cfg the parameter use so macOS does not produce an unused-variable warning;
- do not add a broad `#[allow(unused_variables)]`.

Then run macOS check/clippy/test so the next failure is not merely hidden behind the original compile error.

## 14. Work package J — Windows environment collector cfg hygiene

Fix the exact Windows Clippy failures:

- `cpuinfo_logical_count` should compile only on targets that call it;
- `parse_cpu_range_count` should compile only where needed, with tests cfg-aligned;
- `read_trimmed` should compile only where used;
- other Linux/macOS-only helpers should be audited for the same issue.

For `os_version_label()`:

- preserve an `Option<String>` only if Windows can truthfully return `None`;
- remove `unwrap_or_else(|| "unknown")`;
- if Windows version/release is unavailable, omit the field rather than synthesizing `windows unknown`.

This aligns with M003's absence-not-placeholder contract and should naturally remove `clippy::unnecessary_wraps`.

Do not silence these with target-wide lint allows.

## 15. Work package K — Platform collector regression tests

Add/adjust tests so cfg-specific failures are caught before hosted CI:

- compile tests for platform helper exposure where possible;
- unit tests for parsers should carry the same cfg as their production helper;
- Windows path verifies missing optional version data can remain absent;
- macOS helper signatures compile without unused parameters;
- all-target Clippy remains `-D warnings`.

If cross-compilation is practical in CI or local scripts without installing heavy SDKs, add a lightweight static check; otherwise hosted lanes remain authoritative.

## 16. Work package L — Measurement M001 qualification disposition

Do not modify metric vocabulary or normalization code unless a platform failure directly points there.

The corrective closure must record that Measurement M001:

- remained locally green;
- was historically marked closed before hosted CI;
- receives its missing hosted qualification from the corrected combined HEAD;
- becomes fully closed only after the four-lane CI pass.

If platform corrections alter only M003 environment/CLI code, say so explicitly.

## 17. CI requirements

Fresh hosted run after corrective implementation:

### Linux stable

- fmt;
- workspace check;
- all-target/all-feature Clippy `-D warnings`;
- full workspace tests;
- binary exit-code tests;
- injected cancellation test.

### Linux Rust 1.89

- workspace all-target check;
- core tests including metrics;
- any new core-compatible tests.

### macOS stable

- workspace check;
- all-target/all-feature Clippy;
- full workspace tests;
- process-group cleanup;
- filesystem confinement;
- environment collector;
- cancellation qualification.

### Windows stable

- workspace check;
- all-target/all-feature Clippy;
- core metrics tests;
- CLI validate/doctor/inspect binary tests;
- environment collector tests;
- explicit managed-run unsupported behavior;
- no requirement to qualify managed-process cancellation because Windows managed execution remains unsupported.

Closure requires all four jobs green on the same corrective HEAD.

## 18. Broad local verification

Required:

    cargo fmt --all -- --check
    cargo check --workspace --all-targets --locked
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
    EGGBENCH_PARENT_SENTINEL_26CE=set-in-parent-process cargo test --workspace --all-features --locked
    cargo +1.89.0 check --workspace --all-targets --locked
    cargo +1.89.0 test -p eggbench-core --all-features --locked
    cargo tree --locked
    git diff --check

Also run focused CLI subprocess tests in both JSON and human presentation modes.

## 19. Documentation updates

Update:

- `docs/cli.md`:
  - production has no workload adapter yet;
  - exact exit-code semantics;
  - code 4 preserves bundle;
  - Ctrl-C requests graceful cancellation;
- README:
  - `run` is substrate-only until External Oracles/Eggstack driver lands;
- `docs/environment-fingerprint.md`:
  - optional missing values are omitted;
  - no `unknown` placeholder;
- closure/roadmap/registry documents.

Do not advertise fake-load as a user feature.

## 20. Acceptance criteria

C001 closes only when:

1. production `WorkloadRegistry` contains no fake driver;
2. production `eggbench run` fails before startup when no real adapter is compiled;
3. deterministic fake workload remains injectable in tests;
4. JSON failures return nonzero process codes correctly;
5. human and JSON modes return identical codes for the same outcome class;
6. valid finalized Failed/Cancelled/Invalid runs return code 4 and retain bundle result data;
7. evidence/verify failures return code 5;
8. Ctrl-C cancels the M002 token rather than directly terminating child processes;
9. cancellation produces truthful finalized evidence and mandatory cleanup;
10. the platform-label `unknown` fallback is removed;
11. macOS `cargo check`/Clippy/tests pass;
12. Windows Clippy is clean without broad suppressions;
13. optional platform facts remain absent instead of fabricated;
14. Linux stable and Rust 1.89 remain green;
15. Measurement M001 schemas/semantics remain unchanged;
16. fresh hosted CI is green on all four required jobs.

## 21. Stop conditions

Stop for planning review if:

- removing fake-load requires introducing a real production driver;
- exit-code correctness requires an incompatible CLI envelope schema change;
- signal handling requires bypassing M002 cleanup;
- Windows correctness requires implementing managed process ownership;
- platform collector repair requires unsafe Rust inside Eggbench;
- fixing hosted CI exposes a Measurement M001 semantic/schema defect;
- manifest/trial/metric schema changes become necessary.

## 22. Closure evidence required

Record:

- corrective implementation commits;
- production driver inventory before/after;
- proof production fake path is unreachable;
- command-result/presentation API shape;
- binary exit-code matrix with JSON/human subprocess evidence;
- representative code-4 envelope retaining bundle;
- SIGINT/cancellation trace proving drain + teardown;
- macOS CI failure root cause and landed fix;
- Windows CI failure root cause and landed cfg fix;
- environment absence behavior;
- full local test counts;
- dependency tree/MSRV;
- fresh hosted CI run ID and four job conclusions;
- Measurement M001 qualification disposition;
- Local Runner M003 final disposition;
- unresolved findings by severity.
