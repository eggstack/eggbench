# Tooling, qualification, and CI

The Rust deep dives describe what the code does. This one describes how anyone
knows a change did not break the experiment contract. The answer is a
rejected-pull-request: CI is the gate, qualification assets are the declared
expectations, live harnesses are the interoperability evidence, and the
planning registry is the scope check.

## Purpose

Four non-Rust surfaces carry that answer.

| Surface | What it decides | Failure mode it catches |
|---|---|---|
| [.github/workflows/ci.yml](../.github/workflows/ci.yml) | Whether a change compiles, lints, tests, and keeps its feature-composition and platform guarantees | Compile error, lint regression, broken driver isolation, MSRV break, platform-specific behavior change |
| [.github/workflows/live-tools.yml](../.github/workflows/live-tools.yml) | Whether the machine contracts still interoperate with real pinned sibling binaries | Adapter dialect drift, accepted-wrong-schema, wrong verdict precedence |
| [qualification/synvoid/](../qualification/synvoid/) and [examples/](../examples/) | What "correct" means, as data a reviewer can read | A run that succeeds for the wrong reason; expectations drifting to fit a candidate |
| [plans/](../plans/) | Whether a change is in scope, and what evidence would close it | Unplanned scope, an unclaimed-but-shipped boundary, a "closed" milestone nobody evidenced |

The distinction that matters throughout: **compilation is not qualification**.
A plan can compile, a driver can resolve, and a bundle can finalize while the
experiment contract is broken. Only the gates below decide that.

## Source layout

```text
.github/workflows/
  ci.yml                      four-lane contract gate
  live-tools.yml              five-job real-binary interoperability gate
scripts/qualification/
  provision-external-oracles.sh   pinned, digest-verified third-party oracles
  assert-oracle-run.py            the oracle-evidence gate
  m003-live-tools/                EggReplay + Eggprobe interop harness
  m004a-eggsec/                   Eggsec WAF adapter harness
  m004b-combined/                 combined verdict-precedence harness
  synvoid-m002/                   real SynVoid correctness + perf harness
  synvoid-m003/                   SynVoid telemetry + load-profile harness
qualification/synvoid/
  v1/                         M002a synthetic workspace, owner import contract
  v2/                         M003c/M003d workspace, owner telemetry contract
examples/                     worked plans, including a rejected one
plans/                        registry, ADRs, subsystem roadmaps, closure records
docs/                         18 contract documents
```

## Key types

### CI jobs

Every job is a claim about a property the workspace claims to have.

| Job | Runner | Steps | What it protects |
|---|---|---|---|
| `linux-stable` | `ubuntu-24.04` | fmt, workspace check, feature isolation, clippy, full test, default-feature CLI test | Behavior. The reference lane; every other lane is a restriction of it |
| `linux-msrv` | `ubuntu-24.04`, toolchain `1.89.0` ([ci.yml:46](../.github/workflows/ci.yml)) | check default, check all-features, test `eggbench-core`, test `eggbench-drivers` | The declared floor. A dependency or a language feature that would raise the floor fails here, not at a user's machine |
| `macos-stable` | `macos-14` | check, clippy, full test, plus two named runner tests ([ci.yml:65](../.github/workflows/ci.yml), [ci.yml:67](../.github/workflows/ci.yml)) | Platform portability: process-group cleanup and filesystem symlink confinement are POSIX-shaped claims that only a second platform can refute |
| `windows-stable` | `windows-2022` | check, clippy, core tests with failure lines promoted to annotations ([ci.yml:86](../.github/workflows/ci.yml)), `eggbench-runner --test platform`, `eggbench-drivers --test eggstack_path` | A supported-subset claim, made honestly: it does not run the whole workspace suite |
| `live-tools-linux` | `ubuntu-24.04`, 60 min | release build, `m003-live-tools` harness ([live-tools.yml:35](../.github/workflows/live-tools.yml)) | EggReplay/Eggprobe machine-contract interop |
| `live-eggsec-linux` | `ubuntu-24.04`, 90 min | release build, `m004a-eggsec` harness | Eggsec strict WAF adapter contract |
| `live-m004b-linux` | `ubuntu-24.04`, 90 min | release build, `m004b-combined` harness | Combined verdict precedence |
| `live-synvoid-linux` | `ubuntu-24.04`, 90 min | oracle provisioning ([live-tools.yml:109](../.github/workflows/live-tools.yml)), `synvoid-m002` harness | Real SynVoid correctness/performance qualification |
| `live-m003-linux` | `ubuntu-24.04`, 120 min | oracle provisioning, `synvoid-m003` harness | Subject-telemetry ingestion and M003 load profiles against the real owner contract |

The split is deliberate: **Linux proves behavior, macOS proves the platform
claim, MSRV proves the floor.** `live-tools.yml` is Linux-only because the
four-lane gate already covers sibling platform portability; re-running
interoperability on a second OS would buy nothing
([live-tools.yml:8](../.github/workflows/live-tools.yml)).

`--locked` appears on every cargo invocation. It means the resolved dependency
graph must match `Cargo.lock` exactly, so a CI run cannot silently float onto
newer transitive versions. Without it, "the test passed" is a statement about
a graph nobody pinned, and a green lane stops being reproducible evidence.

### Feature-isolation matrix

The most interesting job in the repository, because it treats the *build* as
the assertion target ([ci.yml:24](../.github/workflows/ci.yml)).

| Command | Assertion |
|---|---|
| `cargo check -p eggbench-drivers --no-default-features` ([ci.yml:28](../.github/workflows/ci.yml)) | The drivers crate compiles bare |
| `--no-default-features --features eggstack-http` | HTTP composition compiles alone |
| `--features gregg` | Telemetry composition compiles alone |
| `--features eggstack-path` | Network-path composition compiles alone |
| `--features eggstack-path,gregg` | Path and telemetry compose without conflict |
| `--features prometheus-http` | Prometheus telemetry composes alone |
| `cargo check -p eggbench-cli --no-default-features --features eggstack-path` | The CLI link survives the same isolation |
| `cargo tree -p eggbench-drivers --no-default-features --edges normal \| grep -E 'eggress\|eggchaos'` → must find nothing ([ci.yml:35](../.github/workflows/ci.yml)) | **Negative**: the route/stream-fault dependencies are not in the normal graph |
| same, with `--features eggstack-http` ([ci.yml:36](../.github/workflows/ci.yml)) | **Negative**: the default Eggstack composition does not drag them in either |
| `cargo run -p eggbench-cli --features eggstack-path -- validate examples/eggstack-path.json` ([ci.yml:37](../.github/workflows/ci.yml)) | A real binary resolves drivers and validates a real plan end to end |

The two `cargo tree` lines are the point of the job. A `cargo check` proves
that a feature combination *compiles*; it says nothing about what got linked.
Only the negative grep proves **unlinking** — that enabling `eggstack-http`
does not transitively pull the route and stream-fault drivers into a build
that was supposed to be HTTP-only. That distinction is test-as-architecture:
the isolation claim lives in the dependency graph, so the assertion has to
read the dependency graph. See
[drivers-catalog-features.md](drivers-catalog-features.md) for the feature
surface these combinations draw from.

### Qualification asset kinds

`qualification/synvoid/` is the vocabulary. The kinds are distinct and are
routinely confused:

| Kind | File | What it is | What it is not |
|---|---|---|---|
| **corpus** | `corpus.json`, `correctness-corpus.json` | Owner-authored request/expectation cases, pinned by a content-tree identity | Not a plan; not derived from a candidate |
| **scenario** | `scenarios/*.json` | One ordinary experiment plan (schema v8–v10) plus, in v2, a `http_corpus_checks` block pinning a corpus identity | Not a profile; does not aggregate |
| **profile** | `profile.json`, `smoke.profile.json`, `perf.profile.json` | The suite: an ordered scenario list, corpus ref, target-config ref, and optional per-scenario `baseline_bundle` and `workload_driver` | Not a plan; does not declare gates itself |
| **target-config** | `target-config.json` | The target *identity*: policy id, expected asset tree, listen/origin bindings, Detect/Pass status mapping | Not a config file to be executed; it is identity + binding only |
| **routes** | `routes.json` (v2), `stubs/` | The controlled origin's exact path table and body sizes | Not a corpus; the origin is the thing under the proxy |
| **telemetry contract** | `telemetry/telemetry-contract.json` (v2) | The owner's declared sample inventory, required flags, aggregation, scrape path, and refresh cadence | Not a mapping; it describes samples, not output names |
| **telemetry mapping** | `telemetry/telemetry-mapping.json` (v2) | `prometheus_name` → `output_name`, kind, unit, aggregation, required | Not a policy; Eggbench never reconstructs it |
| **provenance** | `v1/materialized/provenance.json`, v2 target-config `provenance` | Who produced the assets, from which source SHA, with which digests, and which fixtures were excluded and why | Not a claim of SynVoid authorship when it is a synthetic fixture |
| **baseline** | `baselines/` (empty in git) | Operator-materialized bundles from the accepted revision, referenced by path | Never committed; never auto-discovered |

The contract/mapping pair in v2 is the sharpest example of the ownership
discipline: the contract says *which* samples the owner publishes and how they
aggregate, the mapping says what each becomes inside Eggbench. The live
harness re-derives both from the pinned owner tree and compares the mapping
byte-for-byte against the checked-in fixture
([synvoid-m003 harness:294](../scripts/qualification/synvoid-m003/run-live-qualification.sh)),
so an upstream contract change fails the job rather than silently changing
the evidence.

Baselines and provenance are first-class evidence for the same reason: both
answer "compared against what, and produced by whom". v1's
`upstream-manifest.md` freezes the expected `provenance.json` fields and six
verification checks that must fail closed, and `v1/baselines/README.md` plus
`v2/baselines/README.md:44` state the rule directly — a committed baseline
would let a candidate compare against evidence from an unreviewed run.

### Harness verdict vocabulary

Every harness uses the same three-value line, `[VERDICT] label -- detail`:

| Verdict | Meaning | Exit effect |
|---|---|---|
| `PASS` | The acceptance item held | — |
| `STOPPED` | A plan stop condition fired; this is an engineering finding, not a flake | Harness exits 10 ([m003-live-tools:526](../scripts/qualification/m003-live-tools/run-live-qualification.sh)) |
| `NOT-EXECUTED` | Blocked by an earlier stop, or an optional external binary is absent | Does not fail the job |

The distinction is what makes an absent oracle visible instead of invisible:
`synvoid-m002` reports `NOT-EXECUTED` when `oha` is missing and names the
provisioning command, rather than quietly skipping a proof
([synvoid-m002:280](../scripts/qualification/synvoid-m002/run-live-qualification.sh)).

## How it works

### The four-lane gate, step by step

1. `cargo fmt --all -- --check` ([ci.yml:22](../.github/workflows/ci.yml)) — no
   diff noise in review, so a real change stands out.
2. `cargo check --workspace --all-targets --locked`
   ([ci.yml:23](../.github/workflows/ci.yml)) — everything, including tests and
   benches, type-checks before anything is built or run.
3. Feature isolation ([ci.yml:24](../.github/workflows/ci.yml)) — the matrix
   and negative greps above.
4. `cargo clippy --workspace --all-targets --all-features -- -D warnings`
   ([ci.yml:38](../.github/workflows/ci.yml)) — all features, warnings are
   errors. The macOS and Windows lanes run the byte-identical invocation
   ([ci.yml:62](../.github/workflows/ci.yml),
   [ci.yml:78](../.github/workflows/ci.yml)), so lint policy cannot drift
   between platforms; the lanes differ in *test* scope, not lint scope.
5. `cargo test --workspace --all-targets --all-features --locked`
   ([ci.yml:39](../.github/workflows/ci.yml)) — behavior. Note the deliberate
   scope reductions elsewhere: the MSRV lane's test steps drop
   `--all-targets` ([ci.yml:50](../.github/workflows/ci.yml),
   [ci.yml:51](../.github/workflows/ci.yml)) so the floor is checked against
   library and test targets only, and Windows replaces the workspace suite
   with `eggbench-core` plus two named driver tests.
6. `cargo test -p eggbench-cli --no-default-features --test cli --locked`
   ([ci.yml:40](../.github/workflows/ci.yml)) — the CLI still works with
   nothing compiled in. A user who installs the crate bare must get a clear
   `missing_driver` error, not a link failure.

The macOS and Windows lanes add narrow, named qualifications rather than
repeating the full suite: process-group cleanup and symlink confinement
([ci.yml:65](../.github/workflows/ci.yml),
[ci.yml:67](../.github/workflows/ci.yml)), core-contract tests with failure
lines promoted into GitHub annotations so a Windows failure is readable in the
PR ([ci.yml:86](../.github/workflows/ci.yml)), the platform test, and the
Eggstack path driver test ([ci.yml:94](../.github/workflows/ci.yml)).

### Live-tool qualification

`live-tools.yml` answers a different question: does the adapter still speak
the sibling's real dialect? The `m003-live-tools` harness builds EggReplay at
a pinned rev and Eggprobe twice — once at `v0.1.1` (schema 0.3) as the
**positive control**, once at a commit emitting schema 0.4 as the
**negative control** — then puts each in an isolated `PATH` directory so
resolution cannot accidentally reach a system binary
([m003-live-tools:129](../scripts/qualification/m003-live-tools/run-live-qualification.sh)).

The positive control proves the plan shape is accepted and a matching replay
yields zero findings ([m003-live-tools:159](../scripts/qualification/m003-live-tools/run-live-qualification.sh)).
The negative control proves the *opposite* property: the schema-0.4 binary
accepts a 0.4 plan, emits a 0.4 report, and rejects the qualified 0.3 plan
([m003-live-tools:244](../scripts/qualification/m003-live-tools/run-live-qualification.sh),
[m003-live-tools:267](../scripts/qualification/m003-live-tools/run-live-qualification.sh)).
That second half is worth more than the first. A version-compatible sibling
that would happily accept a plan it does not understand would turn a plan
change into a silently wrong measurement; a positive-only test cannot
distinguish "my adapter is correct" from "the tool accepts anything". The
harness then escalates the same idea to the Eggbench level: a full `eggbench
run` against the 0.4 binary must fail with
`diagnostic_contract_unsupported` and must not leave a bundle behind
([m003-live-tools:304](../scripts/qualification/m003-live-tools/run-live-qualification.sh)).

The rest of the harness is the rest of the contract: probe timings must stay
out of `TrialMetrics` and live only in diagnostic artifacts
([m003-live-tools:359](../scripts/qualification/m003-live-tools/run-live-qualification.sh));
a failing *required* pre-check must block the workload and still tear down
([m003-live-tools:451](../scripts/qualification/m003-live-tools/run-live-qualification.sh));
a failing *optional* post-check must not invalidate a completed run;
cancellation must leave no surviving sibling process
([m003-live-tools:508](../scripts/qualification/m003-live-tools/run-live-qualification.sh)).

### Security-qualification harnesses

| Harness | Family | Terminal claim |
|---|---|---|
| `m004a-eggsec` | Eggsec strict-scope WAF adapter | A safe fixture yields zero bypasses and Pass; the permissive fixture yields a declared bypass and Fail; no raw payload appears anywhere in the bundle ([m004a:210](../scripts/qualification/m004a-eggsec/run-live-qualification.sh)) |
| `m004b-combined` | Combined verdict precedence | Six cases A–F map to exit codes 0/6/6/8/7/0, and a tampered-but-rehashed security artifact yields `Invalid` with a stable `disposition_mismatch` reason rather than a digest error ([m004b:230](../scripts/qualification/m004b-combined/run-live-qualification.sh)) |
| `synvoid-m002` | Real SynVoid correctness + perf | The owner export is translated mechanically, positive Pass and negative Fail are both proven, blocked requests provably never reach the origin, and teardown leaves no port or child behind ([synvoid-m002:422](../scripts/qualification/synvoid-m002/run-live-qualification.sh)) |
| `synvoid-m003` | Subject telemetry + load profiles | The binary advertises the required drivers ([synvoid-m003:125](../scripts/qualification/synvoid-m003/run-live-qualification.sh)), the owner contract and mapping digest match the pin, the metrics endpoint is loopback-only with admin disabled, and required-field / type-drift mappings fail closed |

Two design choices recur. First, the harnesses assert **from the evidence, not
the exit status**: `assert-oracle-run.py` requires the run to have completed
every measured trial ([assert-oracle-run.py:84](../scripts/qualification/assert-oracle-run.py)),
requires each gated metric to have been *observed* at its expected value in
every measured trial rather than merely absent
([assert-oracle-run.py:107](../scripts/qualification/assert-oracle-run.py)),
excludes warmup trials
([assert-oracle-run.py:56](../scripts/qualification/assert-oracle-run.py)), and
requires a producer label so the numbers can be attributed to `oha` rather
than to the native driver
([assert-oracle-run.py:117](../scripts/qualification/assert-oracle-run.py)).
A driver that exits zero having produced nothing looks exactly like success
to an exit-code check; only the evidence check distinguishes them.

Second, the `assert-*.py` scripts are the gate layer, and they read sanitized
published evidence rather than live processes: `assert-origin-log.py` proves
that every pass path was served 200 by the origin *and* that no detect
request-target appears in the log at all, keyed on the full request-target so
a detect case sharing a path with a pass case cannot false-positive.
`assert-subject-telemetry.py` reads trial telemetry artifacts, refuses
non-loopback scrape authorities, and requires zero poll errors and zero
dropped samples. A gate that read the live subject would be testing the
subject; a gate that reads the bundle tests the evidence contract.

### Controlled origins

`controlled-origin.py` and `controlled-origin-m003.py` stand in for a real
backend under the subject-under-test. This is a significant choice: the WAF or
proxy is the subject, and the origin behind it must be a known quantity or a
pass/fail result is unfalsifiable. Both are deliberately inert — exact route
table, fixed `0x42` fill bodies, 501 everywhere else, one log line per request
as `METHOD TARGET STATUS`, no timestamps. The M003 variant adds a throttle
that reads a **signal file** rather than a plan value
([controlled-origin-m003.py:50](../scripts/qualification/synvoid-m003/controlled-origin-m003.py)),
which is what lets a performance regression be injected between two runs of
the same plan without changing the comparison identity, and an enlarged listen
backlog so a throttled trial degrades throughput instead of refusing
connections and reporting a subject failure.

## Invariants

What the gates collectively guarantee:

1. **A green lane means the same thing on every lane.** `--locked` on every
   cargo invocation; the feature-isolation matrix on the reference lane only,
   because it is a property of the graph, not of the OS.
2. **Unlinking is asserted, not assumed.** The two `cargo tree` greps are the
   only assertions in CI that read the resolved graph instead of a compiler
   result.
3. **A wrong-schema tool is rejected, not accommodated.** The Eggprobe
   negative control exists to keep the adapter honest across a sibling's
   unreleased schema work.
4. **Correctness never rides on performance, and performance never overrides
   correctness.** The M004b case matrix encodes the precedence; the M003
   `perf.profile.json` binds policy gates to exactly one scenario and marks
   every other performance observation `intent: diagnostic`
   ([v2 README:86](../qualification/synvoid/v2/README.md)).
5. **Expectations are never derived from the candidate.** A changed corpus or
   target-config makes a profile `Invalid`, not a new baseline.
6. **Provenance outlives the process.** Every live harness records source SHAs,
   `Cargo.lock` digests, and binary SHA-256; `provision-external-oracles.sh`
   refuses an unpinned digest ([provision:70](../scripts/qualification/provision-external-oracles.sh)),
   fails on a digest mismatch ([provision:76](../scripts/qualification/provision-external-oracles.sh)),
   and prints a `provisioned <tool> <version> sha256:<hex>` line the harness
   copies into its verdict.
7. **A stopped stage names evidence that survives.** `m002-live-diagnostics`
   ([synvoid-m002:56](../scripts/qualification/synvoid-m002/run-live-qualification.sh))
   and `m003-live-diagnostics` ([synvoid-m003:73](../scripts/qualification/synvoid-m003/run-live-qualification.sh))
   exist because a temp work directory removed on exit leaves "aggregate
   invalid" unactionable; both are uploaded as artifacts with 30-day retention.
8. **Nothing in this surface reaches production.** The harnesses are
   loopback-only, need no root, install nothing user-global, and kill all
   children on exit.

## Failure model

**Compile/lint failure.** The job fails at one named step with rustc or clippy
output. Nothing downstream runs. Fix the code.

**Isolation failure.** The job fails inside the `Check Eggstack feature
isolation` step. Either a combination stopped compiling, or one of the two
`cargo tree` greps matched. A grep match means a feature now resolves a
route/stream-fault dependency transitively; that is a Cargo.toml change, not
a code change, and the fix is to remove the dependency edge.

**Test failure.** Linux shows plain failure output. Windows re-runs the core
suite into a log and promotes matching lines into `::error` annotations
([ci.yml:86](../.github/workflows/ci.yml)), so a Windows-only failure is
readable without opening the raw log. macOS and Windows run narrow named
tests, so a failure there names a specific portability claim.

**Live-harness `STOPPED`.** The log tail is printed by the workflow, the
harness exits 10, and the retained diagnostics directory is uploaded. Read the
`[STOPPED]` line first: it names the criterion and the plan section whose stop
condition fired. A stop is a planning-review trigger, not a flake to retry.

**Live-harness `NOT-EXECUTED`.** A stage was blocked, or an optional oracle is
absent. `provision-external-oracles.sh` exists specifically to remove the
second cause in CI: an absent `oha` otherwise means the independent-transport
corroboration is missing on exactly the machines nobody watches.

**Case-D-shaped failure — invalid evidence.** Tampering with a finalized
bundle yields `Invalid` with a stable reason instead of a digest crash,
because the digest chain is recomputed consistently. The operator conclusion
is the same either way: the evidence is not trustworthy and must be
regenerated.

**Example validation failure.** `examples/` is exercised by the
feature-isolation smoke run ([ci.yml:37](../.github/workflows/ci.yml)), so a
worked example that stops validating breaks the reference lane.

**Planning mismatch.** The registry is the answer to "is this in scope?": look
up the subsystem row, read its status and blockers
([registry.md:51](../plans/registry.md)), and check the review checklist
([registry.md:281](../plans/registry.md)) — whose final item is that closure
evidence must prove more than compilation.

## Boundaries

**Deferred: distributed execution.** `plans/subsystems/distributed-execution-roadmap.md`
is `Status: deferred` ([roadmap:3](../plans/subsystems/distributed-execution-roadmap.md)),
and the registry records the milestone as "entry gate not met"
([registry.md:51](../plans/registry.md)). The entry gate
([roadmap:22](../plans/subsystems/distributed-execution-roadmap.md)) requires all
four of: local runner lifecycle closed; evidence and comparison schemas stable
enough to carry node provenance; a concrete remote execution provider with a
stable contract; and Eggwork evaluated as the preferred provider. Milestone
status repeats it — "Deferred. No implementation plan should be handed off yet"
([roadmap:64](../plans/subsystems/distributed-execution-roadmap.md)). This
agrees with [overview.md](overview.md), which names it as one of two
intentionally unbuilt ADR-0005 boundaries.

**The other documented deviations**, recorded rather than silently worked
around, are in `v1/README.md`: D1 no static binding interpolation, D2 the
upstream export is a closed contract and Eggbench does not translate
Detect/Pass, D3 the block status is 403 pending live confirmation, D4 `qualify
run` pins no per-scenario workload driver (so oracle scenarios run through an
explicit `run`/`compare` procedure,
[v1 README:158](../qualification/synvoid/v1/README.md)), D5 no Gregg
collector is declared, D6 mixed malicious load, body campaigns, connection
churn, and SynVoid telemetry ingestion are M003 candidates.

**Where the harnesses must not reach.** Harnesses are evidence generators. A
change that makes a harness pass by relaxing an assertion, editing a pinned
digest, or promoting a `STOPPED` to a warning has changed the gate, not the
subject. Pinned digests and stop conditions are the review surface.

**v1 vs v2.** v1 is the M002a synthetic routine workspace: 5-case GET-only
corpus, plan schema v8, and the owner import contract. v2 is the M003
workspace: 8-case POST-capable corpora split into a correctness corpus and a
load corpus, plan schema v9/10, `http_corpus` workload shape, and the owner
telemetry contract and mapping. They are separate versioned assets, not two
revisions of one; the harness copies v1 wholesale and patches free ports
([synvoid-m002:131](../scripts/qualification/synvoid-m002/run-live-qualification.sh)),
so v2 changes cannot leak into M002a evidence.

## Tests and qualification

### The examples convention

`examples/` is a worked-example set, and one entry is deliberately invalid.

| File | Plan schema | Demonstrates |
|---|---|---|
| [eggstack-path.json](../examples/eggstack-path.json) | 3 | `network_path` composition: an Eggress route plus deterministic Eggchaos upstream/downstream faults, with an absolute throughput gate |
| [eggstack-path-paired-unsupported.json](../examples/eggstack-path-paired-unsupported.json) | 3 | **Rejected case**: `paired` ([line 37](../examples/eggstack-path-paired-unsupported.json)) composed with `network_path` is refused before startup |
| [eggstack-replay.json](../examples/eggstack-replay.json) | 4 | `semantic_replay` workload; findings are correctness evidence gated absolutely at 0 |
| [eggstack-diagnostics.json](../examples/eggstack-diagnostics.json) | 5 | Replay plus pre/post Eggprobe diagnostics, one `required` and one optional |
| [security-profile.json](../examples/security-profile.json) + [security-corpus.json](../examples/security-corpus.json) + [security-config.json](../examples/security-config.json) | profile v1 / corpus v1 | The smallest complete `qualify` triple: profile, corpus, target-config identity |
| [security-http-corpus-plan.json](../examples/security-http-corpus-plan.json) | 8 | `http_corpus_checks` with a pinned corpus content identity |

The progression is the schema history in miniature: path composition (v3) →
semantic replay (v4) → diagnostics (v5). Each file is the smallest plan that
exercises one new capability.

Keeping a rejected example is worth the space. A valid example proves a plan
*can* be written; a rejected one pins the boundary of what a plan may
compose, which is otherwise only discoverable by getting it wrong at runtime.
It is referenced from four places precisely because that boundary is easy to
regress: `README.md`, `docs/experiment-plan.md`, `docs/eggstack-http.md:107`,
and `architecture/core-plan-schema.md:464`.

### Planning record

| Element | Where | What it gives a reviewer |
|---|---|---|
| Status vocabulary | [registry.md:12](../plans/registry.md) | Ten states, from `proposed` to `deferred`; `closed` means a closure record was accepted ([registry.md:19](../plans/registry.md)), `conditionally closed` means a named evidence condition remains ([registry.md:20](../plans/registry.md)) |
| Accepted ADRs | [registry.md:27](../plans/registry.md) | Five decisions, one line each, all `accepted` |
| Subsystem roadmap table | [registry.md:37](../plans/registry.md) | Status, document, current milestone, and named dependencies per subsystem — including the stopped/closed history that corrective work must not rewrite ([registry.md:69](../plans/registry.md)) |
| Closure records | `plans/closure/**/00N-status.md` | The evidence a closure claim rests on |
| Review checklist | [registry.md:281](../plans/registry.md) | Eleven pre-handoff conditions, ending in "closure evidence is sufficient to prove more than compilation" ([registry.md:295](../plans/registry.md)) |

The five accepted decisions, in the registry's own words:

| ADR | Decision |
|---|---|
| ADR-0001 | Typed dependency-light core; separate runner/drivers/CLI; explicit capability failures |
| ADR-0002 | Immutable `.eggb` bundles, manifest-last finalization, first-class testbed provenance |
| ADR-0003 | Trial-level inference, practical thresholds, deterministic bootstrap policy, pass/fail/inconclusive/invalid |
| ADR-0004 | Reuse Eggstack ownership; prefer stable seams; retain independent external benchmark drivers |
| ADR-0005 | Local runner first; future remote execution delegated through the ExecutionProvider/Eggwork boundary |

ADR-0004 is the one the live gates operationalize: "prefer stable seams" is
why the harnesses shell out to real sibling JSON CLIs instead of importing
sibling Rust crates, and "retain independent external benchmark drivers" is
why `assert-oracle-run.py` insists on a producer label.

A repository this contract-heavy needs a formal planning registry for a
concrete reason: CI can prove a change did not break anything, but it cannot
say a change *should not exist*. The status vocabulary plus the subsystem
table plus the review checklist is the answer to "is this in scope", and the
closure-record convention is the answer to "was it actually finished".

## Review focus

Questions worth asking of a change to this surface, in rough order of value:

1. **Does a new capability get a gate, or only a compile?** A driver, plan
   field, or verdict with no `assert-*.py` and no CI step is unqualified.
2. **Is a new negative control possible?** A feature with only positive tests
   cannot detect a permissive regression. Which deliberately wrong input
   would this reject, and is it in the harness?
3. **Does anything weaken a pinned identity?** A changed digest in
   `provision-external-oracles.sh`, a harness pin, or a `MAPPING_SHA256` is a
   contract change and needs the reason in the message.
4. **Was a `STOPPED` downgraded to a warning, or an assertion relaxed?** This
   is the highest-severity edit in this surface.
5. **Is a new `NOT-EXECUTED` path reachable in CI?** A proof that can silently
   not run on the hosted runner should be provisioned, as `oha` now is.
6. **Does the change preserve the artifact set?** A new failing stage must
   name evidence that survives `$WORK`, and the workflow must upload it.
7. **Does a new example follow the convention?** Smallest plan exercising one
   capability, correct schema version, and — if it demonstrates a boundary —
   also a rejected counterpart.
8. **Does the planning record need updating in the same change?** A new
   subsystem row, a status transition, or a new deviation D-entry.

## Related

- [overview.md](overview.md) — the surface map and the deferred boundaries
- [drivers-catalog-features.md](drivers-catalog-features.md) — the feature
  surface the isolation matrix draws from
- [core-plan-schema.md](core-plan-schema.md) — the plan schema versions the
  examples encode
- [core-evidence-bundle.md](core-evidence-bundle.md) — the bundle contract the
  `assert-*.py` gates read
- [core-comparison.md](core-comparison.md) — the verdict semantics the
  harnesses assert on
- [core-qualification-security.md](core-qualification-security.md) — the
  `qualify` machinery behind the profiles
- [runner-process-lifecycle.md](runner-process-lifecycle.md) — the cleanup
  contract the teardown proofs assert
- [drivers-external-load-tools.md](drivers-external-load-tools.md) — the oracle
  drivers the provisioning script pins
- [../docs/synvoid-qualification.md](../docs/synvoid-qualification.md),
  [../docs/security-qualification.md](../docs/security-qualification.md),
  [../docs/external-oracles.md](../docs/external-oracles.md) — the operator-facing
  procedures these gates enforce
