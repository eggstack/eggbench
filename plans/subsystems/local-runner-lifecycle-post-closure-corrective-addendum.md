# Local Runner and Lifecycle — Post-Closure Corrective Addendum

Status: closed

Repository audit baseline: `319b5816604e39578af4e20f5880945123b042e1`

Predecessor work:

- `plans/subsystems/local-runner-lifecycle-roadmap.md` — M001 closed; M002/M003 not started.
- `plans/implementation/local-runner-lifecycle/001-managed-process-and-readiness-lifecycle.md`
- `plans/closure/local-runner-lifecycle/001-status.md` — historical predecessor closure evidence.
- `plans/subsystems/foundation-experiment-evidence-post-closure-corrective-addendum.md` — status/verdict correction that must land first.

Long-term references:

- `plans/000-long-term-specification.md#7-topology-and-service-model`
- `plans/000-long-term-specification.md#19-portability-and-toolchain`
- `plans/000-long-term-specification.md#20-resource-and-harness-overhead`
- `plans/003-planning-process.md`

Related ADRs:

- `plans/adrs/ADR-0001-typed-core-and-driver-boundaries.md`
- `plans/adrs/ADR-0002-immutable-evidence-and-testbed-provenance.md`
- `plans/adrs/ADR-0005-local-first-execution-and-remote-provider-boundary.md`

## 1. Purpose and corrective trigger

Local Runner M001 landed the intended lifecycle boundary: argv-direct spawning, dependency-ordered startup, readiness, bounded output draining, cancellation, graceful/forced teardown, Unix process-group cleanup, secret resolution, and evidence integration.

A post-closure audit found four issues that should be corrected before trial orchestration:

1. **Working-directory confinement is lexical, not filesystem-resolved.** `resolve_working_directory()` rejects `..` and absolute paths but does not resolve symlinks. A workspace path such as `workspace/link -> /outside` can pass lexical `starts_with(root)` and later escape when `Command::current_dir()` follows the link.
2. **Platform support is broader than qualification evidence.** `UnixPlatform::support()` reports every `cfg(unix)` platform as supported. Only Linux has actual descendant-cleanup execution evidence; macOS is documented as supported by code-path equivalence but no hosted macOS run exists. There are no GitHub Actions/status checks on current HEAD.
3. **Process environment semantics are implicit.** `spawn_one()` calls `env_clear()` for every managed process. That is a defensible hermetic default, but it is currently an implementation choice rather than an explicit v1 execution contract. Bare executable names may also create unclear PATH-resolution expectations under a cleared environment.
4. **The M001 closure record contains an invalid full implementation SHA.** It records `9387a45e1bbf9c1f9a55fb8ad875b07f6f880d21`, which does not exist. The actual implementation commit is `9387a459103078c1ccdbca7d4db41ae0f6cefc11`.

These findings do not invalidate the core lifecycle design. They require a focused correctness/qualification pass before M002 adds measured phases.

Historical M001 closure remains immutable; this corrective records the erratum rather than rewriting history.

## 2. Work classification

### Invariants

- A declared workspace root is the filesystem confinement boundary for managed working directories.
- Symlink resolution must not allow a managed cwd outside that boundary.
- The advertised platform matrix reflects executed qualification, not code-path intuition.
- Unsupported platforms fail explicitly before spawn.
- Process environment behavior is deterministic, documented, and tested.
- No implicit shell is introduced.
- Secret values remain absent from plans, Debug output, errors, lifecycle metadata, and ordinary bundle metadata.
- Existing lifecycle cancellation/cleanup/log-bound behavior does not regress.
- Historical closure evidence is corrected through explicit errata, not silent rewriting.

### Capabilities

- safely start a managed process in a filesystem-resolved cwd beneath the workspace root;
- know whether the current platform is qualified for managed descendant cleanup;
- run with a documented hermetic process environment.

### Infrastructure

- cwd canonicalization/confinement helpers;
- target-specific process dependency/config cleanup;
- cross-platform CI;
- closure erratum.

### Polish

- platform/environment documentation;
- diagnostics for unqualified platform and ambiguous executable paths.

## 3. Non-goals

Do not:

- implement M002 warmup/trials/cooldown/reset;
- add workload drivers;
- add network probes;
- add Windows Job Object support unless it is small enough to qualify fully in this corrective;
- add remote execution;
- redesign the experiment-plan schema for arbitrary environment inheritance;
- add a shell wrapper;
- attempt adversarial filesystem sandboxing beyond the explicitly documented local-workspace threat model.

## 4. Current implementation evidence

At the audit baseline:

- `resolve_working_directory()` normalizes path components and checks lexical `starts_with(root)`, but calls neither `canonicalize` nor a no-follow walk.
- lifecycle tests cover `../escape`, `/tmp`, and `../../etc`, but no symlink escape.
- `spawn_one()` executes `env_clear()`, then injects only `ProcessSpec.env`.
- subject secret references populate `ProcessSpec.env`; managed command services currently receive an empty environment.
- `tokio::process::Command::new` receives `argv[0]` directly.
- `UnixPlatform::support()` returns `Supported` for every Unix target.
- the root workspace declares `nix` as a general workspace dependency and the runner consumes it generally even though all use sites are Unix-gated.
- no `.github/workflows` CI exists.
- Linux descendant cleanup is exercised by the real child/grandchild lifecycle test.
- macOS has no current hosted qualification evidence.
- Windows is intentionally reported unsupported for managed execution.
- the predecessor closure's implementation SHA is invalid as a full SHA.

## 5. Dependency graph

~~~text
Foundation status/verdict corrective C001
                 |
                 v
Local Runner corrective C001
                 |
                 v
Local Runner M002 planning/implementation
~~~

This corrective was hard-blocked until the foundation status/verdict C001 closed because both workstreams touch lifecycle evidence tests and bundle finalization call sites. That dependency is now satisfied.

## 6. Correct filesystem-confinement contract

The runner must validate the **filesystem-resolved** working directory, not only lexical path components.

Required behavior:

1. workspace root must exist and be a directory;
2. resolve/canonicalize the workspace root before preparing spawn specs;
3. resolve the requested working directory against the canonical root;
4. resolve filesystem symlinks before spawn;
5. require the resulting canonical directory to remain beneath or equal to the canonical workspace root;
6. reject symlink escape to an outside directory;
7. reject missing/non-directory cwd with an explicit pre-spawn error;
8. retain the canonical confined path in `ProcessSpec`.

Symlinks that resolve to a directory still inside the workspace MAY be allowed. If implementation instead rejects all symlink components for simplicity, document that stricter policy and test it.

The local-runner threat model is an operator-controlled workspace, not a hostile concurrent filesystem mutator. If fully race-free cwd confinement would require fd-relative spawn/chroot/sandbox machinery, record the residual TOCTOU limitation rather than adding a large sandbox subsystem here.

## 7. Executable-resolution and environment contract

Freeze the v1 managed-process environment behavior explicitly:

- no ambient environment inheritance by default;
- `env_clear()` is intentional, not incidental;
- only explicitly resolved values in `ProcessSpec.env` enter the child environment;
- subject secret references remain the currently supported environment-injection mechanism;
- managed service `config` MUST NOT be silently interpreted as environment variables;
- future non-secret/inherited environment configuration requires a typed plan-schema milestone rather than ad hoc runner conventions.

To avoid ambiguous PATH behavior under a clean environment:

- require managed command `argv[0]` to be an explicit path: absolute, or workspace/cwd-relative with a path separator;
- resolve relative executable paths against the appropriate confined cwd before spawn where practical;
- reject bare names such as `sleep`, `python`, or `oha` unless a future resolver has supplied an explicit executable path;
- external-driver milestones continue to own deterministic binary discovery/version capture.

If Rust/platform behavior makes a narrower explicit resolution contract more appropriate, preserve the core rule: execution must not depend silently on ambient PATH after `env_clear()`.

Add tests proving a host-only sentinel environment variable is absent from the child and an explicitly injected secret/reference value is present without leaking through Debug/errors/evidence.

## 8. Platform-support truthfulness

Revise the platform adapter so `Supported` means qualified support.

Initial target:

- Linux: supported after existing lifecycle tests plus hosted CI.
- macOS: supported only after the real descendant-cleanup/process-group lifecycle suite runs successfully on macOS CI.
- Windows: compilation/core behavior supported where possible, but managed descendant execution remains an explicit unsupported capability until Job Objects are implemented and qualified.
- Other Unix targets: do not inherit “supported” merely from `cfg(unix)`; report unsupported/unqualified unless specifically tested.

The implementation may distinguish `Supported`, `Unsupported`, and `Unqualified` if that improves diagnostics. Do not call an untested target supported.

Move Unix-only dependencies such as `nix` behind target-specific Cargo dependency sections so a Windows build is not needlessly coupled to Unix process crates.

## 9. Cross-platform CI

Add a minimal GitHub Actions workflow appropriate to the current repository.

It should prove at least:

### Linux

- stable fmt/check/Clippy/tests;
- Rust 1.89 MSRV check;
- real descendant cleanup lifecycle tests.

### macOS

- check/Clippy/tests;
- real process-group descendant cleanup and lifecycle tests.

### Windows

- workspace check/Clippy where supported;
- core tests;
- runner tests that do not require managed Unix execution;
- explicit unsupported managed-execution behavior.

Do not weaken tests merely to make a matrix green. Use target-specific test organization so unsupported behavior is tested as unsupported.

If hosted CI cannot execute during the corrective, implementation may land but closure must remain conditional on the missing platform evidence.

## 10. Historical closure erratum

Do not edit `plans/closure/local-runner-lifecycle/001-status.md` to conceal the original bad SHA.

Add an erratum record under the closure directory that states:

- historical record contains invalid SHA `9387a45e1bbf9c1f9a55fb8ad875b07f6f880d21`;
- actual implementation commit is `9387a459103078c1ccdbca7d4db41ae0f6cefc11`;
- the documentation closure commit is `319b5816604e39578af4e20f5880945123b042e1`;
- no code claim changes beyond correcting provenance.

The registry and corrective closure should link to the erratum.

## 11. Milestone

### C001 — Filesystem confinement, hermetic execution contract, and platform qualification

Class: correctness/security/qualification corrective.

Implementation plan:

- `plans/implementation/local-runner-lifecycle-post-closure-corrective/001-filesystem-environment-and-platform-qualification.md`

Status: ready; Foundation post-closure corrective C001 is closed.

Exit conditions:

- symlink escape is rejected before spawn;
- canonical in-workspace symlink behavior is explicitly tested/documented;
- hermetic environment behavior is normative and tested;
- bare ambient-PATH command resolution is eliminated;
- Linux and macOS support claims are backed by hosted execution evidence, or macOS remains unqualified;
- Windows compilation/unsupported behavior is truthful;
- Unix-only dependencies are target-scoped;
- M001 closure SHA erratum exists;
- prior lifecycle tests remain green.

## 12. Verification strategy

Required focused evidence:

- workspace root missing/not-directory;
- direct cwd inside root;
- lexical `..` escape;
- absolute cwd escape;
- symlink inside root -> outside root;
- symlink inside root -> inside root, according to chosen policy;
- nested symlink path;
- child sees no ambient sentinel env;
- explicit injected env arrives at child;
- Debug/errors/evidence contain no injected secret value;
- bare executable name rejected;
- explicit absolute executable works;
- explicit cwd-relative executable works;
- Linux process-group cleanup;
- macOS process-group cleanup in hosted CI;
- Windows unsupported managed spawn;
- all existing readiness/cancellation/log/cleanup tests.

Broad verification remains format/check/Clippy/all-features tests/MSRV/cargo-tree/diff-check.

## 13. Completion definition

The corrective closes when managed local execution has a truthful filesystem, environment, executable-resolution, and platform-support contract suitable for M002 to run measured phases on top of it.

## 14. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| C001 | closed | `plans/implementation/local-runner-lifecycle-post-closure-corrective/001-filesystem-environment-and-platform-qualification.md` | `plans/closure/local-runner-lifecycle-post-closure-corrective/001-status.md`; historical M001 SHA erratum: `plans/closure/local-runner-lifecycle/001-errata.md` | none |
