# CLI command implementations

## Purpose

How the five `eggbench` commands turn a user request into a core or runner
call and then into a `PresentedCommandResult`. This document covers
`crates/eggbench-cli/src/commands/` only: the per-command control flow, the
orchestration seams each command delegates to, and the decisions encoded in the
wiring.

Argv parsing, envelope schema, and the exit-code table are owned by
[cli-surface.md](cli-surface.md). Driver-inventory construction and adapter
selection are owned by [cli-workload-registry.md](cli-workload-registry.md).
This file answers: does each command stay a presentation adapter, and what
should a reviewer check when it changes.

## Source layout

| File | Lines | Owns | Does not own |
|---|---|---|---|
| `commands/mod.rs` | 7 | Module list: `compare`, `doctor`, `inspect`, `r#run`, `validate` | Dispatch — that is `main.rs` |
| `commands/validate.rs` | 24 | Parse + validate only | Resolution, environment, bundle |
| `commands/compare.rs` | 188 | Mode selection, receipt emission, verdict→exit mapping | Comparison policy, statistics |
| `commands/inspect.rs` | 679 | Read-only bundle verification and summary projection | Bundle writing, normalization |
| `commands/doctor.rs` | 713 | Validate + resolve + preflight reporting | Managed startup, workload execution |
| `commands/run.rs` | 714 | Full pipeline wiring, preflight ordering, cancellation seam | Trial schedule, phase transitions, metric writing |

`run.rs` and `doctor.rs` each carry their own private copies of
`workload_load_mode` (`run.rs:652`, `doctor.rs:676`) and their own
`ResolutionOptions` construction (`run.rs:551`, `doctor.rs:141`). See
[Review focus](#review-focus).

## Key types

All five commands have the signature
`fn(...) -> Result<PresentedCommandResult, CliError>`
(`validate.rs:10`, `compare.rs:26`, `inspect.rs:18`, `doctor.rs:35`,
`run.rs:74`). The envelope type, `ExitCode`, and the four result constructors
are defined in [cli-surface.md](cli-surface.md); what the commands actually
build is:

| Type | Where | Role |
|---|---|---|
| `PlanInput` | `plan_input.rs:15` | Raw bytes + detected format + validated `ExperimentPlan`. Raw bytes are the canonical source for the staged `plan.json` |
| `RunPlan<'a>` | `run.rs:320` | Production/qualification bridge: the loaded plan plus the destination bundle path |
| `PresentedCommandResult` | `envelope.rs:240` | Envelope plus the numeric exit code. Constructed inside the command, serialized only by `main.rs` |
| `RunOutcome` | runner | Returned by `execute_run_with_diagnostics`; carries `manifest`, `bundle_path`, `execution_status`, `primary_failure`, `telemetry_preflight_failure` |
| `CliOutput::*` | `envelope.rs:47` | Per-command payload variants (`Validate`, `Doctor`, `Run`, `Inspect`, `Compare`). The compatibility surface |
| Doctor summary projections | `doctor.rs:234-259` | `DriverSummary`, `EnvironmentSummary`, `NetworkPathDoctorSummary`, `DiagnosticsDoctorSummary`, `SecurityDoctorSummary` are flat string projections of typed core/runner values |

The CLI never holds an orchestration type. Its only long-lived local is the
`CancellationToken` created in `run_impl` (`run.rs:442`).

## How it works

### Per-command pipeline

| Stage | `validate` | `doctor` | `run` | `inspect` | `compare` |
|---|---|---|---|---|---|
| Load argv-provided plan | `load_plan` `validate.rs:14` | `load_plan` `doctor.rs:46` | `load_plan` `run.rs:88` | — | — |
| Capability/network preflight | — | `doctor.rs:115`, `:187` | `run.rs:89`, `:99` | — | — |
| Resolve against driver inventory | — | `resolve_plan` `doctor.rs:219` | `resolve_plan` `run.rs:114` | — | — |
| Construct workload executor | — | — | `production_workload_executor` `run.rs:123` | — | — |
| External tool preflight | — | version probe only, `doctor.rs:462`, `:552` | probe `run.rs:156`, replay `:175`, eggprobe `:199`, eggsec `:227` | — | — |
| Collect environment | — | `LocalEnvironmentCollector::collect` `doctor.rs:222` | `collect_local_environment` `run.rs:366` | — | — |
| Declared-vs-observed subject digest | — | — | `run.rs:376` | — | — |
| Session prepare | — | — | `LocalSession::prepare` `run.rs:398` | — | — |
| Stage bundle | — | — | `prepare_bundle` `run.rs:408` | — | — |
| Telemetry registry | — | config syntax only `doctor.rs:270` | `production_telemetry_registry` `run.rs:432` | — | — |
| Execute | — | — | `execute_run_with_diagnostics` `run.rs:447` | — | — |
| Verify evidence | — | — | — | `BundleReader::open` + `verify` `inspect.rs:19` | `load_*_bundle` `compare.rs:59-67` |
| Policy evaluation | — | — | — | — | `compare` `compare.rs:78` / `compare_paired` `compare.rs:90` |
| Emit artifact | — | — | bundle (runner-owned) | — | receipt `compare.rs:170` |
| Present | `validate.rs:17` | `doctor.rs:296` | `run.rs:515` | `inspect.rs:124` | `compare.rs:125` |

### `run` — actual order vs the documented pipeline

The module doc states the pipeline as `parse → validate → resolve →
doctor/preflight → collect environment + subject snapshot → prepare
BundleWriter → LocalSession::prepare → WorkloadExecutor/reset hooks →
execute_run → machine/human result` (`run.rs:3-6`). The production path in
`run()` actually runs these 24 steps, and it diverges from that list in four
ways:

1. **The executor is built before the environment is collected.** The workload
   executor is constructed from the *resolved* driver at `run.rs:122-145`, well
   before `collect_local_environment` at `run.rs:366`, whereas the doc places
   "WorkloadExecutor" hooks after `LocalSession::prepare`. This is deliberate:
   executor construction is the fail-closed gate for an unsupported workload
   (`run.rs:117-118` calls it "defense in depth"), and it must not be reached
   after staging.
2. **`LocalSession::prepare` runs before `prepare_bundle`** (`run.rs:398` then
   `run.rs:408`); the doc lists `prepare BundleWriter` first. The ordering
   guarantee still holds, because `LocalSession::prepare` starts no process —
   the runner reserves spawning for `LocalSession::startup`
   (`crates/eggbench-runner/src/session.rs:290`). So no managed startup can
   precede evidence staging; only the doc's stage order is wrong.
3. **The doc names `execute_run`; the code calls
   `execute_run_with_diagnostics`** (`run.rs:447`). The plain `execute_run` is
   not called anywhere in the CLI.
4. **The doc omits four real stages**: the managed-executable path resolution
   (`run.rs:353`), the declared-vs-observed subject digest check
   (`run.rs:376`), telemetry registry construction (`run.rs:431`), and the
   best-effort removal of the runner-owned Eggsec scope directory
   (`run.rs:464`).

Two other orderings are load-bearing and undocumented. Resolution happens
*before* the external tool probes (`run.rs:114` then `run.rs:150-165`), so a
missing binary surfaces as `missing_executable_path` from the resolver rather
than as a probe failure. And the four diagnostic/correctness preflights run
between resolution and environment collection, each returning a
`CapabilityPreflight` failure before any staging
(`run.rs:196`, `:223`, `:251`).

The qualification entry point `run_with_qualification` (`run.rs:285`) skips
steps 1-12: it takes an injected `FakeWorkload`, appends a
`fake-service` descriptor so service-bearing plans still resolve
(`run.rs:298`), and enters `run_impl` with `pre_resolved: None` — so resolution
happens once, inside `run_impl` (`run.rs:345-351`). It is unreachable from
`main.rs` and no public flag selects it.

### `run` — cancellation

One signal future is forwarded into a single `CancellationToken`:

- `forward_signal` awaits the future and calls `cancel.cancel()` — nothing
  else (`run.rs:491-494`). It never terminates the process and holds no
  teardown logic.
- `spawn_signal_forwarder` wraps it in a spawned task (`run.rs:496-503`) so the
  signal can be awaited concurrently with `execute_run_with_diagnostics`.
- The token is created at `run.rs:442` and passed to
  `execute_run_with_diagnostics` at `run.rs:456`; drain and teardown stay
  inside the runner.
- After execution the handle is aborted and awaited (`run.rs:459-460`), so no
  detached task survives and a second Ctrl-C is not consumed by this path.
- Production supplies `wait_for_ctrl_c()` (`run.rs:511`, marked
  `#[allow(dead_code)]` at `run.rs:510`); the qualification path injects a
  deterministic future through `commands_run_with_qualification`
  (`lib.rs:210`). `forward_signal` is separately exported for tests
  (`lib.rs:222`).

How cancellation reaches the user: the runner finalizes the bundle with
`ExecutionStatus::Cancelled`, `presented_run_outcome` routes every non-
`Completed` status through `PresentedCommandResult::run_non_success`
(`run.rs:536-547`), and that constructor sets `ok = false` with category
`run_non_success` and exit code 4 (`envelope.rs:272-289`). A Ctrl-C run
therefore exits **4 with the bundle path retained**, not 130, and the failure
text is `run finalized with execution status Cancelled` (`run.rs:542-545`).
Evidence I/O failure after execution exits 5 with category `evidence`
(`run.rs:469`); telemetry problems after execution are attached as envelope
warnings instead of failures (`run.rs:474-482`).

Cancellation is *not* wired into the preflight probes: each preflight builds its
own fresh `CancellationToken` (`run.rs:154`, `:173`, `:198`, `:226`) that no
signal can cancel, because the forwarder is not spawned until `run.rs:446`.

### `doctor` — what it checks and what it must not do

`doctor` runs the same parse and resolve as `run` and then stops. It never
calls `LocalSession::prepare`, `prepare_bundle`, `execute_run*`,
`production_workload_executor`, or `production_service_adapters`. What it
reports:

| Check | Line | Notes |
|---|---|---|
| `eggstack-path` feature present when a path is requested | `doctor.rs:115` | Same gate as `run.rs:89` |
| Platform label validity and support | `doctor.rs:125`, `:692` | An invalid label is an internal defect, not a masked value |
| Environment fingerprint | `doctor.rs:222` | Runner collector, same one `run` uses |
| Full driver inventory with per-driver `binary_present` | `doctor.rs:234-259` | Sorted by name; `has_workload_driver` at `:261` |
| Network-path intent, mode, selected drivers, upstream versions | `doctor.rs:324` | `supported_capabilities` derived from Route/Fault descriptors; `unsupported_capabilities` is a fixed exclusion list (`:383-394`) |
| Paired design from the declaration | `doctor.rs:402` | Read from the plan, not the resolution, so it stays visible when resolution fails for another reason |
| Eggprobe: binary presence, version, schema handshake | `doctor.rs:447` | `not-requested` / `not-attempted` / `missing-binary` / `probe-failed` / `pass` / `unsupported-contract` |
| Eggsec: binary presence, version, HTTP corpus availability | `doctor.rs:486`, `:539` | HTTP corpus availability is read from the production catalog (`:499`) |
| Gregg endpoint config syntax | `doctor.rs:270` | Syntax only; live probing stays in `run` |

Two places where `doctor` *does* execute something, both explicitly not
managed services: the eggprobe version probe and schema handshake
(`doctor.rs:462`, `:471`) and the eggsec version probe (`doctor.rs:552`). Each
is gated on the plan requesting the oracle *and* `binary_present == Some(true)`
(`doctor.rs:451`, `:541`); otherwise `doctor` reports filesystem presence and
spawns nothing. `has_workload_driver=false` is reported truthfully, but the
command still exits 3 because resolution fails with `missing_driver`
(`doctor.rs:634`).

On failure `doctor` keeps its payload: it builds an `ok` envelope and then
flips `ok = false` and attaches the error, once for the telemetry-config case
(`doctor.rs:273-292`) and once for a resolution error
(`doctor.rs:582-597`). This is deliberate — a failed doctor that discarded the
driver inventory and environment would be useless.

### `doctor` — the driver-inventory injection path

`run_with_registry` (`doctor.rs:85`) takes `&[DriverInventoryEntry]` and maps
it to descriptors (`doctor.rs:90-93`), then runs the identical `run_with_input`
flow as production with two differences: it passes `workload_driver: None`
(`doctor.rs:100`), and it uses the *static* diagnostics and security summaries
(`doctor.rs:95-96`), which never spawn a process. The qualification path is
therefore completely process-free. It is exposed as
`commands_doctor_run_with_registry` (`lib.rs:197`).

What the injected inventory does **not** control: the oracle capability reports
still read the ambient environment and the production catalog —
`external_binary_present` for eggprobe/eggsec (`doctor.rs:422`, `:504`) and
`production_catalog().correctness(...)` for the HTTP corpus (`doctor.rs:499`).
The inventory is injection-complete for *resolution*, not for *capability
reporting*. The fake inventory itself is built by `QualificationRuntime`
(`workload_registry.rs:183`); see [cli-workload-registry.md](cli-workload-registry.md).

### `inspect` — read-only verification

`inspect` opens and verifies before reading anything (`inspect.rs:19-20`), then
projects. The command contains no write path: no `fs::write`, no `File::create`,
no `remove_dir`, no `set_len`. Every read goes through `BundleReader`, and
every path it hands to `open_artifact` is taken from the manifest, never from
argv or from a string built by the command:

| Projection | Line | Source |
|---|---|---|
| Driver inventory | `inspect.rs:23-44` | Manifest. `binary_present` is hard-set to `None` (`:41`) because host presence says nothing about the recording host |
| Environment fields | `inspect.rs:46-80` | The `EnvironmentFingerprint` artifact |
| Subject label/revision/digest | `inspect.rs:82-86`, `:651-679` | Manifest `subject` |
| Per-trial terminal status and elapsed ns | `inspect.rs:88-92`, `:153` | Each trial's `result` artifact plus `reader.trial_metrics` |
| Network path | `inspect.rs:101-113` | `network-path.json` presence, then `load_comparison_input` as a full-input load check (`:109`) |
| Semantic replay / diagnostics / security | `inspect.rs:114-116` | Per-artifact evidence, read as `serde_json::Value` |
| HTTP corpus cases | `inspect.rs:420-483` | `security/` artifacts, bounded and contract-validated |
| Artifact count and total bytes | `inspect.rs:94-99` | Manifest |

**Legacy v1 → v2 normalization.** The reader, not the command, owns this.
`BundleReader::open` dispatches on `schema_version` and converts a v1 manifest
through `LegacyManifestV1::into_current_view`
(`crates/eggbench-core/src/evidence.rs:1188-1210`, `:459`). That conversion
maps `Succeeded | Inconclusive → ExecutionStatus::Completed`
(`evidence.rs:460-466`) and forces `comparison_verdict: None` (`evidence.rs:475`).
The original overloaded value is retained on the reader
(`BundleReader::legacy_status`, `evidence.rs:1236`) and `inspect` surfaces it as
a **separate sibling field** (`inspect.rs:135-137`) next to the normalized
`execution_status` (`inspect.rs:129-131`). A legacy `inconclusive` therefore
appears as `execution_status: "completed"` *and* `legacy_status:
"inconclusive"` in the same payload: it is surfaced, never resolved into a
verdict. The core doc is explicit that it cannot be read as either "no
comparison" or an inconclusive comparison (`evidence.rs:1233-1234`), and a v1
bundle is held to `validate_legacy_view()` rather than `validate()`
(`evidence.rs:1211-1215`).

`--manifest-json` (the argv flag; the internal field is `emit_manifest_json`,
`main.rs:77`, `:828`) inlines the *normalized current-view* manifest as
`manifest_json` (`inspect.rs:118-122`).

### `compare` — four mutually exclusive modes

| Mode | Selection | Loader | Policy |
|---|---|---|---|
| Baseline bundle | positional baseline | `load_baseline_bundle` `compare.rs:66` | `compare` (v1) |
| Alias | `--alias` | `load_baseline_alias` `compare.rs:63` | `compare` (v1), digest-pinned |
| Absolute-only | `--absolute-only` | candidate only, `baseline: None` (`compare.rs:60`) | `compare` (v1) |
| Paired | `--paired` | candidate only | `compare_paired` (v2), `compare.rs:90` |

Exclusivity is enforced by three guard clauses and a required-mode check
(`compare.rs:35-57`); each returns `CliError::Internal`. The candidate is
always loaded first (`compare.rs:59`), then the baseline side.

**The command never modifies any `.eggb` directory.** Every loader reads
through `BundleReader` (`crates/eggbench-core/src/comparison.rs:2096-2159`),
`compare` and `compare_paired` take `&ComparisonInput` and return a
`ComparisonReceipt` by value, and the single write in the file is
`std::fs::write` to the `--output` path (`compare.rs:170`) — a receipt file,
not a bundle.

`--seed` is forwarded verbatim into `ComparisonOptions { seed }`
(`compare.rs:78`, `:90`) with no CLI-side default; when absent, core derives it
from bundle digests (`comparison.rs:2493-2496`). Explicit seeding makes a
bootstrap comparison reproducible.

`present_receipt` maps the aggregate verdict to the locked exit matrix through
`PresentedCommandResult::compare_verdict` (`compare.rs:125`,
`envelope.rs:297-324`): `Pass` and no-verdict exit 0, `Fail` 6,
`Inconclusive` 7, `Invalid` 8. `comparison_detail` (`compare.rs:133-165`) keeps
the stderr prose from collapsing the independent gate families — a security
correctness failure says so instead of "metric failed", while the machine JSON
keeps the typed `performance_verdict` / `correctness` / `aggregate_verdict`
fields.

### The presentation boundary

Each command module builds the envelope *and* the exit code, then returns both.
`main.rs::present` (`main.rs:871`) is the only place that serializes stdout or
writes stderr, and it takes the numeric status from `presented.exit_code`
identically in JSON and human mode (`main.rs:873`, `:889`, `:912`).

| Command | Construction | Result retained on failure |
|---|---|---|
| `validate` | `success` `validate.rs:17` | n/a |
| `doctor` | `success` `doctor.rs:296`, or hand-built `ok=false` + error | Yes — driver and environment payload |
| `run` | `success` / `run_non_success` `run.rs:536-547` | Yes — bundle path and status |
| `inspect` | `success` `inspect.rs:124` | n/a |
| `compare` | `compare_verdict` `compare.rs:125` | Yes — receipt |

Human prose is entirely outside the compatibility surface: `envelope.rs:3-5`
and `docs/cli.md:44` both say so, and the stderr lines at `main.rs:894` and
`main.rs:897-903` are the only human output for a normal command. Note that
`CommandOptions` is accepted and **ignored** by the command modules
(`run.rs:79`, `doctor.rs:39`, `run_with_qualification` at `run.rs:289`);
`--json`/`--quiet` are read only in `main.rs`.

## Invariants

- **No orchestration in the CLI.** Every side effect is a runner or core call.
  `run` delegates to `LocalSession::prepare`, `prepare_bundle`,
  `collect_local_environment`, `ResetRegistry`, `ProbeRegistry::with_builtins`,
  and `execute_run_with_diagnostics` (`run.rs:398`, `:408`, `:366`, `:427`,
  `:393`, `:447`); the trial schedule, phase transitions, and metric writing
  stay in the runner. See [runner-orchestration.md](runner-orchestration.md).
- **One normalization point.** `inspect` reports `terminal_status` as the
  stored `TrialExecutionResult` value (`inspect.rs:194`) and never re-derives
  it from metrics; `core::metrics` remains the only writer of `TrialMetrics`.
  See [core-metrics.md](core-metrics.md).
- **Subject snapshot is not built by the CLI.** There is no
  `build_subject_snapshot` call in this crate; the snapshot arrives as the
  second element of `collect_local_environment`'s return pair (`run.rs:366`).
  See [runner-evidence-prep.md](runner-evidence-prep.md).
- **Fail before staging.** Resolution, executor construction, and every
  external preflight return before `prepare_bundle` (`run.rs:114-263` precede
  `run.rs:408`), so a preflight failure publishes no bundle.
- **No silent fallback.** A route/fault/workload capability that cannot be
  satisfied is a categorized failure, never a substitution.
- **`doctor` starts no managed work.** No session, writer, or executor; only
  the two oracle version probes described above.
- **`compare` and `inspect` never write into a bundle.** `compare` writes only
  the optional receipt; `inspect` has no write call at all.
- **Secrets never appear.** `run` installs `MapSecretProvider::empty()`
  (`run.rs:388`), so no plan secret value is ever materialized by the CLI.
- **Deterministic ordering in reported summaries.** Doctor drivers are sorted
  by name (`doctor.rs:257`) and inspect drivers too (`inspect.rs:44`), so
  payloads do not depend on catalog order.

## Failure model

| Family | Categories | Exit | Raised in |
|---|---|---|---|
| Usage | `usage` | 2 | `run.rs:54`, `doctor.rs:71`; argv shape at `main.rs:148` |
| Plan | `plan_validation`, `unsupported_network_path`, `workload_path_incompatible` | 2 / 3 | `doctor.rs:617-671`, `run.rs:89-112` |
| Capability / preflight | `unsupported_workload`, `missing_driver`, `external_tool`, `invalid_fixture`, `diagnostic_contract_unsupported`, `security_driver_missing`, `http_corpus_driver_unavailable`, `telemetry`, `subject_digest_mismatch`, `prepare` | 3 | `run.rs:126-263`, `:371`, `:377`, `:437` |
| Run finalized non-success | `run_non_success` | 4 | `run.rs:539` via `envelope.rs:272` |
| Evidence / bundle | `bundle`, `evidence` | 5 | `run.rs:469`, `:688`; `inspect.rs:19-20`; `compare.rs:170-175` |
| Comparison verdict | `comparison_fail` / `_inconclusive` / `_invalid` | 6 / 7 / 8 | `envelope.rs:311-316` |
| Internal | `CliError::Internal` | 1 | `compare.rs:37`, `:44`, `:49`, `:54`; `lib.rs:176-179` |

Two mapping styles coexist. Typed inputs — `RunnerError`, `BundleError`,
`ResolveError`, `ComparisonError` — are matched exhaustively and mapped to
stable categories (`run.rs:667-690`, `doctor.rs:617-673`,
`inspect.rs` bundle errors, `compare.rs:178-188`). Executor construction and
the replay/eggprobe/eggsec preflights return plain `String` messages, so their
categories are recovered by substring matching on that message
(`run.rs:130-138`, `:178-186`, `:207`, `:239`). Those categories are part of
the compatibility surface, so a driver that rewords an error silently changes a
public category.

`CliError` never reaches the user unwrapped: `lib.rs:174-180` converts it into
`PresentedCommandResult::failure` with the command label
(`lib.rs:183`).

## Boundaries

- **Owns:** stage ordering within a command, preflight category selection,
  summary projection, envelope + exit-code construction, and the cancellation
  forwarder.
- **Does not own:** argv grammar ([cli-surface.md](cli-surface.md)), envelope
  schema and `ExitCode` (same file), driver inventory and adapter
  construction ([cli-workload-registry.md](cli-workload-registry.md)), plan
  parsing and semantic validation
  ([core-plan-schema.md](core-plan-schema.md)), resolution policy
  ([core-driver-resolution.md](core-driver-resolution.md)), bundle writing and
  verification ([core-evidence-bundle.md](core-evidence-bundle.md)), comparison
  policy ([core-comparison.md](core-comparison.md)), session startup
  ([runner-process-lifecycle.md](runner-process-lifecycle.md)), evidence staging
  ([runner-evidence-prep.md](runner-evidence-prep.md)), trial orchestration
  ([runner-orchestration.md](runner-orchestration.md)), oracle behavior
  ([drivers-external-oracle-tools.md](drivers-external-oracle-tools.md)),
  feature registration
  ([drivers-catalog-features.md](drivers-catalog-features.md)).
- **User-facing contract:** [docs/cli.md](../docs/cli.md). Narrative contracts:
  [docs/trial-orchestration.md](../docs/trial-orchestration.md),
  [docs/comparison.md](../docs/comparison.md),
  [docs/evidence-bundle.md](../docs/evidence-bundle.md).

## Tests and qualification

Integration tests drive the commands through `lib::execute` or the
qualification seams in `crates/eggbench-cli/tests/`:

| Behavior | Test |
|---|---|
| `validate` accept/reject paths, stdin format requirement | `cli.rs:21`, `:42`, `:60`, `:75`, `:92` |
| `doctor` production truthfulness and the injection seam | `cli.rs:172`, `:239`, `:253` |
| `run` fails before startup with no workload driver | `cli.rs:273` |
| Qualification run: success 0, failed 4 with bundle, cancellation 4 with bundle | `cli.rs:334`, `:355`, `:384` |
| Signal forwarder cancels the token | `cli.rs:410` |
| `inspect` verifies a bundle; missing bundle fails | `cli.rs:419`, `:438` |
| `compare` verdict→exit matrix, all four modes, usage rejection | `cli.rs:541`, `:582`, `:611`, `:645`, `:673`, `:717`, `:2214`; `binary_exit_codes.rs:225`, `:255` |
| Exit codes identical in JSON and human mode | `binary_exit_codes.rs:42` through `:267` |
| Paired and network-path surfaces | `cli.rs:993`, `:1082`, `:1133`, `:1168`, `:2109`, `:2183` |
| Oracle preflight before startup | `cli.rs:2481`, `:2661` |
| Live binary interop | `tests/m004b_live.rs:235`, `tests/synvoid_m002a.rs`, `tests/synvoid_m002b.rs`, `tests/synvoid_m003.rs` |

The qualification fake is a deterministic `FakeWorkload` injected explicitly;
no public flag selects it, and `production_registry_contains_no_fake_driver`
(`cli.rs:253`) asserts it never appears in the production inventory. See
[tooling-qualification-ci.md](tooling-qualification-ci.md).

## Review focus

1. **Duplicated resolution-option construction.** `run.rs:551-620` and
   `doctor.rs:141-217` build the same `ResolutionOptions` independently: the
   same `DefaultDriverPolicy::Deterministic`, the same eggprobe/eggsec binary
   pinning, the same SemanticReplay-vs-LoadMode capability choice. A change to
   one and not the other makes `doctor` disagree with `run` about whether a
   plan resolves. `workload_load_mode` is likewise duplicated
   (`run.rs:652`, `doctor.rs:676`).
2. **Category-by-substring.** `run.rs:130-138`, `:178-186`, `:207`, `:239`
   derive public failure categories by matching driver error text. Check any
   driver message reword against these.
3. **`inspect` lossy defaults.** A trial result that fails to parse is reported
   as `terminal_status: "completed"` with no measurement
   (`inspect.rs:167-174`); an unparsable environment artifact yields an empty
   field list (`inspect.rs:60-80`); security artifacts over 512 KiB
   (`inspect.rs:436`) or failing `validate_contract` (`inspect.rs:443`) are
   silently skipped. "Missing from `inspect`" does not imply "missing from the
   bundle" — check the manifest before arguing a gate lacked evidence.
4. **`--manifest-json` failure is indistinguishable from absence.**
   `serde_json::to_string_pretty(manifest).ok()` (`inspect.rs:119`) drops a
   serialization error, so `manifest_json: null` means "not requested" or
   "serialization failed".
5. **`compare` has two usage-error surfaces.** Argv-shape problems are caught
   in `main.rs:145-151` and exit 2, while mode-combination problems inside the
   command return `CliError::Internal` (`compare.rs:37`, `:44`, `:49`, `:54`)
   and exit 1. Both are user usage errors with different codes.
6. **Preflight tokens are uncancellable.** Each preflight creates its own
   `CancellationToken` (`run.rs:154`, `:173`, `:198`, `:226`) that the SIGINT
   forwarder cannot reach, because the forwarder starts only at `run.rs:446`.
7. **Hand-built failure envelopes.** `doctor` constructs "failure with a
   retained payload" twice (`doctor.rs:273-292`, `:582-597`) because
   `PresentedCommandResult` has no general constructor for it. A change to
   `CliEnvelope::ok` affects both.
8. **Mutation audit for `inspect`.** Confirm the file still contains no write
   call, and that every `open_artifact` argument comes from the manifest
   (`inspect.rs:60`, `:158`, `:221`, `:314`, `:428`, `:509`).
9. **Doctor's spawned probes.** Any new probe in `doctor` must stay outside
   managed startup and be gated on plan request plus binary presence
   (`doctor.rs:451`, `:541`).

## Related

- [overview.md](overview.md) — command-to-stage table
- [cli-surface.md](cli-surface.md) — argv, envelope, exit codes
- [cli-workload-registry.md](cli-workload-registry.md) — driver inventory and
  adapter construction
- [runner-evidence-prep.md](runner-evidence-prep.md) — `prepare_bundle`,
  `collect_local_environment`
- [runner-orchestration.md](runner-orchestration.md) — `execute_run_with_diagnostics`
- [runner-process-lifecycle.md](runner-process-lifecycle.md) —
  `LocalSession::prepare` vs `startup`
- [core-driver-resolution.md](core-driver-resolution.md) — `resolve_plan`
- [core-evidence-bundle.md](core-evidence-bundle.md) — `BundleReader`,
  manifest v1→v2
- [core-comparison.md](core-comparison.md) — `compare`, `compare_paired`
- [docs/cli.md](../docs/cli.md) — user-facing command contract
