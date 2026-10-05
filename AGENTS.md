# AGENTS.md — eggbench

Rust workspace (edition 2024, MSRV 1.89, `unsafe_code = "forbid"`). Always pass `--locked`.

Local-first experiment and evidence system: it validates versioned experiment plans, resolves
them against typed driver capabilities, runs managed workloads through explicit trial
lifecycles, and finalizes immutable `.eggb` bundles. The runner owns side effects; measurement
normalization and offline comparison are separate capabilities.

## Orientation

Read [`architecture/overview.md`](architecture/overview.md) first — module map, end-to-end
artifact chain, feature matrix, bundle layout, and suggested review order. It indexes every deep
dive. Crate-level rationale lives in `architecture/{core,runner,drivers,evidence}.md`.

| Working on | Deep dive |
|---|---|
| Plan fields, schema versions v1–v10, per-version rejection | [`core-plan-schema.md`](architecture/core-plan-schema.md) |
| Capability descriptors, driver selection, `ResolvedPlan` provenance | [`core-driver-resolution.md`](architecture/core-driver-resolution.md) |
| Metric vocabulary, normalization to observed/missing/invalid | [`core-metrics.md`](architecture/core-metrics.md) |
| Bundle manifest, atomic finalize, read-only inspection | [`core-evidence-bundle.md`](architecture/core-evidence-bundle.md) |
| Bootstrap policy, verdicts, comparison receipt | [`core-comparison.md`](architecture/core-comparison.md) |
| Phase schedule, warmups, measured trials, the cleanup boundary | [`runner-orchestration.md`](architecture/runner-orchestration.md) |
| Driver registration, feature gating, driver-name wiring | [`drivers-catalog-features.md`](architecture/drivers-catalog-features.md) |
| Argv parsing, envelope, exit codes, dispatch | [`cli-surface.md`](architecture/cli-surface.md), [`cli-commands.md`](architecture/cli-commands.md) |
| Live harnesses, CI lanes, qualification assets | [`tooling-qualification-ci.md`](architecture/tooling-qualification-ci.md) |

Contract docs (authoritative for the rules themselves): `docs/experiment-plan.md`,
`docs/cli.md`, `docs/driver-capabilities.md`, `docs/evidence-bundle.md`, `docs/comparison.md`,
`docs/security-qualification.md`. Worked examples: `examples/eggstack-*.json`,
`examples/security-*.json`.

## Agent skills

Procedures in [`.skills/`](.skills/) — load the one that matches the task:

| Skill | Use for |
|---|---|
| [`eggbench-verification-gates`](.skills/eggbench-verification-gates/SKILL.md) | Running the gates before commit, or reading a gate failure |
| [`eggbench-contract-versioning`](.skills/eggbench-contract-versioning/SKILL.md) | Changing a serialized contract or bumping a schema version |
| [`eggbench-docs-truth`](.skills/eggbench-docs-truth/SKILL.md) | Auditing or repairing docs, README, and architecture prose |
| [`eggbench-add-driver`](.skills/eggbench-add-driver/SKILL.md) | Adding a driver and wiring catalog, dispatch, and tests |

## Build / verify

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --all-features --locked
python3 scripts/verify-architecture-docs.py
```

A driver or CLI change also needs the feature-isolation matrix in the
`eggbench-verification-gates` skill; CI runs six feature combinations, not the two below.

```bash
cargo check -p eggbench-drivers --no-default-features --locked
cargo tree -p eggbench-drivers --no-default-features --edges normal | grep -E 'eggress|eggchaos'  # must be empty
```

Focused runs: `cargo test -p <crate> --all-features --locked`, or
`cargo test -p <crate> --test <target> <test_name> --locked`. The MSRV lane runs only
`cargo check` plus core and drivers tests, so a language-level break can pass the full local
matrix and still fail CI.

## CLI

```text
eggbench validate <plan> | doctor <plan> | run <plan> <bundle> | inspect <bundle>
eggbench compare <base.eggb> <cand.eggb> | --alias <base.eggbaseline.json> <cand.eggb>
                 | --absolute-only <cand.eggb> | --paired <b.eggb>   [--output <json>] [--seed <u64>]
eggbench qualify validate|expand <profile> | run <profile> --output <dir> | inspect <receipt.json>
```

`--json`/`--quiet` are global. `-` reads stdin but requires `--input-format`. `run` needs a new
destination ending in `.eggb`; `inspect` needs an existing finalized bundle. Route, fault,
network-path, and seed selection live in the plan — there is no CLI flag for them.
`--workload-driver` accepts any registered `Workload` driver (`oha`, `h2load`, `iperf3`,
`eggreplay-semantic`, `eggsec-load`, feature-gated `eggfetch-http`) and validates only the
name's syntax.

Exit codes: `0` ok, `1` internal, `2` parse/validation, `3` capability/preflight/path, `4` run
finalized non-success, `5` bundle I/O, `6`/`7`/`8` compare Fail/Inconclusive/Invalid.

## Gotchas that cost time

- **Lint config is deliberate.** Workspace clippy runs `pedantic = warn` with
  `assert_is_empty = allow`; do not "fix" `assert!(x.is_empty())`.
- **Plan schemas are closed.** `deny_unknown_fields` everywhere, and each version rejects
  fields belonging to other versions. Version gates are explicit inequality chains, not
  ranges. v7 is a dead end carrying only `http_url` — it accepts none of `semantic_replay`,
  `diagnostics`, `security_checks`, or `http_corpus_checks`.
- **Schema v3 paths are route-first/fault-second**, reject credential-bearing routes, never
  fall back from a requested proxy to Direct, and require an explicit seed for non-empty
  faults. Faults are user-space accepted byte-stream impairments, **never packet loss**.
- **`doctor` starts nothing** except the `eggprobe` handshake when diagnostics are requested.
  `run` preflights tools and features *before* bundle prep, so a path failure exits `3` with no
  bundle written.
- **Bundles are immutable and manifest-last**; a bundle can never contain evidence of its own
  publication. `inspect` is read-only. Comparison is offline and deterministic
  (`eggbench.trial-bootstrap[-network-path|-paired].v1`, receipt v4).
- **Features:** `default=[]`, `eggstack-http`, `eggstack-path` (implies `eggstack-http`),
  `gregg`, `prometheus-http`. Default and `eggstack-http`-only builds must not link
  `eggress*`/`eggchaos*`. There is no public fake-workload flag — `fake-load` exists only
  through test/qualification injection.
- **Two hand-maintained driver-name lists** (catalog and CLI dispatch) are kept equal by a
  guard test, not by construction. Names are shared `*_DRIVER_NAME` constants.
- **`qualify` is structurally separate** from the five plan-chain commands: dispatched in the
  binary, absent from the library `Command` enum, and it bypasses the JSON envelope. Its exit
  code is the aggregate verdict (`0/6/7/8`), so a correct Fail exits `6`. Details in
  [`architecture/cli-surface.md`](architecture/cli-surface.md).
- **Windows:** managed `run` is unsupported; `validate`/`doctor`/`inspect` work and report
  truthful facts.
- **Do not run `scripts/qualification/*` or `live-tools.yml` casually.** They need real pinned
  sibling binaries and produce evidence gated on exact-head hosted runs. Ordinary
  `cargo test` is the gate.

## Planning

`plans/registry.md` is the control surface for milestones and closures; read it before new
feature work. Per-milestone records live under `plans/implementation/` and `plans/closure/`.
Do not add SSH, scheduler, or remote machinery — deferred per ADR-0005. Closure records are
historical evidence of what was accepted; corrective work does not rewrite them.

After editing anything in `architecture/`, run `python3 scripts/verify-architecture-docs.py` —
it checks relative links, `file:line` citations are in range, the ten required deep-dive
sections, and that `overview.md` links every deep dive. It cannot tell whether a citation
points at the *right* line, so a moved reference still needs a human read.
