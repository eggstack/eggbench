# Local Runner M003 — Environment Fingerprint and CLI Lifecycle

Status: ready for handoff

Repository baseline: `9d143584c18cc3d7a49e0271f452587736b118d9`

Source roadmap:

- `plans/subsystems/local-runner-lifecycle-roadmap.md` — M003

Closed prerequisites:

- Local Runner M001 and its platform/filesystem/environment corrective;
- Local Runner M002;
- M002 post-closure evidence-safety corrective C001 at `6900212a5997b3d776e824b115d8ad35d36cd431`;
- hosted corrective qualification CI run `35797812233` green on Linux stable, Linux Rust 1.89, macOS stable, and Windows supported subset.

Controlling ADRs:

- `plans/adrs/ADR-0001-typed-core-and-driver-boundaries.md`
- `plans/adrs/ADR-0002-immutable-evidence-and-testbed-provenance.md`
- `plans/adrs/ADR-0005-local-first-execution-and-remote-provider-boundary.md`

Long-term requirements:

- `plans/000-long-term-specification.md#13-environment-and-testbed-model`
- `plans/000-long-term-specification.md#17-library-and-cli-surface`
- `plans/000-long-term-specification.md#18-crate-architecture`
- `plans/000-long-term-specification.md#19-portability-and-toolchain`
- `plans/000-long-term-specification.md#21-compatibility-and-versioning`

Primary class: capability/infrastructure.

## 1. Objective

Complete the first local-runner vertical slice around the already-correct M002 execution engine:

1. collect a truthful, bounded local `EnvironmentFingerprint`;
2. prepare canonical source-plan/resolved-plan/environment/subject evidence before startup;
3. add the initial `eggbench` CLI surface:
   - `validate`;
   - `doctor`;
   - `run`;
   - `inspect`;
4. provide stable machine-readable output with stderr-only human progress/diagnostics;
5. preserve explicit capability failure when no production workload adapter is compiled.

M003 must not invent a fake production workload merely to make `eggbench run` appear useful. The CLI/run wiring may be qualified with injected/test-only adapters; real workload adapters remain External Oracles/Eggstack Integration work.

## 2. Current repository evidence

At the baseline:

- workspace crates are only `eggbench-core` and `eggbench-runner`;
- `EnvironmentFingerprint` schema v1 already exists in core as a bounded map of:
  - field name;
  - string value;
  - `EnvironmentFieldClass::{ComparisonCritical, WarningOnly, Informational}`;
- `BundleWriter` already requires source plan, resolved plan, and environment artifacts before finalization;
- M002 callers currently pre-stage those required artifacts manually;
- `BundleReader` safely opens/verifies manifest v1/v2 bundles and exposes legacy-status ambiguity;
- `ResolvedPlan` contains selected driver descriptors/capabilities and subject identity;
- managed executable paths are explicit and filesystem-resolved;
- M002 exposes `execute_run` but no production driver registry exists;
- there is no CLI crate and no environment collector;
- Windows managed execution remains intentionally unsupported.

## 3. Required invariants

1. Environment collection occurs before measured work.
2. Environment collection failure cannot leave managed processes running because startup has not begun.
3. Environment values are non-secret and bounded.
4. Dynamic host state is not mislabeled as stable same-testbed identity.
5. Subject identity is not conflated with testbed identity: candidate/baseline binaries are expected to differ.
6. Missing optional host facts remain missing/diagnostic; they are never fabricated.
7. The CLI is a presentation adapter over core/runner contracts, not a second execution engine.
8. JSON stdout contains only the declared machine result; progress/warnings/logs go to stderr.
9. Unsupported workload/service/platform capability fails before managed startup where possible.
10. `inspect` verifies immutable evidence before presenting it by default.
11. Existing manifest v2, trial-result v1, M002 timing, and cleanup contracts remain unchanged.
12. No shell interpolation or ambient-PATH dependency is introduced.
13. Windows must compile and provide validate/doctor/inspect even though managed `run` remains unsupported.

## 4. Explicit non-goals

Do not add:

- Eggfetch/oha/h2load/iperf3 or another production workload adapter;
- metric normalization or comparison;
- Gregg live telemetry;
- automatic Git repository discovery with a heavy Git implementation;
- remote execution;
- daemon/service mode;
- automatic package updating;
- TUI;
- Web UI;
- Windows Job Objects;
- a hidden SSH path;
- secret values in environment artifacts or CLI JSON;
- mutable history/database indexing.

## 5. Crate boundary

Add one new workspace crate:

~~~text
crates/eggbench-cli
~~~

Package binary name: `eggbench`.

Expected dependencies:

- `eggbench-core`;
- `eggbench-runner`;
- a small CLI parser such as `clap`;
- Serde/serde_json;
- Tokio only because `run` calls the async runner.

Do not create `eggbench-drivers` in M003 merely to hold an empty registry or a fake adapter.

A library module inside `eggbench-cli` SHOULD hold command execution/presentation DTOs so integration tests can call command logic without spawning subprocesses for every case. The binary should remain thin.

## 6. Environment collector ownership

Add local host collection to `eggbench-runner` behind a narrow API such as:

~~~text
LocalEnvironmentCollector
  collect() -> EnvironmentFingerprint
~~~

Core continues to own only the versioned DTO/validation.

The collector must not execute arbitrary shell strings.

Prefer std/filesystem/platform APIs. Adding one mature cross-platform system-information dependency is permitted only if it materially reduces platform-specific unsafe/system-command machinery and its dependency tree is reviewed in closure. Do not add unsafe Rust to Eggbench itself.

## 7. Environment fingerprint v1 field policy

Keep `EnvironmentFingerprint` schema version 1 if the existing map contract can represent the required fields. Do not create schema v2 merely because the placeholder now has real producers.

Use stable field names. Initial required/attempted fields:

### Comparison-critical when available

- `os_family`;
- `architecture`;
- `target_family` or equivalent build target class;
- `cpu_model`;
- `logical_cpu_count`;
- `physical_cpu_count` when reliable;
- `total_memory_bytes`;
- `kernel_release` where available.

A missing optional critical field does not get a placeholder string such as `unknown`; absence must remain absence. The comparator can later decide whether absence invalidates strict comparison.

### Warning-only

- `os_version`;
- `hostname` only if explicitly considered useful and privacy-safe; default preference is to omit it;
- `current_cpu_frequency_hz` when reliably available;
- lightweight pre-run load/memory-pressure indicators if collected.

Do not make transient load/frequency values comparison-critical.

### Informational

- `eggbench_version`;
- Rust target/build information available at compile time;
- build profile when reliably known;
- collector implementation/version.

Do not put subject revision/digest into comparison-critical environment fields: candidate and baseline subjects are intentionally different.

## 8. Subject evidence

Add a bounded versioned subject snapshot artifact, preferably `subject.json` with `ArtifactRole::Subject`.

For a managed command, record when available:

- resolved executable path in a portable/redaction-safe representation;
- executable SHA-256;
- plan/resolved revision string;
- plan/resolved declared digest;
- whether a declared digest matches the observed executable digest.

Do not automatically crawl parent directories for Git metadata in M003.

For external/label subjects, preserve declared identity without fabricating a binary digest.

If a declared digest mismatch is detected before startup, `doctor` and `run` must fail closed with a stable diagnostic category.

## 9. Driver/version provenance

Do not duplicate driver descriptors into environment fields. `ResolvedPlan.drivers` and `BundleManifest.drivers` already own selected adapter/upstream version provenance.

`doctor` should display/emit that inventory and flag:

- unresolved required categories;
- executable path missing;
- unsupported platform;
- declared capability unavailable.

Exact external-tool probing belongs to the later driver milestone.

## 10. Bundle-preparation API

Remove CLI-specific boilerplate by adding a runner-level pre-start helper with semantics equivalent to:

~~~text
prepare_bundle(
  destination,
  source_plan,
  resolved_plan,
  environment_fingerprint,
  subject_snapshot,
  bounds
) -> BundleWriter
~~~

The helper should:

- create the writer;
- serialize/stage source plan;
- serialize/stage resolved plan;
- serialize/stage environment;
- stage subject snapshot;
- validate all typed artifacts;
- return the still-unpublished writer to M002 `execute_run`.

All of this happens before managed startup.

Do not merge bundle preparation into `execute_run` if doing so would make runner execution depend on source file syntax or CLI concerns.

## 11. CLI input parsing

Support plan input by explicit or unambiguous format:

- `.toml` -> TOML;
- `.json` -> JSON;
- optionally `--input-format toml|json` for stdin/nonstandard extension.

Reject ambiguous unknown extensions rather than guessing based on content.

Use the existing `ExperimentPlan::from_toml/from_json` contract.

Plan source bytes used for evidence must be the actual input content or a clearly documented canonical serialization. Prefer retaining the exact source bytes as the source-plan artifact and separately storing canonical resolved JSON.

## 12. `eggbench validate <plan>`

Responsibilities:

- parse;
- schema-version check;
- semantic `ExperimentPlan::validate`;
- no driver resolution;
- no filesystem/process startup;
- no environment collection beyond what the parser requires.

Machine result should include:

- output schema version;
- command;
- valid boolean;
- experiment name when parse succeeds;
- stable error category/detail when invalid.

Exit:

- 0 valid;
- nonzero invalid.

## 13. `eggbench doctor <plan>`

Responsibilities:

- all validate behavior;
- resolve against the compiled/registered driver catalog;
- collect local environment;
- check current platform support;
- validate managed cwd/executable/secret references without spawning;
- verify required reset/readiness/workload capability availability as far as known;
- verify subject declared digest when applicable;
- report driver inventory and capability gaps.

Doctor must not start services or run workload traffic.

If the default binary contains no matching production workload adapter, doctor must say so explicitly. That is not a reason to register the M002 fake as production.

## 14. `eggbench run <plan>`

Run command pipeline:

~~~text
parse
 -> validate
 -> resolve
 -> doctor/preflight
 -> collect environment + subject snapshot
 -> prepare BundleWriter
 -> LocalSession::prepare
 -> select registered WorkloadExecutor/reset hooks
 -> execute_run
 -> machine/human result
~~~

The command must accept an explicit output bundle destination.

M003 binary behavior with no production adapter:

- resolution/dispatch fails explicitly before managed startup;
- JSON mode reports a stable unsupported-capability result;
- no partial final bundle is published.

Qualification may inject the deterministic M002 fake through an internal/test-only command runtime to prove the complete CLI plumbing. Do not expose a default `--fake-workload` mode in production help.

Operational run failure that still produces truthful evidence should:

- report the bundle path;
- report `ExecutionStatus`;
- return a documented nonzero exit code;
- not discard the bundle.

Evidence/preflight errors with no finalized bundle use a separate exit category.

## 15. `eggbench inspect <bundle.eggb>`

Default behavior:

1. open via `BundleReader`;
2. verify digests/path/symlink/finalization invariants;
3. emit a stable summary.

Include:

- manifest schema;
- run ID;
- execution status;
- comparison verdict if present;
- explicit legacy v1 ambiguity when applicable;
- subject;
- driver inventory;
- environment fingerprint summary;
- trial IDs/result statuses;
- artifact count/bytes;
- comparison/report artifact references if present.

Offer an explicit option to emit the normalized manifest JSON, but do not bypass verification by default.

Do not interpret performance metrics before Measurement/Comparison M001 lands.

## 16. Machine-output contract

Define a small versioned CLI envelope, e.g.:

~~~text
schema_version
command
ok
result
error
warnings
~~~

Requirements:

- JSON mode emits exactly one JSON document to stdout;
- no ANSI/progress/log noise on stdout;
- stderr may contain human diagnostics unless a quiet flag suppresses them;
- errors have stable category strings plus human detail;
- paths are serialized as strings only when UTF-8 representable, otherwise return a typed presentation error rather than lossy corruption;
- secrets never appear.

Do not promise long-term stability for human prose; machine fields are the compatibility surface.

## 17. Exit-code contract

Define and document a compact stable mapping. Preferred categories:

- 0: requested command completed successfully;
- 2: parse/schema/plan validation error;
- 3: capability/doctor/preflight unsupported or invalid;
- 4: run completed with `ExecutionStatus::Failed|Cancelled|Invalid` and a valid bundle exists;
- 5: evidence/bundle I/O or verification failure;
- 1: internal/unclassified CLI failure.

Exact numbers may differ if documented before implementation, but tests must lock the mapping.

## 18. Cancellation and signals

For `run`:

- Ctrl-C/SIGINT maps to the existing M002 `CancellationToken`;
- first signal requests normal cancellation and mandatory cleanup;
- do not implement a second “force kill everything immediately” signal path in M003;
- JSON result after a clean cancellation should include the finalized cancelled bundle when M002 can produce it.

Platform signal support must be feature/cfg correct.

## 19. Windows behavior

On Windows:

- `validate`, `doctor`, and `inspect` are supported;
- environment collection must return truthful available fields;
- a managed-subject/service `run` fails before spawn with existing unsupported platform semantics;
- plans containing only externally managed topology still require a real workload adapter; absence remains explicit capability failure.

Do not emulate Unix process ownership.

## 20. ARM/SBC considerations

Environment collection must avoid:

- large fixed buffers;
- expensive recursive filesystem scans;
- mandatory external commands;
- assumptions that x86 model strings exist.

Linux ARM64 should produce useful OS/arch/core/memory fields when the platform exposes them.

No prebuilt binary-release work is required in M003.

## 21. Security/privacy

- do not collect environment variables wholesale;
- do not record usernames/home directories by default;
- do not record hostname by default unless explicitly accepted in implementation review;
- no network interface MAC/IP inventory in M003 unless the plan is amended;
- subject secret refs remain refs only;
- hash executables with bounded streaming;
- inspect never follows symlinks because BundleReader already forbids them.

## 22. Focused tests

### Environment

- deterministic field classification;
- missing optional field remains absent;
- all field values pass schema bounds;
- Linux collector fixture/parsing;
- macOS collector behavior;
- Windows supported collector subset;
- ARM-like missing CPU model does not fabricate a value;
- transient load/frequency never comparison-critical;
- no secrets/environment dump.

### Subject

- managed executable digest stable;
- declared digest match/mismatch;
- external subject has no fabricated executable digest;
- large executable hashing uses bounded buffers.

### Bundle preparation

- required four primary artifacts stage correctly;
- failure occurs before startup;
- prepared writer finalizes successfully through injected M002 fake;
- source plan bytes/provenance policy is deterministic.

### CLI

- validate TOML/JSON good/bad;
- unknown extension rejected;
- doctor starts no process;
- unsupported adapter is explicit;
- inspect verifies bundle;
- corrupted/symlink bundle rejected;
- JSON stdout parses as exactly one document;
- diagnostics stay off stdout;
- stable exit-code cases;
- Ctrl-C routes through M002 cleanup in Unix integration test;
- valid failed/cancelled run reports finalized bundle.

## 23. Cross-platform qualification

Linux stable:

- full CLI/environment tests;
- injected fake end-to-end `run`;
- signal cancellation;
- subject executable hashing.

Linux Rust 1.89:

- workspace check plus core/CLI contracts compatible with MSRV.

macOS stable:

- full supported CLI/environment suite;
- injected fake run using qualified process-group lifecycle;
- cancellation/cleanup.

Windows stable:

- workspace check/clippy;
- validate/doctor/inspect;
- environment collector;
- managed run explicitly unsupported.

## 24. Broad verification

Required:

    cargo fmt --all -- --check
    cargo check --workspace --all-targets --locked
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
    cargo test --workspace --all-features --locked
    cargo +1.89.0 check --workspace --all-targets --locked
    cargo tree --locked
    git diff --check

Hosted CI must remain green on the existing four lanes.

## 25. Documentation

Add/update:

- `docs/environment-fingerprint.md`;
- `docs/cli.md`;
- `architecture/runner.md`;
- `architecture/core.md` if subject/fingerprint ownership needs clarification;
- README command examples;
- roadmap/registry on closure.

Document explicitly that M003 ships the CLI execution substrate but does not ship a production traffic generator.

## 26. Acceptance criteria

M003 closes only when:

1. a real local environment fingerprint is collected and validated on supported CI platforms;
2. stable field classes distinguish testbed identity from warning/informational context;
3. subject executable digest evidence is available without conflating candidate identity with environment equality;
4. source plan, resolved plan, environment, and subject evidence can be prepared before startup through a reusable runner API;
5. `validate`, `doctor`, `run`, and `inspect` command surfaces exist;
6. machine JSON/stdout and stderr discipline is regression-tested;
7. CLI run delegates to M002 rather than duplicating orchestration;
8. unsupported workload capability fails explicitly before startup;
9. an injected/test-only adapter proves end-to-end run/bundle plumbing without shipping a fake production driver;
10. Windows managed execution remains truthfully unsupported;
11. existing M001/M002/corrective tests remain green;
12. no manifest/trial schema change is required;
13. hosted qualification is green.

Closing M003 completes the first local-runner roadmap. It does not unblock Eggstack integrations by itself; Measurement/Comparison M001 must also close.

## 27. Stop conditions

Stop for planning review if:

- a useful fingerprint requires schema v2 rather than additive v1 fields;
- CLI run requires embedding a fake or protocol-specific workload into the production binary;
- correct subject provenance requires a heavy Git dependency;
- environment collection requires unsafe code inside Eggbench;
- the change would alter M002 measurement/cancellation/cleanup semantics;
- Windows support would require implementing process ownership rather than reporting the existing capability limit.

## 28. Closure evidence required

Record:

- implementation commits;
- environment field/class table from landed code;
- subject snapshot schema/example;
- bundle-preparation API;
- CLI command/help surface;
- machine JSON examples;
- exit-code table;
- unsupported-driver behavior;
- signal/cancellation evidence;
- end-to-end injected-adapter bundle verification;
- platform test matrix;
- dependency tree;
- Rust 1.89 result;
- hosted CI run ID;
- known limitations;
- unresolved findings/severity;
- disposition.
