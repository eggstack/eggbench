# AGENTS.md — eggbench

Rust workspace (edition 2024, MSRV 1.89, `unsafe_code = "forbid"`). Always pass `--locked`.

## Crate boundaries

- `eggbench-core`: runtime-free domain contracts (plans, validation, comparison, evidence types). No tokio/process/network deps. Bundle reads are bounded sync FS I/O.
- `eggbench-runner`: trial lifecycle, process execution, teardown. Owns `SubjectSnapshot` (hashes executables).
- `eggbench-drivers`: production catalog + adapters. Owns feature-gated deps.
- `eggbench-cli`: thin presentation adapter only. No orchestration logic here; delegate to core/runner.

## Build / verify (mirror CI `linux-stable`)

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --all-features --locked
```

Focused runs:

```bash
cargo test -p <crate> --all-features --locked
cargo test -p <crate> --test <target> <test_name> --locked   # e.g. -p eggbench-runner --test lifecycle descendant_cleanup_reaches_process_group
cargo test -p eggbench-cli --no-default-features --test cli --locked
```

Note: workspace clippy uses `pedantic = warn` with `assert_is_empty = allow` (don't "fix" `assert!(x.is_empty())`). MSRV lane only runs `cargo check` + core/drivers tests on 1.89.

## Feature isolation (must hold)

Features on `eggbench-cli`/`eggbench-drivers`: `default=[]`, `eggstack-http`, `eggstack-path` (implies `eggstack-http`), `gregg`, `prometheus-http`. Default and `eggstack-http`-only builds must not link `eggress*`/`eggchaos*`. Verify:

```bash
cargo check -p eggbench-drivers --no-default-features --locked
cargo check -p eggbench-drivers --no-default-features --features eggstack-path --locked
cargo tree -p eggbench-drivers --no-default-features --edges normal | grep -E 'eggress|eggchaos'  # must be empty
cargo run -p eggbench-cli --features eggstack-path --locked -- validate examples/eggstack-path.json
```

There is no public fake-workload flag; `fake-load` exists only via test/qualification injection.

## CLI surface

```text
eggbench validate <plan> [--input-format toml|json]
eggbench doctor   <plan> [--input-format toml|json] [--workload-driver <name>]
eggbench run      <plan> <bundle> [--input-format toml|json] [--workload-driver <name>]
eggbench inspect  <bundle> [--manifest-json]
eggbench compare <base.eggb> <cand.eggb> | --alias <base.eggbaseline.json> <cand.eggb> | --absolute-only <cand.eggb> | --paired <b.eggb> [--output <json>] [--seed <u64>]
eggbench qualify validate|expand|run|inspect ...
```

- `--json`/`--quiet` are global. `-` stdin requires `--input-format`.
- `run` destination must be new and end in `.eggb`; `inspect` requires an existing finalized bundle.
- No route/fault/network-path/seed CLI flags — those live in the plan. `--workload-driver` pins `oha|h2load|iperf3|eggfetch-http`; otherwise resolver needs a unique marked default.
- Exit codes: 0 ok, 1 internal, 2 parse/validation, 3 capability/preflight/path, 4 run finalized non-success, 5 bundle I/O, 6/7/8 compare Fail/Inconclusive/Invalid.

## Plan / evidence gotchas

- Schemas are strict: unknown fields rejected (`deny_unknown_fields`); each schema version rejects fields from other versions (e.g. v1/v2 reject `network_path`, v4+ replay/diagnostics rules). Check `docs/experiment-plan.md` for current v1–v10 rules.
- Schema v3 path: route-first/fault-second, credential-bearing routes rejected, proxy failure never falls back to Direct, non-empty faults require explicit seed. Faults are user-space byte-stream impairments, never packet loss.
- `doctor` never starts work or connections (except `eggprobe` handshake when diagnostics requested). `run` preflights tools/features before bundle prep; path failures exit 3 with no bundle.
- `.eggb` bundles are immutable, manifest-last; `inspect` is read-only. Comparison is offline/deterministic (`eggbench.trial-bootstrap[-network-path|-paired].v1`, receipt v4). New resolutions write ResolvedPlan v6 (v1–v5 readable).
- Windows managed `run` is unsupported; `validate`/`doctor`/`inspect` still work there. `scripts/qualification/*` and `live-tools.yml` need real pinned sibling binaries — don't run casually; normal `cargo test` is the gate.

## Docs / planning

- Start with `architecture/overview.md` (module map, end-to-end flow, review order); then per-component deep dives: `core-plan-schema`, `core-driver-resolution`, `core-metrics`, `core-evidence-bundle`, `runner-orchestration`, `core-comparison`, plus `drivers-*` / `cli-*` as needed. Crate rationale: `architecture/{core,runner,drivers,evidence}.md`.
- Contract docs: `docs/experiment-plan.md` (plan v1–v10), `docs/cli.md`, `docs/driver-capabilities.md`, `docs/evidence-bundle.md`, `docs/comparison.md`. Examples: `examples/eggstack-*.json`, `examples/security-*.json`.
- `plans/registry.md` is the control surface for milestones/closures (per-milestone records under `plans/implementation/`, `plans/closure/`); consult it before new feature work. Don't add SSH/scheduler/remote machinery (deferred per ADR-0005).
- `scripts/verify-architecture-docs.py` validates `architecture/*.md` links, `file:line` citations, and required deep-dive sections. Run after editing architecture docs.
- No `.skills/` directory exists in this repo; there are no agent skills to keep in sync.
