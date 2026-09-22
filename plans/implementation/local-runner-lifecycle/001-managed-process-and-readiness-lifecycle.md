# Local Runner M001 — Managed Process and Readiness Lifecycle

Status: closed

Repository baseline: `6a803128f1e715b26e7408d3295e6cc4a09d9839`

Source roadmap:

- `plans/subsystems/local-runner-lifecycle-roadmap.md` — M001

Hard dependencies:

- Foundation M001 — closed; typed `ExperimentPlan` v1 and validation.
- Foundation M002 — closed; `ResolvedPlan` v1 and driver provenance.
- Foundation M003 — closed; immutable bundle staging/finalization and inspection.

Controlling ADRs:

- `plans/adrs/ADR-0001-typed-core-and-driver-boundaries.md`
- `plans/adrs/ADR-0002-immutable-evidence-and-testbed-provenance.md`
- `plans/adrs/ADR-0005-local-first-execution-and-remote-provider-boundary.md`

Primary class: capability.

## 1. Objective

Implement runtime-owned local command/service startup, readiness, bounded output collection, graceful shutdown, forced cleanup, and reverse dependency teardown. Expose a library session that M002's phase orchestrator can consume. Build no load generator, trial scheduler, measurement clock, comparison engine, or CLI in this milestone.

M001 proves lifecycle behavior using child-process fixtures and fake readiness probes. It does not claim a benchmark result. A successful lifecycle-only run has no measured trials and is therefore inconclusive if it is finalized as evidence.

## 2. Current implementation evidence

- The workspace has `eggbench-core` only; it owns validated `ExperimentPlan`, `ResolvedPlan`, driver descriptors, and `.eggb` evidence types.
- `ResolvedPlan` retains subject command argv, managed/external service declarations, dependencies, readiness/shutdown requests, timeouts, secret references, and selected driver provenance.
- M003 provides bounded artifact streaming and a manifest requiring source plan, resolved plan, subject identity, and a versioned environment fingerprint.
- No crate currently spawns processes, owns descendants, checks readiness, or streams child output.
- The local-runner roadmap requires Linux process-group ownership, equivalent macOS behavior, and explicit Windows capability limits when descendant cleanup is unavailable.

## 3. Required invariants

- `eggbench-core` remains free of process, Tokio, and concrete network ownership.
- Spawn argv directly; never insert a shell by default.
- Start only managed command requests; never claim ownership of external services.
- Spawn dependencies before dependents and stop in reverse dependency order.
- Readiness and startup are outside any future measured interval.
- Every failure or cancellation after the first successful spawn attempts teardown.
- Preserve the initiating failure when cleanup also fails; report cleanup errors separately.
- Managed descendants are not intentionally orphaned on a platform advertised as supported.
- Process IDs are diagnostics only, never durable subject/service/trial IDs.
- Child stdout/stderr are drained continuously and spooled with explicit byte caps; exceeding a cap truncates retained output but does not stop pipe draining.
- Missing secret references, unsupported service kinds, unsupported readiness probes, and unavailable process-tree guarantees fail explicitly.

## 4. Explicit non-goals

Do not:

- execute workload drivers or generate benchmark traffic;
- schedule warmup/measured/cooldown phases or compute durations/statistics;
- integrate Eggstack binaries or HTTP/TLS clients;
- add a database, daemon, dashboard, or CLI;
- introduce remote execution, SSH, container requirements, or security test semantics;
- store secret values in plan, resolved-plan, manifest, logs, or diagnostics;
- implement generic workflow/DAG execution beyond the already validated service dependency graph.

## 5. Expected production changes

- Add `crates/eggbench-runner` as a runtime/process-owning crate depending inward on `eggbench-core`.
- Add a narrow local session API for `ResolvedPlan`: prepare process specs, spawn managed command subjects/services, report readiness, expose bounded log artifacts/diagnostics, and shut down/drain the owned process tree.
- Keep OS-specific process ownership behind a small platform adapter. Advertise only platforms where descendant cleanup is tested; return a structured unsupported-capability error elsewhere.
- Keep readiness probe dispatch in the runner. Core's named probe request is declarative; runner must reject unknown probe names. Include a deterministic process-alive probe and a fake probe for tests; do not infer HTTP/TCP probe parameters from opaque strings.
- Resolve relative working directories against an explicit caller-provided workspace root. Resolve secret references through an injected provider before spawn. Do not print resolved secret values.
- Stage diagnostic logs and lifecycle metadata through the M003 bundle writer when a run bundle is requested. Do not add a second evidence format.
- Add a small child-fixture executable/test helper to exercise real spawn, output, failure, readiness timeout, signal handling, and descendant cleanup.

## 6. Lifecycle and failure semantics

1. Revalidate the supplied plan/resolved-plan boundary and preflight all required driver, platform, argv, path, and secret-reference inputs before spawning.
2. Start managed services in dependency order, retaining an owned handle per service. External services are observed but never stopped.
3. Apply readiness requests within their declared timeout. Named but unregistered probes fail with an explicit capability error.
4. On spawn failure, readiness failure/timeout, caller cancellation, or explicit shutdown, stop already-started services in reverse dependency order.
5. Request graceful shutdown using the declared service shutdown policy, wait only for the declared grace period, then force-kill the process group/job and descendants on supported platforms.
6. Drain/close output streams and preserve bounded stdout/stderr artifacts. Continue cleanup after one service fails to stop.
7. Return the primary lifecycle outcome and a list of cleanup failures. Do not rewrite a startup/readiness failure as a successful shutdown.

M001 records wall-clock-independent lifecycle durations only as diagnostics if useful. They are not measurement samples and must not enter comparison DTOs.

## 7. Security, portability, and compatibility

- No shell interpolation; arguments remain an argv vector.
- Environment values resolve through references at the process boundary. Redaction applies to `Debug`, errors, command summaries, and bundle metadata.
- Reject cwd/path escapes according to the declared workspace-root policy; do not silently fall back to the current directory.
- Bound log bytes per stream and retain truncation metadata.
- Test Unix process-group cleanup on Linux and macOS CI. Use a Windows Job Object or report managed-descendant cleanup unsupported until its semantics are qualified.
- Keep process handles, Tokio tasks, cancellation tokens, and OS IDs out of `ResolvedPlan` and `.eggb` schema DTOs.
- No core schema migration is expected. If runner work requires new durable lifecycle fields, stop and plan an additive core change before inventing runner-local serialized fields.

## 8. Ordered work packages

1. Inspect current plan/resolved types and select the minimal process/runtime dependencies for the runner crate; record any platform-specific deviation.
2. Define runner-owned process specs, structured lifecycle events/outcomes, typed errors, and injected secret/readiness interfaces.
3. Implement managed process spawning, continuous bounded stdout/stderr spooling, readiness, graceful stop, forced process-tree cleanup, and reverse-order teardown.
4. Integrate optional lifecycle artifacts with `BundleWriter`; verify incomplete/failure paths leave staging distinguishable and final manifests truthful.
5. Add deterministic child-process fixtures and focused lifecycle tests.
6. Document supported platform guarantees, capability failures, log bounds, cancellation behavior, and the fact that this milestone does not perform a measured trial.

## 9. Verification

Required before closure:

    cargo fmt --all -- --check
    cargo check --workspace --all-targets --locked
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
    cargo test --workspace --all-features --locked
    cargo +1.89.0 check --workspace --all-targets --locked
    cargo tree --locked
    git diff --check

Focused lifecycle evidence must cover:

- dependency-ordered startup and reverse teardown;
- successful readiness and named-probe failure;
- spawn failure before readiness and readiness timeout;
- cancellation during startup/readiness;
- graceful shutdown then forced process-tree cleanup;
- descendant cleanup on each advertised platform;
- cleanup failure while preserving the original failure;
- continuous pipe draining with bounded/truncated stdout and stderr;
- external services left running;
- missing secret references fail before spawn and secret values do not appear in logs/Debug/bundles;
- finalized zero-trial lifecycle evidence is inconclusive, not a performance pass.

## 10. Acceptance and stop conditions

M001 closes when the runner crate starts and owns managed command processes, enforces declared readiness/shutdown bounds, cleans up descendants on every supported platform, bounds output, preserves primary errors, and can use the M003 bundle without changing its manifest/evidence model.

Stop for a planning update if the core plan lacks information required to safely spawn a requested process, if cleanup semantics cannot be truthfully qualified on a target platform, or if evidence finalization would require mutating completed bundles.

## 11. Closure evidence required

Record implementation commits, runner/core dependency boundary, supported platform matrix, lifecycle requirement-to-test matrix, descendant cleanup results, log-bound behavior, evidence bundle example, verification/MSRV results, known limitations, unresolved findings, and disposition.
