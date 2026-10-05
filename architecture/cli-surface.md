# CLI surface: argv, machine envelope, exit codes, plan input

## Purpose

This file explains the process-facing contract of the `eggbench` binary: which
argv shapes exist, what is written to stdout versus stderr, how a single
outcome becomes a stable exit status, and how a plan path or `-` becomes a
validated `ExperimentPlan`.

It covers only the surface layer; per-command pipelines are in
[cli-commands.md](cli-commands.md) and the driver inventory split in
[cli-workload-registry.md](cli-workload-registry.md).

The layer is a presentation adapter with no orchestration of its own
(`crates/eggbench-cli/src/lib.rs:1-10`). What it *does* own is the
compatibility surface: the JSON envelope is the stable artifact consumers
parse, and stderr prose is explicitly not (`crates/eggbench-cli/src/envelope.rs:1-5`).

## Source layout

| File | LOC | Responsibility |
|---|---|---|
| `crates/eggbench-cli/src/main.rs` | 913 | `[[bin]] eggbench`: clap argv tree, `Command` conversion, qualification dispatch, stdout/stderr write, process status |
| `crates/eggbench-cli/src/lib.rs` | 227 | `Command`/`InputFormat`/`CommandOptions`, `execute()`, `command_label()`, exported qualification seams |
| `crates/eggbench-cli/src/envelope.rs` | 738 | `CliEnvelope`, `CliOutput`, `CliWarnings`, `CliFailure`, `ExitCode`, `PresentedCommandResult`, all nested summary payload types |
| `crates/eggbench-cli/src/error.rs` | 124 | `CliError` and its single conversion to `CliFailure` (category + exit code) |
| `crates/eggbench-cli/src/plan_input.rs` | 93 | Byte read, format detection/override, parse, package as `PlanInput` |

Adjacent, not duplicated here: `commands/*.rs` (one module per command, wired
in `commands/mod.rs:3-7`) and `workload_registry.rs` (catalog view). The binary
is `src/main.rs` and the library is `src/lib.rs`
(`crates/eggbench-cli/Cargo.toml:8-13`), so the same crate is both the
`eggbench` entry point and the library that qualification harnesses link.

## Key types

### Command reference

Global options are `global = true` and may appear before or after the
subcommand: `--json` and `--quiet` (`main.rs:22-27`). The crate binary name is
`eggbench` (`main.rs:20`).

| argv | `Command` variant | Fields | Notes |
|---|---|---|---|
| `validate <plan> [--input-format toml\|json]` | `Command::Validate` | `plan`, `input_format` | No driver inventory, no resolution |
| `doctor <plan> [--input-format] [--workload-driver <name>]` | `Command::Doctor` | `plan`, `input_format`, `workload_driver` | Resolves and preflights; starts no process |
| `run <plan> <bundle> [--input-format] [--workload-driver <name>]` | `Command::Run` | `plan`, `input_format`, `bundle`, `workload_driver` | Full chain through bundle finalization |
| `inspect <bundle> [--manifest-json]` | `Command::Inspect` | `bundle`, `emit_manifest_json` | Read-only; flag renamed to `emit_manifest_json` at conversion |
| `compare <baseline> <candidate> [--output] [--seed]` | `Command::Compare` | `baseline: Option`, `candidate`, `alias`, `absolute_only`, `paired`, `output`, `seed` | Four mutually exclusive modes, see below |
| `qualify {validate\|expand\|run\|inspect}` | *not a `Command`* | — | Bypasses `execute()`; see "How it works" |

`Command` is defined once in `lib.rs:36-90`; the clap mirror is `CliCommand` in
`main.rs:32-101`. `build_command` (`main.rs:794-863`) is the only translation
point, so argv names and library field names are allowed to differ without
drift. `command_label` (`lib.rs:183-191`) supplies the envelope's `command`
string from the `Command` value, not from clap, so the label is stable for
library callers too.

There are **no** route, fault, network-path, plan-seed, or fake-workload flags
(`main.rs:40-100`). Route and fault intent is plan-declared; the deterministic
comparison seed defaults from bundle digests. The absence of a fake-workload
flag is a boundary, not an omission — see "Boundaries".

### `compare` modes

`compare` takes one or two positionals plus four switches, and exactly one mode
may apply. The clap layer resolves positional arity
(`main.rs:842-851`); the command module enforces exclusivity
(`commands/compare.rs:35-57`).

| Mode | argv | `baseline` | `alias` | `absolute_only` | `paired` |
|---|---|---|---|---|---|
| Two-bundle | `compare <baseline> <candidate>` | set | unset | false | false |
| Alias baseline | `compare --alias <file.json> <candidate>` | unset | set | false | false |
| Candidate only | `compare --absolute-only <candidate>` | unset | unset | true | false |
| Paired arms | `compare --paired <bundle.eggb>` | unset | unset | false | true |

Positional rules (`main.rs:842-851`): two positionals are baseline + candidate;
exactly one positional is only legal when `--alias`, `--absolute-only`, or
`--paired` is present, in which case it is the candidate. Anything else is a
usage error, reported as a single message
(`main.rs:846-849`) before any bundle is opened.

Exclusivity is rejected with `CliError::Internal` at the top of the command
(`commands/compare.rs:35-57`): `--paired` with baseline/alias/`--absolute-only`,
`--absolute-only` with baseline/alias, and baseline with alias. The
`Command::Compare` field docs repeat the same rules
(`lib.rs:74-75`, `lib.rs:78-79`, `lib.rs:82-84`).

### Envelope types

| Type | Role | Shape |
|---|---|---|
| `CliEnvelope` | The stdout document | `schema_version: u32`, `command: String`, `ok: bool`, optional `result`, optional `error`, `warnings` |
| `CliOutput` | Tagged success payload | `#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]` over `Validate`/`Doctor`/`Run`/`Compare`/`Inspect` |
| `CliWarnings` | Warning entry | `category`, `detail` |
| `CliFailure` | Internal failure with process intent | `category`, `detail`, `exit_code: ExitCode` |
| `CliResult` | Alias | `Result<CliEnvelope, CliFailure>` (`envelope.rs:194`) |
| `ExitCode` | Process status category | Closed enum; `code()` is `self as i32` (`envelope.rs:38-40`) |
| `PresentedCommandResult` | Envelope **plus** exit code | `{ envelope, exit_code }` |

Serialization discipline is uniform and deliberate (`envelope.rs:152-191`):
every envelope-level struct and every `CliOutput` variant is
`deny_unknown_fields`, so an added field is a breaking change rather than an
additive one, and `result`/`error`/`warnings` are skipped when absent or empty
(`envelope.rs:163-170`). `schema_version` is fixed at `1`
(`envelope.rs:10`) and stamped only by the `CliEnvelope::ok`/`fail`
constructors (`envelope.rs:330-352`). `Compare` boxes its `receipt` and `Doctor`
boxes its nested summaries (`envelope.rs:44`, `envelope.rs:107-109`) to keep the
enum small. Optional sub-summaries (`network_path`, `diagnostics`, `security`,
`paired`) are skipped when `None` (`envelope.rs:68-78`,
`envelope.rs:138-148`), so a feature-disabled build emits a smaller document at
the same schema version rather than nulls. Paths are wrapped in
`PathBufPayload`, a bare JSON string whose `from_path` yields `None` for a
non-UTF-8 path (`envelope.rs:384-406`, `envelope.rs:391-393`).

### Exit codes

These are the codes the code defines, from `ExitCode` (`envelope.rs:14-33`);
`code()` is a straight discriminant read, so declaration order is irrelevant.

| Code | Variant | Meaning | Producing paths |
|---|---|---|---|
| `0` | `Success` | Command completed successfully | `PresentedCommandResult::success` (`envelope.rs:250-255`) |
| `1` | `Internal` | Internal/unclassified CLI failure | `CliError::Internal` (`error.rs:121`), JSON serialization failure (`main.rs:880-886`), missing error payload (`main.rs:904-911`) |
| `2` | `ParseValidation` | Parse, schema, or ordinary plan-validation error | `PlanIo`, `PlanFormat`, `PlanValidation`, `BaselineAlias` (`error.rs:58-63`, `error.rs:64-82`, `error.rs:116-120`) |
| `3` | `CapabilityPreflight` | Capability/doctor/preflight unsupported or invalid | Network-path `PlanError` categories (`error.rs:70-80`), every `Resolution` variant (`error.rs:113`) |
| `4` | `RunCompletedNonSuccess` | Bundle finalized with `Failed`/`Cancelled`/`Invalid` | `PresentedCommandResult::run_non_success` (`envelope.rs:271-289`) |
| `5` | `EvidenceIo` | Evidence/bundle I/O or verification failure | `CliError::Bundle` (`error.rs:115`) |
| `6` | `ComparisonFail` | Comparison aggregate verdict `Fail` | `compare_verdict` (`envelope.rs:307`) |
| `7` | `ComparisonInconclusive` | Comparison aggregate verdict `Inconclusive` | `compare_verdict` (`envelope.rs:308`) |
| `8` | `ComparisonInvalid` | Comparison aggregate verdict `Invalid` | `compare_verdict` (`envelope.rs:309`) |

The mapping is compact on purpose: no code is introduced for a new failure
family, and `internal` is the declared catch-all so an unclassified failure
still exits with a documented value.

Two composites matter, and both are expanded under "Failure model":
`run_non_success` flips `ok` to `false` while **keeping the run result**, so the
bundle path survives a non-zero exit (`envelope.rs:277-288`); and
`compare_verdict` maps `None | Pass` to success, with `Fail | Inconclusive |
Invalid` producing a result-retaining failure
(`envelope.rs:303-323`). A descriptive-only or verdict-less comparison exits
`0`: non-zero means "the aggregate verdict is not a pass", not "the comparison
failed to run".

### `CliError` → `CliFailure`

One conversion function owns every category/exit pair
(`error.rs:56-123`). Nothing else constructs a `CliFailure` with a hand-picked
code, except the two run-specific constructors in the envelope.

| `CliError` variant | Category | Exit |
|---|---|---|
| `PlanIo` | `plan_io` | `2` |
| `PlanFormat` | `plan_format` | `2` |
| `PlanValidation(PlanError)` | `plan_validation`, or the raw category for network-path categories; `unsupported_schema_version` / `plan_parse` retained | `2`, or `3` for network-path categories |
| `Resolution(ResolveError)` | `plan_validation`, `missing_route_driver`, `missing_fault_driver`, `missing_driver`, `category_mismatch`, `ambiguous_selection`, `unsupported_network_path`, `unsupported_stream_fault_plan`, `unsupported_capability`, `unsupported_platform`, `missing_executable_path`, `incompatible_service`, `duplicate_driver` | `3` |
| `Bundle(BundleError)` | `bundle` | `5` |
| `BaselineAlias { category, detail }` | the supplied `category` verbatim | `2` |
| `Internal` | `internal` | `1` |

The network-path carve-out is the subtle part: an allowlist of eight
categories (`error.rs:39-51`) is the only way a *validation* error escapes
exit `2` and lands on exit `3`, because a path-shape rejection is a capability
limitation of this build rather than a malformed plan
(`error.rs:64-82`). Anything outside the allowlist is collapsed to the single
`plan_validation` category, which keeps the category vocabulary bounded.

## How it works

### argv → `Command` → result → process status

1. `Cli::parse()` parses argv (`main.rs:138`); parse failures never reach the
   layer, since clap prints usage and exits itself.
2. `CommandOptions` is built from the two global flags (`main.rs:139-142`),
   defaulting to `CommandOptions::human()` (`lib.rs:110-119`).
3. `qualify` short-circuits to its own handler (`main.rs:144`). Every other
   subcommand goes through `build_command`; a conversion error is a usage
   failure printed to stderr with status `2` and never becomes an envelope
   (`main.rs:145-151`).
4. `execute` dispatches to one command module, holding the outcome as
   `Result<PresentedCommandResult, CliError>` (`lib.rs:130-173`). Commands
   return an already-presented result, so they never write to stdout.
5. A `CliError` is funnelled through `into_failure()` and
   `PresentedCommandResult::failure` (`lib.rs:174-180`).
6. `present` writes output and returns the status (`main.rs:871-913`). Only
   `main` turns that into a process status, which is what keeps binary
   behavior testable (`main.rs:865-870`).

### stdout versus stderr

`--json`: exactly one pretty-printed envelope on **stdout**
(`main.rs:874-878`), via `CliEnvelope::to_pretty_json` (`envelope.rs:378-380`).
Nothing else is written to stdout on this path. If serialization fails, the
envelope cannot be delivered, so the process writes the detail to **stderr**
and exits `1` (`main.rs:879-887`) — a JSON consumer sees a missing document and
a non-zero code, never a truncated one.

Human mode: stdout stays empty. Success writes one `eggbench: <command> ok` line
to stderr unless `--quiet` (`main.rs:892-895`); failure writes
`eggbench: <command> failed [<category>] <detail>` to stderr
(`main.rs:896-903`), and `--quiet` does **not** suppress it — quiet is scoped to
the success line. An `ok = false` envelope with no `error` payload is treated as
an internal defect and exits `1` (`main.rs:904-911`).

The numeric status is computed once, before the mode branch, from
`presented.exit_code` (`main.rs:873`), and returned unchanged in both branches
(`main.rs:889`, `main.rs:912`), so the two modes cannot disagree about the
outcome of the same run. `--quiet` never changes the exit status, and it is
passed into `execute` (`lib.rs:136`, `lib.rs:143-149`) so command modules can
suppress their own progress prose too.

### Why presentation must use the attached exit code

`PresentedCommandResult` carries the exit code as a sibling of the envelope and
the two are deliberately separable (`envelope.rs:233-245`). The doc comment
states the rule: the envelope is the compatibility surface, the exit code is
process metadata, and presentation must use the attached code and never infer
it from `ok` (`envelope.rs:236-238`, restated in `lib.rs:124-127`).

Three concrete reasons the inference is unsound:

- **The envelope has no field to carry it.** `CliEnvelope` is
  `deny_unknown_fields` with a fixed five-field shape
  (`envelope.rs:155-171`). `CliEnvelope::run_non_success`
  (`envelope.rs:360-362`) exists precisely to build a complete envelope with no
  code attached, which is why the code lives on `PresentedCommandResult`
  instead. A consumer that parses stdout has no in-band way to learn `4` versus
  `6/7/8`; it must read the process status.
- **`ok = false` collapses four documented categories.** Codes `2`, `3`, `5`,
  and the four comparison/run codes are all `ok = false` envelopes
  (`envelope.rs:271-323`, `error.rs:56-123`). `ok` answers "did it succeed",
  not "how".
- **`ok = true` is a constructor coincidence, not a contract.** Today every
  `success` builder pairs `ok: true` with `ExitCode::Success`
  (`envelope.rs:250-255`, `envelope.rs:330-339`), but that pairing is a
  property of two constructors, not a field relationship. Any future outcome
  that is "not a failure" but still needs a distinct status — a
  result-retaining non-success run is already the pattern
  (`envelope.rs:282-284`) — would silently break the inference.

In-process consumers make the same split. `qualify_run` calls `execute` for
each scenario and reads `envelope.ok` to decide whether evidence is publishable
(`main.rs:320`) and `envelope.result` for the payload (`main.rs:313-319`) — the
envelope is the right source for *data*, and never for *process semantics*.

### Plan input loading

`load_plan` (`plan_input.rs:29-41`) is one linear pipeline: read bytes, decide
format, parse, and return all three (`PlanInput.bytes` is retained as the
canonical source for the staged `plan.json` artifact,
`plan_input.rs:14-22`).

- **Bytes** (`plan_input.rs:43-53`): the literal path `-` reads stdin to EOF;
  anything else is `fs::read`. Failure is `PlanIo` — category `plan_io`, exit
  `2`.
- **Format** (`plan_input.rs:55-77`): an explicit override returns
  immediately (`plan_input.rs:59-61`); `-` with no override is a hard error
  (`plan_input.rs:62-66`); otherwise the extension is matched exactly against
  `toml` and `json` (`plan_input.rs:67-69`). An unrecognized extension and a
  missing extension are distinct messages, both `PlanFormat`
  (`plan_input.rs:70-75`). The match is case-sensitive.
- **Parse** (`plan_input.rs:79-93`): non-UTF-8 bytes are a `PlanFormat` error,
  not a parse error; a failed parse is a `PlanValidation` carrying the core
  `PlanError`, so schema and semantic rejections keep their core categories and
  land on exit `2` (or `3` for the network-path allowlist).

The extension is authoritative because format is a property of the input's
declared contract, not something to sniff from content: sniffing must commit to
one parser before knowing which is right, and a wrong guess turns a precise
schema error into an opaque syntax error. Requiring an explicit choice for
stdin and for unrecognized extensions keeps the ambiguity in the user's hands,
where it is resolvable. The override exists for the two cases where the
extension cannot express the format — stdin has no extension at all
(`plan_input.rs:62-66`), and a generated or temporary plan file may carry none
or a foreign one — and it wins over the extension in all cases
(`plan_input.rs:59-61`), so a wrong extension with a correct override is
recoverable and one without it is not.

### `qualify` bypasses the shared surface

`qualify` is parsed in the same clap tree (`main.rs:35-38`, `main.rs:103-117`)
but is not a `Command` variant, so it never reaches `execute` or `present`
(`main.rs:144`). Its four subcommands compose the shared path: `validate` and
`expand` call `eggbench_core::expand_qualification_profile` (`main.rs:187`),
while `run` drives `Command::Run` and `Command::Compare` once per scenario, in
order, with `json: true, quiet: true` (`main.rs:300-312`, `main.rs:360-375`).

Its own guarantees: the output directory must not already exist
(`main.rs:234-236`), the scenario count is bounded to 1..=32
(`main.rs:251-253`), work happens in a staging directory renamed into place
only after the receipt is written (`main.rs:259-267`, `main.rs:458`), and the
first scenario that cannot produce trustworthy evidence stops the loop with
later scenarios recorded as `NotRun` (`main.rs:277-296`, `main.rs:351`).
`qualify inspect` re-derives every typed verdict from on-disk evidence before
summarizing (`main.rs:512-632`) and confines every evidence path to the suite
directory (`main.rs:659-675`). It reimplements the exit mapping literally —
aggregate verdict to `0/6/7/8` (`main.rs:493-498`), matching
`compare_verdict` (`envelope.rs:303-310`).

### Feature forwarding

The CLI crate defines no driver logic. All four features exist only to forward
into `eggbench-drivers` (`crates/eggbench-cli/Cargo.toml:15-20`):

| Feature | Forwards to |
|---|---|
| `eggstack-http` | `eggbench-drivers/eggstack-http` |
| `gregg` | `eggbench-drivers/gregg` |
| `prometheus-http` | `eggbench-drivers/prometheus-http` |
| `eggstack-path` | `eggstack-http` + `eggbench-drivers/eggstack-path` |

`eggstack-path` implies `eggstack-http`, so the path feature can never be
enabled without the HTTP adapters it depends on. The CLI reaches the catalog
only through `workload_registry.rs`, a view over
`eggbench_drivers::DriverCatalog` and the sole construction point for the
production inventory (`workload_registry.rs:1-14`, `workload_registry.rs:380`,
`workload_registry.rs:398`). Enablement is visible in output rather than argv:
`NetworkPathDoctorSummary.feature_enabled` (`envelope.rs:452-454`) and
inspect-side `detailed_evidence_available` (`envelope.rs:476-480`).

## Invariants

- `envelope.schema_version` is always `1` and is set only by the constructors
  (`envelope.rs:10`, `envelope.rs:330-352`).
- `envelope.command` is one of `validate`, `doctor`, `run`, `inspect`,
  `compare`, derived from the `Command` value (`lib.rs:183-191`).
- `result` and `error` are both absent or exactly one is present, except for
  the two composites that deliberately keep `result` with `ok = false` and add
  an error (`envelope.rs:282-284`, `envelope.rs:317-319`).
- In `--json` mode, stdout carries exactly one JSON document; stderr carries
  diagnostics (`main.rs:874-889`).
- The process status comes from `PresentedCommandResult::exit_code` and is
  identical in JSON and human mode (`main.rs:873`, `main.rs:889`,
  `main.rs:912`).
- No command module writes to stdout; only `present` and the `qualify` handler
  do (`main.rs:154-155`).
- Plan input format comes from `--input-format` or the exact file extension,
  never from content (`plan_input.rs:55-77`).
- `deny_unknown_fields` on every envelope-level struct means an added field is
  a compatibility event, not an additive one (`envelope.rs:154`,
  `envelope.rs:175`, `envelope.rs:185`).
- A qualification suite directory is only ever created by atomic rename
  (`main.rs:458`); a partial suite is never published.
- Qualification evidence paths must be confined, relative, and inside the
  suite directory (`main.rs:659-675`).

## Failure model

Failure has exactly one funnel: `CliError::into_failure` (`error.rs:56-123`).
Three properties follow.

**Category and exit are decided together.** A failure has one stable category
and one exit code, both chosen at the same point. The envelope carries the
category; the process carries the code. Because `CliFailure` holds both
(`envelope.rs:197-205`) and `to_payload` deliberately drops the code
(`envelope.rs:225-230`), a serialized failure can never disagree with the
status it was produced with.

**Direct conversion and the in-`execute` failure path are identical.** Both
routes end in the same value: a command that fails internally returns
`PresentedCommandResult::failure`, and `execute` converts a returned
`CliError` with the same `into_failure` and the same constructor
(`lib.rs:174-180`, `envelope.rs:258-264`). The doc comment makes the
equivalence a requirement, not an observation (`lib.rs:126-127`). It matters
because a library consumer — the qualification harness, the `qualify`
subcommands, any test — must be able to run one command, induce a failure
either way, and observe an identical envelope: same `command` label,
`schema_version`, `ok`, `error.category`, `error.detail`, and exit code.
Otherwise a failure's observable shape would depend on whether it was raised
before or after the command chose to own it.

**"Non-success" is not "failed".** The two composites preserve payload under a
non-zero status: a finalized run keeps its bundle path with exit `4`
(`envelope.rs:266-289`), and a comparison keeps its full receipt with exit
`6/7/8` (`envelope.rs:291-324`). Evidence I/O failures keep exit `5` and
discard the result (`error.rs:115`).

Outside this funnel there are three narrower behaviors: a JSON serialization
failure is a code `1` with no envelope (`main.rs:879-887`); a missing error
payload on a non-`ok` envelope is a code `1` (`main.rs:904-911`); and the
`ExitCode` → `u8` narrowing in `present` falls back to `1` if a code ever
exceeds the process range (`main.rs:873`).

## Boundaries

**Production and qualification are not argv-selectable.** Three exported
functions exist only for tests and harnesses, and none has a flag
(`lib.rs:193-227`):

| Seam | Signature | Injects |
|---|---|---|
| `commands_doctor_run_with_registry` | `(&Path, Option<InputFormat>, &[DriverInventoryEntry])` | A driver inventory |
| `commands_run_with_qualification` | `(…, CommandOptions, FakeWorkload, signal)` | A fake workload and the cancellation signal future |
| `commands_run_forward_signal` | `(CancellationToken, signal)` | Nothing; exposes the signal forwarder |

The rule is in the doc comments: production uses `execute`, and no public
production flag selects the fake path (`lib.rs:195-196`, `lib.rs:207-209`). The
binary never calls any of the three — `main` calls `execute` (`main.rs:154`)
and `execute` calls the production command entries (`lib.rs:131-172`). The fake
executor is reachable only by holding a `FakeWorkload` from
`eggbench_runner::test_support`, so selecting it requires compiling against the
seam, not typing a flag. A fake selectable from argv would be one mistyped
flag away from producing evidence, and the evidence contract assumes the
driver came from the production catalog.

**Signals are a runner concern, not a CLI concern.** `main` installs no
handler. `run` passes a `wait_for_ctrl_c()` future into the runner seam
(`commands/run.rs:275`, `commands/run.rs:511-512`); the forwarder turns one
completed signal future into a `CancellationToken` cancel and is aborted when
the run finishes (`commands/run.rs:443-460`, `commands/run.rs:486-501`). The
CLI adds no listener, process, or force-kill path, and the same forwarder is
what the test seam exposes, so cancellation is tested deterministically rather
than by delivering a real signal.

**Presentation is the only writer.** Commands return values; they never print.
That is what makes the envelope shape a library-visible contract rather than a
binary implementation detail.

**The CLI owns no driver logic.** Feature flags forward only
(`crates/eggbench-cli/Cargo.toml:15-20`); the inventory lives in
`workload_registry.rs` as a view over `eggbench-drivers`.

**`qualify` owns its own output contract.** It emits no `CliEnvelope`, routes
human output to stdout rather than stderr, and uses its own JSON shapes and
exit codes (`main.rs:205`, `main.rs:211-215`, `main.rs:493-506`,
`main.rs:649-654`). It is a harness surface, not a compatibility surface, and
nothing in the crate depends on its JSON.

## Tests and qualification

The in-crate qualification e2e test (`main.rs:677-792`,
`#[cfg(all(test, feature = "eggstack-http"))]`) drives `qualify_run` and
`qualify_inspect` directly rather than spawning the binary, and asserts on the
status the handler returns: exit `6` for a fail aggregate
(`main.rs:741`), `StdExitCode::SUCCESS` for the inspection of an untampered
receipt (`main.rs:777-780`), and exit `8` after rewriting a scenario's
`candidate_bundle_path` to an escaping relative path
(`main.rs:786-789`). That last assertion is the confinement rule at
`main.rs:659-675` under test, exercised through the public inspection path.

Note what that test does *not* cover: the envelope's exit-code mapping is
asserted on the qualification path, which reimplements the mapping
(`main.rs:493-498`) rather than using `ExitCode`. Coverage of
`PresentedCommandResult::compare_verdict` and the
`CliError` → `CliFailure` table lives with the command and registry deep dives.

Everything above the layer is testable because `execute` is an ordinary
`async fn` returning a value and only `main` converts to a process status
(`main.rs:865-871`). The qualification seams are `pub` rather than
`#[cfg(test)]` so external harnesses can link them, which is the deliberate
cost of that boundary.

## Review focus

- **Any change to `ok` semantics.** Because `ok` and the exit code are coupled
  only by convention (`envelope.rs:236-238`), a new "non-success but not
  failure" outcome must be built as a `PresentedCommandResult` constructor
  with an explicit code, and must be checked against
  [docs/cli.md](../docs/cli.md)'s exit table.
- **A new `CliError` variant.** It forces a decision in `into_failure`
  (`error.rs:56-123`) about which code and which category, and whether its
  category joins the network-path allowlist (`error.rs:39-51`) that
  reroutes validation errors to exit `3`. A variant added without an arm
  there will change every caller's status.
- **A new envelope field.** `deny_unknown_fields` makes it a schema event
  (`envelope.rs:154`); a bump to `CLI_OUTPUT_SCHEMA_VERSION`
  (`envelope.rs:10`) is the paired decision.
- **Format detection.** Any loosening of the exact-extension match
  (`plan_input.rs:67-69`) removes the only mechanism that keeps a wrong-format
  plan from being parsed by the wrong parser; the stdin rejection
  (`plan_input.rs:62-66`) is that rule where no extension exists.
- **A new `compare` mode.** Two files must agree: the positional resolution
  (`main.rs:842-851`) and the exclusivity checks
  (`commands/compare.rs:35-57`). A mode added to one and not the other is
  either unreachable or mis-parsed.
- **Anything that makes a command print**, or a feature added to the CLI crate
  that does not forward to `eggbench-drivers` and stay orthogonal apart from
  the declared `eggstack-path` → `eggstack-http` edge
  (`crates/eggbench-cli/Cargo.toml:15-20`).
- **Doc drift to re-check.** The command table in
  [docs/cli.md](../docs/cli.md) and the flow table in
  [overview.md](overview.md) describe the same command set; `qualify` is
  implemented in `main.rs` but appears in neither.

## Related

- [cli-commands.md](cli-commands.md) — the per-command pipelines, the
  production/qualification entry points, and each command's evidence
  requirements.
- [cli-workload-registry.md](cli-workload-registry.md) — the
  production/qualification inventory split behind the seams cited above.
- [docs/cli.md](../docs/cli.md) — the user-facing CLI contract: command
  listing, feature isolation, and the exit-code table.- [core-evidence-bundle.md](core-evidence-bundle.md) — the `.eggb` contract
  that `run` produces and `inspect` verifies.
- [core-comparison.md](core-comparison.md) — the receipt whose aggregate
  verdict selects exit `6/7/8`.
- [tooling-qualification-ci.md](tooling-qualification-ci.md) — how the
  qualification suite consumes the seams in this layer.
