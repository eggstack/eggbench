# External Oracles M001 — External Command Driver Substrate

Status: ready for handoff

Repository baseline: `fde253dfcbc7b9263cee3df5802c84fa92b96e2a`

Source roadmap:

- `plans/subsystems/external-oracles-roadmap.md` — M001

Closed prerequisites:

- typed driver descriptors/resolution;
- qualified Local Runner M003 command substrate;
- Measurement M001 normalized metric seam;
- post-M003/M001 corrective C001.

Controlling ADR:

- `plans/adrs/ADR-0004-eggstack-composition-and-independent-oracles.md`

Long-term requirements:

- `plans/000-long-term-specification.md#15-external-driver-policy`
- `plans/000-long-term-specification.md#18-crate-architecture`
- `plans/000-long-term-specification.md#19-portability-and-toolchain`
- `plans/000-long-term-specification.md#20-resource-and-harness-overhead`

Primary class: infrastructure.

## 1. Objective

Create the shared production driver crate and a secure, bounded, testable command-adapter substrate for optional external benchmark tools.

M001 does **not** implement oha, h2load, iperf3, or netem. It establishes the reusable machinery those adapters and later Eggstack drivers consume.

The milestone must deliver:

1. `crates/eggbench-drivers`;
2. production driver-catalog ownership outside the CLI crate;
3. trusted executable resolution;
4. binary identity/version probing;
5. argv-only command execution;
6. bounded stdout/stderr capture;
7. cancellation/timeout cleanup;
8. raw-output artifact helpers;
9. versioned parser/error contracts;
10. deterministic fixture executables and cross-platform qualification.

## 2. Current repository evidence

At the baseline:

- `eggbench-core` owns `DriverDescriptor`, capability selection, normalized metric DTOs, and evidence schemas;
- `eggbench-runner` owns workload execution/cancellation and process lifecycle;
- `eggbench-cli` currently owns `WorkloadRegistry`, `ProductionRuntime`, and `QualificationRuntime`;
- production registry is correctly empty;
- qualification fake execution is isolated from the production binary;
- `WorkloadExecutor` accepts protocol-neutral `WorkloadOutput`;
- no `eggbench-drivers` crate exists despite the long-term architecture;
- there is no reusable external executable resolver/version probe/command capture abstraction.

This is now a demonstrated ownership boundary: adapters no longer belong in CLI presentation code.

## 3. Invariants

1. CLI owns presentation, not driver implementation/registration.
2. Optional external tools never become mandatory Eggbench runtime dependencies.
3. No implicit shell is used.
4. An executable selected from an untrusted working directory is never silently accepted.
5. Exact executable path, canonical target, digest, and version are inspectable.
6. PATH discovery never treats an empty/relative PATH component as trusted search space.
7. Version probing is bounded and cancellable.
8. Measured command execution has explicit timeout/cancellation and cleanup semantics.
9. Stdout/stderr are continuously drained and bounded.
10. Truncation is explicit; output is never silently presented as complete.
11. Raw machine output is retained before semantic normalization.
12. Driver-specific parsers are versioned and fixture-testable.
13. A parser failure cannot mutate the invocation into a different load/protocol semantic.
14. External command failures remain source-attributed and redaction-safe.
15. Minimal Eggbench builds remain useful with zero external binaries installed.
16. No unsafe Rust is added to Eggbench.

## 4. Non-goals

Do not implement:

- oha/h2load/iperf3 mappings;
- tc/netem;
- a package installer;
- automatic binary downloads;
- shell script execution;
- arbitrary user shell commands as benchmark drivers;
- remote execution;
- plugin discovery;
- dynamic library loading;
- Measurement M002 statistics;
- EggServe/Eggfetch/Gregg integration semantics;
- a general-purpose process supervisor.

## 5. Create `eggbench-drivers`

Add:

~~~text
crates/eggbench-drivers
  src/lib.rs
  src/catalog.rs
  src/external/
    mod.rs
    resolver.rs
    version.rs
    command.rs
    parser.rs
    artifact.rs
    error.rs
~~~

Workspace package uses Rust 1.89 and inherited lint policy.

Core dependencies should remain narrow:

- `eggbench-core`;
- `eggbench-runner` only where runner-owned lifecycle/types are reused;
- Tokio/process support;
- SHA-256 through the existing core utility or `sha2` if a small public core helper would be inappropriate;
- Serde only for versioned substrate DTOs that become evidence.

Do not add HTTP/network protocol dependencies in M001.

## 6. Move production catalog ownership out of CLI

Move the production driver inventory/runtime abstraction from `eggbench-cli::workload_registry` into `eggbench-drivers`.

Target shape:

~~~text
DriverCatalog
  descriptors()
  workload(name)
  service(name)
  telemetry(name)
~~~

M001 only needs the workload-facing production inventory to remain behaviorally equivalent, but the type should not preclude later service/telemetry adapters.

Requirements:

- production catalog is empty after M001;
- qualification fake remains test/qualification-only;
- `main.rs` never imports runner `test_support`;
- CLI asks `eggbench-drivers` for production descriptors;
- no fake driver is linked into a normal release path if cfg/feature isolation can avoid it.

Compatibility:

- preserve a narrow deprecated/re-export shim in `eggbench-cli` only if current public API tests show external consumers would otherwise break unexpectedly;
- do not maintain two authoritative registries.

## 7. Driver feature policy

The drivers crate must be cheap when no adapter is selected.

Preferred feature shape:

~~~text
default = []
external-command = [...]
# future:
# external-oha
# external-h2load
# external-iperf3
# eggstack-http
# gregg
~~~

The external command substrate MAY be enabled by the CLI when production external adapters land, but M001 itself must not register a fake real driver.

Feature tests should prove default builds contain no protocol client/server dependency.

## 8. Trusted executable resolver

Define an API equivalent to:

~~~text
BinaryResolver::resolve(
  requested_name,
  explicit_path?,
  search_path?
) -> ResolvedExecutable
~~~

`ResolvedExecutable` includes:

- logical tool name;
- selected path;
- canonical path;
- SHA-256;
- file size;
- platform execution classification.

### Explicit path

- must identify a regular executable file;
- relative explicit paths are rejected unless the caller explicitly resolves them against a trusted configured root before calling;
- canonicalize before identity/hash;
- symlink input MAY be accepted, but execute/hash the canonical target and record both selected and canonical paths;
- missing/non-file/non-executable fails explicitly.

### PATH search on Unix

Enumerate PATH components manually.

- skip empty components;
- skip relative components;
- no implicit current directory;
- candidate must be a regular file with executable mode bits;
- canonicalize and hash the selected target;
- first match in trusted absolute PATH order wins.

### PATH search on Windows

Do not use shell resolution.

- enumerate absolute PATH directories;
- support direct executable suffixes that `Command` can execute without a shell;
- initially permit `.exe` and, if qualified, `.com`;
- reject `.bat`/`.cmd` because they require command-interpreter semantics;
- never search current directory implicitly.

Record the resolution policy in docs/tests.

## 9. Executable identity hashing

Use bounded streaming reads.

Executable digest is diagnostic/provenance, not a trust-signature claim.

Hash rules:

- SHA-256;
- canonical target bytes;
- exact hex representation;
- no mmap requirement;
- errors are typed;
- no unbounded allocation.

The later driver descriptor/evidence path must be able to record this identity.

## 10. Version probe contract

Define:

~~~text
VersionProbeSpec {
  argv_tail
  timeout
  stdout_limit
  stderr_limit
  parser_id
}

VersionProbe::run(executable, spec) -> ToolVersion
~~~

`ToolVersion` includes:

- logical tool name;
- executable identity;
- raw bounded stdout/stderr;
- exit status;
- parsed version string;
- parser identifier/version.

Rules:

- executable path is argv[0];
- no shell;
- stdin null;
- timeout bounded;
- cancellation supported where caller supplies a token;
- nonzero exit is a typed probe failure unless adapter policy explicitly permits it;
- truncated output is visible to the parser;
- parser cannot pretend truncated output is complete unless its own fixture policy proves the required version token was retained.

## 11. External command specification

Define a protocol-neutral command spec:

~~~text
ExternalCommandSpec {
  executable: ResolvedExecutable
  args: Vec<OsString/String>
  cwd: Option<PathBuf>
  env: BTreeMap<OsString, OsString>
  stdout_limit: u64
  stderr_limit: u64
  grace: Duration
}
~~~

Hard rules:

- no shell string;
- no wildcard/glob expansion by Eggbench;
- no inherited arbitrary current directory;
- no implicit environment inheritance beyond narrowly documented platform-required variables;
- args are passed as argv;
- stdin defaults to null.

Adapter-specific environment needs must be explicit.

## 12. Environment policy

Preferred execution environment:

- `env_clear()`;
- explicit driver environment only;
- deterministic locale where supported, e.g. `LC_ALL=C`/`LANG=C` on Unix for human/version output parsers;
- preserve only narrowly required platform variables on Windows if direct process creation requires them.

Do not copy the user's entire environment.

Later tool adapters may explicitly add certificate/proxy/etc. references when their semantics require them, with redaction.

## 13. Bounded output capture

Define a bounded capture type analogous to runner logs:

~~~text
CapturedStream {
  retained: Vec<u8>
  retained_bytes
  dropped_bytes
  total_bytes
  truncated
}
~~~

Requirements:

- stdout/stderr drained concurrently;
- cap applies independently;
- draining continues after cap so the child cannot block on a full pipe;
- no UTF-8 assumption;
- no full-output clone solely for diagnostics;
- default/tool limits remain <= bundle per-artifact bounds once integrated.

## 14. Command outcome

Return a typed `ExternalCommandOutcome` containing:

- executable identity;
- argv metadata in redaction-safe form;
- exit status;
- captured stdout/stderr;
- monotonic execution duration;
- cancellation/timeout state;
- cleanup diagnostics.

Do not put secret-bearing argv/environment values into `Debug`/human error output.

## 15. Cancellation and timeout

Execution must observe:

- invocation cancellation token;
- explicit driver command timeout;
- child exit.

On cancellation/timeout:

1. request bounded termination;
2. ensure owned process cleanup is attempted;
3. continue draining pipes;
4. return a typed cancelled/timed-out outcome/failure.

### Unix

Use a dedicated process group and the already-qualified runner process-group semantics rather than creating a second signal implementation.

If necessary, extract a narrow reusable runner-owned helper for argv process-group spawn/termination and consume it from `eggbench-drivers`.

### Windows

No shell.

M001 may support direct-child cleanup using the owned `tokio::process::Child` handle while explicitly reporting descendant cleanup as `direct_child_only`.

Do not claim Job Object/process-tree semantics.

Tool-specific adapters that might spawn descendants must not advertise Windows support until that behavior is qualified.

## 16. Avoid duplicated process authority

If external-command execution needs runner process-group behavior, prefer a small reusable runner primitive over copying:

- process-group creation;
- TERM/KILL behavior;
- bounded pipe draining.

The primitive must remain lower-level than `LocalSession`; external benchmark tools are workload children, not topology services.

Do not make `eggbench-runner` depend on `eggbench-drivers`.

Dependency direction stays:

~~~text
core
  ^
runner
  ^
drivers
  ^
cli
~~~

where practical.

## 17. Raw artifact helper

Provide a helper that converts one command outcome into deterministic `WorkloadArtifact` candidates:

- `stdout.raw`;
- `stderr.raw`;
- optional small `command-metadata.json`.

Metadata includes:

- tool/version identity;
- executable digest;
- exit code;
- truncation counters;
- parser id where applicable.

Do not normalize tool-specific metrics in M001.

Artifact naming must be safe one-component names compatible with M002 staging.

## 18. Parser contract

Define a tool-parser contract independent of process spawning.

Representative shape:

~~~text
trait ExternalOutputParser {
  fn parser_id(&self) -> &'static str;
  fn parse(&self, outcome: &ExternalCommandOutcome)
      -> Result<ParsedExternalOutput, ExternalParseError>;
}
~~~

`ParsedExternalOutput` is still driver-specific/intermediate.

It may later map to:

- `RawMetricObservation`;
- `RawHistogramInput`;
- error-category counts;
- additional raw artifacts.

M001 should include one trivial fixture parser used only to qualify the contract, not a disguised oha parser.

Parser errors include:

- unsupported version;
- truncated required output;
- malformed machine output;
- missing required field;
- nonfinite/domain-invalid source data.

Do not parse human output when a future tool offers a machine format.

## 19. Timing boundary

M001 does not change M002's measured interval.

The command substrate records child wall duration independently.

Driver-specific adapters in External M002 must document whether parsing occurs:

- incrementally while the tool runs;
- immediately after exit;
- or in a future post-measurement hook.

Do not add expensive report rendering to the measured hot path.

If M002 adapters reveal that parser cost materially contaminates runner measured elapsed, stop and plan a generic post-execution parsing seam rather than hiding the overhead.

## 20. Driver error taxonomy

Add stable redaction-safe categories:

- binary_not_found;
- untrusted_search_path;
- not_executable;
- executable_identity_failed;
- version_probe_timeout;
- version_probe_failed;
- unsupported_version;
- spawn_failed;
- cancelled;
- timed_out;
- nonzero_exit;
- output_truncated;
- parse_failed;
- cleanup_failed.

Error detail can be human-readable but categories form the machine compatibility surface.

## 21. Fixture executable

Add a deterministic test fixture binary in the workspace/test tree that can:

- print version JSON/text;
- print controlled stdout/stderr byte counts;
- exit with selected code;
- sleep until cancelled;
- emit malformed/truncated payloads;
- optionally spawn a child on Unix for process-group cleanup tests.

Use this fixture rather than shell scripts so Windows qualification exercises the same argv-only contract.

Do not publish/install it as a user-facing driver.

## 22. Security tests

Required:

- explicit relative executable rejected;
- PATH with empty component cannot resolve cwd executable;
- PATH with relative component skipped/rejected;
- symlink canonical identity recorded;
- non-executable file rejected on Unix;
- Windows batch/cmd wrapper rejected;
- argv metacharacters remain literal arguments;
- environment not inherited wholesale;
- secret-like env values absent from Debug/errors;
- stdout/stderr truncation does not deadlock.

## 23. Lifecycle tests

Required:

- successful exit;
- nonzero exit;
- timeout;
- cancellation;
- output larger than cap;
- child emits stdout+stderr concurrently;
- Unix descendant cleanup through process group;
- cleanup failure retains primary failure;
- Windows direct-child semantics explicit.

## 24. Catalog/CLI regressions

- production catalog remains empty;
- qualification fake remains isolated;
- `doctor` with no adapters remains truthful;
- production `run` still fails before startup with no adapter;
- CLI no longer owns authoritative driver registry;
- no new public fake-workload route appears.

## 25. Cross-platform qualification

Linux stable:

- resolver/path hardening;
- version probe;
- command lifecycle;
- process-group cancellation/descendant cleanup;
- full workspace tests.

Linux Rust 1.89:

- workspace check;
- drivers crate tests that are MSRV-safe.

macOS stable:

- trusted PATH resolver;
- executable mode;
- version/capture/cancellation;
- Unix process-group cleanup.

Windows stable:

- explicit/PATH `.exe` resolution;
- no cwd implicit lookup;
- no `.cmd`/`.bat` shell semantics;
- bounded capture;
- direct-child cancellation;
- all-target Clippy.

## 26. Documentation

Add/update:

- `architecture/drivers.md`;
- `docs/external-drivers.md`;
- `docs/driver-capabilities.md`;
- CLI architecture docs showing catalog ownership;
- README status;
- external-oracles roadmap/registry.

Document that no actual external benchmark adapter ships in M001.

## 27. Broad verification

Required:

    cargo fmt --all -- --check
    cargo check --workspace --all-targets --locked
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
    EGGBENCH_PARENT_SENTINEL_26CE=set-in-parent-process cargo test --workspace --all-features --locked
    cargo +1.89.0 check --workspace --all-targets --locked
    cargo tree --locked
    git diff --check

Hosted four-lane CI must remain green.

## 28. Acceptance criteria

M001 closes only when:

1. `eggbench-drivers` exists as the sole production adapter/catalog ownership crate;
2. CLI consumes rather than owns production driver registration;
3. minimal production catalog remains empty;
4. explicit executable resolution is safe and deterministic;
5. PATH discovery excludes implicit/untrusted cwd behavior;
6. executable SHA-256 and version provenance are captured;
7. version probes are argv-only, bounded, and fixture-tested;
8. command stdout/stderr are concurrently drained and bounded;
9. cancellation/timeout cleanup is deterministic on qualified platforms;
10. Windows semantics are explicit and do not claim descendant ownership;
11. raw command output can be retained as workload artifacts;
12. parser contract is versioned and independent of spawning;
13. no oha/h2load/iperf3/netem behavior is implemented;
14. no unsafe Rust or shell interpolation is introduced;
15. full hosted CI is green.

Closing External Oracles M001 unblocks:

- External Oracles M002 tool adapters;
- Eggstack Integration M001 plans that consume the shared `eggbench-drivers` crate without creating a competing registry.

## 29. Stop conditions

Stop for planning review if:

- cross-platform command cleanup requires a second full process supervisor;
- a useful resolver requires shell semantics;
- tool identity cannot be recorded without exposing secrets;
- driver catalog migration requires an incompatible CLI machine-schema change;
- Windows support would require unsafe Job Object code inside Eggbench;
- raw-output retention cannot fit existing workload/evidence bounds without changing bundle schemas.

## 30. Closure evidence required

Record:

- implementation commits;
- final crate/dependency graph;
- catalog migration evidence;
- resolver policy examples;
- executable identity/version example;
- bounded command outcome example;
- cancellation/timeout cleanup evidence;
- raw-artifact example;
- parser contract/fixture example;
- security-negative test matrix;
- platform qualification matrix;
- dependency tree;
- Rust 1.89 result;
- hosted CI run ID;
- known limitations;
- unresolved findings/severity;
- disposition.
