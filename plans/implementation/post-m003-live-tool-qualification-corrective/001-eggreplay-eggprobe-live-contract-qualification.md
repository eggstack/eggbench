# Post-M003 Live External-Tool Qualification Corrective C001 — EggReplay + Eggprobe

Status: stopped with evidence (closure plans/closure/post-m003-live-tool-qualification-corrective/001-status.md; successor C002 plans/implementation/post-m003-live-tool-qualification-corrective/002-eggprobe-adapter-contract-correction.md)

Repository baseline: `17a3adefb079dcc4591ade47a88f2b41726e0749`

Corrective authority:

- `plans/subsystems/post-m003-live-tool-qualification-corrective-addendum.md`

Historical M003 remains closed at:

- `plans/closure/eggstack-integration/003a-status.md`
- `plans/closure/eggstack-integration/003b-status.md`

Primary class: qualification corrective / external contract proof.

Expected production semantic change: none.

## 1. Objective

Close the only remaining M003 qualification gap by executing the existing
Eggbench M003 adapters against real sibling binaries and a real combined
controlled experiment.

C001 proves that the machine contracts already implemented in Eggbench match
the real tools:

~~~text
real eggprobe v0.1.1/schema 0.3
        |
        | pre diagnostics
        v
Eggbench runner -> EggServe controlled origin
        |
        | measured semantic workload
        v
real eggreplay @ exact audited revision
        |
        | post diagnostics
        v
real eggprobe v0.1.1/schema 0.3
        |
        v
immutable .eggb + absolute correctness verdict
~~~

The preferred outcome is an evidence-only corrective: qualification helpers,
CI wiring, closure documentation, and no production Rust change.

## 2. Qualification inputs

### 2.1 EggReplay positive-control binary

Use exact source revision:

`d39f4b794620a2d0647688a914e0e7a6be42e184`

This is the audited current EggReplay revision after its EggServe 0.3 runtime
adoption. At planning time it still exposes:

- workspace version 0.1.0;
- Rust 1.89;
- CLI JSON envelope schema 1;
- latest session schema 2;
- RegressionReport schema 2;
- `validate --output json`;
- `replay --output json`;
- documented process exit classes 0/1/2/3/4/5.

Build from a clean checkout:

~~~text
git checkout d39f4b794620a2d0647688a914e0e7a6be42e184
cargo build --locked --release -p eggreplay-cli
~~~

Qualification binary:

~~~text
target/release/eggreplay
~~~

Do not install it globally.

### 2.2 Eggprobe positive-control binary

Use immutable qualified release:

- tag `v0.1.1`;
- commit `53ea53d`;
- machine schema `0.3`;
- Rust 1.89.

Build from a clean checkout of the tag:

~~~text
git checkout v0.1.1
cargo build --locked --release -p eggprobe-cli
~~~

Qualification binary:

~~~text
target/release/eggprobe
~~~

Do not use current Eggprobe main as the positive control.

### 2.3 Eggprobe negative-control binary

Also build the current audited main revision:

`0ce9597aa2acad9a61c45a70c3ffaf56333cf3d5`

This branch still uses the 0.1.1 package line but carries unreleased schema
0.4/native work.

The only purpose of this binary is to prove that Eggbench rejects a real
same-SemVer incompatible machine schema before managed startup.

It MUST NOT be accepted as an M003b producer.

## 3. Provenance record

For every sibling checkout/binary record:

- repository;
- exact commit SHA;
- tag when applicable;
- `Cargo.lock` SHA-256;
- `rustc --version --verbose`;
- `cargo --version`;
- binary path;
- binary byte size;
- binary SHA-256;
- `--version` stdout;
- build command and result.

The closure must include these values.

Do not treat `0.1.0` or `0.1.1` alone as sufficient identity.

## 4. Qualification tooling

A small qualification-only helper may be added under a path such as:

~~~text
scripts/qualification/m003-live-tools/
~~~

It may:

- clone/fetch exact sibling revisions;
- build the binaries;
- create a temporary isolated tool directory;
- create a deterministic local fixture origin;
- create a real EggReplay fixture;
- invoke Eggbench;
- inspect resulting bundles/receipts;
- clean temporary processes/directories.

It MUST NOT:

- become a production dependency;
- alter Eggbench runtime behavior;
- copy EggReplay/Eggprobe implementation code;
- require root;
- bind non-loopback;
- write into user-global config/install locations.

Any parsing of EggReplay's human "recording on ..." readiness line is
permitted only inside this qualification helper to generate a fixture. It is
not an approved production M003 seam.

## 5. Isolated tool resolution

Place the three built binaries in explicit qualification directories and
construct a controlled PATH so Eggbench cannot accidentally resolve a
different installed binary.

Positive path must contain:

- positive EggReplay binary;
- Eggprobe v0.1.1 binary.

Negative-control path must substitute only the schema-0.4 Eggprobe binary.

Before every Eggbench execution:

~~~text
command -v eggreplay
command -v eggprobe
sha256sum <resolved binaries>
eggreplay --version
eggprobe --version
~~~

Record these facts.

No global `cargo install` is allowed.

## 6. Real EggReplay fixture generation

Generate a fixture through the real EggReplay binary rather than hand-writing
the `.eggr` format.

Qualification-only deterministic origin:

- bind `127.0.0.1:0`;
- support one `GET /bench`;
- status 200;
- body exactly 64 bytes of byte `0x42` (`B`);
- deterministic `Content-Length`;
- no changing application payload or timestamp header;
- reject/close unrelated routes deterministically.

Then:

1. launch real `eggreplay record` on loopback with:
   - direct route;
   - secure default redaction;
   - a temporary fixture directory;
2. discover its ephemeral recording listener in the qualification harness;
3. send exactly one `GET /bench` through the recording gateway;
4. terminate the recorder cleanly with the platform-appropriate interrupt;
5. require its finalized JSON success output;
6. run real:
   `eggreplay validate --fixture <fixture> --output json`;
7. require envelope schema 1, session schema 1 or 2, and flow_count = 1;
8. hash the resulting fixture using Eggbench's own M003a identity code during
   later preflight.

Do not commit the generated fixture as proof that the real tool was used.
Regenerate it during qualification and record its aggregate digest.

## 7. Standalone positive EggReplay contract

Before the combined Eggbench run, prove the real binary independently:

### Matching target

Start the same deterministic local origin and execute:

~~~text
eggreplay replay   --fixture <fixture>   --target http://127.0.0.1:<port>/bench   --route direct   --output json
~~~

Require:

- process exit 0;
- envelope schema 1;
- command `replay`;
- all RegressionReport entries schema 2;
- finding_count = 0.

### Mismatch target

Start a deterministic origin with one deliberate semantic change, preferably:

- status 201, or
- 65-byte `B` body.

Replay the same fixture.

Require:

- process execution remains successful according to EggReplay replay semantics;
- valid envelope/report JSON;
- finding_count > 0.

This proves the sibling itself exposes the semantics expected by the adapter
before Eggbench is involved.

## 8. Standalone positive Eggprobe contract

Feed the exact schema-0.3 plan shape generated by Eggbench to real
`eggprobe v0.1.1` using stdin:

~~~text
eggprobe run -
~~~

Use loopback only.

Require:

- valid ProbeReport JSON;
- `schema_version == "0.3"`;
- `tool.name == "eggprobe"`;
- non-empty tool version;
- direct route summary;
- expected DNS/TCP/HTTP behavior for the chosen loopback target;
- documented process exit behavior.

Also execute an intentionally negative reachable/unreachable diagnostic to
prove exit 1 still carries a parseable report where the qualified CLI uses
that code.

Record stdout/stderr separately.

## 9. Real schema-0.4 rejection

Using the negative-control Eggprobe binary at
`0ce9597aa2acad9a61c45a70c3ffaf56333cf3d5`:

1. place it in the controlled PATH;
2. run Eggbench doctor/preflight for the diagnostics example;
3. require:
   - `diagnostic_contract_unsupported` or the exact existing equivalent;
   - no managed service startup;
   - no workload invocation;
   - no fallback to accepting schema 0.4 because the package version says
     0.1.1.

This is release-blocking qualification evidence.

Do not update M003b to schema 0.4 in C001.

## 10. Combined positive Eggbench run

Regenerate/copy the real fixture into the temporary Eggbench workspace at the
path expected by:

`examples/eggstack-diagnostics.json`

Use the positive controlled PATH.

Run the built Eggbench CLI with all required features and select
`eggreplay-semantic`.

Required behavior:

1. schema-v5 plan validates;
2. real Eggprobe v0.1.1 handshake passes before startup;
3. EggServe controlled origin starts on loopback;
4. required pre diagnostics execute successfully;
5. both measured semantic replay trials invoke the real EggReplay binary;
6. each trial reports `semantic_findings = 0`;
7. optional post diagnostics execute before teardown;
8. service teardown succeeds;
9. bundle finalizes successfully;
10. `eggbench inspect` verifies the bundle.

Evidence must show real executable SHA/version facts in:

- semantic replay run evidence;
- diagnostics index/provenance.

## 11. Combined mismatch/gate run

Create a temporary copy of the combined plan changing only the controlled
origin semantic response relative to the recorded fixture, e.g. status 201 or
body length 65.

Run with the same real binaries and same fixture.

Require:

- execution itself is not `Failed` merely because semantic findings exist;
- the bundle finalizes;
- measured trials report `semantic_findings > 0`;
- Eggprobe diagnostic phases still execute according to required/optional
  policy;
- the candidate-only absolute comparison/gate uses the existing comparison
  command;
- the zero-findings gate returns the existing comparison-fail exit code 6;
- the resulting comparison verdict is `Fail`, not `Invalid` solely because
  a semantic mismatch exists.

This is the primary proof that M003a's correctness mapping matches the real
EggReplay behavior.

## 12. Trial-unit proof

The positive combined plan has two measured trials.

Prove from bundle evidence:

- exactly two measured Eggbench trial records;
- each trial corresponds to one complete EggReplay replay invocation;
- per-flow reports are nested/raw evidence and are not promoted to separate
  statistical trials;
- no EggReplay process/client state survives across trial processes.

## 13. Diagnostic timing proof

From the combined run prove:

- pre diagnostics occur after readiness and before warmups/measured work;
- post diagnostics occur after workload drain and before teardown;
- Eggprobe timing fields exist only in diagnostic artifacts;
- TrialMetrics contain no Eggprobe latency sample;
- measured elapsed contains EggReplay workload execution only under the M003
  workload contract.

Do not create a metric from Eggprobe timing merely to make it easier to
assert.

## 14. Required/optional diagnostic live checks

Using the real Eggprobe v0.1.1 binary, execute two bounded negative variants.

### Required pre diagnostic

Make the required pre request fail/negative.

Require:

- no semantic workload starts;
- execution becomes Invalid with diagnostic phase provenance;
- teardown still runs.

### Optional post diagnostic

Make only the optional post request unavailable/negative.

Require:

- completed workload remains completed;
- diagnostic artifact records the negative/unavailable outcome;
- teardown succeeds.

Do not let a post diagnostic replace a pre-existing workload failure.

## 15. Cancellation/cleanup live smoke

Perform one bounded cancellation smoke with real tools:

- begin a run with a long enough semantic replay/diagnostic operation to
  observe a live child;
- send SIGINT/cancellation;
- require Eggbench returns the existing cancelled status;
- child EggReplay/Eggprobe processes do not remain alive;
- managed origin tears down;
- partial evidence follows existing cancellation rules.

This is qualification, not a new process-ownership implementation.

If the existing process substrate cannot clean one of the real children,
stop for a dedicated corrective.

## 16. Existing qualification matrix

After live-tool work, rerun the existing repository gates:

~~~text
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked

cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
EGGBENCH_PARENT_SENTINEL_26CE=set-in-parent-process cargo test --workspace --all-targets --all-features --locked

cargo +1.89.0 check --workspace --all-targets --locked
cargo +1.89.0 check --workspace --all-targets --all-features --locked
cargo +1.89.0 test -p eggbench-core --all-features --locked
cargo +1.89.0 test -p eggbench-runner --all-features --locked
cargo +1.89.0 test -p eggbench-drivers --all-features --locked

cargo tree --locked
git diff --check
~~~

No new production EggReplay/Eggprobe Rust dependency is permitted.

## 17. Hosted live-tool job

Add a dedicated Linux hosted qualification job to CI or a narrowly scoped
qualification workflow.

It must:

1. check out Eggbench corrective HEAD;
2. obtain EggReplay exact revision `d39f4b7...`;
3. obtain Eggprobe `v0.1.1` / `53ea53d`;
4. build both with `--locked --release`;
5. run the automated live qualification harness;
6. upload or print bounded provenance/evidence summaries;
7. fail on any contract mismatch.

Do not fetch sibling `main` as the positive input.

The normal Eggbench four-lane CI must also pass on the same corrective HEAD.

The live external-tool job may be Linux-only because existing four-lane CI
already qualifies Eggbench's platform-specific process/runtime behavior.
Its purpose is machine-contract interoperability, not re-qualifying sibling
platform portability.

## 18. Corrective code-change policy

Expected production change: none.

If live qualification reveals only a qualification-harness problem, fix the
harness.

If a real sibling binary reveals an Eggbench adapter defect:

- do not silently patch it under this evidence-only plan;
- record the exact mismatch;
- stop C001;
- write/register C002 with the narrow production compatibility correction;
- preserve C001 as stopped evidence.

Examples requiring C002:

- EggReplay emits a different valid machine shape than M003a accepted;
- the schema-0.3 Eggprobe plan generated by M003b is not accepted by the
  qualified v0.1.1 binary;
- real exit-code semantics differ from the adapter mapping;
- real cancellation exposes a process-ownership defect.

A sibling defect belongs in the sibling repository, not Eggbench.

## 19. Documentation and evidence updates

Do not rewrite M003a/M003b historical closure records.

After successful C001 qualification:

- add a short addendum/link from the Eggstack integration roadmap;
- update the registry to mark the live-tool corrective closed;
- record that M003 now has additive real-binary interoperability evidence;
- unblock M004 implementation subject to its own implementation plan.

The historical M003 closure continues to describe what was known on
2026-09-25 before live binaries were available.

## 20. Closure record

Create:

`plans/closure/post-m003-live-tool-qualification-corrective/001-status.md`

Record:

- Eggbench implementation/closure SHAs under test;
- corrective/harness SHA;
- EggReplay source SHA, lockfile hash, binary SHA, `--version`;
- Eggprobe positive tag/SHA, lockfile hash, binary SHA, `--version`;
- Eggprobe negative-control SHA/binary SHA;
- fixture aggregate digest/session schema/flow count;
- standalone EggReplay matching/mismatch results;
- standalone Eggprobe schema-0.3 result;
- real schema-0.4 rejection result;
- positive combined bundle identity;
- mismatch combined bundle identity;
- semantic finding counts by trial;
- absolute comparison receipt/verdict/exit code;
- diagnostic pre/post evidence;
- timing-exclusion proof;
- required/optional live diagnostic results;
- cancellation/cleanup result;
- normal local/MSRV result;
- hosted live-tool job/run;
- four normal hosted job conclusions;
- unresolved findings and final disposition.

## 21. Acceptance criteria

C001 closes only when:

1. exact real binaries are built from the pinned sibling revisions;
2. binary SHA/provenance is recorded;
3. real EggReplay validates the generated fixture;
4. real EggReplay matching replay yields zero findings;
5. real EggReplay mismatch replay yields nonzero findings;
6. real Eggprobe v0.1.1 accepts the exact schema-0.3 stdin plan;
7. real schema-0.4 Eggprobe is rejected before startup;
8. combined positive Eggbench run finalizes successfully;
9. two measured trials map to two complete replay processes;
10. combined mismatch run finalizes successfully;
11. semantic mismatch remains a completed workload observation;
12. zero-findings absolute gate produces comparison Fail/exit 6;
13. real pre/post diagnostics execute in the intended lifecycle slots;
14. Eggprobe timings remain absent from TrialMetrics;
15. required pre negative prevents workload and still tears down;
16. optional post negative does not invalidate a successful workload;
17. cancellation leaves no sibling child process behind;
18. no EggReplay/Eggprobe Rust production dependency is added;
19. default/all-feature Clippy/tests remain green;
20. Rust 1.89 remains green;
21. hosted live-tool qualification passes;
22. normal four-lane hosted CI passes on the corrective HEAD;
23. closure/registry/roadmap reconciliation is committed.

## 22. Stop conditions

Stop for planning review and create a follow-up corrective if:

- EggReplay at the pinned revision does not honor envelope/report contracts;
- Eggprobe v0.1.1 rejects the exact generated schema-0.3 plan;
- real exit-code behavior contradicts M003 mappings;
- fixture generation requires production parsing of human readiness text;
- a real child survives cancellation/cleanup;
- semantic findings cannot pass through as a completed workload observation;
- diagnostic timing contaminates TrialMetrics;
- satisfying the tools requires adopting Eggprobe schema 0.4;
- a production Rust sibling dependency appears necessary;
- a substantive sibling defect is discovered.

Do not close C001 by weakening an acceptance criterion.
