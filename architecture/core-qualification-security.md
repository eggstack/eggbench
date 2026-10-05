# Core: qualification, security, and network-path contracts

## Purpose

`qualification.rs`, `security.rs`, and `network_path.rs` are the three smaller
contracts in `eggbench-core`. Each is a *declarative contract* — a vocabulary,
a set of bounds, and a validation relation — with no orchestration, no
execution, and no I/O beyond the file reads in `qualification.rs`. They are
grouped here because they share a shape rather than a purpose: each is the
point where a *security-relevant or path-relevant* intent crosses from a
human-authored file into a typed, bounded, machine-checkable form.

| File | LOC | Question it answers |
|---|---|---|
| [qualification.rs](../crates/eggbench-core/src/qualification.rs) | 1,277 | What does a qualification profile, a fixed HTTP corpus, and a qualification receipt have to contain, and what must never be in them? |
| [security.rs](../crates/eggbench-core/src/security.rs) | 412 | What does a security-correctness *result* look like, and who owns which half of it? |
| [network_path.rs](../crates/eggbench-core/src/network_path.rs) | 281 | What route and stream-fault intent may a plan declare, credential-free and bounded? |

The system-level placement is in [overview.md](overview.md) (module map and
cross-cutting contract table). This document is the component deep dive for
those three rows.

The three contracts differ in *when* they are populated, which is the single
most useful thing to keep in mind while reading:

- `qualification.rs` is populated **before** a run, from a profile, and
  finalized **after** it, into a receipt. It is the only one of the three that
  does filesystem work (`content_tree_identity`).
- `security.rs` is populated **by a driver during a run** and is purely
  declarative: it defines the shape of an already-observed fact and the relation
  that fact must satisfy.
- `network_path.rs` is populated **when a plan is authored** and is consumed
  only as data. It creates no runtime object and performs no I/O.

## Source layout

```text
crates/eggbench-core/src/
  lib.rs            pub use network_path::* / qualification::* / security::*  (17,19,21)
  qualification.rs  profile + corpus inputs, content identity, expansion, receipt
  security.rs       security-correctness evidence contract (Eggstack M004a)
  network_path.rs   declarative route + stream-fault intent (plan schema v3)
```

Supporting types these three borrow from elsewhere in core, rather than
declare:

- `Name`, `SchemaVersion` — [types.rs](../crates/eggbench-core/src/types.rs)
  bounded name and version newtypes.
- `DurationMs`, `PositiveCount` — bounded 1 ms..=1 year
  ([types.rs:107](../crates/eggbench-core/src/types.rs)) and positive
  ≤ 1,000,000 ([types.rs:211](../crates/eggbench-core/src/types.rs)) wrappers
  used by `StreamFaultKind`.
- `AggregateVerdict` — [comparison.rs:214](../crates/eggbench-core/src/comparison.rs),
  the four-valued verdict the qualification aggregation folds.
- `BundleIdentity` — the baseline identity the expansion and receipt reference.
- `ExperimentPlan` / `PlanError` — the plan contract these inputs must not
  duplicate.

## Key types

### `qualification.rs` — bounded, subject-neutral inputs

"Bounded" is literal, and the bounds are declared, not implied
([qualification.rs:17](../crates/eggbench-core/src/qualification.rs)):

| Bound | Value | Line |
|---|---|---|
| `CONTENT_TREE_MAX_FILES` | 1,024 | 17 |
| `CONTENT_TREE_MAX_FILE_BYTES` | 8 MiB | 18 |
| `CONTENT_TREE_MAX_TOTAL_BYTES` | 64 MiB | 19 |
| `MAX_PROFILE_BYTES` (private) | 1 MiB | 20 |
| `MAX_CORPUS_BYTES` (private) | 8 MiB | 21 |
| `MAX_PLAN_BYTES` (private) | 4 MiB | 22 |
| scenarios per profile | 1..=32 | 544 |
| cases per corpus | 1..=1,024 | 604 |
| cases per corpus *result* | 1..=1,024 | 168 |
| header count / bytes per case | 64 / 16 KiB | 636, 665 |
| `path_and_query` bytes | ≤ 8,192 | 625 |
| `status_any_of` entries | 1..=32 | 687 |
| directory depth during hashing | 16 | 756 |
| relative path length | ≤ 512 | 1067, 778 |

"Subject-neutral" is equally concrete. `SecurityQualificationProfileV1`
([qualification.rs:26](../crates/eggbench-core/src/qualification.rs)) has six
fields — `schema_version`, `id`, `owner`, `scenarios`, `corpus`,
`target_config` — and *no* subject, service, or target identity. A scenario
([qualification.rs:37](../crates/eggbench-core/src/qualification.rs)) carries
only an id, a plan path, an optional baseline bundle path, and (v2) a workload
driver name. The subject under test is named by the referenced *plan*, not by
the profile, so one profile can qualify several subjects and the profile digest
stays independent of which binary is being measured. The real asset
([qualification/synvoid/v2/smoke.profile.json](../qualification/synvoid/v2/smoke.profile.json))
confirms this: seven scenarios, four of which pin `"workload_driver":
"eggfetch-http"`, and no subject anywhere in the profile.

The profile vocabulary:

| Type | Representation | Notes |
|---|---|---|
| `SecurityQualificationProfileV1` | struct, `deny_unknown_fields` | accepts `schema_version` 1 or 2 (537) |
| `QualificationScenarioRef` | struct, `deny_unknown_fields` | `workload_driver` is v2-only (554) |
| `ContentInputRef` | struct, single `path: String` | deliberately minimal (50) |
| `QualificationScenarioStatus` | enum `snake_case` | `Completed`, `Invalid`, `Cancelled`, `NotRun` (363) |
| `HttpSecurityCorpusV1` / `HttpSecurityCaseV1` | structs, `deny_unknown_fields` | corpus is a fixed, owner-authored request set (56, 65) |
| `HttpCaseRequestV1` | struct | `method`, `path_and_query`, `headers: Vec<(String,String)>`, `body` (74) |
| `HttpCaseBodyV1` | internally-tagged enum `kind`/`value` | `None`, `InlineUtf8(String)`, `File(String)` (85) |
| `HttpObservableExpectationV1` | **untagged** enum | `Exact{status_exact}` or `AnyOf{status_any_of}` (94) |
| `HttpCorpusCaseDisposition` | enum `snake_case` | `Pass`, `Fail`, `Invalid` (139) |
| `QualificationScenarioRecordV1` | struct | per-scenario performance / correctness / combined verdicts (333) |

Content identity is a value, not a path. `ContentFileIdentity`
([qualification.rs:269](../crates/eggbench-core/src/qualification.rs)) is
`{path, length, sha256}`; `ContentTreeIdentity`
([qualification.rs:276](../crates/eggbench-core/src/qualification.rs)) adds
`aggregate_sha256`, `file_count`, `total_bytes`, and the sorted file list. The
aggregate digest is a plain SHA-256 over `path \0 length_be \0 sha256 \0` per
file in sorted path order
([qualification.rs:982](../crates/eggbench-core/src/qualification.rs)), so it
is order-independent and content-sensitive — the two properties the unit test
at line 1122 pins.

### `security.rs` — the security-correctness evidence contract

The central design decision is the **ownership split**, stated in the module
doc and enforced by the type shapes
([security.rs:1](../crates/eggbench-core/src/security.rs)):

| Eggsec owns | Eggbench owns |
|---|---|
| payload generation | declarative check selection |
| WAF detection | target binding and strict-scope preflight |
| bypass-technique execution | lifecycle placement |
| the meaning of `bypass_successful` | the predeclared threshold |
| | the sanitized typed result |

The split holds in the types. `SanitizedSecurityCase`
([security.rs:70](../crates/eggbench-core/src/security.rs)) has five fields:
`technique`, `severity_label`, `response_status`, `bypass_successful`, and
`payload_sha256`. There is no field in which Eggbench could express its own
opinion about whether a bypass succeeded — `bypass_successful` is copied
verbatim from Eggsec's finding, and the payload appears only as its digest.
Conversely, `SecurityCheckResultV1`
([security.rs:86](../crates/eggbench-core/src/security.rs)) carries the fields
Eggbench does own: the predeclared `allowed_successful_bypasses`, the
`scope_sha256` of the generated manifest, the `producer_sha256` of the
executable, and the typed `disposition`.

`SecurityCheckResultV1` declares `test_type: String`
([security.rs:96](../crates/eggbench-core/src/security.rs)) while the plan
side uses the typed `EggsecWafTestType`
([plan.rs:148](../crates/eggbench-core/src/plan.rs)). The evidence contract
re-validates the string only as 1..=32 chars of `[a-z_]`
([security.rs:145](../crates/eggbench-core/src/security.rs)) — it does *not*
constrain the value to one of the five declared families. A result claiming
`test_type = "ssrf"` for a check the plan declared as `sqli` is rejected, but one
claiming `test_type = "nonsense"` is not, as long as the shape holds. This is
deliberate forward compatibility for a family the tool may add before core
does, but it is a real asymmetry with the plan side.

Evidence vocabulary:

| Type | Representation | Line |
|---|---|---|
| `SecurityDisposition` | enum `snake_case`: `Pass`, `Fail`, `Invalid` | 43 |
| `SecurityCheckResultV1` | struct, `deny_unknown_fields`, `SchemaVersion` | 86 |
| `SecurityCheckIndexRecord` | struct — one row of the run index | 212 |
| `SecurityChecksIndex` | struct — run-level `security-checks.json` | 238 |
| `SecurityConfigIdentity` | struct — comparison-critical config only | 301 |
| `SecurityCheckConfigRow` | struct — one per-check config row | 319 |

Bounds ([security.rs:26](../crates/eggbench-core/src/security.rs)):
`MAX_SECURITY_CHECKS = 16`, `MAX_SECURITY_CASES = 1_024`,
concurrency 1..=32, timeout 1,000..=120,000 ms,
`CORRECTNESS_ADAPTER_SEMANTIC_VERSION = "1"`.

### `network_path.rs` — declarative path intent

Three types and two constants, all of them data
([network_path.rs:20](../crates/eggbench-core/src/network_path.rs)):

| Type | Shape |
|---|---|
| `NetworkPathRequest` | required `route: RouteRequest`, optional `stream_faults` (20) |
| `RouteRequest` | required `driver: Name`, required `mode: RouteMode` (31) |
| `RouteMode` | internally tagged: `Direct` or `ProxyChain{chain}` (41) |
| `StreamFaultPlanRequest` | `driver: Name` plus ordered `upstream` / `downstream` (101) |
| `StreamFaultRequest` | `id: Name`, `kind: StreamFaultKind` (115) |
| `StreamFaultKind` | seven variants (129) |

`StreamFaultKind` variants and their fields:

| Variant | Fields | Lines |
|---|---|---|
| `Latency` | `delay_ms`, `jitter_ms`, `max_buffer_bytes` | 131 |
| `Bandwidth` | `bytes_per_second`, `burst_bytes` | 140 |
| `Blackhole` | `close_after_ms: Option<DurationMs>` | 147 |
| `LimitData` | `bytes` | 152 |
| `SlowClose` | `delay_ms` | 157 |
| `Slice` | `average_size`, `variation: u64`, `delay_ms` | 162 |
| `Disconnect` | `after_ms` | 171 |

The retained identities
([network_path.rs:13](../crates/eggbench-core/src/network_path.rs)):

- `NETWORK_PATH_SEMANTICS_VERSION = "route-first-fault-second-v1"` — the
  ordering contract, carried into `ResolvedNetworkPath.semantics_version`
  ([resolved.rs:727](../crates/eggbench-core/src/resolved.rs)) and re-checked
  at bundle verification
  ([evidence.rs:1721](../crates/eggbench-core/src/evidence.rs)) and at dial
  time ([dialer.rs:104](../crates/eggbench-drivers/src/eggstack/path/dialer.rs)).
- `NETWORK_PATH_RNG_VERSION = "splitmix64-v1"` — the Eggchaos RNG identity,
  present only when faults are
  ([resolved.rs:721](../crates/eggbench-core/src/resolved.rs)).

**There is no seed field in `network_path.rs`.** The seed lives on the plan
(`ExperimentPlan.seed`) and the requirement is enforced in the plan validator:
a non-empty fault plan with no seed is rejected as `missing_fault_seed`
([plan.rs:1005](../crates/eggbench-core/src/plan.rs)). The seed is copied into
the resolved plan unconditionally
([resolved.rs:752](../crates/eggbench-core/src/resolved.rs)); the
fault-conditional *namespacing* of the seed into `seed_namespace` happens in
per-run path evidence
([path/evidence.rs:131](../crates/eggbench-drivers/src/eggstack/path/evidence.rs)).
This is the drift already noted in
[overview.md](overview.md#known-documentation-drift).

## How it works

### Qualification: profile → expansion → receipt

`expand_qualification_profile`
([qualification.rs:808](../crates/eggbench-core/src/qualification.rs)) is the
one function that reads the filesystem; its order matters:

1. Confine and read the profile, ≤ 1 MiB; parse; `validate()`
   ([qualification.rs:815](../crates/eggbench-core/src/qualification.rs)).
2. Content-identify `corpus.path` and `target_config.path`
   (818, 862).
3. If the corpus is a single file, parse it as `HttpSecurityCorpusV1` and
   validate it (821–823), then fold every `File` body reference into the corpus
   identity under a `body/<case-id>/` prefix and re-check the aggregate bounds
   (827–849). A body file's *content* therefore participates in the corpus
   digest; changing a body changes the identity.
4. For each scenario, confine and read the plan, dispatch on extension
   (`toml` → `ExperimentPlan::from_toml`, else `from_json`)
   ([qualification.rs:870](../crates/eggbench-core/src/qualification.rs)),
   call `parsed.validate()`, and digest the exact bytes read.
5. If a `baseline_bundle` was declared, load it and freeze its
   `BundleIdentity` **now**, before any candidate runs
   ([qualification.rs:881](../crates/eggbench-core/src/qualification.rs)).

The result is a `QualificationExpansionV1`
([qualification.rs:284](../crates/eggbench-core/src/qualification.rs)) whose
`expansion_policy` is `…v1` or `…v2` by profile schema (896–901) — an immutable
description of *what will be run*. The same content-identity algorithm re-runs
at run time in `load_http_security_corpus`
([qualification.rs:916](../crates/eggbench-core/src/qualification.rs)), which
compares the recomputed aggregate digest against the declared one and fails
otherwise (972–978). The test at line 1217 pins the round trip: same identity,
then a body edit makes the loader fail.

Path confinement is the security-relevant part. `canonical_confined_path`
([qualification.rs:1003](../crates/eggbench-core/src/qualification.rs)) walks
every `Component::Normal` and rejects any symlinked component, then re-checks
`canonical.starts_with(root)`. `validate_relative`
([qualification.rs:1063](../crates/eggbench-core/src/qualification.rs)) rejects
empty, absolute, over-512-byte, or non-`Normal`-only paths first. Symlinks are
rejected again during traversal
([qualification.rs:719](../crates/eggbench-core/src/qualification.rs),
[762](../crates/eggbench-core/src/qualification.rs)); special files are
rejected outright (785).

### Predeclared expectations vs observed results

This is the distinction the corpus contract exists to make.

A case carries an `expectation`, validated at input time
([qualification.rs:677](../crates/eggbench-core/src/qualification.rs)):
`status_exact` in 100..=599, and `status_any_of` non-empty, ≤ 32 entries, all
100..=599. Nothing about it may change between declaration and execution — the
corpus is content-addressed and the digest is pinned in both the expansion and
the plan's `corpus_sha256`.

The result carries the *observation*
([qualification.rs:126](../crates/eggbench-core/src/qualification.rs)):
`observed_status: Option<u16>`, a `disposition`, an optional `reason`.
`validate_contract` ([qualification.rs:167](../crates/eggbench-core/src/qualification.rs))
recomputes the disposition and rejects disagreement; `case_disposition_mismatch`
at 236 is the load-bearing check. Relations enforced:

- `Invalid` ⟹ no observed status (218); `Invalid` ⟺ a `reason` is present (224).
- Non-`Invalid` ⟹ an observed status is present (226 → 229).
- The `reason` vocabulary is exactly four strings
  ([qualification.rs:205](../crates/eggbench-core/src/qualification.rs)):
  `transport_failure`, `invalid_input`, `unsupported_protocol`, `check_timeout`.
- `Pass` ⟺ the observed status matches the expectation (235–240).

A transport failure is therefore `Invalid`, never a silent `Fail`, and a
correctness failure cannot be relabelled. `counts()`
([qualification.rs:248](../crates/eggbench-core/src/qualification.rs)) returns
`(evaluated, passed, failed, invalid)`.

**Why one-shot rather than statistical.** These are *categorical correctness*
verdicts, not measurements: no sample, distribution, interval, or bootstrap.
Each case runs once against a fixed request and the only observable is an HTTP
status. Neither corpus nor security evidence writes `TrialMetrics`
([security.rs:8](../crates/eggbench-core/src/security.rs)), and
`aggregate_qualification_verdicts`
([qualification.rs:381](../crates/eggbench-core/src/qualification.rs)) folds
`AggregateVerdict` values by the fixed precedence
`Invalid > Fail > Inconclusive > Pass` — a lattice join, not an inference.
`aggregate_qualification_scenarios`
([qualification.rs:404](../crates/eggbench-core/src/qualification.rs)) adds one
rule: any scenario not `Completed`, or lacking a `combined_verdict`, makes the
aggregate `Invalid`. A partially executed profile can never report `Pass`.

The SynVoid asset exercises exactly this
([qualification/synvoid/v2/corpus.json](../qualification/synvoid/v2/corpus.json)):
`status_exact: 200` for benign requests, `status_any_of: [403]` for traversal
and XSS probes, and `category` labels like `path_traversal/detect` treated as
opaque owner metadata (bounded only by `validate_text`, 616).

### Corpus validation: what a fixed corpus may not contain

`HttpSecurityCorpusV1::validate`
([qualification.rs:596](../crates/eggbench-core/src/qualification.rs)) is
mostly a safety list. A case is rejected when:

- `method` is empty, > 32 bytes, or not an RFC token (619).
- `path_and_query` is not a bounded relative origin-form path: it starts with
  `/`, not `//`, and excludes any byte ≤ 0x20, 0x7f, `#`, or `\` (624–635) —
  absolute targets, fragments, and backslash traversal are excluded by
  construction.
- a header name is not a token, or a value contains CR, LF, or NUL (642).
- a header is one of nine credential-bearing or transport-control names —
  `authorization`, `proxy-authorization`, `cookie`, `set-cookie`, `host`,
  `connection`, `proxy-connection`, `transfer-encoding`, `content-length`
  (648–663, rejected at 660). A corpus able to set its own `Host` or
  `Content-Length` would let the owner, not the target, fix the framing.
- an inline body exceeds the per-file content bound (669).

### `security.rs`: recomputation, not interpretation

`SecurityCheckResultV1::recomputed_disposition`
([security.rs:119](../crates/eggbench-core/src/security.rs)) is a total
function of three counts:

- `evaluated_cases == 0` or `successful_bypasses > evaluated_cases` →
  `Invalid`;
- `successful_bypasses <= allowed` → `Pass`;
- otherwise → `Fail`.

`validate_contract` requires the stored `disposition` to equal that result
([security.rs:166](../crates/eggbench-core/src/security.rs)), so a tampered
disposition is rejected. It also requires
`sanitized_cases.len() == evaluated_cases`
([security.rs:163](../crates/eggbench-core/src/security.rs)), a 64-hex producer
and scope digest, a bounded producer version, and per-case bounds on `technique`
(≤ 128) and `severity_label` (≤ 32). It never re-derives `successful_bypasses`
from the case list — that is the driver's job
([eggsec.rs:914](../crates/eggbench-drivers/src/external/eggsec.rs)), and the
driver is also where the payload-digest boundary is enforced
([eggsec.rs:963](../crates/eggbench-drivers/src/external/eggsec.rs)).

`SecurityChecksIndex::validate_contract`
([security.rs:264](../crates/eggbench-core/src/security.rs)) is deliberately
weaker: it fixes `driver == "eggsec-waf"` and bounds the count and string
lengths, but does not re-check each record. The index is a routing table; the
per-check artifacts are the authoritative records.

`SecurityConfigIdentity`
([security.rs:301](../crates/eggbench-core/src/security.rs)) is the
comparison-critical slice: ordered check ids, per-check config rows, producer
version and digest, scope digest, adapter semantic version. Its doc comment
states the rule that matters — "Result values (pass/fail/counts) are NOT
configuration identity" — so a candidate failing the same way as its baseline
stays comparable, and one whose *configuration* differs does not.

### `HttpCorpusCheckRequest`: two kinds of correctness

`HttpCorpusCheckRequest` is declared in `plan.rs`, not `security.rs`
([plan.rs:160](../crates/eggbench-core/src/plan.rs)): `id`, `source`, `target`,
`corpus_ref`, `corpus_sha256`, `timeout_ms`, `case_timeout_ms`. Both are
"correctness checks" and only one involves an oracle tool:

| | WAF bypass (`SecurityCheckRequest`) | HTTP corpus (`HttpCorpusCheckRequest`) |
|---|---|---|
| Declared in | `plan.rs:140` | `plan.rs:160` |
| Plan schema | v6 | v8 |
| Oracle | external `eggsec` binary, preflighted | in-process Eggfetch, no external tool |
| What is observed | `bypass_successful` per case | HTTP response status per case |
| Who decides correctness | Eggsec, then Eggbench applies a predeclared allowance | the owner, via a predeclared exact/allowed-set status expectation |
| Threshold | `max_successful_bypasses` (u32) | none; the expectation *is* the threshold |
| Result type | `SecurityCheckResultV1` | `HttpCorpusCheckResultV1` |
| Comparison family | `waf_bypass` | `http_observable` |
| Policy identity | `eggbench.security-correctness.v1` | `…v2` ([comparison.rs:63](../crates/eggbench-core/src/comparison.rs)) |
| Incompositions | rejects `network_path` ([plan.rs:975](../crates/eggbench-core/src/plan.rs)) | rejects `paired` and `network_path` ([plan.rs:1892](../crates/eggbench-core/src/plan.rs), 1897) |

Both run after readiness, outside every measured interval, and neither touches
`TrialMetrics`. The policy identity is chosen by family presence at comparison
time ([comparison.rs:2283](../crates/eggbench-core/src/comparison.rs)), so
adding an `http_observable` record to an otherwise WAF-only receipt switches the
whole receipt to v2.

### Supported vs unsupported operation and test-type sets

Core declares only the supported side; the explicit unsupported list lives in
the adapter, because it describes the tool's surface rather than Eggbench's.

| Set | Value | Location |
|---|---|---|
| Accepted preflight operations | `waf`, `waf-detect` | [eggsec.rs:67](../crates/eggbench-drivers/src/external/eggsec.rs) |
| Supported WAF families | `sqli`, `xss`, `ssrf`, `cmd`, `traversal` (no `all`) | [eggsec.rs:69](../crates/eggbench-drivers/src/external/eggsec.rs) |
| Audited operation label | `waf --json --bypass` | [eggsec.rs:62](../crates/eggbench-drivers/src/external/eggsec.rs) |
| Explicitly unsupported (14) | `scan`, `ci`, `stress`, `packet`, `nse`, `db-pentest`, `web-proxy`, `c2`, `postex`, `daemon`, `rest`, `mcp`, `agent`, `evasion` | [eggsec.rs:71](../crates/eggbench-drivers/src/external/eggsec.rs) |
| Typed plan-side families | `Sqli`, `Xss`, `Ssrf`, `Cmd`, `Traversal` | [plan.rs:183](../crates/eggbench-core/src/plan.rs) |
| Corpus source | `eggbench-http-corpus`, family `http_observable` | [security.rs:24](../crates/eggbench-core/src/security.rs), [comparison.rs:67](../crates/eggbench-core/src/comparison.rs) |

The plan-side enum converts both ways through a CLI token
([plan.rs:199](../crates/eggbench-core/src/plan.rs)), which is how a typed
family reaches `eggsec --test-type` without a stringly-typed plan. `all` is
deliberately unexposed ([plan.rs:179](../crates/eggbench-core/src/plan.rs)):
one typed family per check keeps each check's expectation and evidence
attributable and bounded.

### `network_path.rs`: intent only, validated elsewhere

This module performs no I/O, holds no handle, and constructs no runtime object.
`NetworkPathRequest` is a plan field
([plan.rs:49](../crates/eggbench-core/src/plan.rs)) that deserializes, then is
validated by `validate_network_path_contract`
([plan.rs:2110](../crates/eggbench-core/src/plan.rs)) and *only there*:

- The chain must be 1..=1,024 bytes (2115), contain no control characters
  (2124), and contain none of `@`, `%`, `?`, `#` (2130) — rejected under
  category `route_credentials_not_supported` (2132), which is why userinfo,
  percent-encoded credentials, query, and fragment data are all impossible
  rather than merely discouraged.
- `validate_proxy_chain_shape` ([plan.rs:2156](../crates/eggbench-core/src/plan.rs))
  splits on `__` and requires each hop to be `scheme://host:port` with scheme in
  `http | socks4 | socks4a | socks5` (2164), then
  `validate_route_endpoint` ([plan.rs:2215](../crates/eggbench-core/src/plan.rs))
  requires a bracketed IPv6 host or a non-empty DNS/IPv4 host and a port in
  1..=65,535.
- `validate_stream_fault_plan` ([plan.rs:2313](../crates/eggbench-core/src/plan.rs))
  bounds each direction to 128 faults, rejects duplicate ids within a direction,
  and enforces `slice.variation < slice.average_size` (2340).

Schema gating happens in the plan validator: `network_path` requires schema
version 3 exactly ([plan.rs:987](../crates/eggbench-core/src/plan.rs)), and the
key's presence is significant even when empty. The credential-free property is
then defended three more times downstream, which is why it is structural here:

1. **Serialization refuses.** `RouteMode`'s hand-written `Serialize`
   ([network_path.rs:51](../crates/eggbench-core/src/network_path.rs)) re-runs
   the chain check and errors with
   `credential-bearing or invalid proxy chain cannot be serialized` (79), so a
   credentialed mode cannot reach a plan, resolved plan, or evidence artifact
   even if built in memory. The test at 247 also asserts the error text does not
   echo the offending input.
2. **Debug redacts.** `Debug` prints `[REDACTED]` for any `ProxyChain`
   ([network_path.rs:85](../crates/eggbench-core/src/network_path.rs)), so
   diagnostics and assertion messages cannot leak a chain.
3. **Resolution pins the driver.** `RouteRequest.driver` is not free choice at
   run time: the resolver requires it to match the `Route`-category descriptor
   ([resolved.rs:701](../crates/eggbench-core/src/resolved.rs)) and pins
   `eggress-route` / `eggress-outbound` with a bounded adapter version
   ([resolved.rs:989](../crates/eggbench-core/src/resolved.rs)).

Deserialization is deliberately *not* the validating step: `RouteMode` derives
`Deserialize` and accepts what the validator later rejects. The asymmetry is
intentional — a credentialed chain must be representable in memory long enough
to be rejected with a specific `PlanError`, but never serializable or
printable. There is no `seed` here; see the note above.

## Invariants

**Qualification.**

- Every path a profile names is workspace-relative, `Normal`-component-only,
  ≤ 512 bytes, symlink-free at every component, and confirmed to
  `starts_with(root)` after canonicalization
  ([qualification.rs:1063](../crates/eggbench-core/src/qualification.rs),
  [1003](../crates/eggbench-core/src/qualification.rs)).
- Aggregate content bounds (1,024 files, 8 MiB/file, 64 MiB total) are
  re-checked after body files are folded in, not only on the corpus file itself
  ([qualification.rs:840](../crates/eggbench-core/src/qualification.rs),
  [964](../crates/eggbench-core/src/qualification.rs)).
- A corpus's identity covers its body files' contents, so an expectation cannot
  be kept while its request bytes change.
- Scenario ids and plan paths are each unique within a profile (564, 569; reason
strings at 566, 571).
- The profile aggregate is `Invalid` unless every scenario is `Completed` with
  a `combined_verdict` ([qualification.rs:407](../crates/eggbench-core/src/qualification.rs)).
- No credential-bearing header, absolute target, fragment, or symlinked content
  can appear in a corpus.
- The profile itself never names a subject.

**Security.**

- `disposition` is a function of `(evaluated_cases, successful_bypasses, allowed)`;
  a stored value that disagrees is a contract violation
  ([security.rs:166](../crates/eggbench-core/src/security.rs)).
- `sanitized_cases.len() == evaluated_cases`, and `evaluated_cases` is 1..=1,024
  for a real pass/fail.
- No payload bytes, titles, or descriptions cross the boundary — only
  `payload_sha256` ([security.rs:80](../crates/eggbench-core/src/security.rs)).
- `bypass_successful` is Eggsec's, never recomputed or re-weighted by core.
- Security results never enter `TrialMetrics`
  ([security.rs:8](../crates/eggbench-core/src/security.rs)).
- `SecurityConfigIdentity` carries configuration only, never outcome values.
- In comparison, a performance pass cannot override a correctness failure and an
  invalid correctness contract cannot be hidden by a pass
  ([comparison.rs:1890](../crates/eggbench-core/src/comparison.rs)).

**Network path.**

- A route chain is credential-free, control-character-free, ≤ 1,024 bytes, and
  consists only of `http`/`socks4`/`socks4a`/`socks5` hops with explicit ports.
- A `RouteMode` that would not pass `validate_proxy_chain_shape` is neither
  serializable nor `Debug`-printable.
- Fault counts are ≤ 128 per direction, ids unique per direction, and
  `variation < average_size`.
- `network_path` is schema-v3 only and incompatible with `SemanticReplay`
  ([plan.rs:945](../crates/eggbench-core/src/plan.rs)), `diagnostics`
  ([plan.rs:957](../crates/eggbench-core/src/plan.rs)), `security_checks`
  ([plan.rs:975](../crates/eggbench-core/src/plan.rs)), and an `External`
  subject ([plan.rs:1001](../crates/eggbench-core/src/plan.rs)).
- Non-empty faults require an explicit plan seed
  ([plan.rs:1005](../crates/eggbench-core/src/plan.rs)).

## Failure model

| Failure | Representation | Site |
|---|---|---|
| Malformed JSON, unknown field, bad identifier | `QualificationInputError::Invalid` | 508 |
| Path escape, symlink component, non-regular file | `UnsafePath` | 510 |
| Any byte/length/count bound exceeded | `Bound` (stable short label) | 512 |
| Filesystem read failure | `Io` | 514 |
| Referenced plan fails plan validation | `Plan(PlanError)` | 516 |
| Serialization failure | `Json` | 518 |
| Corpus result violates the v1 contract | `&'static str` reason code, 11 distinct values | 167 |
| `Invalid` case with a status, or a status-bearing case without one | `invalid_case_has_status` / `observed_case_missing_status` | 221, 229 |
| `Invalid` disposition without a `reason`, or vice versa | `invalid_case_reason_mismatch` | 224 |
| Stored disposition disagrees with the recomputation | `case_disposition_mismatch` | 236 |
| Security result schema / source / digest / length violation | `&'static str` reason | 138 |
| Security disposition disagrees with recomputation | `security result disposition disagrees with recomputation` | 167 |
| Security index driver or bound violation | `&'static str` reason | 264 |
| Credentialed or malformed route chain in a plan | `PlanError::Validation` `invalid_route` / `unsupported_route` / `route_credentials_not_supported` | 2110 |
| Credentialed or malformed `RouteMode` on the way out | `S::Error::custom`, message never echoes input | [network_path.rs:79](../crates/eggbench-core/src/network_path.rs) |
| Too many / duplicate / malformed stream faults | `invalid_fault_plan` / `invalid_bound` / `duplicate_identity` | [plan.rs:2313](../crates/eggbench-core/src/plan.rs) |
| Missing seed with faults present | `missing_fault_seed` | [plan.rs:1011](../crates/eggbench-core/src/plan.rs) |

The two error conventions are worth noting: the qualification and security
contracts return static reason codes because their results are persisted as
evidence and a stable string is part of the artifact contract; route/fault
errors are `PlanError::Validation` with a `category` + `detail` because a plan
is human-authored input. Neither module invents a bespoke error tree for a
case the plan layer already models.

## Boundaries

**What these modules own.** The vocabulary of qualification, security, and path
*declarations*; the bounds on those declarations; the relation an observed
result must satisfy; the content-identity computation over workspace files; and
the static credential rejection on route serialization.

**What they do not own.**

- *Subject identity* — named by referenced plans, never by a profile
  ([qualification.rs:26](../crates/eggbench-core/src/qualification.rs)).
- *Payload semantics* — Eggsec's ([security.rs:1](../crates/eggbench-core/src/security.rs)).
- *Path lowering and dialing* — `eggbench-drivers`; see
  [drivers-network-path.md](drivers-network-path.md) and the path-ownership
  sections of [drivers.md](drivers.md). The route is established first, the
  fault wrapper second, and a failed route is a failed dial with no fallback.
- *Driver selection for a path* — the resolver
  ([resolved.rs:694](../crates/eggbench-core/src/resolved.rs)); see
  [core-driver-resolution.md](core-driver-resolution.md).
- *Corpus execution* — `eggbench-drivers`
  ([http_corpus.rs](../crates/eggbench-drivers/src/eggstack/http_corpus.rs))
  behind the runner's `CorrectnessExecutor` seam, which carries the request in
  `http_corpus_request` ([correctness.rs:59](../crates/eggbench-runner/src/correctness.rs)).
- *Staging and publishing* — the runner
  ([orchestration.rs:2553](../crates/eggbench-runner/src/orchestration.rs))
  validates the result contract and writes `security-checks.json`
  ([orchestration.rs:2732](../crates/eggbench-runner/src/orchestration.rs));
  the receipt is built and published by `qualify run` in the CLI
  ([main.rs:433](../crates/eggbench-cli/src/main.rs)).
- *Comparison policy* — [core-comparison.md](core-comparison.md).

### Placement against the rest of core

| Contract | Purpose | Populated when | Primary consumer |
|---|---|---|---|
| **Qualification** (`qualification.rs`) | Freeze a set of security scenarios into immutable input identities, then certify the run as a whole | Profile authored; expansion before the first scenario; receipt published after the last | `eggbench qualify` CLI; referenced by the receipt, not by comparison |
| **Security correctness** (`security.rs`) | Define the shape of an already-observed oracle verdict and its recomputation relation | During a run, by the `eggsec-waf` adapter | Runner staging; `comparison.rs` correctness section |
| **Network path** (`network_path.rs`) | Declare a credential-free route and deterministic fault composition | Plan authoring; frozen into `ResolvedPlan` at resolution | Resolver; `eggstack-path` dialer; path evidence |
| Plan (`plan.rs`) | The whole experiment declaration | Authoring | Resolver, runner, evidence staging |
| Evidence (`evidence.rs`) | Publish immutable `.eggb` artifacts, manifest-last | End of run | `inspect`, comparison, anything reading a bundle |
| Metrics (`metrics.rs`) | Normalize raw observations into `TrialMetrics` | Per measured trial | Comparison |
| Comparison (`comparison.rs`) | Offline policy over two finalized bundles | On `compare` | Receipt, CLI |

The distinction a reviewer should hold onto: the three contracts in this
document all feed *evidence*, and none of them is evidence. They are the
contracts that decide what is legal to record and what the recording means.

## Tests and qualification

Unit tests live in-module, which is where the invariants are cheapest to pin:

| Test | Pins | Line |
|---|---|---|
| `qualification_aggregation_obeys_locked_precedence` | `Invalid > Fail > Inconclusive > Pass` | 428 |
| `end_to_end_matrix_a_to_g_has_conservative_outcomes` | all seven scenario-status/verdict combinations; F and G (`NotRun`, `Cancelled`) are `Invalid` | 436 |
| `profile_v1_shape_is_unchanged_and_v2_freezes_valid_driver_names` | v1 round-trips without `workload_driver`; v2 requires a driver-name-shaped string | 1101 |
| `content_identity_is_order_independent_and_content_sensitive` | sorted-path determinism; a content edit changes the aggregate | 1122 |
| `path_escape_and_symlinks_are_rejected` | `../secret` and a symlinked component both fail | 1142 |
| `corpus_rejects_credentials_and_invalid_targets` | `Authorization`, then `Host`, then an absolute target, then a backslash target — four rejections | 1156 |
| `corpus_result_recomputes_status_disposition_and_counts` | a valid result, `counts() == (1,1,0,0)`, and a flipped observed status yielding exactly `case_disposition_mismatch` | 1185 |
| `corpus_loader_binds_body_file_content_into_profile_identity` | expansion identity is accepted by the loader, and a body edit makes it fail | 1217 |
| `disposition_recomputation_matches_threshold` | the four recomputation cases, including `(2,3,5) → Invalid` | 372 |
| `valid_results_pass_contract_and_mismatch_fails` | pass/fail validate, `Invalid` does not, tampered disposition is rejected | 392 |
| `parse_network_path_minimal` / `_full` | both shapes decode, including the `__`-joined two-hop chain | 182, 196 |
| `unknown_fields_in_route_rejected` | a `credential` key is an "unknown field" error, not a silent drop | 236 |
| `credentialed_route_serialization_fails_without_exposing_input` | serialization fails and the error omits `secret-marker` | 247 |
| `zero_or_zero_value_in_bandwidth_rejected_by_underlying` | `PositiveCount` rejects 0 at deserialize | 257 |
| `duration_bounds_are_shared` | 50 ok; 0 and 31,536,000,001 rejected | 268 |
| `positive_count_round_trips` | 64 ok; 0 rejected | 276 |

Integration coverage that matters for these contracts:
[crates/eggbench-drivers/tests/eggsec_live.rs](../crates/eggbench-drivers/tests/eggsec_live.rs)
(real `eggsec` interoperation) and
[crates/eggbench-drivers/tests/eggstack_path.rs](../crates/eggbench-drivers/tests/eggstack_path.rs)
(a `ResolvedNetworkPath` built from the two retained identities).

Qualification assets are real inputs to these contracts, not illustrations:
[qualification/synvoid/v2/](../qualification/synvoid/v2/) supplies
`corpus.json` (the `HttpSecurityCorpusV1`),
`correctness-corpus.json`, `smoke.profile.json` and `perf.profile.json`
(the `SecurityQualificationProfileV1`), and thirteen
`scenarios/*.json` ordinary plans (schema v9 and v10) that the profile
references by relative path. See
[synvoid-qualification.md](../docs/synvoid-qualification.md) and
[tooling-qualification-ci.md](tooling-qualification-ci.md).

## Review focus

1. **Any change to a bound is an evidence-compatibility change.** The bounds in
   `security.rs` and the corpus result validator are part of persisted
   artifacts. Raising `MAX_SECURITY_CASES` changes what an old reader accepts.
2. **The `pass`/`fail`/`invalid` trichotomy has no escape hatch.** Any path that
   wants to report a transport problem as a correctness failure is a bug. Check
   that new dispositions are not introduced casually.
3. **The Eggsec/Eggbench split.** If a new field on `SanitizedSecurityCase`
   expresses Eggbench's opinion about bypass success, the split has been
   violated. The field list at [security.rs:70](../crates/eggbench-core/src/security.rs)
   is the whole surface.
4. **Route credential rejection is defense in depth.** It runs at plan
   validation, at `Serialize`, and in `Debug`. Changing any one of the three
   leaves the other two; changing all three is a redesign of a documented
   property.
5. **Content identity coverage.** If a new corpus input is added that can
   change request bytes, it must be folded into `ContentTreeIdentity` before the
   expansion digest is computed, or the expectation is no longer frozen. The
   body-file fold at [qualification.rs:828](../crates/eggbench-core/src/qualification.rs)
   is the pattern.
6. **Any filesystem read added to these modules** must go through
   `canonical_confined_path` and `read_bounded`. A direct `fs::read` on a
   profile-supplied path reintroduces the escape the whole module is built to
   prevent.
7. **`test_type` in `SecurityCheckResultV1` is a string, not the plan's enum**
   ([security.rs:96](../crates/eggbench-core/src/security.rs)). If the plan
   enum gains a variant, nothing forces the evidence string to follow.
8. **Aggregation must stay conservative.** `aggregate_qualification_scenarios`
   returning `Invalid` for a partially executed profile is the property that
   keeps an interrupted run from certifying anything.

Known rough edges a reviewer will notice, recorded here rather than in the code
comments: `plan.rs:1889` hardcodes the string "at most 32 HTTP corpus checks"
while the enforced bound is `MAX_SECURITY_CHECKS = 16`
([security.rs:26](../crates/eggbench-core/src/security.rs)) — the *check* is
correct, only the message is stale, unlike the sibling message at
[plan.rs:1783](../crates/eggbench-core/src/plan.rs) which formats the constant.
`SecurityConfigIdentity` and `SecurityCheckConfigRow`
([security.rs:301](../crates/eggbench-core/src/security.rs),
[319](../crates/eggbench-core/src/security.rs)) are declared but not
constructed anywhere in the workspace; comparison builds an equivalent
`SecurityEvidenceIdentity` with formatted string rows
([comparison.rs:696](../crates/eggbench-core/src/comparison.rs),
[2041](../crates/eggbench-core/src/comparison.rs)). `SECURITY_PROFILE_SCHEMA_VERSION`
and `HTTP_SECURITY_CORPUS_SCHEMA_VERSION`
([qualification.rs:13](../crates/eggbench-core/src/qualification.rs)) are
declared but unreferenced; `validate` compares against literals instead
(537, 597). `expand_qualification_profile` recomputes the corpus aggregate
digest inline ([qualification.rs:851](../crates/eggbench-core/src/qualification.rs))
rather than calling `aggregate_content_files`
([qualification.rs:982](../crates/eggbench-core/src/qualification.rs)); the two
are byte-identical today and the round-trip test at line 1217 is what keeps them
honest.

## Related

- [overview.md](overview.md) — module map, cross-cutting contract table,
  known documentation drift
- [core.md](core.md) — crate-level ownership: why core holds contracts and not
  adapters
- [core-plan-schema.md](core-plan-schema.md) — `SecurityCheckRequest`,
  `HttpCorpusCheckRequest`, `EggsecWafTestType`, and the `network_path`
  sub-contract
- [core-driver-resolution.md](core-driver-resolution.md) — how a `RouteRequest`
  and `StreamFaultPlanRequest` become a frozen, driver-pinned
  `ResolvedNetworkPath`
- [core-comparison.md](core-comparison.md) — the correctness section,
  `combine_verdicts`, and the v1/v2 policy identities
- [core-evidence-bundle.md](core-evidence-bundle.md) — manifest-last publication
  for the artifacts these contracts define
- [drivers.md](drivers.md) — path ownership, no-route-fallback rule, feature
  isolation
- [drivers-eggstack-http.md](drivers-eggstack-http.md) — the Eggfetch-backed
  fixed-corpus executor
- [tooling-qualification-ci.md](tooling-qualification-ci.md) — qualification
  harnesses and CI enforcement
- [security-qualification.md](../docs/security-qualification.md) — the
  operator-facing qualification and security-check contract
- [synvoid-qualification.md](../docs/synvoid-qualification.md) — the SynVoid
  qualification suite and its upstream asset contract
- [eggstack-http.md](../docs/eggstack-http.md) — Eggfetch/EggServe behavior
  that the corpus executor depends on
