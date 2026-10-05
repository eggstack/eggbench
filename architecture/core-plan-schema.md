# Core plan schema

`eggbench-core`'s plan module is the input contract of the whole system: it is
the only thing a human authors, the first thing the CLI parses, and the first
thing the runner resolves. It is also the only schema in the workspace with ten
declared versions, so almost every downstream feature has a footnote here.

## Purpose

This deep dive answers three questions:

1. What is the shape of a validated `ExperimentPlan`, section by section?
2. How does schema versioning v1→v10 actually work in the code — not what the
   release notes say?
3. What does a plan *reject*, and why is each rejection the right one?

It is a companion to [core.md](core.md), which explains why the crate boundary
exists. [overview.md](overview.md) places it in the artifact chain.

The module is deliberately runtime-free: no file reads, no process execution,
no clock, no network. Validation is total and pure, so the same input always
produces the same `Result`.

## Source layout

| File | Lines | Role |
|---|---|---|
| [`crates/eggbench-core/src/plan.rs`](../crates/eggbench-core/src/plan.rs) | 3,209 | Plan types, decode, `validate()`, network-path contract, unit tests |
| [`crates/eggbench-core/src/types.rs`](../crates/eggbench-core/src/types.rs) | 255 | `SchemaVersion`, `Name`, `SecretRef`, `DurationMs`, `RateMilliRps`, `BasisPoints`, `PositiveCount`, `BoundError` |
| [`crates/eggbench-core/src/lib.rs`](../crates/eggbench-core/src/lib.rs) | 45 | Re-exports, `CORE_SCHEMA_NAMESPACE`, the ten `EXPERIMENT_PLAN_SCHEMA_VERSION*` constants |

Supporting types referenced but not owned here:

- `NetworkPathRequest`, `RouteMode`, `StreamFaultRequest` — [network_path.rs](../crates/eggbench-core/src/network_path.rs)
- `MAX_SECURITY_CHECKS`, `MIN/MAX_SECURITY_CONCURRENCY`, `MIN/MAX_SECURITY_TIMEOUT_MS`, `MAX_SECURITY_CASES`, `SECURITY_SOURCE_EGGSEC_WAF` — [security.rs](../crates/eggbench-core/src/security.rs)
- `DriverDescriptor`, `ResolvedPlan` — [core-driver-resolution.md](core-driver-resolution.md)

Non-Rust context: [../docs/experiment-plan.md](../docs/experiment-plan.md),
[../docs/paired-experiments.md](../docs/paired-experiments.md),
[../docs/eggstack-http.md](../docs/eggstack-http.md),
[../docs/security-qualification.md](../docs/security-qualification.md), and the
worked plans in [../examples/](../examples/).

## Key types

### Top-level sections

One struct, `ExperimentPlan` (plan.rs:16), `deny_unknown_fields` (plan.rs:15).
Additive fields carry `#[serde(default)]`; versioned optional fields carry the
three-attribute pattern that makes *presence* significant.

| Section | Type | Field | Schema | Owns |
|---|---|---|---|---|
| `schema_version` | `SchemaVersion` | plan.rs:18 | all | Explicit input version, a transparent `u32` newtype |
| `experiment` | `Name` | plan.rs:20 | all | Stable run identity, bound to `Name::MAX_LEN` |
| `subject` | `Subject` | plan.rs:22 | all | `managed_command` / `external` / `label` provenance declaration |
| `services` | `Vec<Service>` | plan.rs:25 | all | Topology: names, kinds, lifecycle, acyclic `depends_on`, bindings |
| `workload` | `Workload` | plan.rs:27 | all | Load shape and target; six variants |
| `trials` | `TrialPolicy` | plan.rs:29 | all | Measured/warmup counts, cooldown, reset, phase timeouts |
| `telemetry` | `Vec<TelemetryRequest>` | plan.rs:32 | all | Requested collector fields and whether absence is fatal |
| `metrics` | `Vec<MetricRequest>` | plan.rs:35 | all | Metric name/unit/direction/intent and optional gate |
| `environment_policy` | `EnvironmentPolicy` | plan.rs:37 | all | `strict_same_testbed` / `warn_on_mismatch` / `cross_testbed_descriptive` |
| `seed` | `Option<u64>` | plan.rs:39 | all | Deterministic seed; mandatory for some workloads |
| `paired` | `Option<PairedDesign>` | plan.rs:42 | v2+ | Baseline/candidate arms, alternating schedule |
| `network_path` | `Option<NetworkPathRequest>` | plan.rs:49 | **v3 only** | Route-first/fault-second stream composition |
| `diagnostics` | `Option<Vec<DiagnosticRequest>>` | plan.rs:58 | v5+ | Pre/post-workload probes outside measured intervals |
| `security_checks` | `Option<Vec<SecurityCheckRequest>>` | plan.rs:69 | v6+ | Eggsec strict-scope WAF bypass correctness |
| `http_corpus_checks` | `Option<Vec<HttpCorpusCheckRequest>>` | plan.rs:76 | v8+ | Fixed-corpus HTTP correctness |
| `bounds` | `ArtifactBounds` | plan.rs:78 | all | Later evidence-writer ceilings |

`seed`, `telemetry`, `metrics`, and every `Option` are required *keys* unless
`#[serde(default)]` is present: `services`, `telemetry`, `metrics`, and the
four versioned optionals default; `schema_version`, `experiment`, `subject`,
`workload`, `trials`, `environment_policy`, `seed`, and `bounds` must all be
spelled out (possibly as `null`). Every `Service` likewise requires
`readiness`, `shutdown`, `working_directory`, and `log_limit_bytes`
(plan.rs:284-290) — only `depends_on`, `config`, `http_url`, `static_bindings`,
`binding_args`, and `binding_env` are omittable.

### Workload variants

`Workload` is `#[serde(tag = "kind", deny_unknown_fields)]` (plan.rs:368), so
the wire form is `{"kind": "...", ...}` and both the variant name and its
fields are strict.

| Variant | Introduced | Shape |
|---|---|---|
| `ClosedLoop` | v1 | `target`, `concurrency`, exactly one of `requests`/`duration_ms` |
| `OpenLoop` | v1 | `target`, `rate_milli_rps`, exactly one of `requests`/`duration_ms` |
| `FiniteCount` | v1 | `target`, `requests`, `concurrency` |
| `TimeBounded` | v1 | `target`, `duration_ms`, `mode`, and exactly one of `concurrency`/`rate_milli_rps` |
| `SemanticReplay` | v4 | `target`, relative `fixture` path; one fixture replay = one trial |
| `HttpCorpus` | v10 | `target`, `corpus_ref`, `corpus_sha256`, `schedule`, `concurrency`, `connection_policy`, `default_headers` |

`Workload::target()` and `with_target()` (plan.rs:1359, plan.rs:1367) exist for
the paired runner: `with_target` rewrites only the destination so the load shape
is provably identical across arms.

### The bounded type vocabulary (types.rs)

These are not conveniences. Each exists because the value crosses into evidence,
path construction, or a resource limit.

| Type | Bound | Enforced at | Why it exists |
|---|---|---|---|
| `SchemaVersion(pub u32)` | none | — | Explicit version on every serialized contract; `Ord` lets evidence compare versions |
| `Name(String)` | non-empty, non-whitespace, no control chars, ≤128 bytes | types.rs:22-34 | Names become artifact path components and reference keys; an unbounded or control-bearing name would escape bundle paths |
| `SecretRef { reference: Name }` | structure only | types.rs:80-83 | A plan field that *can only* hold a name. `Debug`/`Display` both emit `[REDACTED]` (types.rs:84-96) |
| `DurationMs(u64)` | 1 ms … 1 year (31,536,000,000) | types.rs:107-113 | Whole-millisecond positivity; zero would mean "no timeout" |
| `RateMilliRps(u64)` | 0.001 … 1,000,000 rps | types.rs:141-149 | Milli precision avoids a float on the wire |
| `BasisPoints(u16)` | 0 … 10,000 bp | types.rs:177-183 | Percentages as integers; no float comparison in a gate |
| `PositiveCount(NonZeroU32)` | 1 … 1,000,000 | types.rs:211-218 | `NonZeroU32` makes "zero is invalid" unrepresentable in the type |

Each numeric type uses `#[serde(try_from = "u64", into = "u64")]`, so the bound
is checked *during decode* and a violation surfaces as a parse error rather than
a later validation error.

## How it works

### Versioning is a validation concern, not a type concern

There is **one struct per plan, not one per version**. No enum of versions, no
untagged union, no per-version DTO, no `#[serde(untagged)]`. All ten versions
decode through the same `ExperimentPlan`, with `deny_unknown_fields` applied
uniformly at every level.

`schema_version` is pure data. The compatibility decision lives entirely in
`ExperimentPlan::validate()` (plan.rs:843), expressed as per-field allowlists of
version constants. The top gate (plan.rs:844-856) rejects any version not in
1..=10 with `PlanError::UnsupportedVersion`; everything after it is a chain of
`if <field is present> && <version not in {…}> { fail }`.

Consequences worth being precise about:

- **A v1 plan read as v10 succeeds** if it uses only v1 fields. `validate()`
  returns `Ok` and the run proceeds on v1 semantics. Nothing is upgraded or
  rewritten; the staged `plan.json` keeps `schema_version: 1`.
- **A v1 plan carrying a v2+ field decodes, then fails validation.** `paired` is
  a *known* field on the single struct, so a v1 plan with `paired` parses
  cleanly and fails at plan.rs:2006-2008 with `UnsupportedVersion(1)`. The test
  `v1_plan_with_paired_design_fails_closed_at_validation` (plan.rs:2671) pins
  this exact path.
- **A field that exists in no version fails at decode.** `"foo": 1` under
  `deny_unknown_fields` produces `PlanError::Parse`, not a
  `Validation { category }`. So a future-version field is *more* visible than a
  known-future-version field.
- **`UnsupportedVersion` is overloaded.** It means both "no such version"
  (plan.rs:855) and "this version cannot carry this field" (plan.rs:2007). A
  caller matching only on `Validation.category` will miss the second case. This
  is deliberate for `paired` (its v1 rejection predates the category mechanism)
  and is the one asymmetry in the error model.

The wording in [core.md](core.md) — "v1/v2 remain readable and reject the
field" — is accurate about behaviour but imprecise about mechanism: the field is
*accepted by the decoder* and rejected by `validate()`.

### Presence is significant: `deserialize_present_optional`

Fields that arrived in a later version use a three-attribute pattern
(plan.rs:44-48, repeated for `diagnostics`, `security_checks`,
`http_corpus_checks`):

```rust
#[serde(
    default,
    skip_serializing_if = "Option::is_none",
    deserialize_with = "deserialize_present_optional"
)]
```

`deserialize_present_optional` (plan.rs:651) is `T::deserialize(d).map(Some)`.
Combined with `default`, this distinguishes three states:

| Input | Decoded |
|---|---|
| key absent | `None` |
| key present, value empty list/object | `Some(empty)` |
| key present, `null` | **parse error** |

`Some(empty)` is the point. An explicitly empty `diagnostics: []` on a v4 plan
fails closed with `unsupported_option` (plan.rs:891-905) rather than being
silently treated as "no diagnostics requested", which would let a v4 plan carry
a field whose meaning it cannot honour. `skip_serializing_if` keeps the
distinction on the write side so round-trips are byte-stable.

Note that `paired` (plan.rs:41-42) uses a plain `#[serde(default)]` and so does
*not* have this property: `"paired": null` decodes to `None` indistinguishably
from absent. Only the four post-v3 optionals get the strict treatment.

### Decoding, and recovering a category from a bad decode

`from_json` (plan.rs:800) and `from_toml` (plan.rs:810) decode then call
`validate()`. Both funnel serde errors through `map_deserialize_error`
(plan.rs:762), which re-parses the input as `serde_json::Value` and uses
`raw_faults_invalid` (plan.rs:661) / `raw_route_invalid` (plan.rs:738) to
re-derive a *stable validation category* when serde's message is generic — e.g.
a `DurationMs` bound violation nested inside `network_path.stream_faults`
surfaces as `invalid_fault_plan`, not as a parse error the caller cannot
classify.

This re-parse is JSON-only. For a TOML input `serde_json::from_str::<Value>`
fails, so the raw-inspection branch is dead and only the message-substring
fallback (plan.rs:768, plan.rs:780) applies. TOML inputs therefore get a
coarser classification of nested network-path errors. Low severity — the plan
still fails closed — but the two formats are not symmetric.

`to_json` (plan.rs:820) and `to_toml` (plan.rs:831) both call `self.validate()`
first. A mutated in-memory plan that fails validation cannot be serialized.

### Validation order

`validate()` is one function, deliberately kept in a single reviewable pass
(`#[allow(clippy::too_many_lines)]`, plan.rs:842). The order encodes precedence:

1. Version gate (plan.rs:844) — unknown version, `UnsupportedVersion`.
2. Per-field version allowlists (plan.rs:860-939) — `security_checks`,
   `http_corpus_checks`, `diagnostics`, `SemanticReplay`, `HttpCorpus`.
3. Composition incompatibilities, *before* the generic gates, so callers get the
   specific category: `workload_path_incompatible` (plan.rs:945),
   `diagnostic_path_incompatible` (plan.rs:957),
   `security_path_incompatible` (plan.rs:975). The comments at plan.rs:940-944,
   951-956, and 968-974 state this intent explicitly.
4. `network_path` version gate (plan.rs:987) — v3 only — then its contract.
5. Per-service checks in a loop (plan.rs:1028-1169).
6. Reference resolution: workload target (plan.rs:1170), `depends_on`
   (plan.rs:1183), acyclicity (plan.rs:1199).
7. Delegated validators: `validate_workload` (plan.rs:1448),
   `validate_paired` (plan.rs:1999), `validate_diagnostics` (plan.rs:1684),
   `validate_security_checks` (plan.rs:1775), `validate_http_corpus_checks`
   (plan.rs:1883).
8. Trial, metric, and bounds rules (plan.rs:1225-1319).

Because the composition gates precede the version gate, a v4 SemanticReplay
plan with `network_path` reports `workload_path_incompatible` rather than
`unsupported_option` — pinned by `semantic_replay_rejects_network_path`
(plan.rs:2875).

### The network-path sub-contract

`validate_network_path_contract` (plan.rs:2110) is `pub` and reused by
[evidence.rs](../crates/eggbench-core/src/evidence.rs#L1659),
[comparison.rs](../crates/eggbench-core/src/comparison.rs#L887), and the
dialer, so a route accepted at plan time is rejected identically everywhere
else. It bounds the chain to 1,024 bytes, rejects `@ % ? #` as
`route_credentials_not_supported` (plan.rs:2130), and delegates per-hop shape to
`validate_proxy_chain_shape` (plan.rs:2156) → `validate_route_endpoint`
(plan.rs:2215), which allows only `http`/`socks4`/`socks4a`/`socks5` with an
explicit port and parses bracketed hosts as `Ipv6Addr`.

`canonical_proxy_chain_text` (plan.rs:2185) normalizes `socks4a` → `socks4` and
re-emits `[host]:port`. It calls `.expect()` four times and is therefore
only safe on an already-validated chain — every call site
(comparison.rs:916, network_path.rs:70) validates first.

## Invariants

### Rejection categories

`PlanError` (plan.rs:629) has three variants. `Validation { category, detail }`
carries a `&'static str` category intended to be machine-readable and stable;
tests assert on these strings, so they are part of the contract.

| Category | Raised for |
|---|---|
| `Parse` | Syntax, unknown field, unknown variant, type-level bound violation |
| `UnsupportedVersion` | Version outside 1..=10, or `paired` on v1 |
| `invalid_bound` | Numeric/count/size violations, invalid digest hex, unsafe path shape |
| `unsupported_option` | Field present on a version that cannot carry it; pinned source labels |
| `contradictory_configuration` | Mutually exclusive or semantically impossible combinations |
| `missing_reference` | Target/service/dependency/reset name not declared |
| `duplicate_identity` | Repeated service, metric, diagnostic, security check, fault, or probe ID |
| `cycle` | Service dependency cycle (`ensure_acyclic`, plan.rs:2064) |
| `invalid_route` | Route chain/endpoint shape |
| `route_credentials_not_supported` | Credentials in a route chain or a path-scoped service config |
| `invalid_fault_plan` | Malformed stream-fault plan |
| `unsupported_route` | Route hop scheme outside the supported set |
| `invalid_binding` / `invalid_fixture` / `invalid_reference` / `invalid_digest` | `http_url` shape; fixture path escaping its bounds; duplicate corpus case ID; non-hex corpus digest |
| `missing_fault_seed` / `missing_required_metric` / `unsupported_gate` | Faulted path or `http_corpus` without `seed`; `http_corpus` without the absolute-zero mismatch gate; relative/statistical gate on `semantic_findings` |
| `paired_security_not_supported` | Correctness checks composed with a paired design |
| `*_path_incompatible`, `paired_network_path_not_supported` | Composition rejections: see the ordering note above |

### Structural bounds

| Subject | Bound | Line |
|---|---|---|
| Services per plan | ≤ 256 | plan.rs:1025 |
| Static bindings per service | ≤ 32 | plan.rs:1044 |
| Static binding value | non-empty, ≤ 2048 bytes, no controls, not secret-looking | plan.rs:1050-1063 |
| Diagnostic requests / probes / timeout | ≤ 32 requests; 1..=16 unique probes; ≤ 600,000 ms (`MAX_DIAGNOSTIC_TIMEOUT_MS`, plan.rs:1679) | plan.rs:1689, 1727, 1744 |
| Diagnostic ID | 1..=64 ASCII `[A-Za-z0-9_-]`; becomes `diagnostics/<phase>/<id>.json` | plan.rs:1700-1703 |
| Security checks / ID | ≤ `MAX_SECURITY_CHECKS` (16); same ID alphabet | plan.rs:1780, 1800 |
| Security concurrency / timeout / allowance | 1..=32; 1,000..=120,000 ms; ≤ `MAX_SECURITY_CASES` (1,024) | plan.rs:1826, 1840, 1854 |
| HTTP corpus checks | ≤ `MAX_SECURITY_CHECKS` (16), reused from the security bound; the detail string says 32 | plan.rs:1888 |
| `http_corpus` schedule / concurrency / total | 1..=128 unique case IDs; ≤ 256 workers; ≤ 1,000,000 requests (checked_add) | plan.rs:1500, 1552-1564 |
| `http_corpus` default headers | ≤ 32, name ≤ 256, value ≤ 8192, no hop-by-hop/auth headers | plan.rs:1506-1542 |
| Metrics / telemetry requests / warmups | ≤ 256 / ≤ 128 / ≤ 1,000 | plan.rs:1236, 1225 |
| Stream faults per direction | ≤ 128, unique IDs, `slice.variation < average_size` | plan.rs:2313-2352 |
| Route chain | ≤ 1,024 bytes | plan.rs:2111 |
| Fixture path | 1..=512 bytes, ≤ 16 components, ≤ 128 bytes each | plan.rs:1612-1673 |
| Service `static_bindings` | ≤ 32; value non-empty, ≤ 2048 bytes, no controls, not secret-looking | plan.rs:1044, 1050-1063 |
| `ArtifactBounds` / `log_limit_bytes` | `artifact_bytes > 0`, `total_bytes ≥ artifact_bytes`; each log limit ≤ `artifact_bytes` | plan.rs:1311, 1095 |

### Cross-field consistency

- Exactly one of `requests`/`duration_ms` for `ClosedLoop` and `OpenLoop`
  (plan.rs:1460); exactly one of `concurrency`/`rate_milli_rps`, matching
  `mode`, for `TimeBounded` (plan.rs:1474).
- Workload target must be a declared service or `Subject::External.target`
  (plan.rs:1170-1182). The same allowance is repeated for each diagnostic
  (plan.rs:1754), security check (plan.rs:1865), and HTTP corpus check
  (plan.rs:1969).
- `depends_on` must resolve and must not be self-referential
  (plan.rs:1183-1197); the graph must be acyclic (plan.rs:1199).
- Managed `ServiceKind::Command` needs a program (plan.rs:1104); a managed
  `Subject::ManagedCommand` needs one too (plan.rs:1016).
- An `External` command service may not request `shutdown` (plan.rs:1157).
- `static_bindings.http_url` must agree with the legacy `http_url` field
  (plan.rs:1064-1074) → `contradictory_configuration`.
- Runtime binding destinations require a `Command` service (plan.rs:1114); a
  binding source must be a declared dependency (plan.rs:1329); an argv
  destination must be `index != 0`, in range, and unique (plan.rs:1122).
- A diagnostic-metric or informational metric may not carry a gate
  (plan.rs:1259, plan.rs:1274); `TargetRange` must be finite and ordered
  (plan.rs:1250); an `Absolute` gate value must be finite (plan.rs:1266).
- `http_corpus` requires a primary `expected_outcome_mismatch_rate`, unit
  `ratio`, `lower_is_better`, `Gate::Absolute { value: 0.0 }` (plan.rs:1297-1310)
  — an exact tuple, not merely "a gate on that metric".
- Non-empty stream faults require an explicit `seed` (plan.rs:1009);
  `http_corpus` always requires one (plan.rs:933).

### Secret discipline

`SecretRef` appears in exactly one plan field: `Subject::ManagedCommand.
environment: BTreeMap<String, SecretRef>` (plan.rs:232). A plan therefore
*cannot* express a credential value in that position. Elsewhere the discipline
is convention plus heuristics, not structure:

- `Service.config` values are checked for credentials **only when `network_path`
  is present** (`validate_path_service_configs`, plan.rs:2260), and that check
  first whitelists the keys to `path`/`body_bytes`/`status`.
- `looks_secret` (plan.rs:1598) is a six-marker substring heuristic
  (`password=`, `token=`, `secret=`, `authorization:`, `bearer `, `api_key=`)
  applied to static-binding values and `http_corpus` default headers. A bare
  token without a marker passes.
- `argv`, `working_directory`, and `Subject::ManagedCommand.argv` receive no
  credential screening at all.

A plan carries no runtime objects: no handles, no trait objects, no closures, no
paths that are not bounded strings. `Service.readiness` is either a `DurationMs`
delay or a `Name` probe to be interpreted by an adapter elsewhere.

## Failure model

Every rejection is total: there is no partial acceptance, no repair, no
`Warning`-and-continue path at this layer. The four failure surfaces, in the
order a caller sees them:

1. **Decode fails** → `PlanError::Parse`. Causes: syntax, unknown field
   (`deny_unknown_fields`), unknown enum variant, a type-level bound violation
   from `DurationMs`/`RateMilliRps`/`PositiveCount`/`BasisPoints`/`Name`, or an
   explicit `null` on one of the four versioned optionals.
2. **Unknown version** → `PlanError::UnsupportedVersion(n)`. Only for values
   outside 1..=10.
3. **Semantic validation fails** → `PlanError::Validation { category, detail }`.
   `detail` is human-facing and may change; `category` is asserted by tests and
   must not.
4. **Deferred to the adapter.** Bounds that need runtime facts are *not* checked
   here and say so: fixture existence and symlink escape (plan.rs:1570-1573),
   diagnostic probe availability against startup-established bindings
   (plan.rs:1681-1683), security-check target confinement (plan.rs:1771-1774),
   `http_url` listener reality, corpus digest content. The plan validates the
   *request*; the driver preflight validates the *world*.

`from_toml` is a peer of `from_json`, not a lower-fidelity path: both validate
identically, which `toml_round_trip_preserves_semantics` (plan.rs:2525) and the
per-version round-trip tests assert.

One structural caveat: `ExperimentPlan` derives a public `Deserialize`, so any
crate can call `serde_json::from_str::<ExperimentPlan>` and obtain an
unvalidated plan. The CLI does not —
[plan_input.rs](../crates/eggbench-cli/src/plan_input.rs#L80) routes through
`from_json`/`from_toml` — but "a plan is always validated" is a convention
enforced at entry points, not a property of the type. The module's own tests
rely on the raw path (plan.rs:2359, plan.rs:2553).

## Boundaries

**Owns:** the wire shape of a plan, the version compatibility decision, all
cross-field semantics, the numeric/identifier bounds that make a plan storable
as evidence, and the network-path contract (shared with evidence and
comparison).

**Does not own:**

- Capability resolution. `plan.rs` never names a driver catalog; `route.driver`
  and `source` labels are opaque `Name`s validated only against a pinned literal
  (`eggprobe`, plan.rs:1718; `eggsec-waf`, plan.rs:1815;
  `eggbench-http-corpus`, plan.rs:1932). Whether `eggfetch-http` supports a
  workload shape is [core-driver-resolution.md](core-driver-resolution.md)'s
  question.
- Filesystem existence, digests, and symlink confinement (driver preflight).
- Metric arithmetic. `Gate` is a *request*; no statistical calculation happens in
  this module (plan.rs:557).
- Evidence layout. `ArtifactBounds` is a ceiling handed forward
  (plan.rs:77-78), not an enforcement point.
- Secret resolution. Names go out; values come back through the runner's
  `SecretProvider` ([runner/src/secret.rs](../crates/eggbench-runner/src/secret.rs)).

The paired boundary is worth naming because it is a contract handoff, not a
copy: `PairedArm` (plan.rs:597) carries its own `Subject` for provenance only —
"the runner never launches or digests them" (plan.rs:584) — and the runner
retargets trials via `Workload::with_target` (plan.rs:1367).

## Tests and qualification

30 unit tests in `plan.rs` (from plan.rs:2380) and 1 in `types.rs`
(types.rs:246); 23 integration tests in
[tests/network_path.rs](../crates/eggbench-core/tests/network_path.rs) exercise
the shared route contract through resolution and bundle finalization.

Fixture-based tests pin the error categories end to end:
`invalid_fixtures_fail_with_stable_categories` (plan.rs:2538) maps
`invalid-cycle.json` → `cycle`, `invalid-unknown-service.json` →
`missing_reference`, `invalid-contradictory-workload.json` →
`contradictory_configuration`, and checks `invalid-zero-trials.json` is
rejected (via `PositiveCount`, i.e. at decode, not validation).

Per-version tests follow a consistent shape: decode a hand-built
`serde_json::Value`, mutate one field, assert the exact category.

| Test | Line | Pins |
|---|---|---|
| `v5_empty_diagnostics_is_valid_but_explicit_field_rejected_before_v5` | plan.rs:2937 | Presence-significant decode, v1-v4 rejection, empty-is-valid on v5 |
| `v6_security_checks_round_trip_and_v1_v5_compat` | plan.rs:3069 | Same for v1-v5 |
| `security_check_validation_matrix_fails_closed` | plan.rs:3098 | All six rejection paths plus 16-accepted/17-rejected |
| `v1_v3_reject_semantic_replay_and_v4_stays_compatible` | plan.rs:2839 | v1-v3 readable, replay rejected |
| `semantic_fixture_paths_fail_closed` | plan.rs:2854 | Seven traversal/absolute/backslash shapes |
| `semantic_findings_rejects_relative_and_statistical_gates` | plan.rs:2888 | `unsupported_gate` |
| `http_corpus_workload_requires_schema_seed_and_zero_mismatch_gate` | plan.rs:2381 | Version, seed, and the exact required metric tuple |
| `http_corpus_intent_is_v8_only_and_confined` | plan.rs:2477 | `http_corpus_checks` version gate and `../` corpus ref |
| `static_binding_map_merges_legacy_url_and_rejects_conflict` | plan.rs:2432 | v9 `static_bindings` / legacy `http_url` agreement |
| `security_checks_reject_paired_and_network_path_composition` | plan.rs:3184 | Both composition categories |
| `json_round_trip_and_version` | plan.rs:2513 | Round-trip equality and `UnsupportedVersion(99)` |

`bounded_fields_reject_zero` (plan.rs:2609) covers the three zero-bound types;
`secret_reference_debug_and_display_redact_values` (types.rs:246) asserts the
`SecretRef` redaction contract directly.

**Coverage gaps a reviewer should know about:**

- No `to_toml` round-trip test for v7, v8, v9, or v10. In particular
  `Workload::HttpCorpus::default_headers: Vec<(String, String)>` (plan.rs:437)
  is the only tuple-vec field in the plan and its TOML shape is untested.
- No test asserts that `paired` is accepted on v4-v10, only that it is rejected
  on v1 (plan.rs:2684). The code permits it.
- `TrialPolicy.timeouts: BTreeMap<Name, DurationMs>` (plan.rs:484) is decoded
  and never validated — no key allowlist, no count bound, no phase-name check.
  Every other map in the plan is bounded.
- `TelemetryRequest.fields` (plan.rs:511) has no length or duplicate check; only
  the outer `telemetry` vector is bounded (plan.rs:1236).

Qualification assets that exercise the plan contract end to end live outside
this crate: [../examples/](../examples/) (including the intentionally rejected
`eggstack-path-paired-unsupported.json`), [../qualification/](../qualification/),
and `scripts/qualification/`.

## Review focus

Ordered by how much damage a mistake does.

1. **Allowlist edits.** Every version-gating `if` in `validate()` is a hand-
   written chain (plan.rs:860-995, plan.rs:1029-1088). Adding a version means
   editing five to seven of them, and the error *messages* are hand-maintained
   strings that can silently fall behind. Check that each new version is added
   consistently, and that message text matches the accepted set.
2. **`skip_serializing_if` / `default` / `deserialize_with` triples.** Changing
   one of the three attributes on a versioned optional breaks presence
   detection, which is the only thing preventing a v4 plan from carrying v5
   semantics.
3. **New validation placed after a composition gate.** The specific-category
   ordering at plan.rs:945-985 is load-bearing for callers and for tests.
4. **New `String` fields.** Every one is a chance to add an unbounded value to a
   contract that deliberately has no unbounded strings. Check the bound, the
   control-character rule, and `looks_secret` where the value is opaque.
5. **Anything that weakens fail-closed.** `telemetry`, `metrics`, `timeouts`,
   and `services` are `#[serde(default)]` vectors/maps with no per-item
   structural bound on two of them.
6. **`.expect()` in `canonical_proxy_chain_text`** (plan.rs:2189-2203). Sound
   only because every caller validates first; a new call site without
   validation panics instead of returning `PlanError`.
7. **Error category strings.** Tests assert on them; they are contract.

## Related

- [overview.md](overview.md) — module map and the position of this file in the
  suggested review order
- [core.md](core.md) — why the crate boundary exists
- [core-driver-resolution.md](core-driver-resolution.md) — what happens to a
  validated plan next
- [core-metrics.md](core-metrics.md) — what `MetricRequest` becomes
- [core-evidence-bundle.md](core-evidence-bundle.md) — where `bounds` is
  enforced and where `plan.json` is staged
- [core-qualification-security.md](core-qualification-security.md) — the
  correctness contract `security_checks` and `http_corpus_checks` feed
- [../docs/experiment-plan.md](../docs/experiment-plan.md) — the user-facing
  schema narrative
- [../docs/paired-experiments.md](../docs/paired-experiments.md) — the paired
  design and its schedule
- [../docs/eggstack-http.md](../docs/eggstack-http.md) — the `http_url` binding
  contract
- [../docs/security-qualification.md](../docs/security-qualification.md) — WAF
  and HTTP-corpus correctness
