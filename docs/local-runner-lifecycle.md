# Local runner lifecycle (M001)

`eggbench-runner` owns managed local processes behind the runtime-free
`eggbench-core` contracts. It spawns argv vectors directly (never a shell),
starts managed services in dependency order with the managed subject first,
applies declared readiness bounds, drains bounded stdout/stderr continuously,
and tears down the owned process tree in reverse dependency order.

## Session shape

`LocalSession::prepare` revalidates the resolved-plan boundary and builds
ordered specs without starting anything. `startup` spawns and readies,
`shutdown` stops everything in reverse order, and `run` performs one
start-readiness-stop pass for the phase orchestrator planned in M002.

Every failure or cancellation after the first successful spawn tears down
already-started processes. The initiating failure is preserved; teardown
problems are reported alongside it, never as a replacement.

## Supported platforms

| Platform | Guarantee |
|---|---|
| Linux | Supported. Each child gets its own process group; graceful SIGTERM is followed by SIGKILL group cleanup. Descendant cleanup tested. |
| macOS | Supported by the same process-group contract; no Linux-only procfs assumptions. Qualification beyond Linux awaits platform CI. |
| Windows | Unsupported. Managed spawn fails with a structured capability error until Job Object semantics are qualified. |

Process identifiers appear in lifecycle events as diagnostics only and are
never durable subject, service, or trial identity.

## Readiness

- `Delay` waits out the declared duration while watching for early exit and
  caller cancellation.
- `Probe` dispatches to the runner registry. Built-ins are `process-alive`,
  `fake-ok`, `fake-fail`, and `fake-never` (the fakes exist for deterministic
  tests). Unknown probe names fail with an explicit capability error before
  any spawn. HTTP/TCP parameters are never inferred from opaque strings.

Readiness and startup sit outside any future measured interval.

## Shutdown and cancellation

Graceful shutdown sends SIGTERM to the process group, waits only for the
declared `grace_ms` (5 s default), then SIGKILLs the group and waits bounded
for reaping. Every owned process is attempted even when one stop fails.
Cancellation is observed before each spawn and throughout readiness waits;
cancellation before the first spawn reports cleanly without teardown.

## Log bounds

Each stream retains the leading bytes up to the service `log_limit_bytes`
(the subject uses a 1 MiB default). Output past the cap is counted as
dropped while pipe draining continues, so a verbose child can never block on
a full pipe. Truncation metadata travels with the snapshot.

## Secrets

Subject environment references resolve through the injected `SecretProvider`
before spawn; missing references fail before any spawn. Values enter only
the live process environment. Specs, events, errors, and staged bundle
artifacts carry references and redaction markers, never values. Subjects
that echo secrets to stdout remain the caller's responsibility: retained
logs are byte-for-byte.

## Evidence

Lifecycle logs and metadata stage through the M003 `BundleWriter` with
`Stdout`/`Stderr` roles and a `lifecycle`-labeled metadata artifact. No
second evidence format exists. A lifecycle-only run records execution as
`completed`, has no comparison verdict, and may have zero trials. Execution
completion does not imply a performance pass. Manifest v1 bundles retain
their original status as explicitly ambiguous legacy evidence; new writes use
manifest v2's separate execution and comparison fields.

## Non-goals (deferred to later milestones)

No workload drivers, trial scheduler, measurement clock, comparison engine,
CLI, remote execution, database, or security semantics. The session stays
usable as the M002 phase orchestrator's process owner.
