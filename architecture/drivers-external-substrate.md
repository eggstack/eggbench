# External command substrate

## Purpose

`crates/eggbench-drivers/src/external/{mod,resolver,command,version,parser,artifact,preflight,common,error}.rs`
(1,926 lines) is the machinery that lets Eggbench run third-party benchmark
binaries — `oha`, `h2load`, `iperf3`, and the Eggstack-owned `eggprobe`,
`eggsec`, `eggsec-load`, `eggreplay-semantic` tools — as if they were first-class
adapters, without any of them being trusted.

This is a trust boundary, not a convenience layer. Everything downstream of
this module treats tool output as attacker-shaped data: a report parser
inherits whatever argv the plan resolved to, whatever locale the tool honoured,
and whatever the process wrote on a pipe. The module's job is to make that
data arrive inside stated bounds — a resolved absolute path, a hashed
executable, an argv vector with no shell, a hard timeout, capped capture, a
cancellation token — and to hand back a typed outcome that records what was
lost rather than silently accepting it.

The commitment that makes the rest feasible: **the parser contract is
independent of process spawning** (`parser.rs:1`, `parser.rs:39-51`). Tool
report formats are unit-testable as pure functions of a captured outcome, with
no binary installed. The `eggbench_fixture` binary covers the spawn side;
`FixtureParser` covers the parse side.

The tool adapters are separate deep dives:
`drivers-external-load-tools.md` (oha/h2load/iperf3) and
`drivers-external-oracle-tools.md` (eggsec/eggsec_load/eggreplay/eggprobe).
For the ownership split that motivates the module see `common.rs:1-7`, and for
the crate-level rationale see [drivers.md](drivers.md).

## Source layout

| Path | Role |
|---|---|
| `crates/eggbench-drivers/src/external/mod.rs` | Module tree and the public re-export inventory |
| `crates/eggbench-drivers/src/external/resolver.rs` | `BinaryResolver`, `ResolvedExecutable`: trusted executable resolution and identity |
| `crates/eggbench-drivers/src/external/command.rs` | `run_command`: argv-only bounded execution, capture caps, timeout/cancel cleanup |
| `crates/eggbench-drivers/src/external/version.rs` | `VersionProbe`, `VersionProbeSpec`, `ToolVersion`: bounded argv-only version probe |
| `crates/eggbench-drivers/src/external/preflight.rs` | `external_binary_present`, `executable_path_for`, `probe_external_workload` |
| `crates/eggbench-drivers/src/external/parser.rs` | `ExternalOutputParser` trait, `ParsedExternalOutput`, `FixtureParser` |
| `crates/eggbench-drivers/src/external/artifact.rs` | `artifact_candidates`, `command_metadata_json`, `workload_output_from_outcome` |
| `crates/eggbench-drivers/src/external/common.rs` | Crate-internal adapter helpers: env policy, URL authority, version floor, category mapping |
| `crates/eggbench-drivers/src/external/error.rs` | `DriverError`, `ErrorCategory`: stable redaction-safe taxonomy |
| `crates/eggbench-drivers/src/catalog.rs` | The seven unconditional descriptors this substrate feeds |
| `crates/eggbench-drivers/src/bin/eggbench_fixture.rs` | Deterministic test-only fixture executable (spawn-side tests) |

`mod.rs:7-21` is the whole tree. Eight of the nine modules are private
(`artifact`, `command`, `common`, `error`, `parser`, `preflight`, `resolver`,
`version`); the four tool modules are `pub`. `mod.rs:23-69` re-exports a
curated surface: `artifact`, `command`, `error`, `parser`, `preflight`,
`resolver`, `version`.

`common.rs` is the one module with **no re-export at all** (`mod.rs:9`,
absent from `mod.rs:23-69`). Its helpers are a crate-internal adapter API, not
part of the substrate's public surface. That is deliberate and load-bearing:
the env allowlist, the version floor, and the failure-category mapping are
policies an adapter must not re-derive, but they are not contracts external
consumers can depend on.

The seven external-process descriptors register unconditionally
(`catalog.rs:59-67`) and are the only inventory a featureless build contains
(`catalog.rs:41-47`). See
[drivers-catalog-features.md](drivers-catalog-features.md).

## Key types

### `ResolvedExecutable` and `BinaryResolver`

`crates/eggbench-drivers/src/external/resolver.rs:24-37` and `:40`

`ResolvedExecutable` is executable *identity*, not executable *approval*: "diagnostic
provenance, not a trust signature" (`resolver.rs:22`). It carries the logical
tool name, the selected path as discovered, the canonical path after symlink
resolution, the SHA-256 of the canonical bytes, the file size, and a platform
class (`resolver.rs:26-36`). Both selected and canonical paths are recorded
because the difference is evidence: it says whether the thing on `PATH` was a
symlink.

`BinaryResolver::resolve` (`resolver.rs:52-70`) has three inputs and one exit: a
requested bare name, an optional explicit path, an optional `PATH`-style search
string. The name is rejected outright if empty or if it contains `/` or `\`
(`resolver.rs:57-65`) — a name that looks like a path never reaches the search.

### `ExternalCommandSpec` and `ExternalCommandOutcome`

`crates/eggbench-drivers/src/external/command.rs:32-55` and `:119-138`

The spec is protocol-neutral: resolved executable, `args: Vec<OsString>`,
optional `cwd`, an explicit `env: BTreeMap<OsString, OsString>`, stdin mode,
independent stdout/stderr caps, and a `timeout`. No field can hold a shell
string, and no field means "inherit the environment".

The outcome deliberately does **not** mirror the spec. It carries `argc: usize`
rather than the argument vector (`command.rs:122-123`), so an outcome can be
`Debug`-printed, serialized into evidence, or logged without leaking argv
values. It carries the resolved executable identity, the exit code, both
`CapturedStream`s, monotonic `duration`, the `cancelled` / `timed_out` flags,
and `cleanup_notes` (`command.rs:120-137`).

### `CapturedStream`

`crates/eggbench-drivers/src/external/command.rs:60-66`

Five facts about one pipe, all derived: retained bytes, retained count, dropped
count, total drained, and a `truncated` flag set whenever `total > retained`
(`command.rs:104-114`). Truncation is a first-class, inspectable property
rather than an error — a tool emitting 10 MB of JSON and one emitting 10 KB are
distinguishable from the outcome alone.

### `ToolVersion` and `VersionProbeSpec`

`crates/eggbench-drivers/src/external/version.rs:28-43` and `:13-24`

`ToolVersion` is version *provenance*: the logical tool, the full resolved
executable identity, both bounded streams, the exit code, the parsed version
string, and the `parser_id` expected to interpret this tool's report format
(`version.rs:30-42`). The version string alone would be a claim; the retained
bytes beside it are the evidence, and they go into the trial.

### `ExternalOutputParser`

`crates/eggbench-drivers/src/external/parser.rs:39-51`

Two methods. `parser_id() -> &'static str` (`:41`) is the stable
`<tool>/<format>/<version>` identity — `FIXTURE_PARSER_ID` is
`"eggbench-fixture/v1"` (`parser.rs:7`), and production ids follow the same
shape (`oha-json/v1`, `h2load-text/v1`, `iperf3-json/v1`). `parse` takes
`&ExternalCommandOutcome` and returns `Result<ParsedExternalOutput, DriverError>`
(`:50`): a pure function with no process, no I/O, no ambient clock, and no
mutation of the invocation.

`ParsedExternalOutput` (`parser.rs:14-21`) is deliberately thin — parser id,
tool version seen in the report, truncation flag. Adapters widen it into
`RawMetricObservation`, histograms, error counts, or extra artifacts
(`parser.rs:10-12`).

### `WorkloadArtifact` candidates

`crates/eggbench-drivers/src/external/artifact.rs:13-34`

Three fixed names, always the same three, in the same order: `stdout.raw`,
`stderr.raw`, `command-metadata.json` (`artifact.rs:19-32`). The names are
single path components with no separators, and a test asserts exactly that
(`artifact.rs:115-121`), because these become paths inside an immutable bundle.

## How it works

### Resolution: absolute paths, and what `PATH` search still means

The stated policy is explicit absolute paths only
(`resolver.rs:5-7`, `docs/external-drivers.md`). An explicit path that is
relative is rejected as `UntrustedSearchPath` (`resolver.rs:82-87`), and an
empty one as `BinaryNotFound` (`resolver.rs:76-81`).

`PATH` search still exists, and the reason matters: the CLI pins a canonical
path into resolution *from* a successful resolve
(`preflight.rs:54-73`), and `doctor` needs to answer "is this tool here?"
without a plan. So `resolve_search` (`resolver.rs:91-142`) enumerates
components **manually** rather than delegating to the platform lookup — and
manual enumeration is the only way to get the security property the module
doc claims at `resolver.rs:8-9`:

```rust
for component in raw.split(separator) {
    if component.is_empty() {
        continue;
    }
    let dir = Path::new(component);
    if dir.is_relative() {
        continue;
    }
```

`resolver.rs:105-112`. Two skips, both load-bearing:

- **Empty components.** In POSIX semantics an empty `PATH` component *means
  the current directory*. Skipping it means a tool sitting in the cwd can
  never be selected. There is a test that constructs exactly that situation —
  writes `cwd-tool` into a tempdir, then searches with `":/nonexistent…"` —
  and asserts `BinaryNotFound` rather than a pick-up
  (`resolver.rs:316-336`).
- **Relative components.** A relative entry is resolved against the cwd at
  search time, so its contents are attacker-influenceable by anyone who can
  write to the working directory. Skipping them keeps the search set to
  administrator- or image-controlled absolute directories
  (`resolver.rs:338-347`).

Acceptance is also stricter than "file exists". On Unix a candidate must be a
regular file **with at least one execute mode bit** (`resolver.rs:202-212`,
re-checked at `resolver.rs:157` via `resolver.rs:214-230`). On Windows the
search tries only the bare name, `.exe`, and `.com` (`resolver.rs:120-129`);
`.bat`/`.cmd` wrappers are rejected because they require command-interpreter
semantics (`resolver.rs:11-14`). That rejection is applied twice — once under
`cfg(windows)` including `.ps1` (`resolver.rs:164-176`), then again
unconditionally for `.bat`/`.cmd` on every platform (`resolver.rs:180-188`).
The comment at `resolver.rs:177-179` calls the second block "defensive", but
the consequence is asymmetric: a `.ps1` is refused on Windows and accepted on
Unix if it carries exec bits.

`identify` (`resolver.rs:144-199`) then establishes identity: metadata must
resolve and report a regular file (`resolver.rs:145-156`), the path is
canonicalized (`resolver.rs:158-163`), and the **canonical** target is hashed
in 8 KiB chunks with SHA-256 alongside its size (`resolver.rs:189`,
`resolver.rs:243-268`). The hex encoder is a 13-line private module
(`resolver.rs:291-303`) rather than a dependency. Classification is
`unix-executable` off Windows, `windows-exe` / `windows-com` on it
(`resolver.rs:270-288`).

### The ordering guarantee: a missing binary fails before managed startup

This is the single most important structural property of the module, and it is
a consequence of *where* the two probes are called rather than of an explicit
check.

`BinaryResolver::resolve` is fully synchronous — it does filesystem metadata,
canonicalization, and a full file hash, with no async and no process
(`resolver.rs:52-199`). The adapters call it from their **executor factory**,
and `preflight.rs:1-8` states the placement directly: "Binary resolution is
synchronous and runs in the executor factory (so a missing binary fails before
managed startup)".

Why that ordering is load-bearing rather than cosmetic:

1. **No orphaned lifecycle.** A missing or unusable binary is discovered
   before the runner has started an origin service, allocated a port, or
   spawned anything it would then have to tear down. There is no
   half-started run to unwind.
2. **A distinct failure signal, not a workload failure.** Inside a trial,
   `failure_category` (`common.rs:92-99`) maps everything except `Cancelled`
   and `TimedOut` to `FailureCategory::WorkloadFailed` — so a
   `BinaryNotFound` raised during a trial would be indistinguishable from a
   tool that ran and failed its load. Pre-startup, the same condition is
   reported as a capability error (`missing_executable_path`,
   `docs/external-oracles.md:21-25`).
3. **The digest is pinned into the resolved plan before anything runs.**
   `executable_path_for` (`preflight.rs:61-73`) resolves once and returns
   `canonical_path` (`:72`) so the caller can place it in the driver's
   `executable_path`, which makes it a *requirement* of resolution rather than
   a convenience. When no path can be resolved, the value is `None` and
   resolution fails with `MissingExecutablePath` — the explicit
   missing-binary signal (`preflight.rs:58-59`).

The version probe rides the same pre-startup boundary but takes a different
route, because it must spawn a process.

### `version.rs` versus `preflight.rs`

A prior review flagged the split as confusing; the distinction is *what is
being asked and when it can be asked*.

| | `version.rs` | `preflight.rs` |
|---|---|---|
| Question | "What version is this binary, and is it acceptable?" | "Can this driver be used at all, before startup?" |
| Cost | spawns one bounded process per tool | resolution is filesystem-only; probing is the spawn |
| Reached from | `VersionProbe::run`, called by each adapter's `probe()` | the CLI `run` preflight and the `doctor` inventory |
| Blocking? | no — async, cancellable, bounded | resolution: yes; probe: async |

`VersionProbe::run` (`version.rs:56-111`) builds a plain
`ExternalCommandSpec` with the probe argv tail, `cwd: None`, the deterministic
`LC_ALL=C`/`LANG=C` environment (`version.rs:115-120`), **stdin null**
(`version.rs:70`), the probe's own caps, and its own short timeout
(`version.rs:65-75`). Three rules make it safe:

- Execution errors are re-categorized: `TimedOut`/`Cancelled` from the
  command layer become `VersionProbeTimeout`, everything else becomes
  `VersionProbeFailed` (`version.rs:76-85`).
- **Nonzero exit is unconditionally a probe failure** (`version.rs:86-91`).
  The doc comment notes that an `allow_nonzero` escape hatch is described but
  unused in this milestone (`version.rs:52-55`).
- The version must be extracted from *retained* bytes
  (`version.rs:92-98`). `parse_version_token` (`:123-139`) takes the first
  whitespace/comma-delimited token that begins with an ASCII digit and
  contains a `.`. If the token was not retained, the probe fails rather than
  reporting a version read from truncated output. Truncation elsewhere in the
  stream is acceptable *because the token was found*, and stays visible via
  the retained `CapturedStream` carried on `ToolVersion`
  (`version.rs:99-101`, `:34-36`).

`preflight.rs` sits above that and answers the driver-level question.
`is_external_workload` (`preflight.rs:25-34`) names the five external
*workload* drivers — `oha`, `h2load`, `iperf3`, `eggreplay-semantic`,
`eggsec-load`. `external_binary_present` (`preflight.rs:41-52`) is the
`doctor` probe: **filesystem-only, no process spawned** (`preflight.rs:36`).
It covers all seven external drivers including the diagnostic `eggprobe` and
the correctness `eggsec-waf`, and returns `None` for in-process drivers so one
boolean field serves every category. `probe_external_workload`
(`preflight.rs:80-123`) resolves, probes, **and constructs the adapter** —
`OhaWorkload::new(executable, probed.version)` (`:92`) and its three siblings
discard the constructed value purely so the adapter's version floor runs
during preflight. `eggsec-load` is the exception: it probes and returns without
constructing (`preflight.rs:85-88`). Anything not matched is
`BinaryNotFound` (`preflight.rs:118-121`); `eggprobe` and `eggsec` have their
own dedicated preflights exported at `mod.rs:32` and `:44`.

Executors additionally self-probe on first execution so direct `execute_run`
consumers get the same floor without going through the CLI
(`preflight.rs:5-8`).

### The version floor: from string to accept/reject

`check_min_version` (`common.rs:138-160`) is the accept/reject policy. It
parses only the leading numeric `major.minor.patch` triple, ignoring extras
(`common.rs:162-188`, documented at `common.rs:135-137`), and compares
lexicographically. Two properties are deliberate:

- **Unparseable fails closed.** If any component yields no digits,
  `parse_version_tuple` returns `None` and the floor is not satisfied
  (`common.rs:143-159`). `"not-a-version"` is `UnsupportedVersion`, not a pass.
  Test: `common.rs:229`.
- **The boundary is inclusive at the patch level** and exclusive above it
  (`common.rs:146-148`), which is the correct lexicographic `>=` on the
  triple. `1.0.0` satisfies a `(1,0,0)` floor; `0.9.9` does not
  (`common.rs:224-226`).

The comparison is fed by a *string*, not by a structured version type. That is
the substrate's single weakest link in the version chain and the reason
`h2load` needed a bespoke extraction path — the generic token scanner cannot
isolate the nghttp2 release from `h2load nghttp2/<release>`
(`docs/external-oracles.md:63-65`).

### Bounded execution

`run_command` (`command.rs:151-303`) is one auditable sequence, annotated as
such at `command.rs:150`.

**Pre-spawn.** Oversized stdin is refused before any process exists:
`MAX_STDIN_BYTES` is 64 KiB (`command.rs:28`) and the check returns
`UnsupportedOption` (`command.rs:155-164`). The comment at `command.rs:24-27`
gives the rationale: generated machine plans are small deterministic JSON, so
a larger payload is a bug or an attack, not a big plan — and it is rejected
rather than streamed.

**Construction.** argv[0] is the **canonical** path, not the selected one
(`command.rs:166`), so a symlink is resolved away before exec. Arguments are
passed as argv, never through a shell (`command.rs:167`); a unit test
constructs a spec whose args are `"; rm -rf /"` and `"$(evil)"` and asserts
they remain two literal argv entries (`command.rs:393-410`). The environment
is cleared and then repopulated from the explicit map
(`command.rs:168-171`) — the user environment is never inherited wholesale
(`command.rs:4-8`). `cwd` is set only when supplied (`command.rs:172-174`).
stdout and stderr are always piped (`command.rs:180-181`); stdin is piped when
a payload is present, otherwise null (`command.rs:175-179`).

**Process group.** On Unix, `process_group(0)` puts the child in a new group
(`command.rs:182-189`) with the explicit comment that this reuses the runner
session's signal mechanism rather than introducing a second one.
`kill_on_drop(true)` (`command.rs:191`) is the backstop if the reaping path is
ever missed. On Windows, the outcome is annotated `direct_child_only`
(`command.rs:281-290`) and the module doc states plainly that Job Object
process-tree semantics are never claimed (`command.rs:10-12`).

**Draining.** One task per pipe (`command.rs:218-229`). `drain_stream`
(`command.rs:319-340`) keeps reading after the cap is reached and only stops
*retaining* — `room` goes to zero, `total` keeps climbing
(`command.rs:327-335`). This is the property that makes a cap safe: stopping
the read at the cap would block the child forever on a full pipe and turn an
output bound into a hang.

**Waiting.** `select!` over three futures: child exit, cancellation token, and
an explicit `sleep(spec.timeout)` (`command.rs:232-239`). There is no default
timeout anywhere — a caller that forgets one gets a hang, not a surprise kill.

**Cancellation and timeout.** Both arms converge on the same cleanup
(`command.rs:248-272`): abort the stdin writer (`command.rs:250-252`),
`terminate_child`, then a **bounded** five-second re-wait so pipes drain and
the child is reaped (`command.rs:254-256`), then drain the pipe tasks so a
cancelled child cannot block on a full pipe (`command.rs:257-258`). Only then
is a typed `TimedOut` or `Cancelled` error returned
(`command.rs:259-271`). `terminate_child` on Unix sends `SIGTERM` to the
process **group**, waits 500 ms, then sends `SIGKILL` to the group
unconditionally (`command.rs:357-370`). The `killpg` call uses the child's own
PID as the PGID (`command.rs:362`) — correct precisely because
`process_group(0)` set PGID = PID, so the two must be read as a pair. The
unconditional `SIGKILL` is explained at `command.rs:365-367`: liveness races
make a conditional kill unreliable. The non-Unix arm kills only the direct
child (`command.rs:372-375`).

**Outcome.** `exit_code` is `s.code()`, so a signal-terminated child yields
`None` rather than a synthetic negative code (`command.rs:280`).

### Target confinement

Resolution pins *which binary*; confinement pins *what it may be pointed at*.
The two are separate controls and both live in the external module.

`confine_target_url` (`eggsec.rs:471-497`) accepts only the local/private
target class: the scheme must be `http` or `https` (`eggsec.rs:472-475`), the
authority must parse (`eggsec.rs:477-478`), the host must be 1–253 characters
(`eggsec.rs:479-481`), and the host must either be an IP in the confined set
(`eggsec.rs:482-489`) or an explicitly local name — `localhost` or
`*.localhost` (`eggsec.rs:490-496`). `is_confined_ip`
(`eggsec.rs:504-515`) covers `127.0.0.0/8`, RFC1918, and IPv6 ULA, and
deliberately excludes link-local, because a zone-scoped literal cannot be
matched against a scope manifest unambiguously — so it fails closed
(`eggsec.rs:499-503`).

`lower_target` (`eggprobe.rs:346-…`) is the softer, differently-motivated
version. It derives the diagnostic target from **startup-established runtime
bindings**, not from a plan string (`eggprobe.rs:336-341`): it requires the
target's `http_url` binding (`eggprobe.rs:356-361`), runs it through
`require_loopback_url` (`eggprobe.rs:362`), and refuses to infer TLS from a
port number — a requested TLS probe without an explicit `https_url` binding
fails when required and is recorded as unavailable when optional
(`eggprobe.rs:370-395`).

The helper both rely on is `authority_host_port` (`common.rs:46-80`), which
parses `host[:port]` without a URL dependency, honours IPv6 brackets
(`common.rs:58-71`), takes the text after the last `@` so userinfo cannot
smuggle a host (`common.rs:52-54`), and rejects rather than guesses: empty
authority, malformed IPv6, trailing text, empty host, and non-numeric ports
all error (`common.rs:55-79`, `common.rs:82-85`). Tests cover loopback with
and without a port, `::1`, and three malformed forms (`common.rs:203-220`).

### Artifacts, and where normalization is *not*

`artifact_candidates` (`artifact.rs:13-34`) turns one outcome into exactly
three artifacts. The module doc states the rule the evidence design depends
on: "No tool-specific metric normalization happens here" (`artifact.rs:1-2`).

`command-metadata.json` (`artifact.rs:53-86`) is the deterministic record that
makes a trial self-describing: `schema_version: 1`, logical tool, executable
SHA-256, executable size, `argc`, exit code, retained/dropped/truncated counters
for both streams, plus the `parser_id` when one applies
(`artifact.rs:70-84`). Note what is absent: the executable **path**, the
version string, and every argv value. `argc` is a count, not a vector
(`artifact.rs:75`). Serialization failure degrades to `"{}"` rather than
panicking (`artifact.rs:85`).

`workload_output_from_outcome` (`artifact.rs:38-49`) is the metrics-free
constructor: artifacts plus four empty collections. Normalization happens later
and elsewhere — in `eggbench-core`'s `metrics` module, under the normalize-once
rule that only `core::metrics` writes `TrialMetrics` ([overview.md](overview.md),
[core-metrics.md](core-metrics.md)). Adapters map their parser output into
`RawMetricObservation`; nothing in this module interprets a number.

### Errors: what "redaction-safe" requires concretely

`ErrorCategory` (`error.rs:10-41`) is a 15-variant flat enum with stable
`snake_case` labels (`error.rs:46-64`) and a `Display` that emits only the
label (`error.rs:67-71`). The doc comment is the contract: "Categories form
the compatibility surface; human detail may change but category identities
must remain stable" (`error.rs:5-8`).

`DriverError` (`error.rs:78-111`) has four variants — `Resolution`, `Probe`,
`Execution`, `Parse` — each carrying a category and a detail string. The
variant distinguishes *which stage* failed; the category distinguishes *what
went wrong*; the detail is explicitly the only part allowed to change
(`error.rs:75-76`).

What "redaction-safe" means here, verified against the construction sites:

- **Never echo tool stdout or stderr into a `DriverError`.** Every
  `DriverError` construction in the substrate formats a logical tool name, a
  filesystem path, an io error, or a duration — `command.rs:196`,
  `command.rs:244`, `command.rs:262-265`, `version.rs:89`,
  `version.rs:96`, `common.rs:154-157`, `parser.rs:80`, `parser.rs:85`. No
  retained stream bytes appear in any detail string. This is the concrete
  requirement: a tool that prints a bearer token, a signed URL, or an internal
  hostname cannot launder it into an error message.
- **Never echo argv values.** `ExternalCommandOutcome` stores `argc`
  (`command.rs:122-123`) precisely so that neither the outcome nor anything
  derived from it needs the argument vector.
- **Version-probe re-wrapping is lossless in the safe direction.**
  `VersionProbe::run` re-wraps a `DriverError` by calling `.to_string()` on it
  (`version.rs:81`, `version.rs:83`), which renders only the already-safe
  category and detail.
- **Paths *are* echoed.** Resolution errors interpolate
  `selected.display()` (`resolver.rs:148`, `resolver.rs:154`,
  `resolver.rs:161`, `resolver.rs:220`, `resolver.rs:226`, `resolver.rs:237`)
  and the authority parser interpolates the URL and its host
  (`common.rs:56`, `common.rs:63`, `common.rs:75`;
  `eggsec.rs:474`, `eggsec.rs:487`). The confinement errors are the
  deliberate exception: the *host* is the finding, so it must be reported.
  These are local filesystem paths and target authorities, not secret values;
  the taxonomy's guarantee is about tool output and credentials, not about
  suppressing all local paths.
- **`Debug` is a separate obligation, and it is only half-held.** See
  [Invariants](#invariants), item 8.

`failure_category` (`common.rs:92-99`) is the narrowing step: `Cancelled` and
`TimedOut` survive; resolution, probe, spawn, nonzero exit, and parse all
collapse to `WorkloadFailed` with the stable category preserved in the detail
string. `probe_failure_category` (`common.rs:121-130`) adds one correction
that is easy to miss: because `VersionProbe` folds cancellation into
`VersionProbeTimeout` (`version.rs:77-81`), the token must be re-checked
directly, or a cancelled run would be recorded as a workload failure. The
token check is first (`common.rs:126-128`) and there is a dedicated test
(`common.rs:258-276`).

### The ownership split

`common.rs:1-7` states the division, and it is the reason this module is small:

> Tool ownership: oha/h2load/iperf3 own load generation and their
> machine-output semantics. Eggbench owns binary resolution, version policy,
> argv construction from the plan workload, bounded execution, raw retention,
> metric mapping, and evidence. No tool output format is redefined here;
> parsers validate the documented shapes and fail closed.

The split is right because the two halves have opposite failure modes. Tool
output formats drift on their own release schedule and are outside Eggbench's
control; that variation must be absorbed by a small, swappable, per-tool parser.
Process spawning, environment policy, timeouts, and reaping are security and
lifecycle properties where *uniformity* is the property that matters — one
place to audit, one place to fix, no per-tool divergence. Every additional
in-process detail Eggbench takes ownership of is a future CVE surface it cannot
delegate upstream. `parser.rs:50` makes the contract explicit from the parser's
side: "A parser must never mutate the invocation into a different
load/protocol semantic."

Violating it in either direction is expensive. If a parser redefines a tool's
semantics, the substrate stops being a trust boundary and becomes one adapter
with a wrapper. If a tool adapter re-implements resolution or timeout handling,
there is no longer a single point where "no shell, cleared environment, explicit
timeout, capped capture" is enforced — and that point is the reason the module
exists.

## Invariants

1. **No shell, ever.** There is no code path from a plan, workload, or tool
   report to a shell string. `ExternalCommandSpec` has no such field; argv
   entries are passed literally (`command.rs:167`, `command.rs:393-410`);
   `.bat`/`.cmd` wrappers are refused at resolution
   (`resolver.rs:180-188`).
2. **`PATH` search never resolves against the current directory.** Empty and
   relative components are skipped (`resolver.rs:105-112`); explicit relative
   paths are rejected outright (`resolver.rs:82-87`).
3. **Explicit paths are absolute, and resolution is synchronous.** Every
   failure mode of `BinaryResolver` occurs before any process exists
   (`resolver.rs:52-199`; `preflight.rs:4-5`).
4. **The environment is cleared, then repopulated from an explicit map**
   (`command.rs:168-171`). The only values the substrate supplies are
   `LC_ALL=C` and `LANG=C` (`common.rs:21-26`, `version.rs:115-120`), so tool
   output is locale-deterministic.
5. **A timeout is always explicit.** `timeout: Duration` is a required spec
   field with no `Default` (`command.rs:53-54`) and no fallback
   (`command.rs:232`).
6. **Capture is bounded, and draining never stops at the cap.** A retained
   bound is never a read bound (`command.rs:319-340`); truncation is recorded
   explicitly rather than being an implicit short read (`command.rs:104-114`).
7. **A nonzero exit is a typed outcome, not a silent pass**
   (`command.rs:147-149`, `command.rs:280`); the version probe treats it as an
   unconditional failure (`version.rs:86-91`). It is *not* automatically a
   trial failure for load tools: `oha` and `h2load` exit 0 with every request
   failed, so success is read from the report, not the status
   (`docs/external-oracles.md:53-57`, `docs/external-oracles.md:80-82`).
8. **Redaction-safe means no tool output and no argv in errors, and `Debug`
   must be held to the same standard.** `ExternalCommandOutcome` satisfies it
   by construction (`command.rs:122-123`). `ExternalCommandSpec` does **not**:
   it derives `Debug` (`command.rs:31`) and holds `args` and `env` verbatim
   (`command.rs:36`, `command.rs:41`). The built-in environment is only
   `LC_ALL`/`LANG`, so nothing leaks today, but the type is public
   (`mod.rs:25`) and a future adapter could pass a richer environment.
9. **Parsing is a pure function of a captured outcome.** No process, no I/O,
   no clock (`parser.rs:39-51`). A parser may fail but may not reinterpret the
   load (`parser.rs:49-50`).
10. **Raw bytes are retained before interpretation.** `stdout.raw` and
    `stderr.raw` are produced from the outcome with no tool-specific reading
    (`artifact.rs:19-27`), and truncation counters are always recorded
    (`artifact.rs:77-82`).
11. **Only `core::metrics` writes normalized metrics.** This module produces
    artifacts and raw observations only (`artifact.rs:38-49`; see
    [core-metrics.md](core-metrics.md)).
12. **Child processes cannot outlive the wait.** A dedicated process group on
    Unix (`command.rs:182-189`), `kill_on_drop` as backstop
    (`command.rs:191`), bounded re-wait (`command.rs:254-256`), and an honest
    `direct_child_only` note on Windows (`command.rs:281-290`).
13. **Cancellation and timeout stay first-class.** Both map to their own
    `FailureCategory` (`common.rs:94-97`), and cancellation is re-detected for
    probes (`common.rs:126-128`).

## Failure model

| Condition | Category | Where | Reported as |
|---|---|---|---|
| Empty or path-shaped tool name | `BinaryNotFound` | `resolver.rs:57-65` | resolution failure |
| Relative explicit path | `UntrustedSearchPath` | `resolver.rs:82-87` | resolution failure |
| Not found in any trusted absolute `PATH` component | `BinaryNotFound` | `resolver.rs:138-141` | resolution failure, pre-startup |
| Candidate missing / not a regular file / no exec bits | `NotExecutable` | `resolver.rs:145-156`, `resolver.rs:214-230` | resolution failure |
| `.bat` / `.cmd` (and `.ps1` on Windows) | `NotExecutable` | `resolver.rs:164-188` | resolution failure |
| Canonicalize or hash failed | `ExecutableIdentityFailed` | `resolver.rs:158-163`, `resolver.rs:243-268` | resolution failure |
| Spawn failed | `SpawnFailed` | `command.rs:193-198` | execution failure |
| Explicit timeout expired | `TimedOut` → `VersionProbeTimeout` for probes | `command.rs:259-267`, `version.rs:77-81` | `FailureCategory::TimedOut` |
| Cancellation token fired | `Cancelled` → `VersionProbeTimeout` for probes | `command.rs:268-271`, `version.rs:76-84` | `FailureCategory::Cancelled` |
| `child.wait()` errored | `CleanupFailed` | `command.rs:243-245` | execution failure |
| stdin payload > 64 KiB | `UnsupportedOption` | `command.rs:155-164` | rejected **before spawn** |
| Probe exited nonzero | `VersionProbeFailed` | `version.rs:86-91` | pre-startup capability failure |
| No version token in retained bytes | `VersionProbeFailed` | `version.rs:92-98` | pre-startup capability failure |
| Tool below `check_min_version` floor, or unparseable | `UnsupportedVersion` | `common.rs:152-158` | pre-startup capability failure |
| Parser rejects the outcome | `ParseFailed` | `parser.rs:50` (impls, e.g. `:70-87`) | `WorkloadFailed` |
| Target outside the confined class | *(no `DriverError`)* — returns `Err(String)` | `eggsec.rs:471-497` | pre-startup, before spawn |

Two boundaries of this table are worth stating explicitly.

**A run that times out or is cancelled produces no outcome at all.** Both
arms `return Err` (`command.rs:259-271`) after explicitly discarding the
drained bytes: `drop(join_pipes(stdout_task, stderr_task).await)`
(`command.rs:258`). So the partial stream is *not* returned. The consequence
is that `stdout.raw` cannot be produced for a cancelled or timed-out command
through this path, and the `cancelled` / `timed_out` fields on
`ExternalCommandOutcome` (`command.rs:133`, `:135`) are only ever `false` on
the one path that constructs a successful outcome (`command.rs:241-247`,
`:299-300`) — the timeout and cancellation flags cannot be observed by a
consumer, because a consumer never sees a timeout. This contradicts
`docs/external-oracles.md:111-112`, which states that "partial stdout is
still staged when the runner retains the invocation".

**A `join_pipes` timeout degrades silently.** `join_pipes`
(`command.rs:342-355`) bounds each join at five seconds and maps a timeout or
a panicking join to `(Vec::new(), 0_u64)` via `unwrap_or_default`
(`command.rs:346-353`). On the success path that produces an outcome whose
stdout is empty and whose `truncated` flag is **false** — a parser then reports
"malformed output" where the truth is "we gave up draining". The same
`unwrap_or_default` on the cancellation path is deliberate
(`command.rs:258`).

`ErrorCategory::OutputTruncated` (`error.rs:36`) is declared and has a stable
label (`error.rs:60`) but is never constructed anywhere in the crate.
Truncation is surfaced as data (`CapturedStream::truncated`) and as a parse
failure, never as this category — so a consumer matching on it matches
nothing. `ErrorCategory::NonzeroExit` is likewise never raised by the substrate
itself; it is constructed only by tool adapters
(`external/eggreplay.rs:504`, `external/eggsec.rs:649`,
`external/eggsec_load.rs:806`), consistent with the ownership split.

`ExternalParseError` (`parser.rs:25-36`) is a five-variant public enum that is
declared and re-exported (`mod.rs:62`) but never constructed: the trait
signature returns `DriverError` (`parser.rs:50`), and the only implementation
returns `ParseFailed` for every rejection (`parser.rs:70-87`).

## Boundaries

**What this module owns**

- Which binary runs, and proof of what it was: absolute-path resolution,
  `PATH` hardening, canonicalization, SHA-256 identity, platform class
  (`resolver.rs:144-199`).
- What the process sees: argv only, cleared environment with an explicit
  allowlist, null or bounded stdin, explicit cwd when supplied
  (`command.rs:166-179`).
- When the process stops and what is left behind: explicit timeout,
  cancellation, process group, bounded re-wait, honest cleanup notes
  (`command.rs:235-290`).
- What is kept: capped raw streams with explicit truncation accounting and
  deterministic metadata (`command.rs:319-340`, `artifact.rs:53-86`).
- The shape of the parse contract (`parser.rs:39-51`).

**What it deliberately does not own**

- Any tool's report format. Each tool ships its own `ExternalOutputParser`
  with its own `<tool>/<format>/<v>` id; the substrate only fixes the
  signature and the truncation obligation.
- Metric vocabulary and normalization. `artifact.rs:1-2` and
  `artifact.rs:38-49`; see [core-metrics.md](core-metrics.md).
- Load generation and workload→argv mapping. `common.rs:1-7`. A plan
  dimension with no honest tool flag fails with `UnsupportedOption` rather
  than being coerced (`docs/external-oracles.md:91-95`).
- Managed process lifecycle. Services, readiness, teardown ordering, and
  reverse shutdown belong to `eggbench-runner`
  ([runner-process-lifecycle.md](runner-process-lifecycle.md)); the substrate
  reuses the same process-group signal mechanism rather than duplicating it
  (`command.rs:184-187`).
- The driver inventory. `catalog.rs` registers the seven external descriptors
  unconditionally and knows nothing about resolution
  ([drivers-catalog-features.md](drivers-catalog-features.md)).
- Credential handling. The substrate has no secret path at all; that lives in
  the runner's secret indirection ([runner-evidence-prep.md](runner-evidence-prep.md)).

**Where the trust boundary is honest about its limits**

- The digest is provenance, not a signature (`resolver.rs:22`). It is
  computed at resolution and written into `command-metadata.json`
  (`artifact.rs:73`). Nothing re-checks the digest at exec time, and
  `run_command` spawns `canonical_path` (`command.rs:166`) without
  re-hashing. A binary replaced between resolution and spawn is recorded with
  the *old* digest. Eggbench's claim is "this is what we resolved and hashed",
  not "this is what ran".
- Windows cleanup is direct-child-only, and the outcome says so
  (`command.rs:281-290`, `command.rs:10-12`). A Windows tool that spawns
  descendants may leave them behind on timeout.
- `UntrustedSearchPath` is raised for an untrusted *explicit* path
  (`resolver.rs:84`) but not for an untrusted `PATH` string: relative and
  empty components are silently *skipped*
  (`resolver.rs:105-112`) rather than rejected. The behavior is safe — nothing
  untrusted is used — but a `PATH` containing only relative entries produces
  `BinaryNotFound`, not a diagnostic that `PATH` was untrusted.
- Confinement (`eggsec.rs:471-497`, `eggprobe.rs:346-…`) is a per-adapter
  policy in the tool modules, not a substrate guarantee. A new adapter that
  forgets to confine is not stopped by this module. See
  `drivers-external-oracle-tools.md`.
- `external_binary_present` (`preflight.rs:41-52`) collapses every
  resolution error to a single `false` via `.is_ok()`. A binary that exists
  but lacks exec bits, or is a `.bat` wrapper, is indistinguishable from one
  that is not installed in the `doctor` inventory.
- `executable_path_for` returns `canonical_path.to_string_lossy()`
  (`preflight.rs:72`). A non-UTF-8 path is lossy-converted, and the mangled
  string would then be fed back through `BinaryResolver::resolve` as an
  explicit path, which would fail with `NotExecutable` rather than resolving.

## Tests and qualification

Unit tests live in-module. The ones that encode policy rather than mechanics:

**Resolution policy** (`resolver.rs:305-381`)

| Test | Asserts |
|---|---|
| `explicit_relative_path_is_rejected` (`:310-314`) | `UntrustedSearchPath` for a relative explicit path |
| `empty_path_component_cannot_resolve_cwd_executable` (`:316-336`) | An empty `PATH` component is skipped; a cwd-adjacent file is never picked up |
| `relative_path_component_is_skipped` (`:338-347`) | A relative `PATH` component is skipped |
| `non_executable_file_is_rejected_on_unix` (`:349-364`) | A `0o644` file is `NotExecutable` |
| `batch_wrapper_is_rejected` (`:366-373`) | `tool.bat` is `NotExecutable` |
| `argv_metacharacters_are_not_expanded` (`:375-381`) | `"tool; rm -rf /"` is a not-found name, not a command |

**Execution contract** (`command.rs:377-422`)

| Test | Asserts |
|---|---|
| `captured_stream_truncation_is_explicit` (`:381-391`) | retained / dropped / total / `truncated` arithmetic |
| `command_spec_is_argv_only_by_construction` (`:393-410`) | Two argv entries stay two entries; no shell string exists in the type |

**Parser contract** (`parser.rs:114-190`)

| Test | Asserts |
|---|---|
| `fixture_parser_accepts_version` (`:161-168`) | `eggbench-fixture 1.2.3` → `tool_version == "1.2.3"`, `parser_id` matches |
| `fixture_parser_rejects_malformed_output` (`:170-176`) | `ParseFailed` |
| `fixture_parser_rejects_nonzero_exit` (`:178-183`) | Nonzero exit is a parse failure |
| `fixture_parser_rejects_truncated_required_output` (`:185-190`) | Truncation of required output fails rather than yielding a partial parse |

These run with **no binary present**: `outcome_with_stdout`
(`parser.rs:121-137`) constructs an `ExternalCommandOutcome` by hand, and
`CapturedStream::collect` is a `pub(crate)` constructor
(`command.rs:99-101`) so a test can synthesize any retained/total pair. That
separation is the payoff of `parser.rs:1`.

**Shared helpers** (`common.rs:199-296`): loopback and malformed authority
forms (`:203-220`); version-floor accept/reject including an unparseable
string (`:222-230`); category preservation of cancel/timeout (`:232-242`);
collapse of probe/spawn/resolution/parse to `WorkloadFailed` (`:244-256`);
cancellation precedence in `probe_failure_category` (`:258-276`); and
`metric_u64_as_f64` bit-exact against the historical `as f64` cast including
the `2^53` representation limit (`common.rs:278-296`).

**Artifact naming** (`artifact.rs:94-122`): exactly three artifacts, names
non-empty and free of `/` and `\`, with `command-metadata.json` third.

**Spawn-side coverage** lives outside this module: the deterministic
test-only `eggbench_fixture` binary
(`crates/eggbench-drivers/src/bin/eggbench_fixture.rs`) covers version text,
bytes, exit status, sleep, malformed output, and child-spawn behavior, so the
same argv-only contract is qualified on Windows without shell scripts
(`docs/external-drivers.md:67-71`). See
[tooling-qualification-ci.md](tooling-qualification-ci.md) for CI and
real-binary interoperability (`.github/workflows/live-tools.yml`).

**Not covered by any test in this module**, and therefore worth review
explicitly: the timeout and cancellation cleanup paths
(`command.rs:248-272`, `command.rs:357-375`), the `join_pipes` timeout
degradation (`command.rs:342-355`), `run_command` itself
(`command.rs:151-303` has no test), `preflight.rs` (no test module at all),
and `executable_path_for`'s `to_string_lossy` round-trip (`preflight.rs:72`).

## Review focus

Ordered by what would hurt most if it regressed.

1. **Shell re-entry.** Any new field on `ExternalCommandSpec` that could hold
   a command string, or any adapter that formats argv into one. The type is
   argv-only by construction (`command.rs:32-55`); the regression is adding a
   convenience.
2. **`PATH` search hardening.** The two `continue` statements at
   `resolver.rs:106-112` are the entire anti-cwd-execution property. Removing
   either — "just use `std::env::split_paths`", "just handle the empty
   component" — silently reintroduces current-directory execution. The
   `explicit_path` branch must also keep bypassing search entirely
   (`resolver.rs:66-68`).
3. **Resolution before startup.** That `BinaryResolver::resolve` is
   synchronous and called from the executor factory (`preflight.rs:4-5`) is
   what makes a missing binary a clean pre-startup error. Making it async, or
   deferring it into the trial, would lose that with no compile error.
4. **Draining after the cap.** `drain_stream` must keep reading past `limit`
   (`command.rs:327-335`). An "optimization" that breaks out of the read loop
   at the cap converts an output bound into a hung child and a hung run.
5. **Timeout and cancellation cleanup.** The sequence abort-writer →
   terminate → bounded re-wait → drain → typed error (`command.rs:250-271`)
   must stay in that order. Removing the five-second bound on the re-wait
   (`command.rs:255`) reintroduces an unbounded wait; the `join_pipes` bound is
   the same protection for the pipes.
6. **The process-group / `killpg` pairing.** `process_group(0)`
   (`command.rs:188`) and `Pid::from_raw(child_pid)` (`command.rs:362`) are
   only correct together. If the `process_group` call were removed, `killpg`
   would signal an unrelated process group. Windows must not gain a claim of
   process-tree semantics (`command.rs:10-12`).
7. **Redaction discipline at new construction sites.** Every new
   `DriverError` detail must be checked against
   `ExternalCommandOutcome::stdout`/`stderr` and against argv values. The
   `Debug` derive on `ExternalCommandSpec` (`command.rs:31`) is the standing
   hazard: it will print argv and env. If the spec ever carries more than
   `LC_ALL`/`LANG`, that becomes a leak.
8. **Parser purity and truncation.** A parser must consult
   `outcome.stdout.truncated()` before trusting a required field
   (`parser.rs:76-87`) and must not reinterpret the load (`parser.rs:49-50`). A
   new `parser_id` is a new versioned contract and belongs in the docs table
   with the others.
9. **The version floor's fail-closed edge.** Any change to
   `parse_version_tuple` (`common.rs:162-188`) or `check_min_version`
   (`common.rs:144-148`) must keep the unparseable case on the reject side.
   Also note that `eggsec-load` is the only external workload that probes
   without constructing its adapter (`preflight.rs:85-88` vs `:89-117`), so
   whatever version floor lives in its constructor is not exercised by
   preflight.
10. **Dead and vestigial declarations.** `ErrorCategory::OutputTruncated`
    (`error.rs:36`) is never constructed; `ExternalParseError`
    (`parser.rs:25-36`) is never constructed. Either construct them for their
    documented purpose or remove them from the public surface — a consumer
    matching on `output_truncated` currently matches nothing.
11. **`CapturedStream::collect` ignores its `limit` argument**
    (`command.rs:99-101`, the parameter is literally `_limit`). Truncation is
    computed from `total - retained` in `from_parts` (`command.rs:104-114`),
    which is correct only because `drain_stream` already applied the cap. Every
    test caller passes a limit too (`parser.rs:143`, `parser.rs:147`,
    `artifact.rs:125`), hiding the fact that it does nothing. A second
    truncation site added later would silently lose the cap.
12. **Predicate scope.** `is_external_workload` (`preflight.rs:25-34`) covers
    five drivers; `external_binary_present` and `executable_path_for` cover
    seven (`preflight.rs:41-52`, `:61-73`). Do not use the narrow predicate to
    decide whether a driver needs an external binary.
13. **`cwd: None` semantics.** `run_command` sets a working directory only when
    supplied (`command.rs:172-174`), so `None` leaves the child inheriting the
    parent's cwd at the OS level. `VersionProbe::run` passes `None`
    (`version.rs:68`), and `EggReplayWorkload::new` is called with an explicit
    `std::env::current_dir()` (`preflight.rs:113`) — cwd is a real input to
    tool behavior, not a formality.
14. **No feature gates the substrate.** The external command substrate is compiled
    unconditionally: `crates/eggbench-drivers/Cargo.toml` declares only `default`,
    `eggstack-http`, `gregg`, `prometheus-http`, and `eggstack-path`, and no
    `#[cfg(feature = ...)]` in `crates/eggbench-drivers/src` guards the external
    module. A missing tool binary is therefore a resolution/preflight failure, not
    a link-time one, which is what makes the seven external drivers register on a
    no-feature build. An `external-command` feature that was once declared with no
    referent has been removed.

## Related

- [drivers.md](drivers.md) — crate-level rationale for keeping external
  tooling separate from native Eggstack drivers, and the "future integrations
  should prefer stable sibling-owned crates or process/protocol seams" rule
  from ADR-0004.
- [drivers-catalog-features.md](drivers-catalog-features.md) — the seven
  unconditional external descriptors this substrate feeds
  (`catalog.rs:59-67`) and the CI-enforced feature isolation.
- `drivers-external-load-tools.md` — `oha`, `h2load`, `iperf3`: workload→argv
  mapping and per-tool report semantics on top of this substrate.
- `drivers-external-oracle-tools.md` — `eggsec`, `eggsec_load`, `eggreplay`,
  `eggprobe`, including the confinement policy this module references.
- [core-metrics.md](core-metrics.md) — where raw observations actually become
  `TrialMetrics`; the normalize-once rule this module deliberately stops
  short of.
- [core-driver-resolution.md](core-driver-resolution.md) — why external
  drivers require a non-empty explicit executable path and the
  `ExternalBinary` capability, and why the resolver does no `PATH` search
  itself.
- [runner-process-lifecycle.md](runner-process-lifecycle.md) — the process
  group, readiness, and teardown machinery whose signal mechanism
  `command.rs:184-187` reuses rather than re-implements.
- [runner-adapter-seams.md](runner-adapter-seams.md) — the
  `WorkloadExecutor` / `DiagnosticExecutor` / `CorrectnessExecutor` traits
  these adapters implement.
- [runner-evidence-prep.md](runner-evidence-prep.md) — secret indirection and
  the absence of any secret path in this module.
- [core-qualification-security.md](core-qualification-security.md) — the
  security-correctness evidence contract that `confine_target_url` and
  `generate_scope_manifest` feed.
- [overview.md](overview.md) — the module map and the cross-cutting
  "no silent fallback" contract ("a missing external binary fails before
  managed startup").
- [../docs/external-drivers.md](../docs/external-drivers.md) — user-facing
  resolution, execution, and evidence policy.
- [../docs/external-oracles.md](../docs/external-oracles.md) — per-tool
  mapping tables, version floors, and the evidence guarantee for external
  trials.
