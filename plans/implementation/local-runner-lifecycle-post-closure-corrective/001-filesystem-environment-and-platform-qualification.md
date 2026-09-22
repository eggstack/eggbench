# Local Runner Post-Closure Corrective C001 — Filesystem, Environment, and Platform Qualification

Status: closed

Closure record: `plans/closure/local-runner-lifecycle-post-closure-corrective/001-status.md`

Repository planning baseline: `f10e03d224a3ca62cec04cd124154ace7685398f` (Foundation corrective C001 implementation).

Source corrective:

- `plans/subsystems/local-runner-lifecycle-post-closure-corrective-addendum.md`

Hard dependency:

- Foundation post-closure corrective C001 is closed in `plans/closure/foundation-experiment-evidence-post-closure-corrective/001-status.md`.

Predecessor evidence:

- `plans/closure/local-runner-lifecycle/001-status.md`
- `plans/implementation/local-runner-lifecycle/001-managed-process-and-readiness-lifecycle.md`

Controlling ADRs:

- `plans/adrs/ADR-0001-typed-core-and-driver-boundaries.md`
- `plans/adrs/ADR-0005-local-first-execution-and-remote-provider-boundary.md`

Primary class: correctness/security/qualification corrective.

## 1. Objective

Harden the closed Local Runner M001 lifecycle boundary before measured trial orchestration begins.

This corrective must:

1. make cwd confinement filesystem-resolved rather than merely lexical;
2. make the hermetic child-process environment an explicit, tested v1 contract;
3. eliminate implicit ambient-PATH executable lookup;
4. make platform support claims match executed qualification;
5. add the first cross-platform CI evidence;
6. record the M001 closure-SHA erratum without rewriting historical evidence.

No warmup/trial scheduler is added.

## 2. Current defect evidence

At the planning baseline:

- `normalize_workspace_root()` and `resolve_working_directory()` manipulate path components lexically.
- `resolve_working_directory()` checks `resolved.starts_with(root)` but does not resolve a symlink that may point outside the root.
- lifecycle tests contain no symlink-cwd escape case.
- `spawn_one()` calls `env_clear()` and injects only `ProcessSpec.env`.
- service `config` is opaque and currently ignored by process spawning.
- `Command::new(&spec.argv[0])` receives the user/resolved string directly, with no runner-owned explicit executable-resolution policy.
- `UnixPlatform::support()` reports every Unix build as supported.
- Linux has actual descendant-cleanup test evidence.
- macOS has code-path-equivalence documentation but no hosted execution result.
- Windows managed execution is deliberately unsupported.
- no GitHub Actions workflow exists at current HEAD.
- runner depends on `nix` generally even though process-group use is Unix-only.
- historical M001 closure lists nonexistent implementation SHA `9387a45e1bbf9c1f9a55fb8ad875b07f6f880d21`.
- actual implementation SHA is `9387a459103078c1ccdbca7d4db41ae0f6cefc11`.

## 3. Required invariants

- No shell.
- Working directory cannot resolve outside the workspace root.
- A supported platform has executed lifecycle qualification evidence.
- Unsupported/unqualified platforms fail before managed spawn.
- Child environment is deterministic and does not inherit ambient host variables by default.
- Executable selection is deterministic and does not silently depend on ambient PATH.
- Secrets remain redacted from Debug/errors/evidence.
- Existing dependency-order/readiness/cancellation/cleanup/log-bound semantics remain intact.
- `eggbench-core` remains process/runtime free.
- Historical closure is corrected by erratum rather than rewritten.

## 4. Ordered work package A — Filesystem-resolved cwd confinement

Replace lexical-only confinement with filesystem-aware preflight.

Required behavior:

1. Require the supplied workspace root to exist and be a directory.
2. Canonicalize/resolve the workspace root once during prepare.
3. Resolve the requested cwd from that canonical root.
4. Require the requested directory to exist and be a directory before spawn.
5. Canonicalize/resolve the requested cwd.
6. Reject if the resolved cwd is not beneath or equal to the resolved workspace root.
7. Store the resolved confined cwd in `ProcessSpec`.
8. Produce a typed `InvalidWorkingDirectory` or a narrower new error without leaking unrelated filesystem details.

Tests must include:

- normal direct child directory;
- lexical parent escape;
- absolute escape;
- symlink child -> outside root;
- nested symlink -> outside root;
- symlink -> in-root directory under the selected allow/reject policy;
- missing cwd;
- cwd path resolving to a file.

A local operator may mutate the filesystem after preflight. Do not attempt to solve adversarial TOCTOU with a large sandbox subsystem in this corrective. Document that the confinement guarantee is pre-spawn canonical resolution under an operator-controlled workspace.

If a simple no-follow directory-handle approach can strengthen the contract without introducing unsafe code or major platform divergence, it may be used, but it is not required to close this corrective.

## 5. Ordered work package B — Hermetic environment contract

Document and test the existing intentional v1 behavior:

- clear the ambient environment for every managed process;
- inject only entries explicitly present in `ProcessSpec.env`;
- keep subject secret references as the currently supported injected values;
- do not interpret service `config` keys as environment variables;
- do not inherit HOME, PATH, TMPDIR/TEMP, locale, proxy, credential, or toolchain variables implicitly.

Add a deterministic child-fixture command that reports whether a named sentinel environment variable exists.

Tests:

- set a sentinel in the parent test process and prove it is absent in the child;
- inject a value through the supported explicit path and prove it is present;
- prove the value is not present in ProcessSpec Debug, RunnerError Debug/Display, lifecycle metadata, or bundle manifest content.

Do not add arbitrary environment inheritance to the current plan schema in this corrective.

If practical real workloads later require non-secret environment fields or explicit inheritance allowlists, that must be a separate typed plan-schema milestone before the first integration that needs it.

## 6. Ordered work package C — Explicit executable resolution

Managed execution must not rely on ambient PATH behavior.

Preferred v1 rule:

- argv[0] with an absolute path is accepted;
- argv[0] with an explicit relative path containing a path separator is resolved against the confined cwd and must resolve to a file before spawn;
- a bare executable name with no path separator is rejected with an actionable typed error.

The exact platform separator handling must be portable.

Do not search PATH in the runner.

Future external-tool driver work owns explicit binary discovery/version capture and can put the resolved executable path into the spawn request.

Tests:

- absolute fixture executable works;
- cwd-relative fixture executable works;
- bare command name is rejected before spawn;
- missing executable path is rejected;
- directory passed as executable is rejected.

If Windows executable extension resolution materially complicates the current unsupported-managed-execution path, keep Windows managed spawn unsupported and test the preflight/capability behavior only.

## 7. Ordered work package D — Platform support and dependency scoping

Change platform capability reporting so “supported” means qualified.

Target behavior after this corrective:

- Linux: supported and tested.
- macOS: supported only if hosted macOS lifecycle tests pass in this corrective.
- Windows: managed local execution unsupported, but the workspace should compile/test its supported code paths.
- other Unix: unqualified/unsupported unless explicit evidence exists.

It is acceptable to add:

~~~text
Supported
Unqualified
Unsupported
~~~

instead of a boolean-like two-state enum if this produces more truthful diagnostics.

Do not broaden the platform enum solely for cosmetic reasons.

Move `nix` to a Unix-target dependency section in the runner/workspace dependency structure where possible.

Review all Unix-specific imports/tests for target gating.

## 8. Ordered work package E — Cross-platform CI

Add a minimal `.github/workflows/ci.yml` or equivalently named workflow.

Required matrix/evidence:

### Linux stable

- cargo fmt --all -- --check
- cargo check --workspace --all-targets --locked
- cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
- cargo test --workspace --all-features --locked
- descendant cleanup integration test executes

### Linux MSRV

- Rust 1.89.0
- cargo check --workspace --all-targets --locked
- run focused core tests if inexpensive

### macOS stable

- cargo check/clippy/tests
- real descendant cleanup integration test
- cwd symlink-confinement tests

### Windows stable

- cargo check --workspace --all-targets --locked
- cargo clippy where supported
- core tests
- runner non-managed/platform-capability tests
- managed execution must report unsupported rather than fail to compile

Keep CI bounded. Do not add release packaging, coverage, fuzzing, or benchmark jobs in this corrective.

## 9. Ordered work package F — Closure erratum and docs

Add:

- `plans/closure/local-runner-lifecycle/001-errata.md`

It must record:

- invalid historical implementation SHA:
  `9387a45e1bbf9c1f9a55fb8ad875b07f6f880d21`
- actual implementation SHA:
  `9387a459103078c1ccdbca7d4db41ae0f6cefc11`
- historical documentation closure SHA:
  `319b5816604e39578af4e20f5880945123b042e1`
- no change to the substantive M001 requirement matrix from the SHA correction itself.

Do not edit the old closure record merely to make the typo disappear.

Update:

- `docs/local-runner-lifecycle.md`;
- architecture docs if needed;
- registry links/status;
- platform matrix;
- environment semantics;
- cwd confinement semantics;
- executable resolution rule.

## 10. Interaction with the foundation status corrective

This plan was reviewed after Foundation corrective C001 closed. `BundleWriter::finalize` now takes `ExecutionStatus` and an optional `ComparisonVerdict`; the lifecycle-only call site has already migrated and records completed/no-comparison. No additional comparison concept belongs in this corrective.

The runner's lifecycle-only bundle test must use the new separated execution-status/comparison-verdict API.

Do not reintroduce a mixed execution/comparison status type. `LegacyRunStatus` is read-only manifest-v1 provenance and must not be used for new writes.

If the foundation corrective materially changes bundle writer arguments, update this plan's code touch points while preserving all lifecycle invariants.

## 11. Focused tests

Required focused regression set:

- cwd direct in-root;
- cwd lexical escape;
- cwd absolute escape;
- cwd symlink outside root;
- nested symlink outside root;
- in-root symlink declared behavior;
- missing cwd;
- cwd is file;
- ambient env sentinel absent;
- explicit env value present;
- secret absent from Debug/errors/evidence;
- bare program rejected;
- absolute program accepted;
- cwd-relative program accepted;
- dependency order;
- readiness success/failure/timeout;
- cancellation before and after spawn;
- graceful shutdown;
- forced descendant cleanup;
- cleanup failure preserving primary failure;
- bounded log draining;
- external services not owned;
- platform support matrix;
- current lifecycle evidence records completed/no-comparison under corrected core schema.

## 12. Verification

Required locally:

    cargo fmt --all -- --check
    cargo check --workspace --all-targets --locked
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
    cargo test --workspace --all-features --locked
    cargo +1.89.0 check --workspace --all-targets --locked
    cargo tree --locked
    git diff --check

Required hosted evidence:

- green Linux stable workflow;
- green Linux MSRV workflow/job;
- green macOS lifecycle qualification before macOS is marked supported;
- green Windows compile/core/platform behavior.

If macOS CI is unavailable or fails for infrastructure-only reasons, do not mark macOS supported. Close conditionally or leave it unqualified.

## 13. Acceptance

C001 closes when:

1. symlink-based cwd escape is impossible under the documented pre-spawn confinement model;
2. the process environment is explicitly hermetic and tested;
3. ambient PATH lookup is not required for managed execution;
4. supported platform labels match actual hosted evidence;
5. Unix-only dependencies are appropriately target-scoped;
6. existing lifecycle correctness tests remain green;
7. the M001 bad-SHA erratum is committed and registered;
8. lifecycle evidence uses the corrected foundation status/verdict contract;
9. Local Runner M002 can safely build on the resulting session.

Closing this corrective makes Local Runner M002 planning dependency-ready.

## 14. Stop conditions

Stop for planning review if:

- race-free filesystem confinement would require a sandbox/chroot architecture beyond local-runner scope;
- real platform behavior shows macOS process-group cleanup is materially different from the assumed Unix design;
- Windows compilation requires a broad runner rewrite;
- environment requirements force an immediate plan-schema revision;
- the foundation status corrective is not closed.

## 15. Closure evidence required

Record:

- implementation commit;
- foundation corrective baseline;
- cwd path/symlink test matrix;
- executable-resolution test matrix;
- environment hermeticity/redaction evidence;
- platform support table;
- hosted workflow run identifiers/results;
- dependency-tree target scoping;
- closure erratum path;
- broad verification results;
- known limitations/TOCTOU statement;
- disposition.
