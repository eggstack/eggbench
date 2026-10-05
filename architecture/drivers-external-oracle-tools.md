# External oracle and diagnostic tools: Eggsec, Eggsec load, EggReplay, Eggprobe

## Purpose

Four adapters in `crates/eggbench-drivers/src/external/` implement the correctness
and diagnostic family of external tools. They share one philosophy and one
division of labour:

- **The tool owns a domain meaning.** Eggsec owns payload generation, WAF
  detection, bypass-technique execution, and what `bypass_successful` means.
  EggReplay owns fixture format, replay order, candidate execution, and what a
  semantic finding means. Eggprobe owns DNS/TCP/TLS/HTTP probe execution and
  what a `ProbeReport` status means. Eggsec load owns request execution and
  report semantics for one reviewed corpus case.
- **Eggbench owns when to invoke and how to record.** Target binding, local
  confinement, manifest generation, binary/schema provenance, lifecycle
  placement, predeclared thresholds, bounds, and the sanitized or bounded
  evidence projection are all Eggbench's.

None of the four are timing sources for `TrialMetrics`. That is a hard rule in
each module header (`eggsec.rs:20-22`, `eggprobe.rs:18-20`,
`eggreplay.rs:18-21`, `eggsec_load.rs:3-6`).

The `eggsec.rs` ownership split points at a specific evidence contract:
`SecurityCheckResultV1` in [`core/src/security.rs`](../crates/eggbench-core/src/security.rs)
owns the disposition algebra, and this adapter owns producing a value that
satisfies it.

| Property | eggsec-waf | eggsec-load | eggreplay-semantic | eggprobe |
|---|---|---|---|---|
| Tool owns | payloads, WAF detection, bypass execution, `bypass_successful` | request execution, load report semantics | fixture format, replay order, finding meaning | DNS/TCP/TLS/HTTP probe execution, report status |
| Eggbench owns | scope manifest, preflight gate, threshold, sanitized result | scope + execution policy, argv bounds, expectation mismatching | fixture identity, confinement, evidence contract, metrics mapping | target lowering, required/optional policy, timeout bound, evidence |
| Driver category | `Correctness` (`eggsec.rs:1021`) | `Workload` (`eggsec_load.rs:838`) | `Workload` (`eggreplay.rs:875`) | `Diagnostic` (`eggprobe.rs:900`) |
| Runner seam | `CorrectnessExecutor` (`correctness.rs:98`) | `WorkloadExecutor` | `WorkloadExecutor` | `DiagnosticExecutor` (`diagnostics.rs:195`) |
| Relative to measurement interval | after readiness, before warmups; outside every interval | inside the measured invocation | inside the measured invocation (occupies the trial) | pre after readiness, post after drain; outside every interval |
| Emits | correctness evidence (`SecurityCheckResultV1`) | workload metrics + raw report | workload metrics (counts only) | diagnostic evidence (`ProbeReport` raw) |
| Cardinality | one-shot per declared check | one corpus case per invocation | one fixture replay per invocation | one-shot per diagnostic slot (pre + post) |
| Never a metric | bypass counts, sanitized cases | — (it *is* the workload) | `semantic_findings`, `semantic_flows` counts | every probe timing, finding count |
| Evidence artifact | `security-checks.json` + `security/<id>.json` | per-trial raw + `eggsec-load-*.json` | `semantic-replay.json` | `diagnostics.json` + `diagnostics/<phase>/<id>.json` |

The **external workload question** is the load-bearing distinction. EggReplay
and Eggsec load are `DriverCategory::Workload` and implement
`WorkloadExecutor::execute`, so the runner polls them inside `execute_invocation`
and their process time is the measurement (EggReplay deliberately reports
`measurement_elapsed: None`, `eggreplay.rs:764`, so even there the adapter
declines to claim the duration; Eggsec load does report the tool's own
`total_duration_ms`, `eggsec_load.rs:517`). Eggsec and Eggprobe are
`Correctness` / `Diagnostic` and implement one-shot bounded seams that the
orchestrator runs in their own phases — see
[runner-adapter-seams.md](runner-adapter-seams.md) for the trait shapes and
[runner-orchestration.md](runner-orchestration.md) for the phase order. This
matters because only `WorkloadExecutor` implementations may contribute
`RawMetricObservation` values that reach `TrialMetrics`; the one-shot seams have
no metric path at all.

`is_external_workload` (`preflight.rs:26-34`) lists only the five workload
adapters (oha, h2load, iperf3, eggreplay-semantic, eggsec-load). `eggsec-waf`
and `eggprobe` are deliberately absent — they are resolved and probed through
different entry points (`preflight_eggsec`, `preflight_eggprobe`).

## Source layout

| File | Lines | Role |
|---|---|---|
| [`eggsec.rs`](../crates/eggbench-drivers/src/external/eggsec.rs) | 1,404 | Strict-scope WAF correctness executor, scope/policy manifest generators, preflight parser, WAF report parser |
| [`eggprobe.rs`](../crates/eggbench-drivers/src/external/eggprobe.rs) | 1,336 | Pre/post diagnostic executor, target lowering, schema-0.3 plan generation, report parser, handshake |
| [`eggreplay.rs`](../crates/eggbench-drivers/src/external/eggreplay.rs) | 1,149 | Semantic replay workload, fixture identity, validate/replay envelope parsers, run evidence |
| [`eggsec_load.rs`](../crates/eggbench-drivers/src/external/eggsec_load.rs) | 1,064 | One reviewed HTTP corpus case as a bounded Eggsec load command |

Seams they implement and the runner files that own them:
[`correctness.rs`](../crates/eggbench-runner/src/correctness.rs) and
[`diagnostics.rs`](../crates/eggbench-runner/src/diagnostics.rs) in `eggbench-runner`;
`WorkloadExecutor` in the same crate. The external command substrate they call
(`run_command`, `ExternalCommandSpec`, `BinaryResolver`, `VersionProbe`,
`artifact_candidates`) is documented in
[drivers-external-substrate.md](drivers-external-substrate.md) and is not
re-described here. Registry facts (descriptors, capabilities, categories) are
summarized in [drivers-catalog-features.md](drivers-catalog-features.md).

## Key types

| Type | Location | Notes |
|---|---|---|
| `EggsecWafExecutor` | `eggsec.rs:137` | Holds `ResolvedExecutable`, optional pinned version, runner-owned `scope_dir`, `scope_dir_created` |
| `EggsecPreflight` | `eggsec.rs:109` | version / executable SHA / scope SHA; the parser returns all three **empty** (`eggsec.rs:777-781`) |
| `WafObservationParsed` | `eggsec.rs:120` | `cases: Vec<SanitizedSecurityCase>` plus `successful_bypasses` |
| `ConfinedTarget` | `eggsec.rs:129` | host + port accepted by `confine_target_url` |
| `generate_load_policy_manifest()` | `eggsec.rs:570` | Lives in `eggsec.rs`, consumed only by `eggsec_load.rs:161` |
| `EggsecLoadWorkload` | `eggsec_load.rs:49` | executable + optional version; no scope state (per-invocation temp dir instead) |
| `EggsecLoadReport` | `eggsec_load.rs:323` | `deny_unknown_fields` wire struct for the load report |
| `ManifestFiles` | `eggsec_load.rs:247` | RAII holder; `Drop` removes both manifests and the directory (`eggsec_load.rs:253-259`) |
| `EggReplayWorkload` | `eggreplay.rs:124` | adds `workspace_root`, `fixture`, `cached_identity`, `cached_preflight` |
| `FixtureIdentity` | `eggreplay.rs:88` | aggregate SHA-256, file count, total bytes |
| `ValidatePreflight` | `eggreplay.rs:99` | envelope schema, fixture session schema, flow count, fixture digest |
| `SemanticReplayEvidence` | `eggreplay.rs:792` | `deny_unknown_fields`, implements `RunEvidenceContract` (`eggreplay.rs:817`) |
| `ReplayParsed` | `eggreplay.rs:116` | total `finding_count` and `report_count` only |
| `EggProbeExecutor` | `eggprobe.rs:122` | executable + pinned version; stateless across slots |
| `LoweredTarget` | `eggprobe.rs:83` | host, port, `use_tls`, `http_url`, `dns_applicable` |
| `HandshakeProof` | `eggprobe.rs:98` | version / executable SHA / negotiated schema |
| `ProbeReportParsed` | `eggprobe.rs:109` | report status, ordered `(family, status)` pairs, warnings, finding count |
| `ProbeReportWire` | `eggprobe.rs:596` | permissive `Option`-typed wire shape, validated by hand |

Runner-side counterparts these adapters feed: `CorrectnessOutput`
(`correctness.rs:78`), `CorrectnessExecutionRecord` (`correctness.rs:189`),
`SecurityCheckResultV1` (`security.rs:86`), `DiagnosticOutput`
(`diagnostics.rs:60`), `DiagnosticsIndex` (`diagnostics.rs:124`).

## How it works

### Eggsec (`eggsec-waf`) — strict-scope WAF correctness

One `CorrectnessExecutor::execute` call (`eggsec.rs:252`) is the whole check:

1. `ensure_probed` probes `--version` on first use and enforces the `(0,1,0)`
   floor (`eggsec.rs:212-226`, `eggsec.rs:58`).
2. The bound `http_url` service binding is read from `RuntimeBindings`
   (`eggsec.rs:258-263`) and passed to `confine_target_url`
   (`eggsec.rs:264`, `eggsec.rs:471`).
3. `generate_scope_manifest` produces the deterministic strict scope
   (`eggsec.rs:267`, `eggsec.rs:526`).
4. The manifest is written under a runner-owned `scope_dir` with a check-id stem
   validated as `[A-Za-z0-9_-]` (`eggsec.rs:290-307`) and mode `0o600`
   (`eggsec.rs:427-460`), and removed on every path after the check
   (`eggsec.rs:283`).
5. `run_guarded_preflight` runs `eggsec --scope … --strict-scope --json preflight
   waf --target <url> --profile guarded` (`eggsec.rs:616-661`). **A denial fails
   closed before Eggsec sends any security traffic** (`eggsec.rs:329-333`).
6. Only then is the WAF command built: `--bypass --test-type <family>
   --concurrency <n> --timeout <s>` (`eggsec.rs:335-349`). No shell, no
   header-bypass/smuggling/evasion flags, no credentials, no proxies.
7. `parse_waf_stdout` (`eggsec.rs:840`) requires the target to match the
   requested URL exactly, rejects a `waf_detection.request_error`, requires
   1..=`MAX_FINDINGS` findings, `summary.total_findings` to equal the array
   length, a finite `bypass_success_rate` in 0..=100, and the rate to agree with
   the counted `bypass_successful` booleans within 0.05 percentage points
   (`eggsec.rs:105`, `eggsec.rs:906-911`).
8. `sanitize_finding` (`eggsec.rs:926`) keeps only technique, severity label,
   response status, the bypass boolean, and the **SHA-256 of the payload
   string**. Payload bytes, titles, and descriptions never cross
   (`eggsec.rs:961-971`).
9. Disposition is `Pass` iff `successful_bypasses <=
   context.max_successful_bypasses` (`eggsec.rs:376-380`). `Pass` and `Fail`
   are both `Ok`; only operational failures are `Err`.
10. The result is validated against `SecurityCheckResultV1::validate_contract`
    (`eggsec.rs:404`, `security.rs:138`) and re-derived with
    `debug_assert_eq!(result.recomputed(), result.disposition)`
    (`eggsec.rs:408`, `security.rs:200`).

Raw stdout carrying payload strings is parsed in memory and never staged.

**Strict scope.** `confine_target_url` accepts only `http`/`https`, only
`127.0.0.0/8`, `::1`, RFC1918, IPv6 ULA `fc00::/7`, `localhost`, and
`*.localhost` (`eggsec.rs:462-497`, `eggsec.rs:504-515`). Link-local is
deliberately rejected because a zone-scoped literal would not survive
scope-manifest matching (`eggsec.rs:499-503`). `generate_scope_manifest` emits
`require_explicit_scope = true`, one `[[allowed_targets]]` entry with the exact
host and, for an IP literal, an exact `/32` or `/128`; content is
timestamp-free so the SHA-256 is the security configuration identity
(`eggsec.rs:517-558`).

**Preflight acceptance.** `parse_preflight_stdout` (`eggsec.rs:673`) requires
bounded JSON after stripping tool log lines (`strip_tool_logs`,
`eggsec.rs:795`), operation identity in `["waf", "waf-detect"]` — both spellings
accepted because dispatch maps `waf` to `waf-detect` (`eggsec.rs:63-67`) —
`outcome_kind == "allow"` with `decision.allowed == true`, zero
`required_confirmation_classes`, `manual_override_honored == false`,
`scope_source == "cli-scope-file"`, and a reported target whose confined host
equals the requested host (`eggsec.rs:696-774`).

**Supported vs unsupported sets.**

| Set | Location | Content | Consumer |
|---|---|---|---|
| `EGGSEC_SUPPORTED_TEST_TYPES` | `eggsec.rs:69` | `sqli`, `xss`, `ssrf`, `cmd`, `traversal` (no `all`) | doctor via `eggsec_supported_test_type_names` (`doctor.rs:512`) |
| `EGGSEC_UNSUPPORTED_OPERATIONS` | `eggsec.rs:71` | 14 names: `scan`, `ci`, `stress`, `packet`, `nse`, `db-pentest`, `web-proxy`, `c2`, `postex`, `daemon`, `rest`, `mcp`, `agent`, `evasion` | `doctor.rs:516` |
| `EGGSEC_PARSER_ID` | `eggsec.rs:49` | `eggsec-waf-json/v1` | attached to the `--version` probe (`eggsec.rs:168`) and the raw artifact candidate list |
| `EGGSEC_PREFLIGHT_PARSER_ID` | `eggsec.rs:51` | `eggsec-preflight-json/v1` | declared and re-exported; not referenced by any `VersionProbeSpec` or artifact list in-tree |
| `EGGSEC_OPERATION` | `eggsec.rs:62` | `waf --json --bypass` | mirrors `security_operation_label()` (`correctness.rs:182`), which is the copy the index actually uses (`orchestration.rs:2737`) |

`EGGSEC_SUPPORTED_TEST_TYPES` is a **declared surface, not an adapter
enforcement point**. The family reaches the adapter as the raw
`CorrectnessContext::test_type` string (`correctness.rs:44`) and is passed
straight into argv (`eggsec.rs:344`). Membership is enforced upstream, where
the plan field is typed `EggsecWafTestType` and unknown variants fail at parse
([`core/src/plan.rs`](../crates/eggbench-core/src/plan.rs):148, 211).

`waf_argv_tail` (`eggsec.rs:589`) is documented as "exposed for contract tests"
and is used only by `waf_argv_is_exact_and_forbids_evasion_overrides`
(`eggsec.rs:1216`). The production argv is built inline at `eggsec.rs:335-349`
and does not call it, so the exact-argv guarantee is asserted against a
parallel construction rather than the executed one.

### Eggsec load (`eggsec-load`) — one reviewed corpus case per invocation

`EggsecLoadWorkload::execute` (`eggsec_load.rs:118`) requires
`Workload::HttpCorpus` with `schedule.len() == 1` (`eggsec_load.rs:139-141`):
one reviewed case per invocation is a hard precondition, not a default. It then
binds the immutable case:

| Step | Location | Rule |
|---|---|---|
| Probe + version floor | `eggsec_load.rs:67-85` | `check_min_version` runs **inside `probe`** (`eggsec_load.rs:83`) |
| Request/concurrency bounds | `eggsec_load.rs:145-147` | `<= 1_000_000` requests, `<= 256` concurrency |
| Corpus load by digest | `eggsec_load.rs:149-151` | `load_http_security_corpus(workspace, corpus_ref, corpus_sha256)` |
| Case selection | `eggsec_load.rs:152-156` | exact `case.id` match against `schedule[0].case_id` |
| Local confinement | `eggsec_load.rs:158` | reuses `eggsec::confine_target_url` |
| Scope + policy manifests | `eggsec_load.rs:159-164` | `generate_scope_manifest` + `generate_load_policy_manifest` |
| Private temp dir | `eggsec_load.rs:290-319` | `0o700` dir, `create_new` `0o600` files, nonce retry, `Drop` cleanup |
| Guarded preflight | `eggsec_load.rs:167-178` | `preflight load-test --profile guarded`; only `is_err()` is consulted |
| Body resolution | `eggsec_load.rs:179`, `621-682` | inline UTF-8 or file; canonicalized and required to stay under the corpus parent |
| URL append | `eggsec_load.rs:684-698` | rejects `//`, control chars, `#`, and an `@` in the authority |
| argv bounds | `eggsec_load.rs:529-619` | method shape, header count ≤ 32, name/value charset, body ≤ 16 KiB, six-hop header denylist (`eggsec_load.rs:586-594`) |
| Report parse | `eggsec_load.rs:368-408` | target match, nonzero total, `successful + failed == total`, finite latency percentiles |
| Metric mapping | `eggsec_load.rs:473-507` | `throughput`, `latency_p95`, `error_rate` from `error_kinds`, and `expected_outcome_mismatch_rate` only when responses > 0 |

Two details worth stating explicitly. First, `--quiet` is never passed because
Eggsec suppresses the machine report under it (`eggsec_load.rs:566-567`), so
report semantics stay Eggsec's. Second, the method artifact records
`transport_independent: false` and `transport: "eggfetch"`
(`eggsec_load.rs:451-452`, `eggsec_load.rs:469`): Eggsec load reuses the
Eggstack transport, so its numbers are corroboration, not an independent oracle.

**Difference from `eggsec.rs`.** Eggsec load executes *reviewed, already-written
requests* and measures them; Eggsec generates payloads and judges a WAF. The
load path therefore needs a second manifest — `generate_load_policy_manifest`
authorizes exactly `allow_load_testing` and denies fuzzing, stress, raw
packets, credential testing, and remote execution (`eggsec.rs:570-581`) — and
its preflight is `load-test`, not `waf`, with a single accepted operation string
(`eggsec_load.rs:797`). It also has no threshold/allowance concept: the
`HttpObservableExpectationV1` mismatch is a **metric**, not a pass/fail
(`eggsec_load.rs:421-426`).

**Preflight asymmetry (verified).** `probe_external_workload`'s
`EGGSEC_LOAD_DRIVER_NAME` arm (`preflight.rs:85-88`) resolves and calls
`EggsecLoadWorkload::probe`, then returns — it never constructs an adapter.
The `OHA_DRIVER_NAME` arm calls `OhaWorkload::new` (`preflight.rs:92`),
`IPERF3_DRIVER_NAME` calls `Iperf3Workload::new` (`preflight.rs:104`), and
`EGGREPLAY_DRIVER_NAME` calls `EggReplayWorkload::new` (`preflight.rs:110`).
`EggsecLoadWorkload` also has no `new` at all: only `from_resolved`
(`eggsec_load.rs:89`). The version floor is therefore not exercised through a
constructor by preflight — but it **is** enforced, because `probe` itself calls
`check_min_version` at `eggsec_load.rs:83`, which the preflight path does
invoke. The gap is the constructor, not the floor.

### EggReplay (`eggreplay-semantic`) — semantic replay

**One fixture replay = one trial.** `execute` (`eggreplay.rs:259`) runs
`eggreplay replay --fixture <abs> --target <url> --route direct --output json`
(`eggreplay.rs:275-285`) once per `InvocationContext`, so the runner's measured
invocation is a single fixture replay. `schedule` is not consulted and there is
no per-case loop.

**Findings are correctness evidence, never latency.** The mapping is
deliberately minimal: `replay_output` (`eggreplay.rs:709`) emits exactly two
observations, `semantic_findings` and `semantic_flows`, both
`Aggregation::Direct` with unit `count` (`eggreplay.rs:735-757`), no histograms,
no error counts, and `measurement_elapsed: None` (`eggreplay.rs:764`). A
nonzero finding count is a **successful** observation: the envelope's `success`
field must merely be present (`eggreplay.rs:649-653`), so `success == false`
with a well-formed payload does not become `WorkloadFailed`; only a nonzero exit
(`eggreplay.rs:300`) or a parse/contract failure does. There is no code path in
this adapter that maps a timing to a metric, and the test
`replay_output_maps_semantic_metrics_without_latency` asserts no metric name
starts with `latency` (`eggreplay.rs:1036`).

**Digest-based fixture identity.** `validate_fixture_path_syntax`
(`eggreplay.rs:395`) rejects absolute paths, drive letters, backslashes, `.`,
`..`, control/NUL, components over 128 bytes, paths over 512 bytes, and depth
over 16 (`eggreplay.rs:76-77`). `compute_fixture_identity`
(`eggreplay.rs:456`) delegates to `content_tree_identity` and rejects an empty
file set; identity is the aggregate SHA-256 over canonical ordered file records,
never the path. `join_workspace` (`eggreplay.rs:436`) rebuilds the path
component-wise as a second confinement gate.

**Envelope contract.** `EGGREPLAY_ENVELOPE_SCHEMA = 1` and
`EGGREPLAY_REPORT_SCHEMA = 2` (`eggreplay.rs:58`, `eggreplay.rs:60`) are both
enforced. `parse_validate_envelope` (`eggreplay.rs:533`) requires envelope
schema 1, `command == "validate"`, `success == true`, bounded warnings, a
`flow_count` under 100,000, and a fixture session schema in
`EGGREPLAY_ACCEPTED_FIXTURE_SCHEMAS = [1, 2]` (`eggreplay.rs:62`).
`parse_replay_envelope` (`eggreplay.rs:625`) requires envelope schema 1,
`command == "replay"`, a reports array (accepting `reports`,
`regression_reports`, or `RegressionReport`), per-report schema exactly 2,
bounds of 10,000 reports / 10,000 findings each / 100,000 total, and
`payload.finding_count` equal to the summed per-report finding count
(`eggreplay.rs:693-701`).

`ensure_preflighted` (`eggreplay.rs:216`) caches the identity and validate
preflight so the expensive contract check runs once per adapter, not once per
trial.

`preflight_semantic_replay` (`eggreplay.rs:889`) is the CLI-facing helper: it
resolves, probes, constructs the adapter (which enforces the floor and the path
syntax), computes the identity, runs the validate contract, and then fills in
`executable_version`, which `run_validate_contract` leaves empty
(`eggreplay.rs:510`, `eggreplay.rs:905`).

### Eggprobe (`eggprobe`) — pre/post diagnostics

`DiagnosticExecutor::execute` (`eggprobe.rs:206`) is one bounded execution:

1. The `MAX_DIAGNOSTIC_TIMEOUT_MS` bound (600,000 ms, `eggprobe.rs:79`) is
   checked first; exceeding it returns a `Failed` output with report status
   `timeout_policy` rather than an error (`eggprobe.rs:231-236`). The same
   bound is enforced in core plan validation
   ([`core/src/plan.rs`](../crates/eggbench-core/src/plan.rs):1744), so the
   value is pinned twice.
2. `lower_target` (`eggprobe.rs:346`) requires the target's `http_url` binding,
   requires loopback, derives host/port, and decides DNS applicability. TLS
   requires an explicit `https_url` binding and is never inferred from the port;
   a required TLS probe without one is an error, an optional one is recorded as
   `unavailable` (`eggprobe.rs:370-397`).
3. `build_probe_plan` (`eggprobe.rs:453`) emits the schema-0.3 plan: a
   `{host, port}`-only target (no `tls`/`http_url` members, because the sibling
   `TargetSpec` is `deny_unknown_fields`), typed probe objects, a microsecond
   `deadline`, `repetitions: 1`, `retries: 0`, and empty `assertions`
   (`eggprobe.rs:466-508`). When an HTTP probe is present the target authority
   is read back from the bound HTTP URL so the sibling's cross-check passes
   (`eggprobe.rs:487-497`).
4. Skipped probes are deterministic: DNS against a literal IP is
   `not_applicable`; TLS without an `https_url` binding is `unavailable`
   (`eggprobe.rs:408-444`). Both are reported as probe statuses, and both add
   a warning (`eggprobe.rs:459-465`).
5. `parse_probe_report` (`eggprobe.rs:638`) accepts **only** the string schema
   `"0.3"`; a `0.4` report is rejected even at a matching SemVer
   (`eggprobe.rs:647-655`, test at `eggprobe.rs:1078`). It also requires
   `tool.name == "eggprobe"`, a bounded tool version, a bounded non-empty
   `execution_id`, an object target, and `route.kind == "direct"`
   (`eggprobe.rs:656-681`).
6. `validate_report_probes` (`eggprobe.rs:729`) enforces set equality: every
   reported family was requested, no family repeats, and every requested family
   appears exactly once. An extra probe, a missing probe, or a duplicate is a
   parse failure, not a partial result.
7. `classify_probe_exit` (`eggprobe.rs:577`) maps exit codes: `0` with an
   `ok` report is `positive`; `0` with any other status is a contract surprise
   and fails; `1` with a parsed report is `negative` — valid evidence, not a
   process failure; `130` is `cancelled`; everything else fails.
8. A parse failure of a completed process still produces a bounded output —
   `parse_failure_output` (`eggprobe.rs:315`) preserves the raw bytes, sets
   `Failed`, and puts the redaction-safe detail in `warnings`.

**Route safety.** The plan always declares `route: {"kind": "direct"}`
(`eggprobe.rs:504`) and the report must echo `route.kind == "direct"`
(`eggprobe.rs:678-681`). No proxy, redirect, or alternative-route option is
generatable, and `require_loopback_url` (`eggprobe.rs:536`) rejects anything
outside `127.0.0.0/8`, `::1`, `localhost`.

**Families.**

| Set | Location | Content |
|---|---|---|
| `EGGPROBE_SUPPORTED_FAMILIES` | `eggprobe.rs:49` | `Dns`, `Tcp`, `Tls`, `Http` |
| `EGGPROBE_UNSUPPORTED_FAMILIES` | `eggprobe.rs:56` | `icmp`, `udp`, `trace`, `pmtu` (doctor surface, `doctor.rs:433`) |
| `EGGPROBE_MACHINE_SCHEMA` | `eggprobe.rs:47` | `"0.3"`, a dotted string with no `SchemaVersion(u32)` form; `machine_output_schema` is `None` (`eggprobe.rs:906`) |
| `EGGPROBE_PARSER_ID` | `eggprobe.rs:43` | `eggprobe-json/v1`, attached to the `--version` probe (`eggprobe.rs:151`) |

`handshake_eggprobe` (`eggprobe.rs:782`) runs before managed startup: an empty
schema-0.3 plan on loopback first, and **only** on exit code 2 does it retry once
with a single DNS probe for `localhost` (`eggprobe.rs:794-809`,
`is_handshake_exit_2` at `eggprobe.rs:814`). The retry is string-matched on the
error message and is the only place in these four adapters that branches on
error text.

## Invariants

| # | Invariant | Where |
|---|---|---|
| 1 | No correctness or diagnostic value ever becomes a `TrialMetrics` field or satisfies a `MetricRequest` | `eggsec.rs:20-22`, `eggprobe.rs:18-20` |
| 2 | Eggsec findings are sanitized: only technique, severity label, status, bypass flag, and payload SHA-256 persist | `eggsec.rs:920-973` |
| 3 | Eggsec scope is exact-local, single-target, credential-free, and digest-addressed; link-local is refused | `eggsec.rs:462-558` |
| 4 | A guarded-preflight denial fails closed before any Eggsec network traffic | `eggsec.rs:329-333`, `eggsec_load.rs:176-178` |
| 5 | Zero-case WAF output is an error, never a vacuous `Pass` | `eggsec.rs:872`, test `eggsec.rs:1360` |
| 6 | Eggsec bypass disposition is a comparison against a predeclared allowance, and `Fail` is a valid `Ok` observation | `eggsec.rs:376-380`, `correctness.rs:8-17` |
| 7 | Eggsec load runs exactly one reviewed corpus case per invocation | `eggsec_load.rs:139-141` |
| 8 | Eggsec load's argv carries no sensitive hop-by-hop or credential header | `eggsec_load.rs:586-594` |
| 9 | Eggsec load's manifests and directory are private, exclusively created, and removed on drop | `eggsec_load.rs:253-259`, `eggsec_load.rs:290-319` |
| 10 | Eggsec load records its transport as non-independent | `eggsec_load.rs:451`, `eggsec_load.rs:469` |
| 11 | One EggReplay fixture replay is one trial observation | `eggreplay.rs:18-21`, `eggreplay.rs:259-306` |
| 12 | EggReplay findings are counts, never latency, and never a `WorkloadFailed` | `eggreplay.rs:735-764`, `eggreplay.rs:1036` |
| 13 | EggReplay fixture identity is the digest, not the path | `eggreplay.rs:86-95`, `eggreplay.rs:456-472` |
| 14 | EggReplay enforces envelope schema 1 and report schema 2, and reconciles `finding_count` against the report sum | `eggreplay.rs:640`, `eggreplay.rs:669`, `eggreplay.rs:697` |
| 15 | EggReplay targets are loopback-only | `eggreplay.rs:272`, `eggreplay.rs:363-392` |
| 16 | Eggprobe accepts only machine schema `0.3` | `eggprobe.rs:647-655` |
| 17 | Eggprobe route is `direct` in both the generated plan and the report | `eggprobe.rs:504`, `eggprobe.rs:678-681` |
| 18 | Eggprobe probe statuses are a set match against the requested families | `eggprobe.rs:729-765` |
| 19 | Eggprobe diagnostics run outside every measured interval | `eggprobe.rs:18-20`, `diagnostics.rs:1-10` |
| 20 | A `negative` eggprobe exit (1) is valid evidence, not a process failure | `eggprobe.rs:572`, `eggprobe.rs:1121` |
| 21 | Every per-diagnostic timeout is bounded at 600,000 ms | `eggprobe.rs:231`, `plan.rs:1744` |
| 22 | Raw external stdout is bounded on every path (4 MiB stdout / 256 KiB stderr) | `eggsec.rs:88`, `eggprobe.rs:60`, `eggreplay.rs:68`, `eggsec_load.rs:37` |

## Failure model

| Origin | Eggsec | Eggsec load | EggReplay | Eggprobe |
|---|---|---|---|---|
| Resolution | `CorrectnessFailed` | `WorkloadFailed` | `WorkloadFailed` | `DiagnosticFailed` / failed output |
| Version floor (< floor) | `CorrectnessFailed` (`eggsec.rs:185`) | `WorkloadFailed` (`eggsec_load.rs:83`) | `WorkloadFailed` (`eggreplay.rs:175`) | `DiagnosticFailed` (`eggprobe.rs:163`) |
| Nonzero exit | `CorrectnessFailed` (`eggsec.rs:366`) | n/a (exit code is not checked; the report is) | `WorkloadFailed` (`eggreplay.rs:300`) | classified, not failed blindly (`eggprobe.rs:283`) |
| Preflight denial | `CorrectnessFailed` | `WorkloadFailed` | n/a (validate failure) | handshake `UnsupportedOption` |
| Parse failure | `CorrectnessFailed` | `WorkloadFailed` | `WorkloadFailed` | `Failed` disposition + preserved raw bytes |
| Timeout | `TimedOut` | `TimedOut` | `TimedOut` | `TimedOut` |
| Cancellation | `Cancelled` (checked before mapping) | `Cancelled` | `Cancelled` | `Cancelled` |
| Valid negative result | not modelled — `Fail` is `Ok` | not modelled — mismatch is a metric | not modelled — findings are `Ok` | `Negative` disposition, `Ok` |

The two mappings worth comparing: `probe_failure_category`
(`eggsec.rs:1068`) checks the cancellation token **before** mapping the
category, so a cancelled token reports `Cancelled` rather than a failure;
`eggprobe`'s `execute_probe` does the same inline (`eggprobe.rs:268-273`).
`classify_probe_exit` (`eggprobe.rs:577`) is the only place in this family where
a nonzero exit is a *successful* result, and only with a parsed report in hand.

`SEMANTIC_REPLAY_EVIDENCE`'s `validate_contract`
(`eggreplay.rs:820-843`) is what a direct `execute_run` consumer without CLI
preflight hits: `executable_version` must be non-empty, but
`run_validate_contract` returns it empty (`eggreplay.rs:510`), so the only
producer that can stage this artifact is one that ran a probe first.

## Boundaries

- **Siblings and substrate.** `command.rs`, `parser.rs`, `resolver.rs`,
  `version.rs`, and `artifact.rs` are owned by
  [drivers-external-substrate.md](drivers-external-substrate.md). These four
  adapters only call their public APIs.
- **Runner seams are sibling-neutral.** `correctness.rs` and `diagnostics.rs`
  both state this in their module headers (`correctness.rs:18-21`,
  `diagnostics.rs:8-10`). That claim holds for types; it does not hold for two
  string pins. `DiagnosticsIndex::validate_contract` compares
  `self.driver != "eggprobe"` (`diagnostics.rs:169`) and
  `self.machine_schema != "0.3"` (`diagnostics.rs:173`) as literals inside a
  module that documents itself as sibling-neutral, duplicating
  `EGGPROBE_DRIVER_NAME` (`eggprobe.rs:39`) and `EGGPROBE_MACHINE_SCHEMA`
  (`eggprobe.rs:47`) across a crate boundary. A schema bump must change both
  sites. `SecurityChecksIndex` avoids this by referencing
  `eggbench_core::EGGSEC_WAF_DRIVER_NAME` (`orchestration.rs:2733`,
  `security.rs:18`).
- **Shared manifest generators.** `generate_scope_manifest` and
  `generate_load_policy_manifest` are defined in `eggsec.rs` (`526`, `570`) and
  consumed by `eggsec_load.rs:11`. The load adapter is therefore coupled to
  the correctness adapter's module, not only to the substrate.
- **The driver name that Eggsec does not own.** `EGGSEC_DRIVER_NAME` is
  `"eggsec-waf"` (`eggsec.rs:45`) while the upstream tool is `eggsec`
  (`eggsec.rs:47`) and the load sibling registers as `eggsec-load`
  (`eggsec_load.rs:34`). Registry lookups and doctor summaries key on these
  distinct names (`preflight.rs:13-14`, `doctor.rs:166`, `doctor.rs:180`).
- **Operation label duplication.** `EGGSEC_OPERATION` (`eggsec.rs:62`) and
  `security_operation_label()` (`correctness.rs:182`) carry the same string;
  the index uses the runner copy (`orchestration.rs:2737`).
- **Capability declaration only.** `Capability::SecurityCheck { family:
  waf_bypass }` (`eggsec.rs:1007`) and the four `Capability::DiagnosticProbe`
  entries (`eggprobe.rs:892`) are descriptors for resolution; they are not the
  enforcement points for test-type membership or timeout bounds.
- **Not a network path.** None of the four advertises `NetworkPath` or
  `StreamFaultPlan`; all four set `external_process: true` and
  `compatible_service_types` is empty except EggReplay's `eggserve-origin`
  (`eggreplay.rs:869`).
- **Docs and plans.** User-facing claims for these tools live in
  [external-oracles.md](../docs/external-oracles.md),
  [security-qualification.md](../docs/security-qualification.md),
  [external-drivers.md](../docs/external-drivers.md),
  [driver-capabilities.md](../docs/driver-capabilities.md), and
  [eggstack-http.md](../docs/eggstack-http.md).

## Tests and qualification

| Area | Test | Location |
|---|---|---|
| Descriptor claims only WAF-bypass correctness | `descriptor_claims_only_waf_bypass_correctness` | `eggsec.rs:1138` |
| Local accepted / public refused | `target_confinement_accepts_local_rejects_public` | `eggsec.rs:1167` |
| Manifest determinism and digest addressing | `scope_manifest_is_deterministic_exact_and_digest_addressed` | `eggsec.rs:1196` |
| Exact argv, evasion overrides absent | `waf_argv_is_exact_and_forbids_evasion_overrides` | `eggsec.rs:1216` |
| Preflight allow/deny, wrong operation, wrong target | `preflight_parser_accepts_allow_rejects_denial` | `eggsec.rs:1248` |
| Bypass counting and payload sanitization | `waf_parser_counts_bypasses_and_sanitizes_payloads` | `eggsec.rs:1341` |
| Zero-case / inconsistent / tampered refusal | `waf_parser_rejects_zero_case_inconsistent_and_tampered_shapes` | `eggsec.rs:1360` |
| Binding key is the `http_url` target binding | `bindings_helper_uses_target_http_url_key` | `eggsec.rs:1387` |
| Report parses after JSON log lines | `parses_bounded_report_after_json_log_lines` | `eggsec_load.rs:880` |
| Body binding and sensitive-header rejection | `strict_argv_binds_body_and_rejects_sensitive_headers` | `eggsec_load.rs:892` |
| Policy authorizes only load testing | `load_policy_authorizes_only_load_testing` | `eggsec_load.rs:941` |
| Manifest privacy, exclusivity, drop cleanup | `manifest_files_are_private_exclusive_and_removed_on_drop` | `eggsec_load.rs:962` |
| Last standalone document wins; truncated is not a decision | `machine_document_is_the_last_standalone_document` | `eggsec_load.rs:1016` |
| Expectation mismatch is not a transport error | `expected_status_mismatch_is_not_a_transport_error` | `eggsec_load.rs:1036` |
| Validate envelope accepted | `validate_envelope_accepts_contract` | `eggreplay.rs:969` |
| Wrong schema and `success: false` rejected | `validate_envelope_rejects_wrong_schema_and_failure` | `eggreplay.rs:977` |
| Finding counting and consistency | `replay_parser_counts_findings_and_checks_consistency` | `eggreplay.rs:996` |
| Counts only, no latency metric | `replay_output_maps_semantic_metrics_without_latency` | `eggreplay.rs:1012` |
| Path-independent bounded identity | `fixture_identity_is_path_independent_and_bounded` | `eggreplay.rs:1046` |
| Symlink escape fails closed | `fixture_symlink_escape_fails_closed` | `eggreplay.rs:1076` |
| Secret-like fixture values do not leak into method evidence | `secret_like_fixture_value_does_not_leak_into_method_evidence` | `eggreplay.rs:1094` |
| Descriptor claims only M003b families | `descriptor_claims_only_m003b_families` | `eggprobe.rs:1013` |
| Schema 0.3 accepted, legacy mock dialect refused | `report_parser_accepts_schema_03_contract`, `report_parser_rejects_legacy_mock_dialect` | `eggprobe.rs:1037`, `eggprobe.rs:1049` |
| Schema 0.4 refused despite matching SemVer | `report_parser_rejects_schema_04_despite_matching_semver` | `eggprobe.rs:1078` |
| Exit 1 is negative evidence | `exit_code_mapping_treats_one_as_negative_evidence` | `eggprobe.rs:1121` |
| Lowering across HTTP/TCP/DNS and TLS policy | `target_lowering_covers_http_tcp_dns_and_tls_policy` | `eggprobe.rs:1147` |
| Typed schema-0.3 plan dialect, TLS port, HTTP authority, credential-free | four plan tests | `eggprobe.rs:1183`, `1228`, `1259`, `1290` |
| Debug redaction of executable identity | per-file | `eggsec_load.rs:108`, `eggreplay.rs:248`, `eggprobe.rs:192` |

Runner-side doubles for the same seams are `FakeCorrectnessExecutor`
(`correctness.rs:224`) and `FakeDiagnosticExecutor` (`diagnostics.rs:310`); a
live end-to-end path exists at
[`tests/m004b_live.rs`](../crates/eggbench-cli/tests/m004b_live.rs):327.
Gaps in the unit layer: `run_load_preflight`'s acceptance branch, the
`EGGSEC_UNSUPPORTED_OPERATIONS`/load-case precondition checks, and
`create_manifest_files` nonce collision are not directly exercised.

## Review focus

1. **`waf_argv_tail` is not the executed argv.** The exact-argv contract test
   (`eggsec.rs:1216`) asserts a helper that production does not call
   (`eggsec.rs:589` vs `eggsec.rs:335-349`). Any change to the real argv needs a
   matching change here or the test proves nothing about the process.
2. **The duplicated Eggprobe pin.** `diagnostics.rs:169` and `:173` hardcode
   `eggprobe` and `0.3` in a module documented as sibling-neutral. A schema or
   driver-name change must be made in two crates.
3. **`EGGSEC_PREFLIGHT_PARSER_ID` has no consumer.**
   `eggsec.rs:51` is declared and re-exported (`mod.rs:40`, `lib.rs:47`) but no
   `VersionProbeSpec` or artifact list references it; the guarded preflight
   builds a bare `ExternalCommandSpec` (`eggsec.rs:634`). Either attach it or
   drop it.
4. **`parse_preflight_stdout` discards its digest argument and returns empty
   provenance.** `let _ = expected_executable_sha;` (`eggsec.rs:678`) and
   `EggsecPreflight` with three empty strings (`eggsec.rs:777-781`) mean the
   caller's `Ok(proof)` at `eggsec.rs:331-333` carries no information; the
   result is dropped. Same pattern for `execution_id` in
   `parse_probe_report` (`eggprobe.rs:674`) and `executable_version` in
   `run_validate_contract` (`eggreplay.rs:510`).
5. **`is_handshake_exit_2` string-matches an error message**
   (`eggprobe.rs:814-816`). Any change to `DriverError`'s `Display` silently
   removes the DNS-plan retry, and the failure mode is a confusing handshake
   error rather than a clear one.
6. **The `exit_code` → disposition table is a policy decision.**
   `classify_probe_exit` (`eggprobe.rs:577`) treats exit 1 as a valid negative
   observation. A new Eggprobe exit code silently becomes `failed`; the
   doc comment enumerates 2 and 3 with no code to match them.
7. **`preflight_semantic_replay` is the only path that fills
   `executable_version`.** A consumer that calls `execute_run` without CLI
   preflight gets `None` from `run_evidence`
   (`eggreplay.rs:317-319`) or a contract rejection
   (`eggreplay.rs:830-831`).
8. **The Eggsec load preflight arm** (`preflight.rs:85-88`) is the only one of
   the five external workloads that does not construct its adapter. The floor is
   still enforced inside `probe` (`eggsec_load.rs:83`), but the shape differs
   from oha/h2load/iperf3/eggreplay. Related: `load_output` divides by total
   requests (`eggsec_load.rs:494`), and the nonzero divisor is guaranteed by
   `parse_load_report` (`eggsec_load.rs:381`) rather than at the division site.

## Related

- [drivers.md](drivers.md) — crate-level ownership; the "External command
  substrate and tool adapters" section names only `oha`, `h2load`, and `iperf3`
  as the substrate's tool adapters, which does not cover these four.
- [drivers-external-substrate.md](drivers-external-substrate.md) — the
  `run_command` / `BinaryResolver` / `VersionProbe` APIs these adapters call.
- [drivers-catalog-features.md](drivers-catalog-features.md) — descriptor and
  capability registration for all four.
- [runner-adapter-seams.md](runner-adapter-seams.md) — `CorrectnessExecutor`
  and `DiagnosticExecutor` trait shapes and the one-shot contract.
- [runner-orchestration.md](runner-orchestration.md) — phase order: correctness
  before warmups, diagnostics post-readiness and post-drain, both outside
  `execute_invocation`.
- [core-qualification-security.md](core-qualification-security.md) —
  `SecurityCheckResultV1` and the disposition algebra.
- [evidence.md](evidence.md) — artifact roles, redaction, and staging.
- [../docs/external-oracles.md](../docs/external-oracles.md),
  [../docs/security-qualification.md](../docs/security-qualification.md),
  [../docs/external-drivers.md](../docs/external-drivers.md)
