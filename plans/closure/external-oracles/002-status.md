# External Oracles M002 — HTTP and Capacity Oracles — Closure

Disposition: **closed**
Closed: 2026-09-24
Implementation commit: (this commit) on top of planning baseline `69707fd`
(plus authored plan `plans/implementation/external-oracles/002-http-and-capacity-oracles.md`).
Hosted qualification: not run in this pass; local verification only. The
four-lane hosted CI remains required before release qualification claims.
Windows/macOS coverage is code-present (argv-only, no POSIX-only calls in
new code) but locally unexercised.

## 1. Requirement-to-evidence matrix

| Plan § / requirement | Evidence | Outcome |
|---|---|---|
| §3 exact path+SHA+version in every trial; machine output where available | `command-metadata.json` per trial (substrate, unchanged); parsers for oha JSON / iperf3 JSON / anchored h2load text | Pass |
| §3 no load-model fallback | `oha_argv`/`h2load_argv`/`iperf3_argv` reject count+duration pairs, OpenLoop-on-h2load, count-bound iperf3, non-http(s) targets with `unsupported_option`; capability matrices match | Pass |
| §3 versioned fixture-tested parsers; nonfinite rejected | `oha-json/v1`, `h2load-text/v1`, `iperf3-json/v1`; per-tool invalid/malformed/truncated/version matrices; `finite_non_negative` guards | Pass |
| §3 unachieved load visible, never auto-invalid | oha `error_rate`+`oha:*` counts, h2load failed/errored/timeout counts, iperf3 `error` member → trial failure; trials complete with evidence instead of invalid verdicts | Pass |
| §3 absence is capability error, minimal build intact | `BinaryResolver` + `MissingExecutablePath`/`missing_driver`; `cargo tree` shows zero new dependencies; feature-off suite green | Pass |
| §3 argv-only, stdin null, LC_ALL=C, bounded, cancellable | `ExternalCommandSpec` construction in all three executors; cancellation test; timeout via runner deadline | Pass |
| §5 oha mapping (count/duration/rate, nullable timing, parity metrics, status artifact) | `oha.rs`; live oha 1.16.0 loopback (trial + CLI e2e); all-fail null-percentile handling | Pass |
| §5 h2load mapping (count/duration, `--h1` cleartext, anchored rows, parity subset) | `h2load.rs`; live nghttp2-1.59.0 loopback; nghttp2-release probe parsing | Pass |
| §5 iperf3 mapping (duration-only, host/port from URL authority, `-t` ceil, `-P`, bps names) | `iperf3.rs`; live iperf 3.16 loopback via fixture server; `error`-member failure | Pass |
| §5 descriptors (external_process, capabilities, schema v1, non-default) | `*_descriptor()` + catalog test asserting flags | Pass |
| §5 doctor presence/capability matrix without load; run constructs | `binary_present` (filesystem-only) + capability labels; `production_workload_executor` + run preflight probe | Pass |
| §5 CLI selection without schema change | `--workload-driver` on `doctor`/`run` → `ResolutionOptions::selections`; unknown → `missing_driver`, malformed → `usage` | Pass |
| §6 additive-only | No core/runner/plan/manifest/envelope-schema changes; one additive `DriverSummary.binary_present` (`Option`, `serde(default)`); one additive `ErrorCategory::UnsupportedOption` | Pass |
| §8 failure taxonomy and pre-startup gates | `failure_category`/`probe_failure_category`; missing binary + bad version fail exit 3 before startup; cancellation preserved through probe | Pass |
| Acceptance 1–6 | Live loopback trials for all three with retained raw + provenance + metrics and verified bundles (oha CLI e2e asserts bundle verify); missing-binary/version gates; parse-failure closure; load-shortfall visibility; no denominator coercion; matrices green incl. MSRV | Pass |

Planned-but-adjusted (recorded, not hidden):
- `offered_load_shortfall` warning label (§5): `WorkloadOutput` has no
  warnings channel, so shortfall visibility is carried by `error_rate` /
  `error_counts` / raw evidence instead. No silent loss.
- `--get-server-output` (§7): omitted; server output embedding needs
  foreign-server cooperation contrary to the explicit-host design.
- h2load probe bypasses `VersionProbe::run` (generic token extractor
  cannot isolate `nghttp2/<release>`); direct `run_command` + explicit
  marker parse with identical bounds.
- `doctor` reports binary *presence* only, not probed versions (spawning
  tools would break the doctor side-effect-free precedent set by Gregg
  M001b); versions gate `run` preflight and trial evidence.
- oha all-fail `latencyPercentiles` are JSON `null` (live ground truth
  corrected the plan's 0.0 assumption); they stay missing, never zeroed.
- iperf3 fixture servers run persistent `-s` (no `-1`): the TCP readiness
  probe would otherwise consume the single-test slot.

No stop condition (§11) triggered: live outputs matched the plan's ground
truth except the null-percentile correction above (handled by accepting
`Option`); no schema change was needed; no server orchestration was needed
for loopback trials.

## 2. Tests/guards run and outcomes

- `cargo fmt --all -- --check` — pass
- `cargo check --workspace --all-targets --locked` — pass
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` — pass (pedantic-clean)
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — pass (feature-off clean)
- `EGGBENCH_PARENT_SENTINEL_26CE=set-in-parent-process cargo test --workspace --all-targets --all-features --locked` — pass: **303 passed (18 suites)**, incl. 11 new `oracles` integration tests (live oha/h2load/iperf3 loopback, unreachable-target visibility, cancellation, raw determinism), per-tool unit matrices, 4 new CLI tests (oha e2e, unknown/malformed selection, presence)
- Feature-off `cargo test --workspace --all-targets --locked` — pass: **263 passed (18 suites)**
- `cargo +1.89.0 check --workspace --all-targets --all-features --locked` — pass
- `cargo +1.89.0 test --workspace --all-targets --all-features --locked` — pass: 303 passed (18 suites), 0 failures
- `cargo tree --locked -p eggbench-drivers` — zero new dependencies (substrate + serde_json only)
- `git diff --check` — pass
- Live versions: oha 1.16.0, h2load nghttp2/1.59.0, iperf 3.16, all against loopback

## 3. Schema/migration/compatibility evidence

- Additive only: three driver names, three parser ids, three status/raw
  artifact names, `bps`/`count` units in plan-declared descriptors,
  `ErrorCategory::UnsupportedOption` (`unsupported_option`),
  `DriverSummary.binary_present` (optional, defaulted). Plan v1,
  ResolvedPlan v1, manifest v2, TrialMetrics v1, envelope v1, exit codes
  unchanged (new failure *categories* reuse exit 3/2).
- Behavior change (not schema): the production catalog now always
  registers the three oracles, so minimal builds report
  `has_workload_driver=true` and default resolution without a unique
  default fails `ambiguous_selection` (previously `missing_driver` on an
  empty catalog). Stale tests/docs updated; the failure stays explicit
  and pre-startup.

## 4. Security and lifecycle evidence

- argv-only construction, stdin null, `LC_ALL=C`/`LANG=C`, bounded
  capture (4 MiB/1 MiB stdout, 256 KiB stderr), 10 s version probes;
  trusted resolution (no cwd search, executable bits, canonical SHA-256).
- No credentials exist in these flows; provenance carries host/port only.
- Cancellation honored end to end incl. through version probing
  (`probe_failure_category` re-checks the token around the substrate's
  `VersionProbeTimeout` remap); timeouts propagate via the runner
  deadline; executors hold no persistent state (drain is trivially clean).
- iperf3 server is never managed by Eggbench (explicit-host design);
  test fixture servers are killed by scope guards.

## 5. Documentation/operational evidence

- Added `docs/external-oracles.md` (selection, per-tool mapping tables,
  failure visibility, evidence layout).
- Updated `docs/external-drivers.md` (M002 adapters landed),
  `docs/driver-capabilities.md` (oracle matrices + selection semantics),
  `docs/cli.md` (`--workload-driver`, presence, failure categories),
  `architecture/drivers.md`, `docs/eggstack-http.md`, `README.md`
  (catalog always carries oracles), `main.rs` doc comment.

## 6. Known limitations

- Option coverage is the plan §4 minimal subset (no oha bodies/auth/
  redirect-breakdown/CSV, no h2load scripts/headers/H3, no iperf3
  UDP/reverse/server-output/window tuning).
- iperf3 against a foreign host is untested beyond loopback; server
  orchestration stays out of scope by design.
- Hosted four-lane CI not run in this pass; Windows/macOS tool behavior
  unexercised locally (new code is platform-neutral argv/parse logic).
- `default_workload()` registry helper is test-only surface; production
  selection flows through core `select_driver`.
- Environment note: this host gained `oha` 1.16.0 at `/usr/local/bin/oha`
  (cargo-built) and `iperf3` 3.16 (apt) for grounding; neither is an
  Eggbench dependency.

## 7. Unresolved findings

| Severity | Finding | Disposition |
|---|---|---|
| None | No unresolved M002 correctness, security, lifecycle, or portability finding | No corrective work required |
| Note | Hosted CI qualification outstanding (Linux stable, Linux 1.89, macOS, Windows) | Tracked as follow-up; local matrix is green incl. MSRV 1.89 tests |

## 8. Disposition

**Closed.** oha/h2load/iperf3 workload drivers on the M001 substrate with
versioned parsers, honest load-model mapping, explicit selection and
pre-startup gates, live loopback evidence with verified bundles,
parity/diagnostic metrics, and documentation are landed and
regression-tested. External Oracles M002 is complete; M003 netem remains
future.
