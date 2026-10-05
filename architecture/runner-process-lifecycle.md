# Managed process and service lifecycle

Component deep dive for `crates/eggbench-runner/src/{session,spec,platform,probe}.rs`,
plus the adapter seam in `service.rs` that `session.rs` unifies with OS processes.

## Purpose

`LocalSession` is the runner's only owner of runtime side effects. It answers
one question completely: given a `ResolvedPlan` asking for managed services,
what exactly is started, in what order, when is it ready, what output is
retained, and what is guaranteed to be stopped. The crate doc states the scope
as argv-direct spawning with no shell, dependency-ordered startup, readiness
bounded by declared timeouts, continuous bounded stdout/stderr spooling,
graceful shutdown with forced process-tree cleanup, and reverse-order teardown
that preserves the initiating failure (`lib.rs:3-8`). Process identifiers never
enter durable identity; they appear in lifecycle events as diagnostics
(`session.rs:93-96`).

The platform support matrix is part of the contract, not an implementation
detail: Linux and qualified macOS advertise `Supported`, other Unix targets
advertise `Unqualified`, and Windows managed execution reports an
unsupported-capability error (`lib.rs:10-12`, `platform.rs:51-58`).

This is not the trial schedule. `execute_run` owns phase order and delegates
process work here; see [runner.md](runner.md) and
[runner-orchestration.md](runner-orchestration.md). This milestone performs no
load generation, and a lifecycle-only run records no trials
(`session.rs:10-11`).

## Source layout

| File | Lines | Role |
|---|---|---|
| [session.rs](../crates/eggbench-runner/src/session.rs) | 1,266 | `LocalSession`: prepare, startup, readiness, bounded logs, teardown, lifecycle evidence |
| [spec.rs](../crates/eggbench-runner/src/spec.rs) | 529 | `SpawnPlan` construction, platform gate, path confinement, topological order |
| [platform.rs](../crates/eggbench-runner/src/platform.rs) | 157 | `PlatformAdapter`, `PlatformSupport`, `UnixPlatform`, `UnsupportedPlatform`, `is_process_alive` |
| [probe.rs](../crates/eggbench-runner/src/probe.rs) | 239 | `ReadinessProbe` trait, `ProbeRegistry`, built-in and fake probes |
| [service.rs](../crates/eggbench-runner/src/service.rs) | 290 | `ManagedServiceAdapter`/`ManagedServiceHandle`, `RuntimeBindings`, runtime-topology evidence |
| [error.rs](../crates/eggbench-runner/src/error.rs) | 175 | `RunnerError`, `CleanupFailure` |

Within `session.rs`: options and report types (`session.rs:46-148`), spool and
running-service state (`session.rs:150-238`), `LocalSession` and its `Debug`
(`session.rs:241-285`), `prepare` (`session.rs:295-327`), `startup`
(`session.rs:465-622`), adapter start and readiness (`session.rs:625-723`),
`shutdown` (`session.rs:731-768`), teardown free functions
(`session.rs:1027-1266`). Operator-facing statements of the same rules are in
[../docs/local-runner-lifecycle.md](../docs/local-runner-lifecycle.md);
[../docs/environment-fingerprint.md](../docs/environment-fingerprint.md)
explains why the fingerprint is collected before managed startup, so a failed
collection cannot leave descendants running.

## Key types

`PrepareOptions` is the preparation input: `workspace_root`, an injected
`SecretProvider`, the `PlatformAdapter` used for capability gating, and the
`ServiceAdapterRegistry` (`spec.rs:167-176`). `LocalSession::prepare` builds it
from `RunnerOptions` and calls `prepare` (`session.rs:295-302`); nothing starts
during preparation.

`SpawnPlan` holds three parallel views of one decision (`spec.rs:91-98`):
`specs: Vec<ProcessSpec>` (OS processes), `adapters: Vec<AdapterSpec>`
(in-process named services), and `launch_order: Vec<LaunchEntry>` (the single
ordering across both). `LaunchEntry { identity, kind }` with `LaunchKind::Process
| LaunchKind::Adapter` (`spec.rs:121-135`) makes the unification explicit: an
entry names *what* starts, `kind` selects the machinery. `order()` and
`teardown_order()` project identities out of `launch_order`
(`spec.rs:140-151`); `teardown_order` is literally `order().rev()`
(`spec.rs:150`). There is no second ordering to keep consistent.

`ProcessSpec` is a fully resolved spawn request (`spec.rs:33-62`): identity,
argv, resolved `cwd`, `env`, `secret_references`, `log_limit_bytes`, optional
`readiness` and `shutdown`, `is_subject`, `http_url`, `static_bindings`, and the
deferred `binding_args` / `binding_env` destinations. Its `Debug` redacts every
environment value (`spec.rs:64-82`) and shows only `binding_env` *keys*
(`spec.rs:79`). `AdapterSpec` is the in-process analogue
(`spec.rs:102-117`): identity, `service_type`, opaque `config`, `http_url`,
`static_bindings`, `readiness`, and a resolved `grace: Duration` — no argv, no
working directory, no environment.

| Constant | Value | Meaning |
|---|---|---|
| `SUBJECT_IDENTITY` | `"subject"` (`spec.rs:29`) | Identity of the managed subject process; core declares no log bound for it |
| `DEFAULT_SUBJECT_LOG_LIMIT_BYTES` | 1 MiB (`spec.rs:25`) | Retained bytes per subject stream |
| `DEFAULT_GRACE_MS` | 5,000 (`spec.rs:27`) | Graceful shutdown allowance when no `Shutdown` policy is declared |

`LocalSession` retains the plan, options, external identities, the `running`
stack, retained spools, events, a preparation `Instant`, a sequence counter, and
the accumulated `RuntimeBindings` plus `BindingConsumption` records
(`session.rs:241-254`). Bindings survive teardown so `runtime_topology()` can
still produce final evidence (`session.rs:250-252`).
`RunningManagedService` carries the unification through the whole lifecycle
(`session.rs:217-222`): `Process(Box<RunningProcess>)` or `Adapter`. Its doc
comment states the rule that keeps the abstraction honest — an in-process
service is never represented as process-owned; adapters carry no PID and shut
down through their handle (`session.rs:212-216`).

| Evidence type | Shape | Where |
|---|---|---|
| `LifecycleEventKind` | `Spawned \| Ready \| Stopping \| Stopped` | `session.rs:73-82` |
| `LifecycleEvent` | `seq`, `identity`, `kind`, `pid: Option<u32>`, `elapsed_ms` | `session.rs:86-97` |
| `BoundedOutput` | `data`, `truncated`, `retained_bytes`, `dropped_bytes`, `total_bytes` | `session.rs:101-112` |
| `ServiceLogs` | `stdout` + `stderr` `BoundedOutput` | `session.rs:116-121` |
| `StartupReport` | `started: Vec<String>` in spawn order | `session.rs:125-128` |
| `ShutdownReport` | `stopped_order`, `failures: Vec<CleanupFailure>` | `session.rs:130-137` |
| `LifecycleOutcome` | `started`, `stopped_order`, `cleanup` | `session.rs:141-148` |

`elapsed_ms` is monotonic since session preparation, diagnostic only
(`session.rs:95-96`); `seq` is a session-local counter incremented per push
(`session.rs:807-816`). Adapter entries always record `pid: None`.

`PlatformSupport` is three-valued, and the third value is the point
(`platform.rs:13-20`): `Supported` means descendant cleanup is tested;
`Unqualified` means managed spawn stays disabled until platform-specific
qualification passes; `Unsupported` means managed spawn must fail with a
structured capability error. `PlatformAdapter` is the whole OS boundary:
`support`, `label`, `is_alive`, `terminate_group`, `kill_group`
(`platform.rs:23-44`). Both signaling methods return `Result<(), String>` with
redaction-safe messages rather than a typed error, because a signal failure is a
cleanup observation, not a primary lifecycle failure. `is_process_alive` uses
signal zero: `Ok(())` or `EPERM` means the process exists, because a permission
error still proves liveness (`platform.rs:134-141`).
`UnixPlatform.support()` decides from `cfg!` alone (`platform.rs:51-58`), so the
macOS `Supported` answer is unconditional in code; the "only while hosted
lifecycle and symlink-confinement CI passes" condition in
[../docs/local-runner-lifecycle.md](../docs/local-runner-lifecycle.md):26 is
enforced by CI, not by a runtime check.

`ReadinessProbe` is object-safe and single-shot: `check` is called under a
caller-side timeout, and the probe owns no deadline (`probe.rs:53-59`).
`ProbeContext` gives it identity, an optional diagnostic PID, an `alive`
observation, and the static `http_url` binding (`probe.rs:29-40`);
`ProbeFailure` is a single redaction-safe string (`probe.rs:44-50`).
`ProbeRegistry` is a `BTreeMap<String, Arc<dyn ReadinessProbe>>`
(`probe.rs:65-92`); `with_builtins()` registers five probes (`probe.rs:72-80`):

| Constant | Value | Behavior |
|---|---|---|
| `PROCESS_ALIVE_PROBE` | `process-alive` | Ready while the process is alive (`probe.rs:96-111`) |
| `TCP_LOOPBACK_PROBE` | `tcp-loopback` | TCP-connect to a declared loopback `http_url` (`probe.rs:124-156`) |
| `FAKE_OK_PROBE` | `fake-ok` | Always ready (`probe.rs:205-211`) |
| `FAKE_FAIL_PROBE` | `fake-fail` | Always not-ready (`probe.rs:215-226`) |
| `FAKE_NEVER_PROBE` | `fake-never` | Never ready; forces the caller timeout (`probe.rs:230-239`) |

`tcp-loopback` exists because a fixed startup delay was not portable; its doc
comment names macOS Python startup and Apple Silicon cold cache as the
motivating variance (`probe.rs:114-119`). It refuses to infer parameters from
opaque strings: `parse_loopback_http_url` requires an `http://` scheme, an
explicit port, an IPv4 literal, a loopback host, a non-zero port, and no
credentials, and errors out rather than performing network I/O otherwise
(`probe.rs:171-201`); the target's `host` is an `Ipv4Addr`
(`probe.rs:160-163`), so a non-IPv4 binding cannot parse into a target at all.
The connect runs on `spawn_blocking` and the stream is dropped immediately — a
liveness signal, not a request (`probe.rs:141-153`). The fakes exist to make
readiness outcomes deterministic in tests. They are registered by
`with_builtins()` like any other probe, so a plan naming `fake-ok` in a real run
gets an instant, meaningless "ready"; nothing in the runner blocks that.

## How it works

### Preparation: preflight before any side effect

`prepare` (`spec.rs:195-306`) runs a fixed order, and every rejection happens
here rather than mid-startup:

1. **Schema.** `resolved.schema_version` must equal
   `RESOLVED_PLAN_SCHEMA_VERSION` (`spec.rs:199-203`).
2. **Platform gate.** `ensure_platform` runs before any path work
   (`spec.rs:204`), and applies only when the plan needs managed spawn — a
   managed subject, or any `Lifecycle::Managed` service (`spec.rs:330-336`).
   `Unqualified` and `Unsupported` both produce
   `RunnerError::UnsupportedPlatform` with different details
   (`spec.rs:315-324`). A plan of only external services passes anywhere.
3. **Workspace root.** Absolute, must exist, must be a directory
   (`spec.rs:338-356`).
4. **Subject.** `Subject::ManagedCommand` becomes a `ProcessSpec` with identity
   `subject`, argv resolved against the workspace root, secret references
   resolved and sorted, `log_limit_bytes` defaulted to 1 MiB, and
   `readiness`/`shutdown` forced to `None` (`spec.rs:209-252`). It is pushed to
   `launch_order` first, unconditionally ahead of services
   (`spec.rs:248-251`).
5. **Services, topologically.** `topological_order` runs Kahn's algorithm over
   `depends_on` (`spec.rs:468-529`); unknown dependencies and cycles are
   `InvalidPlan`, the cycle detected by comparing ordered length to service
   count (`spec.rs:515-519`). Only `Lifecycle::Managed` services enter the
   launch order (`spec.rs:255-257`).

Path confinement happens here, and both halves matter. A working directory must
be relative, must exist, must be a directory, and — after symlink resolution via
`fs::canonicalize` — must still start with the canonical root
(`spec.rs:358-392`); absolute paths are rejected outright (`spec.rs:367-372`).
An executable must be absolute *or* a relative path containing a separator, must
canonicalize to an existing file, and `argv[0]` is replaced with the resolved
path (`spec.rs:394-430`), so a bare name such as `python` never reaches ambient
`PATH` lookup. Named services are validated against the registry here, so an
unimplemented `ServiceKind::Named` type is `UnsupportedService` before any
process starts (`spec.rs:434-451`), and `AdapterSpec.grace` resolves from the
declared policy or `DEFAULT_GRACE_MS` (`spec.rs:459-464`).

### Spawning

`spawn_one` (`session.rs:818-861`) is the whole spawn path. argv passes through
directly — `Command::new(&spec.argv[0])` plus `args(&spec.argv[1..])` — with no
shell, no `sh -c`, and no string interpolation anywhere in the crate
(`session.rs:819-822`). `current_dir(&spec.cwd)` sets the directory
(`session.rs:823`). `env_clear()` then `envs(&spec.env)`
(`session.rs:824-825`) makes the child environment hermetic: no inherited
`HOME`, `PATH`, locale, proxy, or toolchain. A managed command service's `env`
starts empty (`spec.rs:276`) and is populated only by resolved `binding_env`
values (`session.rs:535`).

`stdin(Stdio::null())` and both output streams `piped()` (`session.rs:826-828`)
mean a child can never block waiting for input. `kill_on_drop(false)`
(`session.rs:829`) is why teardown cannot be delegated to Rust's drop semantics:
a forgotten `shutdown` leaves a live process rather than an implicit kill, so
the guarantee is enforced by `startup`'s error paths, not by the type system.
`process_group(0)` under `#[cfg(unix)]` (`session.rs:830-834`) then makes the
child the leader of a *new* process group with pgid equal to its own pid. Every
descendant it forks inherits that pgid unless it deliberately leaves it, which
is what makes group signaling reach the whole tree. Descendant cleanup is
therefore one syscall shape: `killpg` on the group id (`platform.rs:151-156`),
issued for `SIGTERM` then `SIGKILL` (`platform.rs:75-97`), with `ESRCH` treated
as success — a missing group means already gone (`platform.rs:154`). On non-Unix
builds the same two functions return the unsupported-capability string
(`platform.rs:80-84`, `platform.rs:92-96`).

Two drain tasks are spawned per process, one per pipe, each reading 8 KiB chunks
until EOF (`session.rs:849-852`, `session.rs:1162-1178`, constant at
`session.rs:43`). Draining is continuous and unconditional: it is not gated on
the byte cap.

### Startup sequence

| Step | Process path | Adapter path | Failure effect |
|---|---|---|---|
| 0 | `check_probes_registered` over `specs` (`session.rs:469`, `session.rs:793-805`) | not checked here | `UnsupportedProbe` before any spawn |
| 1 | empty `launch_order` → empty `StartupReport` (`session.rs:470-474`) | same | — |
| 2 | cancellation? → `cancel_after_start` (`session.rs:477-479`) | same | `CancelledBeforeSpawn` or `Cancelled` + cleanup |
| 3 | resolve `binding_args` / `binding_env`; each value ≤2048 bytes and control-free (`session.rs:492-547`, check at `session.rs:505`) | `start_adapter` (`session.rs:597`, `session.rs:625-694`) | `teardown_running`, then preserve primary |
| 4 | `spawn_one` (`session.rs:558`) | adapter `start` returns after adapter-owned readiness (`session.rs:649-660`) | `teardown_running`, then preserve primary |
| 5 | push `Spawned` with pid, push onto `running` (`session.rs:570-573`) | push `Spawned` with `pid: None` (`session.rs:599`) | — |
| 6 | `apply_readiness` (`session.rs:574`, `session.rs:863-961`) | `apply_adapter_readiness` (`session.rs:601`, `session.rs:702-723`) | `teardown_running`, then preserve primary |
| 7 | push `Ready` (`session.rs:578`) | merge bindings, push `Ready` (`session.rs:605-608`) | see Failure model |
| 8 | publish `http_url` + `static_bindings` (`session.rs:579-584`) | declared bindings merged inside `start_adapter` (`session.rs:665-684`) | see Failure model |

Step 3 makes the ordering load-bearing rather than decorative: a service's argv
and environment can reference bindings published by a service that started
earlier, so dependency order and binding availability are the same constraint.
Each consumption is recorded as a `BindingConsumption` carrying a SHA-256 of the
resolved value and no payload (`session.rs:1070-1087`, digest at
`session.rs:1084`), accumulated into the runtime-topology artifact.
`RunningProcess` keeps the live `Child`, pid, both spools, and the drain join
handles (`session.rs:195-202`); `RunningAdapter` keeps identity, service type,
handle, bindings, and grace (`session.rs:204-210`).

### Readiness

Three cases, all bounded:

- **`None`.** `check_alive` only (`session.rs:870`). A service with no declared
  readiness must merely still be alive when startup reaches it.
- **`Delay { after_ms }`.** `wait_for_exit_cancel` polls the child every 10 ms
  (`session.rs:881`, `session.rs:1192-1215`, `POLL_INTERVAL` at
  `session.rs:37`) for the declared duration, watching for early exit and
  cancellation. Elapsed → `check_alive`; exited → `ProcessExitedEarly`. A
  `try_wait` error is reported as `Elapsed` (`session.rs:1205`), so a poll error
  surfaces as "not ready" or "exited" rather than a distinct fault.
- **`Probe { probe, timeout_ms }`.** A retry loop (`session.rs:895-959`) under
  two bounds. The overall deadline is the declared `timeout_ms`; each attempt
  gets `min(250 ms, remaining)` (`session.rs:908`, `session.rs:924`), so a hung
  probe — a stalled TCP connect, or `fake-never` — cannot consume a whole backoff
  cycle. After each non-ready attempt the loop sleeps
  `min(250 ms, remaining)` (`session.rs:950-951`) so a fast-failing probe does
  not spin the CPU. The deadline is re-checked before and after each attempt
  (`session.rs:917-923`, `session.rs:940-946`), so `ReadinessTimeout` reports the
  *declared* `timeout_ms`, not elapsed wall time.

`check_alive` (`session.rs:973-1003`) is deliberately two-stage: it calls
`child.try_wait()` to reap first, and only then consults
`platform.is_alive(pid)`. Its doc comment states why — signal-zero liveness
alone can mistake an unreaped zombie for a live process (`session.rs:963-972`).

`apply_adapter_readiness` (`session.rs:702-723`) differs structurally. Adapter
`start` already returned after adapter-owned readiness (`session.rs:661-663`), so
a plan-level `Readiness::Delay` is honored as a post-ready sleep, but a
plan-level `Readiness::Probe` is rejected with `UnsupportedProbe` because there
is no PID to probe (`session.rs:718-721`). In-process services cannot be probed
by a PID-based probe; saying so is the contract.

### Bounded output

`SpoolState` (`session.rs:151-193`) keeps retained bytes, a cap, a dropped
counter, and a total counter. `push` (`session.rs:168-182`) adds to `total` for
every chunk, retains up to the cap, and attributes the remainder to `dropped` —
including the whole chunk when the spool is already full. `snapshot`
(`session.rs:184-192`) projects that into `BoundedOutput`, with `truncated`
derived as `dropped > 0` (`session.rs:187`).

The load-bearing property: the cap bounds *retention*, never *draining*. The
drain tasks keep reading to EOF regardless (`session.rs:1169-1175`), so a
verbose child cannot fill a pipe and block. `retained_bytes` and `total_bytes`
are distinct fields precisely so a reader can tell "quiet" from "very loud".

`logs(identity)` (`session.rs:364-381`) looks first in `running`, then in
`retained`, and returns `None` for an identity that never started. Retained
spools are created during teardown for every process (`session.rs:748-752`,
`session.rs:1042-1046`) by cloning the `Arc`, so post-mortem inspection keeps
working after the process is gone. In-process adapter services own no logs and
always yield `None` (`session.rs:362-363`).

### Teardown

`shutdown` (`session.rs:731-768`) pops from the end of `running`, which is
reverse launch order by construction. It shares its loop with
`teardown_running` (`session.rs:1027-1059`) and differs only in the return: a
`ShutdownReport` versus a bare `Vec<CleanupFailure>`. For a process: push
`Stopping` with the pid, compute grace from the spec or `DEFAULT_GRACE_MS`
(`shutdown_grace`, `session.rs:1118-1123`), call `stop_process`, record any
failure as `CleanupFailure`, push `Stopped` with the same pid, retain the spools
(`session.rs:738-753`). For an adapter: push `Stopping` with `pid: None`, call
`handle.shutdown(grace)`, record failure, push `Stopped` with `pid: None`
(`session.rs:754-761`).

`stop_process` (`session.rs:1217-1245`) is the forced-cleanup ladder:

| Step | Action | Failure handling |
|---|---|---|
| 1 | `Unsupported` platform → immediate `Err` (`session.rs:1222-1224`) | terminal `CleanupFailure` reason |
| 2 | `terminate_group` for SIGTERM (`session.rs:1226`) | failure deliberately discarded (`session.rs:1228`) so the kill path still runs |
| 3 | wait for exit, bounded by plan grace (`session.rs:1230`, `session.rs:1247-1260`) | expiry routes to SIGKILL |
| 4 | `kill_group` the whole group (`session.rs:1234`) | failure *is* a reason, prefixed `forced process-tree cleanup failed` (`session.rs:1236`) |
| 5 | wait again, bounded by `KILL_WAIT_LIMIT` of 5 s (`session.rs:39`, `session.rs:1237`) | expiry → `Err("process N did not exit after forced cleanup")` (`session.rs:1241-1244`) |
| 6 | `join_drains` on either success path (`session.rs:1262-1266`) | each handle under `DRAIN_JOIN_LIMIT` of 5 s (`session.rs:41`), dropped on timeout (`session.rs:1264`) |

Every owned service is attempted even when an earlier stop fails: the loop
pop-continues and accumulates (`session.rs:734`, `session.rs:1029-1057`).
`ShutdownReport`'s doc comment states the rule — failures never replace a
primary lifecycle outcome (`session.rs:130`).

### Why one ordering

Processes and adapters are ordered by the same topological pass and recorded in
the same `launch_order`, and torn down by the same reverse-pop loop. Two
orderings would have to be reconciled, and any disagreement would surface as a
service consuming a binding that has not been published, or a dependency being
stopped before its dependent. `RunningManagedService` is the mechanism: startup,
readiness, teardown, and evidence projection each branch once on the variant, so
the process-only surface — pid, pipes, signals, process groups — cannot leak into
adapter behavior, and the adapter-only surface — handles, adapter-owned
readiness — cannot be faked for a process. `runtime_topology()` makes the
distinction evidence rather than convention: `LaunchKind::Process` always records
`ServiceOwnership::Process` with no service type, while `LaunchKind::Adapter`
records `ServiceOwnership::Adapter` plus the service type resolved from the plan,
falling back to the live adapter, then to retained bindings
(`session.rs:407-425`). External services are appended after the launch order
with `ServiceOwnership::External` (`session.rs:433-445`).

## Invariants

1. **One ordering.** Every runner-owned service, process or adapter, appears
   exactly once in `launch_order`, and teardown order is its exact reverse
   (`spec.rs:140-151`, `session.rs:734`).
2. **Prepared means preflighted.** Schema, platform capability, path
   confinement, executable resolution, secret resolution, adapter registration,
   and probe registration all fail before any process starts
   (`spec.rs:195-204`, `spec.rs:434-451`, `session.rs:469`).
3. **The unsupported path is a typed error, never a degradation.** Windows and
   unqualified Unix targets produce `RunnerError::UnsupportedPlatform` at
   prepare; there is no fallback that spawns without descendant cleanup
   (`spec.rs:315-324`). `stop_process` re-checks `Unsupported` defensively
   (`session.rs:1222`).
4. **No shell, no ambient lookup, no inherited environment.** argv passes
   through directly (`session.rs:819-822`); `argv[0]` is a canonicalized
   filesystem path (`spec.rs:394-430`); the child environment is cleared then
   repopulated (`session.rs:824-825`).
5. **Process ids are diagnostics only.** They appear in `LifecycleEvent.pid` and
   in cleanup reason strings; they never enter plans, bindings, artifacts, or
   identity (`session.rs:93-94`, `session.rs:241-254`).
6. **External services are observed, never owned.** They appear in
   `external_services()` and in runtime topology with
   `ServiceOwnership::External`, and never enter `running`
   (`session.rs:303-314`, `session.rs:341-345`, `session.rs:433-445`).
7. **Readiness is bounded by declared timeouts.** Every wait has a
   plan-derived deadline plus a per-attempt and a backoff ceiling
   (`session.rs:895-959`, `session.rs:1192-1215`). Nothing waits indefinitely.
8. **Log caps bound retention, not draining.** Draining continues to EOF past
   the cap; overflow is counted, not buffered (`session.rs:168-182`,
   `session.rs:1169-1175`).
9. **Secret values never enter the evidence path.** Values exist only in
   `ProcessSpec.env` and the live child environment; `Debug` redacts them
   (`spec.rs:64-86`), and consumption evidence records only a SHA-256
   (`session.rs:1084`).
10. **Teardown is attempted for every owned service, always** (`session.rs:734`,
    `session.rs:1029-1057`).
11. **Cleanup never replaces the primary failure** (see Failure model).
12. **Adapter services are never process-shaped.** No pid, plan-level
    `Readiness::Probe` rejected, shutdown through the handle
    (`session.rs:212-216`, `session.rs:718-721`, `session.rs:754-761`).

## Failure model

`RunnerError` separates preflight failures, which happen with nothing running,
from post-spawn failures, which carry cleanup evidence (`error.rs:44-152`).
`CleanupFailure` is deliberately minimal — an identity and a redaction-safe
reason (`error.rs:13-18`). `RunnerError::cleanup()` (`error.rs:157-174`) returns
`&[]` for every preflight variant, which is how a caller distinguishes "nothing
was running" from "we tried and had trouble".

The preservation mechanism is `attach_cleanup` (`session.rs:1125-1160`). It
rewrites only the five post-spawn variants — `SpawnFailed`, `ReadinessFailed`,
`ReadinessTimeout`, `ProcessExitedEarly`, `Cancelled` — replacing their cleanup
vector, and returns every other variant untouched via `other => other`
(`session.rs:1158`). The primary cause is always the returned variant; the
cleanup list rides alongside it.

| Failure | Raised at | Cleanup attached |
|---|---|---|
| `InvalidPlan`, `UnsupportedService`, `UnsupportedProbe`, `UnsupportedPlatform`, `MissingSecret`, `InvalidWorkingDirectory`, `InvalidExecutablePath`, `EmptyArgv` | prepare / pre-spawn | none — nothing was running |
| `CancelledBeforeSpawn` | cancellation with `running` empty (`session.rs:1062-1064`) | none |
| `SpawnFailed` | `spawn_one` error (`session.rs:560-567`) | yes |
| `ReadinessTimeout` | probe deadline (`session.rs:918`, `session.rs:941`) | yes |
| `ProcessExitedEarly` | early exit or `is_alive` false (`session.rs:887`, `session.rs:997`) | yes |
| `Cancelled` | cancellation after at least one start (`session.rs:1061-1067`) | yes |
| `CleanupFailure` (collected) | per-service stop failure (`session.rs:745`, `session.rs:758`) | reported, never thrown |

Two exit paths inside `startup` are not covered by that pattern, and both are
real gaps. `publish_spec_bindings(...)?` at `session.rs:584`, and
`self.bindings.merge_checked(&bindings).map_err(...)?` at `session.rs:607`, both
run after the service was pushed onto `running` and marked `Ready`. Both use `?`
on a `RunnerError::InvalidPlan`, so they return from `startup` without calling
`teardown_running`. Already-started services stay on `self.running`;
`InvalidPlan::cleanup()` is `&[]` (`error.rs:164`), so the caller is told nothing
about them; and `run()` (`session.rs:780-791`) returns at its own `?` without
calling `shutdown`. Contrast the sibling adapter path at `session.rs:673-683`,
where a binding conflict is detected and the adapter is explicitly shut down
before the error is returned — the same class of problem, handled two ways.

Cancellation has a similar asymmetry at `session.rs:610-616`: when
`start_adapter` fails *and* the token is cancelled, the original error is
discarded and `cancel_after_start()` runs instead, returning `Cancelled`. The
initiating failure is lost in that interleaving.

Beyond lifecycle errors, `stop_process` and `join_drains` make three local
decisions that look like omissions but are not: SIGTERM failure is ignored so the
SIGKILL path still runs (`session.rs:1226-1229`); a drain join exceeding 5 s is
abandoned rather than awaited (`session.rs:1262-1266`); and
`wait_for_child_exit` returns `false` on a `try_wait` error, treating the wait
as expired (`session.rs:1253`) and routing to SIGKILL.

## Boundaries

**In scope.** The spawn plan, the platform gate, process-group creation,
readiness dispatch and bounding, bounded spooling, lifecycle events, teardown —
plus the adapter seam's participation in all of the above.

**Out of scope, by design.** *Scheduling*: `execute_run` owns warmups, measured
trials, reset hooks, and phase events; the session never schedules and never
measures (`session.rs:10-11`). *Evidence finalization*: staging of lifecycle
logs and runtime topology goes through `BundleWriter`; the session only produces
the data (`session.rs:364-451`). *External services*: observed, never started,
stopped, or ordered. *Secret storage*: `SecretProvider` is injected; the session
only resolves references (`spec.rs:221-231`). *Hostile filesystems*:
canonicalization plus a containment check is a pre-spawn guarantee for an
operator-controlled workspace, not a sandbox; a concurrent filesystem change
remains a time-of-check/time-of-use race
([../docs/local-runner-lifecycle.md](../docs/local-runner-lifecycle.md):40-42).
*Network reach*: `tcp-loopback` refuses any non-loopback IPv4 binding, so the
probe's reach cannot widen through a schema change (`probe.rs:165-170`).

**Environment posture.** Every managed child starts with an empty environment;
the runner does not inherit `HOME`, `PATH`, temp directories, locale, proxy, or
credential variables
([../docs/local-runner-lifecycle.md](../docs/local-runner-lifecycle.md):50-58).
Subjects that echo secrets to stdout are the caller's responsibility; retained
logs are byte-for-byte
([../docs/local-runner-lifecycle.md](../docs/local-runner-lifecycle.md):86-93).

**Upstream.** `eggbench-core` is runtime-free and names a readiness request
without defining transport semantics; the runner supplies those semantics and
rejects unknown probe names (`probe.rs:1-6`, `session.rs:793-805`). The crate
forbids `unsafe_code` (`lib.rs:29`).

## Tests and qualification

`crates/eggbench-runner/tests/lifecycle.rs` is the main suite, driving real child
processes through `CARGO_BIN_EXE_eggbench-child-fixture`
([eggbench-child-fixture.rs](../crates/eggbench-runner/src/bin/eggbench-child-fixture.rs):1-6).
The fixture is argv-driven with fixed modes — `exit`, `sleep`, `exit-after`,
`has-env`, `emit-stdout`, `emit-stderr`, `emit-both`, `emit-sleep`,
`term-exit`, `term-ignore`, `descendant`, `bind-after-sleep` — and never reads
secret values (`eggbench-child-fixture.rs:12-16`, `:31-57`).

| Contract | Tests (line) |
|---|---|
| One ordering, subject first, reverse teardown | `dependency_order_startup_and_reverse_teardown` (133), `subject_managed_command_spawns_first` (1071), `mixed_services_start_in_dependency_order_and_stop_in_reverse` (1233) |
| Readiness bounding and probe dispatch | `readiness_delay_and_fake_ok_succeed` (166), `unknown_probe_fails_before_spawn` (187), `tcp_loopback_probe_waits_for_port_then_succeeds` (208), `tcp_loopback_probe_rejects_non_loopback_binding` (252) |
| Spawn, readiness, and cancellation failure paths | `spawn_failure_before_readiness` (277), `readiness_timeout_tears_down_started_process` (311), `cancellation_during_readiness_tears_down` (362), `cancellation_before_spawn_reports_cleanly` (383) |
| Teardown ladder and descendant cleanup | `graceful_shutdown_path` (396), `forced_cleanup_after_ignored_termination` (412), `descendant_cleanup_reaches_process_group` (433), `cleanup_failure_preserves_primary_failure` (497) |
| Bounded output, external non-ownership | `bounded_output_truncates_but_keeps_draining` (538), `external_services_are_observed_but_never_owned` (581) |
| Hermetic env, redaction, path confinement | `missing_secret_fails_before_spawn_and_values_stay_redacted` (612), `managed_environment_is_hermetic_...` (667), `working_directory_escapes_are_rejected` (864), `explicit_executable_paths_are_resolved_and_bare_names_rejected` (957) |
| Typed capability failures and support matrix | `unsupported_service_and_platform_fail_explicitly` (1031), `supported_platform_matrix_is_truthful` (1089) |
| Adapter participation in the unified order | `conflicting_static_and_adapter_http_bindings_fail_closed_and_stop_adapter` (1297), `adapter_probe_readiness_is_rejected_and_service_is_torn_down` (1317), `adapter_shutdown_failure_becomes_cleanup_evidence` (1349) |

[tests/platform.rs](../crates/eggbench-runner/tests/platform.rs) holds the
platform matrix contract: the truthfulness assertion against `cfg!(target_os)`
(`tests/platform.rs:97-106`) and, on Windows only,
`windows_managed_execution_is_rejected_before_spawn` asserting
`LocalSession::prepare` yields `UnsupportedPlatform`
(`tests/platform.rs:110-114`). `FailKillPlatform` (`lifecycle.rs:471-484`) is a
test adapter that always fails forced cleanup — it exists to prove the primary
failure survives teardown trouble.

Not covered: the two `?` exits identified in Failure model (`session.rs:584`,
`session.rs:607`) have no test asserting teardown, and macOS qualification of
descendant cleanup is a CI gate rather than a unit assertion.

## Review focus

Ordered by cost of a wrong change.

1. **Any new `?` inside `startup` after step 5.** A plain `?` on a
   preflight-shaped error skips `teardown_running` and reports no cleanup.
   Existing instances: `session.rs:584`, `session.rs:607`. The correct pattern
   is already in the file at `session.rs:574-577`.
2. **`attach_cleanup`'s variant list** (`session.rs:1125-1160`). Adding a
   post-spawn `RunnerError` variant without adding it here silently drops its
   cleanup evidence through `other => other` (`session.rs:1158`).
3. **Symmetry between `shutdown` and `teardown_running`.** Two copies of the
   same loop (`session.rs:731-768`, `session.rs:1027-1059`); a change to one is
   not enforced on the other.
4. **`stop_process`'s discard points** (`session.rs:1222`,
   `session.rs:1226-1229`, `session.rs:1253`). Each prefers reclaiming
   descendants over reporting a precise cause; removing one silently changes
   failure classification.
5. **`launch_order` construction** (`spec.rs:248-251`, `spec.rs:287-290`,
   `spec.rs:294-297`). Subject-first is unconditional, not dependency-derived;
   changing the ordering changes which bindings are visible when.
6. **Probe budget arithmetic** (`session.rs:908`, `session.rs:917-924`,
   `session.rs:940-951`). Two deadlines, a per-attempt cap, and a backoff cap;
   `ReadinessTimeout` reports declared rather than elapsed time.
7. **`ProcessSpec.env` population** (`spec.rs:276`, `session.rs:520-544`). The
   hermetic-environment guarantee depends on this staying empty except for
   validated binding values.
8. **Spool cap arithmetic** (`session.rs:168-182`). `dropped` accounting must
   stay consistent with `total`, and draining must stay unconditional.
9. **`is_process_alive` and `check_alive` interaction** (`platform.rs:134-141`,
   `session.rs:973-1003`). Dropping the `try_wait` reap step reintroduces
   zombie-as-alive false positives.
10. **Adapter versus process evidence** (`session.rs:407-445`). Any new field
    must not let an adapter acquire a pid or a process acquire a service type.

## Related

- [overview.md](overview.md) — system shape, cross-cutting contracts, evidence
  bundle layout, review order.
- [runner.md](runner.md) — crate-level ownership; the named-adapter section and
  the post-start cleanup contract this component feeds.
- [runner-orchestration.md](runner-orchestration.md) — `execute_run` phase
  schedule and the single cleanup boundary above `LocalSession`.
- [../docs/local-runner-lifecycle.md](../docs/local-runner-lifecycle.md) —
  operator-facing lifecycle, platform table, filesystem and environment rules.
- [../docs/environment-fingerprint.md](../docs/environment-fingerprint.md) —
  why collection precedes managed startup.
- [../docs/eggstack-http.md](../docs/eggstack-http.md) — the first named
  in-process adapter consumer.
