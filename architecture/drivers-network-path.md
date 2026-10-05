# Listener-free network path: Eggress route then Eggchaos stream faults

## Purpose

`eggbench-drivers/src/eggstack/path/` is the M002 adapter that lets a schema-v3 plan place an
Eggfetch workload behind an explicitly selected Eggress route and a deterministic Eggchaos
stream-fault policy, without Eggbench ever owning a listener, a subprocess, or a shell command.

It answers three questions the rest of the system only references: how a `RouteMode` lowers into one
canonical `OutboundConnector` over the base TCP profile (`route.rs:63`); how a
`StreamFaultPlanRequest` lowers into two directional `LivePolicy` values (`dialer.rs:173`); and what
a run proves about itself in a versioned, redacted, self-validating `network-path.json`
(`evidence.rs:387`). The declarative intent is `eggbench_core::NetworkPathRequest`
([../crates/eggbench-core/src/network_path.rs](../crates/eggbench-core/src/network_path.rs)); the
descriptors published are `eggress-route` and `eggchaos-stream` (`mod.rs:33`, `mod.rs:35`).

**Eggchaos faults are user-space accepted byte-stream impairments. They are never packet loss and
never datagram loss.** The `FaultLayer` marker in evidence is closed to one variant for this reason
(`evidence.rs:101`), and the `Disconnect` lowering hardcodes `hard_reset: false` (`fault.rs:141`).
A byte accepted by the kernel socket is observed, delayed, throttled, sliced, or withheld by
user-space code; nothing below the socket is impaired. Faults do not independently affect DNS,
proxy TCP setup, HTTP CONNECT / SOCKS negotiation, or individual hops — those complete before any
fault wrapper exists.

## Source layout

| Path | LOC | Role |
|---|---|---|
| `eggstack/path/mod.rs` | 126 | Module ownership, re-export inventory, driver names, version constants, `route_descriptor` / `fault_descriptor` / `path_descriptors`, `validate_request` |
| `eggstack/path/route.rs` | 94 | `parse_route_chain`, `build_route_chain`, `redacted_chain_text`; per-hop option policy |
| `eggstack/path/fault.rs` | 145 | `build_fault_plan`, `lower_kind`; per-kind `StreamFaultKind` → `FaultKind` config |
| `eggstack/path/dialer.rs` | 367 | `lower_dialer`, `EggstackPathDialer`, `AsyncStreamDialStream`, `dial_error`, `route_failure_label` |
| `eggstack/path/evidence.rs` | 628 | `NetworkPathEvidence` and its validate contract, `SharedPathDiagnostics`, `build_evidence`, `load_network_path_evidence` |
| [`../crates/eggbench-core/src/network_path.rs`](../crates/eggbench-core/src/network_path.rs) | 281 | Declarative intent: `NetworkPathRequest`, `RouteMode`, `StreamFaultKind`, `NETWORK_PATH_SEMANTICS_VERSION`, `NETWORK_PATH_RNG_VERSION` |
| [`../crates/eggbench-core/src/plan.rs`](../crates/eggbench-core/src/plan.rs) | — | `validate_network_path_contract` ([plan.rs:2110](../crates/eggbench-core/src/plan.rs#L2110)) |
| [`../crates/eggbench-core/src/resolved.rs`](../crates/eggbench-core/src/resolved.rs) | — | `ResolvedNetworkPath` / `ResolvedStreamFaults` provenance and required capabilities ([resolved.rs:260](../crates/eggbench-core/src/resolved.rs#L260)) |
| [`../crates/eggbench-drivers/src/eggstack/fetch.rs`](../crates/eggbench-drivers/src/eggstack/fetch.rs) | 1,520 | Client construction, run evidence staging, per-invocation `network_path` object |
| [`../crates/eggbench-cli/src/workload_registry.rs`](../crates/eggbench-cli/src/workload_registry.rs) | 1,005 | The only production caller of `lower_dialer` |

## Key types

### Descriptors

| Type | Location | Advertises |
|---|---|---|
| `ROUTE_DRIVER_NAME = "eggress-route"` | `mod.rs:33` | `DriverCategory::Route`, capability `ProxyRouting` only (`mod.rs:55`) |
| `FAULT_DRIVER_NAME = "eggchaos-stream"` | `mod.rs:35` | `DriverCategory::Fault`, capability `StreamFaultPlan` only (`mod.rs:61`) |

Both set `external_process: false` (`mod.rs:79`, `mod.rs:97`) and both carry
pinned upstream versions scraped from `Cargo.lock` at build time: `eggress-outbound`
(`mod.rs:39`), `eggress-uri` (`mod.rs:41`), `eggchaos-core`
(`mod.rs:43`). `path_descriptors()` returns both in stable name order
(`mod.rs:105`); `eggchaos-stream` sorts before `eggress-route`.

Neither descriptor advertises `NetworkPath`. That capability belongs to the *workload* driver
([../crates/eggbench-drivers/src/eggstack/mod.rs:98](../crates/eggbench-drivers/src/eggstack/mod.rs#L98)),
because the route and fault plumbing lives in the executor's dialer. Core enforces the split: a
`network_path` request inserts required `NetworkPath` (workload), `ProxyRouting` (route), and —
only when a fault plan is present — `StreamFaultPlan` (fault)
([resolved.rs:447](../crates/eggbench-core/src/resolved.rs#L447)).

### `EggstackPathDialer`

`dialer.rs:207`. Immutable after `lower_dialer`, shared as `Arc`, one instance per
workload executor for the whole run.

| Field | Purpose |
|---|---|
| `resolved: ResolvedNetworkPath` | Frozen provenance, re-emitted into evidence |
| `seed_namespace: Option<u64>` | Fault-conditional Eggchaos RNG namespace |
| `upstream_policy` / `downstream_policy: LivePolicy` | The two directional static fault policies |
| `connector: Arc<OutboundConnector>` | The single route decision, built once (`dialer.rs:159`) |
| `diagnostics: Arc<SharedPathDiagnostics>` | Bounded run counters |
| `connection_ordinal: AtomicU64` | Monotonic per-physical-dial counter, seeded from the client |
| `has_faults: bool` | Whether *any* fault is active, latched at lowering time |
| `redacted_chain` / `chain_config_digest` / `configured_hop_count` | Canonical credential-free route identity |

`faults_active()` (`dialer.rs:252`) returns the latched `has_faults`, not a live
query against the plan.

### `AsyncStreamDialStream`

`dialer.rs:29`. A safe adapter from `eggress_core::BoxStream` to Eggfetch's
`DialStream` trait object. It forwards `AsyncRead`, all four `AsyncWrite` methods, and
`is_write_vectored` with no raw-pointer cast (`dialer.rs:41`, `dialer.rs:51`).
The workspace is `#![forbid(unsafe_code)]`; the wrapper exists precisely so the boxed-streams-to-trait-object
step stays inside safe code.

### Evidence types

| Type | Location | Notes |
|---|---|---|
| `NetworkPathEvidence` | `evidence.rs:29` | The `network-path.json` document; `deny_unknown_fields` |
| `NetworkPathDriverEvidence` | `evidence.rs:63` | `name`, `adapter_version`, `upstream_name`, `upstream_version` |
| `PathSemantics` | `evidence.rs:77` | `ordering_version` + the four closed markers |
| `PathOrdering` | `evidence.rs:93` | One variant, `RouteFirstFaultSecond` |
| `FaultLayer` | `evidence.rs:101` | One variant, `UserSpaceStream` |
| `StreamDirection` | `evidence.rs:109` | `ClientToTarget` / `TargetToClient` |
| `PathPolicyMode` | `evidence.rs:119` | One variant, `Static` |
| `StreamFaultEvidence` | `evidence.rs:127` | `request`, `seed_namespace`, `rng_version` |
| `PathDiagnosticsSnapshot` | `evidence.rs:139` | The bounded counters, serialized into both the run artifact and the per-invocation object |
| `SharedPathDiagnostics` | `evidence.rs:217` | `Mutex<PathDiagnosticsState>` live counters |

`PathOrdering`, `FaultLayer`, and `PathPolicyMode` have exactly one variant each. They are not
runtime state; they are compile-time-closed wire strings that a consumer can read without knowing
this crate. `validate` re-asserts all of them (`evidence.rs:515`), so a hand-edited
artifact cannot claim a different layer or ordering.

`NETWORK_PATH_EVIDENCE_SCHEMA_VERSION = SchemaVersion(1)` (`evidence.rs:21`) and
`NETWORK_PATH_EVIDENCE_MAX_BYTES = 128 * 1024` (`evidence.rs:23`) bound the
artifact. `network_path_role_label()` returns the stable manifest role `network-path`
(`evidence.rs:626`), and `NetworkPathEvidence` implements
`eggbench_runner::RunEvidenceContract` (`evidence.rs:616`) so the runner
re-validates it at staging time.

## How it works

### 1. Intent and pre-runtime rejection

A plan's `network_path` is parsed and bounded by core before any driver code runs.
`validate_network_path_contract` ([plan.rs:2110](../crates/eggbench-core/src/plan.rs#L2110)) rejects:

| Condition | Category | Location |
|---|---|---|
| Empty or > 1024-byte chain | `invalid_route` | [plan.rs:2115](../crates/eggbench-core/src/plan.rs#L2115) |
| Control characters in the chain | `invalid_route` | [plan.rs:2124](../crates/eggbench-core/src/plan.rs#L2124) |
| `@`, `%`, `?`, or `#` anywhere in the chain | `route_credentials_not_supported` | [plan.rs:2130](../crates/eggbench-core/src/plan.rs#L2130) |
| Hop without `scheme://host:port` | `invalid_route` | [plan.rs:2158](../crates/eggbench-core/src/plan.rs#L2158) |
| Scheme outside `http`/`socks4`/`socks4a`/`socks5` | `unsupported_route` | [plan.rs:2164](../crates/eggbench-core/src/plan.rs#L2164) |
| Non-ASCII or non-`host:port` endpoint | `invalid_route` | [plan.rs:2170](../crates/eggbench-core/src/plan.rs#L2170) |
| `network_path` + `paired` | `paired_network_path_not_supported` | [plan.rs:2009](../crates/eggbench-core/src/plan.rs#L2009) |
| `network_path` + `Subject::External` | `workload_path_incompatible` | [resolved.rs:471](../crates/eggbench-core/src/resolved.rs#L471) |

**The plan's path request is credential-free by construction.** Two independent layers enforce
this, and neither ever echoes the offending text:

- `RouteMode`'s hand-written `Serialize` impl refuses to emit a chain containing `@`, `%`, `?`, or
  `#`, with a fixed message that does not include the input
  ([network_path.rs:65](../crates/eggbench-core/src/network_path.rs#L65), message at
  [network_path.rs:78](../crates/eggbench-core/src/network_path.rs#L78)).
- `RouteMode`'s hand-written `Debug` impl prints `chain: "[REDACTED]"` and never the value
  ([network_path.rs:89](../crates/eggbench-core/src/network_path.rs#L89)).

So a rejected credential-bearing chain cannot reach a resolved plan, a manifest, or evidence
through serialization or debug output. In this adapter module, `validate_request`
(`mod.rs:115`) re-asserts the credential rule against the *canonical* chain text at the
preflight layer, and `validate_route_spec` (`route.rs:24`) refuses per-hop
credentials, rules, local binds, TLS, insecure mode, plugins, and auth prefixes at lowering time.
Both run before the connector exists, so an unsupported option cannot be established and then
rejected.

### 2. `lower_dialer` — one immutable dialer per run

`dialer.rs:134` is a pure lowering function with a fixed order:

1. `validate_resolved_provenance` (`dialer.rs:140`) re-checks the resolved descriptors against
   this adapter's own constants — name, `DriverCategory`, `external_process`,
   `executable_path.is_some()`, `upstream_name`, exact `upstream_version`, required capability
   (`dialer.rs:87`, `dialer.rs:107`) — plus `semantics_version == NETWORK_PATH_SEMANTICS_VERSION`
   (`dialer.rs:104`) and, when faults are present, `rng_version == NETWORK_PATH_RNG_VERSION`
   (`dialer.rs:119`). A resolved path naming any other adapter, version, or semantics identity is
   refused here.
2. The `ResolvedNetworkPath` is folded back into a `NetworkPathRequest` and re-run through
   `validate_network_path_contract` (`dialer.rs:141`). Validation is not assumed to have happened
   earlier; it is repeated at the runtime boundary.
3. `has_faults` is latched: true only when at least one of `upstream` / `downstream` is non-empty
   (`dialer.rs:153`). A `stream_faults` request with two empty lists is *present but inactive*.
4. If `has_faults` and the seed namespace is `None`, lowering fails with `missing_fault_seed`
   (`dialer.rs:155`). An inactive or absent fault request needs no seed.
5. The route connector is built exactly once and wrapped in `Arc` (`dialer.rs:159`). For a
   `ProxyChain`, the canonical credential-free text, its SHA-256 digest, and the hop count are
   computed and retained here (`dialer.rs:161`) — the digest covers the *canonical* text, so two
   spellings of one route produce one identity.
6. Two `LivePolicy` values are built, one per direction, from the same seed (`dialer.rs:173`). With
   no fault request both are `FaultPlan::empty()` (`dialer.rs:179`). `build_plan`
   (`dialer.rs:200`) is the only bridge to `fault.rs`; a lowering failure aborts construction.

The resulting `EggstackPathDialer` is immutable: no field is reachable for mutation, and the
`Debug` impl is hand-written to print only names, the seed, the hop count, and the fault flag
(`dialer.rs:223`).

### 3. Route lowering

`route.rs:63` is total over `RouteMode`:

| Mode | Lowering |
|---|---|
| `Direct` | `OutboundConnector::direct()` (`route.rs:67`) |
| `ProxyChain` | `parse_route_chain` then `OutboundConnector::from_chain` (`route.rs:68`) |

`parse_route_chain` delegates parsing to `eggress_uri::parse_proxy_chain` — the native parser, not a
regex — and then applies `validate_route_spec` (`route.rs:17`):

- at least one hop (`route.rs:25`);
- per hop, none of: `credentials`, `rule`, `local_bind`, `tls`, `server_name`, `insecure`,
  `plugins`, `auth_prefix` (`route.rs:29`);
- per hop, exactly one protocol and it must be `Http`, `Socks4`, or `Socks5`
  (`route.rs:42`).

**No shell or child process is involved, and this is structural rather than conventional.** The
whole module's dependency surface is `eggress_outbound::{OutboundConnector, OutboundError}` and
`eggress_uri::{ProtocolSpec, ProxyChainSpec, RedactedUri}` (`route.rs:8`). There is no
`std::process`, no `Command`, and no raw `TcpStream` anywhere under `eggstack/path/`. The chain
string is data handed to a library constructor; the only way out is a TCP connect through
`connect_tcp_timeout_detailed`.

`redacted_chain_text` (`route.rs:79`) re-emits the parsed chain through
`RedactedUri::to_string`, which is the native canonical scheme. `socks4a` normalizes to `socks4`
(asserted at `route.rs:88`).

### 4. Fault lowering

`fault.rs:33` maps an ordered `&[StreamFaultRequest]` to a sibling
`eggchaos_core::FaultPlan`:

| Plan `StreamFaultKind` | `FaultKind` config | Notes |
|---|---|---|
| `Latency` | `LatencyConfig` | `max_buffer_bytes` must be positive (`fault.rs:61`) |
| `Bandwidth` | `BandwidthConfig` | both rate and burst must be positive (`fault.rs:80`) |
| `Blackhole` | `BlackholeConfig` | `close_after` is optional (`fault.rs:102`) |
| `LimitData` | `LimitDataConfig` | bytes must be positive (`fault.rs:106`) |
| `Slice` | `SliceConfig` | `average_size` positive (`fault.rs:123`) |
| `SlowClose` | `SlowCloseConfig` | delays shutdown only |
| `Disconnect` | `DisconnectConfig` | `hard_reset: false`, hardcoded (`fault.rs:141`) |

**Activation probability semantics.** Every fault is lowered with
`Probability::new(1.0)` (`fault.rs:41`). `1.0` is exact — there is no per-connection
roll, no sampling, and no "sometimes on" mode. A lowered fault is active for every byte stream the
policy wraps. Plan order is preserved because `requests` is iterated in order into a `Vec` of specs
(`fault.rs:34`); `FaultPlan::new` rejects duplicate ids within a direction
(`fault.rs:45`). Non-positive counts fail closed as `FaultLowerError::Invalid`
(`fault.rs:22`) — the plan-level `PositiveCount` bound is re-checked here rather than
trusted.

What is deliberately absent: no datagram faults, no live mutation, no time-varying scenarios, no
hard reset. Those are stated as contract in the module header (`fault.rs:4`) and
enforced by the closed `match` in `lower_kind` (`fault.rs:53`) — a new upstream
`FaultKind` cannot be expressed without editing this match.

### 5. Route-first, fault-second — the central invariant

The ordering is enforced by control flow in `EggfetchDialer::dial` (`dialer.rs:292`),
not by convention. One physical Eggfetch dial is this sequence:

```text
dial(target)                                          dialer.rs:293
  ordinal = next_ordinal()                            dialer.rs:294
  diagnostics.record_dial_attempt()                   dialer.rs:295
  clone connector / policies / diagnostics            dialer.rs:296-301
  ┌─ async block ─────────────────────────────────────────────────────┐
  │ connector.connect_tcp_timeout_detailed(host, port, timeout)      │  dialer.rs:303
  │   .map_err(record_route_failure + dial_error)?                   │  dialer.rs:306-309
  │        ↑ the `?` RETURNS HERE on failure                          │
  │ record_successful_dial(info.hop_count, ordinal)                  │  dialer.rs:310
  │ let stream: DialStream = AsyncStreamDialStream::new(stream)     │  dialer.rs:312
  │ if !has_faults { return Ok(stream) }                             │  dialer.rs:313
  │ BidirectionalChaosStream::new_live(                             │  dialer.rs:316
  │     stream, upstream, downstream, "eggress", ordinal)            │
  │   .inspect(record_fault_wrapped_connection)                      │  dialer.rs:325
  └──────────────────────────────────────────────────────────────────┘
```

Four properties make this ordering auditable:

1. **The route result is a hard `?`.** On any `OutboundConnectError`, the closure records the
   failure label and returns `Err` (`dialer.rs:306`). Control never reaches line 316. There is no
   `match` arm, no `unwrap_or_else`, and no retry anywhere in the module.
2. **The route decision happens once, not per dial.** `connector` is an `Arc<OutboundConnector>`
   built at lowering time (`dialer.rs:159`) and only *cloned* per dial (`dialer.rs:296`). No
   per-dial `build_route_chain` call site exists, so there is no mechanism by which a second route
   could be chosen.
3. **The fault path is gated on a pre-computed flag.** `has_faults` is captured before the async
   block (`dialer.rs:301`) and short-circuits at `dialer.rs:313`. An inactive fault plan yields a
   bare `AsyncStreamDialStream`; no Eggchaos type is ever constructed.
4. **Direction is fixed at construction.** `upstream` is always the first `LivePolicy` and
   `downstream` the second (`dialer.rs:173`), passed in that positional order to `new_live`
   (`dialer.rs:316`). Upstream is client to final target, downstream final target to client.
   Eggbench never swaps them at runtime, and the ordering is asserted by the production-argument
   test at
   [`../crates/eggbench-drivers/tests/eggstack_path.rs:623`](../crates/eggbench-drivers/tests/eggstack_path.rs#L623).

**A static fault policy remains attached to a reused physical connection.** The wrapper is created
once per physical connection and lives for that connection's lifetime. Because the policy is
static for the run and the runner does not force a reconnect per trial, a pooled connection opened
during a warmup carries its faults into the measured trial. The wrapper is not re-evaluated on
each request, so fault behaviour is a property of the connection, not of the invocation.

### 6. Client lifetime and the per-invocation object

One Eggfetch client is owned by one workload executor for the whole run, built in
`with_path_dialer` ([fetch.rs:461](../crates/eggbench-drivers/src/eggstack/fetch.rs#L461)) and
stored beside the dialer ([fetch.rs:83](../crates/eggbench-drivers/src/eggstack/fetch.rs#L83)). Its
pool and physical connection state therefore survive warmups and measured trials, which is why
physical dial attempts are a distinct quantity from request counts. The full client-lifetime
contract belongs to [drivers-eggstack-http.md](drivers-eggstack-http.md) and is not restated here.

Two evidence surfaces come out of this module:

| Surface | Where staged | Content |
|---|---|---|
| Run-level `network-path.json` | `run_evidence` ([fetch.rs:540](../crates/eggbench-drivers/src/eggstack/fetch.rs#L540)) | Full `NetworkPathEvidence`, `Sensitivity::Redacted`, role `network-path` |
| Per-invocation `network_path` object | `build_method_evidence` ([fetch.rs:1295](../crates/eggbench-drivers/src/eggstack/fetch.rs#L1295)) | The invocation's `PathDiagnosticsSnapshot` plus `faults_active` |

The per-invocation object is a **bounded delta**, not a cumulative counter. `begin_invocation`
captures a pre-invocation snapshot and resets the invocation window (`evidence.rs:303`);
`invocation_delta` saturating-subtracts each field and drops zero-delta map entries
(`evidence.rs:313`, `evidence.rs:175`). The snapshot is inserted as one additional key on the
*existing* `eggfetch-method.json` document
([fetch.rs:1295](../crates/eggbench-drivers/src/eggstack/fetch.rs#L1295)). It is not a new
normalized metric: `build_output` receives it as an opaque `Option<&serde_json::Value>` used only
for evidence assembly ([fetch.rs:1152](../crates/eggbench-drivers/src/eggstack/fetch.rs#L1152)).

### 7. The seed namespacing is fault-conditional, and it lives in evidence

`StreamFaultEvidence.seed_namespace` is `Option<u64>` (`evidence.rs:131`), and
`build_evidence` sets it only when a fault plan is *active*:

```rust
let active = !faults.request.upstream.is_empty() || !faults.request.downstream.is_empty();
seed_namespace: active.then_some(seed_namespace).flatten(),      // evidence.rs:396-399
```

`Option::then_some` with a `None` argument, then `flatten`, yields `None` in three cases: no fault
request, a present-but-empty fault request, or a missing seed. The retained value is the Eggchaos
namespace; the RNG identity is `NETWORK_PATH_RNG_VERSION = "splitmix64-v1"`
([network_path.rs:15](../crates/eggbench-core/src/network_path.rs#L15)), copied from the resolved
path (`evidence.rs:400`). The stable composition identity is
`NETWORK_PATH_SEMANTICS_VERSION = "route-first-fault-second-v1"`
([network_path.rs:13](../crates/eggbench-core/src/network_path.rs#L13)), echoed as
`semantics.ordering_version` (`evidence.rs:414`).

This is the one point where a sibling document is wrong, and it matters because the resolved plan
has no seed field at all: `ResolvedNetworkPath` is `{route, route_driver, semantics_version,
stream_faults}` ([resolved.rs:260](../crates/eggbench-core/src/resolved.rs#L260)) and
`ResolvedStreamFaults` is `{request, fault_driver, rng_version}`
([resolved.rs:279](../crates/eggbench-core/src/resolved.rs#L279)). The fault-conditional seed
*namespacing* is a property of this run's evidence, enforced at lowering
(`dialer.rs:155`), recorded in `StreamFaultEvidence`
(`evidence.rs:131`), re-derived at validation
(`evidence.rs:555`), and independently re-checked at comparison time against the
resolved seed ([../crates/eggbench-core/src/comparison.rs:945](../crates/eggbench-core/src/comparison.rs#L945)).

## Invariants

| # | Invariant | Enforced at |
|---|---|---|
| I1 | Route before fault, per physical dial, with no interleaving | `dialer.rs:303` then `dialer.rs:316`, separated by a `?` |
| I2 | A failed route is a failed dial | The `?` at `dialer.rs:306`; no recovery arm exists in the module |
| I3 | No shell, subprocess, or Eggbench listener | Only library types in `route.rs:8`; `external_process: false` at `mod.rs:79`, `mod.rs:97` |
| I4 | No second route decision | `Arc<OutboundConnector>` built at `dialer.rs:159`, cloned at `dialer.rs:296` |
| I5 | No direct fallback for an unavailable requested route | `build_route_chain` is total on the *requested* mode only (`route.rs:63`); `Direct` is reachable only when the plan asked for it |
| I6 | Credential-bearing or extended route options fail before runtime | [plan.rs:2130](../crates/eggbench-core/src/plan.rs#L2130), `mod.rs:120`, `route.rs:29` |
| I7 | Resolved provenance must match this adapter exactly | `dialer.rs:87` |
| I8 | Faults are user-space accepted byte-stream impairments | Closed `FaultLayer` (`evidence.rs:101`); closed `match` in `lower_kind`; `hard_reset: false` (`fault.rs:141`) |
| I9 | Every lowered fault activates with probability exactly 1.0 | `fault.rs:41` |
| I10 | Upstream is client→target, downstream is target→client, fixed at lowering | `dialer.rs:173`, `dialer.rs:316` |
| I11 | The fault policy is static for the run | `PathPolicyMode::Static`, one variant (`evidence.rs:119`); re-asserted at `evidence.rs:520` |
| I12 | Route-failure buckets are bounded | 16 buckets (`mod.rs:49`), 256-byte keys (`evidence.rs:24`), applied at `evidence.rs:266` |
| I13 | Diagnostics are internally consistent and re-validated | `evidence.rs:579` and mirrored in [../crates/eggbench-core/src/comparison.rs:966](../crates/eggbench-core/src/comparison.rs#L966) |
| I14 | An artifact is self-validating: canonical route identity is re-derived | `evidence.rs:525` |

The `validate` contract is worth reading as the executable form of these invariants
(`evidence.rs:483`). Notable cross-field checks: `hop_count_distribution` must sum
to exactly `successful_dials`; no observed hop count may exceed `configured_hop_count`, so a `Direct`
run (hop count 0) cannot record a proxy hop; `successful_dials` ≤ `physical_dial_attempts`; and

```text
fault_wrapped_connections + fault_wrapper_construction_failures
    == successful_dials   when a fault plan is active
    == 0                  otherwise                              // evidence.rs:594-601
```

That identity is the route-first/fault-second invariant restated as a counting law: every successful
route either received a fault wrapper or failed to, and a successful route with faults configured can
never go unwrapped.

## Failure model

| Failure | Detection point | Surface | Result |
|---|---|---|---|
| Unknown plan field | serde `deny_unknown_fields` ([network_path.rs:19](../crates/eggbench-core/src/network_path.rs#L19)) | plan parse | reject |
| Credential-bearing chain | [plan.rs:2130](../crates/eggbench-core/src/plan.rs#L2130) | plan validate | `route_credentials_not_supported`, input never echoed |
| Unsupported hop scheme | [plan.rs:2164](../crates/eggbench-core/src/plan.rs#L2164) | plan validate | `unsupported_route` |
| Extended hop options (TLS, rule, bind, plugin) | `route.rs:29` | `lower_dialer` | `Err(String)`, no connector built |
| Provenance mismatch (name, version, category, capability, `external_process`, `executable_path`) | `dialer.rs:87` | `lower_dialer` | `Err` before any socket work |
| Unsupported semantics or RNG version | `dialer.rs:104`, `dialer.rs:119` | `lower_dialer` | `Err` |
| Active fault plan without a seed | `dialer.rs:155` | `lower_dialer` | `missing_fault_seed` |
| Non-positive fault byte counts or duplicate ids | `fault.rs:61`, `fault.rs:45` | `lower_dialer` | `FaultLowerError::Invalid`, fail-closed |
| Route unreachable / proxy refuses / hop timeout | `dialer.rs:303` | per dial | **failed dial**, `Err` to Eggfetch; no fallback |
| Eggchaos wrapper construction fails | `dialer.rs:318` | per dial | `DialErrorKind::Other`; the route was already established and is counted |
| Diagnostics mutex poisoned | `evidence.rs:239` and siblings | counter update | that counter update is skipped, run continues |

Eggress `OutboundConnectErrorKind` maps to Eggfetch `DialErrorKind` at `dialer.rs:343`:
`Timeout → Timeout`, `Authentication → Authentication`, `Policy → Rejected`, everything else →
`Connection`. The source error is preserved via `DialError::with_source`, so a caller can still
distinguish categories the flattened kind collapses.

Route failures are aggregated into a stable label of `kind:stage[:hop=N][:protocol=P]`
(`dialer.rs:330`) and bucketed. The bucket count is capped at 16
(`evidence.rs:275`); overflow increments `route_failure_buckets_dropped` rather
than growing the map, and the artifact records how many observations were dropped
(`evidence.rs:149`). Labels are additionally filtered at validation by
`safe_route_failure_label` (`evidence.rs:164`), which permits only
`[A-Za-z0-9:=_.-]` up to 256 bytes and rejects any label containing `secret`, `password`, or
`token`.

## Boundaries

**Feature gate.** The whole module is behind the `eggstack-path` cargo feature, which implies
`eggstack-http`. `path_descriptors()` is added to the production catalog only under that `cfg`
([../crates/eggbench-drivers/src/catalog.rs:53](../crates/eggbench-drivers/src/catalog.rs#L53)), and
the `eggfetch-http` descriptor gains `Capability::NetworkPath` under the same gate
([../crates/eggbench-drivers/src/eggstack/mod.rs:97](../crates/eggbench-drivers/src/eggstack/mod.rs#L97)).
Default and `eggstack-http`-only builds link neither Eggress nor Eggchaos; the boundary, the CI
proof, and the catalog projection are covered in
[drivers-catalog-features.md](drivers-catalog-features.md).

**Ownership.**

| Concern | Owner |
|---|---|
| Plan and resolved contracts, `validate_network_path_contract` | `eggbench-core` |
| Route/fault lowering, dialer, evidence | `eggbench-drivers` (`eggstack/path/`) |
| Executor/session contracts, trial orchestration, evidence contract re-validation | `eggbench-runner` |
| Descriptor selection and `lower_dialer` invocation | `eggbench-cli` |
| Comparison-time path identity | `eggbench-core::comparison` |

**What this module does not own.**

- No listener, no bound port, no listener-backed routing mode. The route is client-side; nothing
  listens.
- No managed service, no entry in `lifecycle/runtime-topology.json`. A route is not a service.
- No normalized metrics. Only `eggbench-core` writes `TrialMetrics`.
- No retries. Eggbench adds no retry; the `standard-http1` Eggfetch profile omits logical
  retry/redirect support by construction.
- No fallback. `ResetPolicy::Service` does not imply reconnect or restart semantics for a route.

**Downstream consumption.** `load_network_path_evidence` (`evidence.rs:443`)
reads the artifact from a verified bundle: it selects the single manifest record whose role is
`Other { label: "network-path" }` (`evidence.rs:451`), rejects a bundle with two
such artifacts (`evidence.rs:456`), enforces `NETWORK_PATH_EVIDENCE_MAX_BYTES`
against the manifest's recorded size before reading (`evidence.rs:461`), parses
with `deny_unknown_fields`, and calls `validate`. A bundle whose resolved plan has no
`network_path` but which carries the artifact is an error, not a warning
([../crates/eggbench-core/src/comparison.rs:1018](../crates/eggbench-core/src/comparison.rs#L1018)).
The comparison layer keeps only `chain_config_digest` and `eggress_uri_version` as the
comparison-critical identity ([../crates/eggbench-core/src/comparison.rs:1054](../crates/eggbench-core/src/comparison.rs#L1054)).

## Tests and qualification

[`../crates/eggbench-drivers/tests/eggstack_path.rs`](../crates/eggbench-drivers/tests/eggstack_path.rs),
gated `#![cfg(feature = "eggstack-path")]` (line 3):

| Test | Line | What it pins |
|---|---|---|
| `direct_http_socks_and_multi_hop_routes_carry_real_http_traffic` | 166 | Real traffic over Direct, single-hop, and multi-hop chains against an embedded Eggress proxy fixture |
| `unavailable_requested_proxy_never_falls_back_to_reachable_origin` | 213 | **The no-fallback guarantee**: a live reachable origin does not rescue a dead requested proxy |
| `eggfetch_eggress_eggchaos_eggserve_composition_records_static_faults` | 259 | Full Eggfetch → Eggress → Eggchaos → EggServe composition |
| `one_client_reuses_faulted_physical_connections_across_invocations` | 305 | Client reuse across a warmup and a measured trial; the idle measured trial opens zero new connections |
| `no_fault_path_diagnostic_completes_without_overhead_gate` | 373 | Direct Eggfetch baseline for the path overhead claim |
| `cancellation_drops_in_flight_route_work_without_detached_tasks` | 422 | No orphaned route task after cancellation |
| `every_supported_fault_kind_activates_deterministically` | 531 | Each of the seven kinds activates |
| `slow_close_delays_shutdown_after_payload_forwarding` | 596 | Shutdown-phase-only delay semantics |
| `production_argument_order_keeps_upstream_and_downstream_faults_distinct` | 623 | **I10**, direction identity through the production constructor |
| `every_supported_fault_kind_lowers_with_probability_one` | 665 | **I9**: all seven kinds at probability 1.0, identical across repeated lowering, disconnect without hard reset |
| `credentialed_and_extended_routes_fail_before_runtime` | 733 | **I6**: `trojan`, `ssh`, and `quic+http` chains all fail `lower_dialer`; bracketed IPv6 succeeds |
| `invocation_diagnostics_do_not_accumulate_across_invocations` | 766 | Deltas are per-invocation while the run snapshot is cumulative |
| `diagnostic_updates_are_bounded_and_concurrency_safe` | 799 | Concurrent counter updates from 32 threads |

In-module: `redacted_chain_uses_the_native_canonical_scheme`
(`route.rs:88`) pins `socks4a` → `socks4` canonicalization.

Core-side qualification lives in
[`../crates/eggbench-core/tests/network_path.rs`](../crates/eggbench-core/tests/network_path.rs),
including `resolution_fails_closed_without_path_capability_or_fault_driver` (line 582), which
removes `NetworkPath` from the workload descriptor and requires resolution to fail. Core unit tests
cover parse round-trips, unknown-field rejection, credentialed-serialization failure without
exposing input, `PositiveCount` and `DurationMs` bounds
([../crates/eggbench-core/src/network_path.rs:177](../crates/eggbench-core/src/network_path.rs#L177)).

Runner- and CLI-level staging are covered by
[`../crates/eggbench-runner/tests/orchestration.rs`](../crates/eggbench-runner/tests/orchestration.rs)
(network-path run-evidence metadata validation) and
[`../crates/eggbench-cli/tests/cli.rs:1036`](../crates/eggbench-cli/tests/cli.rs#L1036) (artifact
presence in a finished bundle).

## Review focus

Ordered by how much a reader should distrust a change that touches them.

1. **Any new failure arm inside `dial`.** The `?` at `dialer.rs:306` *is* the no-fallback
   guarantee. Adding an `.or_else`, a `match` arm, or a second `connect_tcp_timeout_detailed` call
   below line 310 silently restores a fallback path.
   [eggstack_path.rs:213](../crates/eggbench-drivers/tests/eggstack_path.rs#L213) is the check.
2. **Route construction moving inside `dial`.** `OutboundConnector` must stay built once at
   `dialer.rs:159`. Rebuilding per dial would re-run `parse_route_chain`, re-derive the chain
   identity, and make "one route decision" untrue.
3. **`FaultLayer` gaining a variant** (`evidence.rs:101`). A second variant is a semantics change
   requiring `NETWORK_PATH_SEMANTICS_VERSION` to move, because `ordering_version`
   (`evidence.rs:414`) is the comparison key.
4. **The fault-conditional seed.** `active.then_some(seed_namespace).flatten()` (`evidence.rs:399`)
   is the only place the seed is namespaced, and the comparison layer re-derives the same rule
   independently
   ([../crates/eggbench-core/src/comparison.rs:945](../crates/eggbench-core/src/comparison.rs#L945)).
   Changing one without the other breaks bundle comparison. `docs/eggstack-http.md` and this
   document agree here; [drivers.md:13](drivers.md) does not, and
   [overview.md](overview.md#known-documentation-drift) already flags that drift.
5. **Duplicated bounds.** The 16-bucket and 256-byte limits are implemented twice: at
   `evidence.rs:164` / `evidence.rs:275`, and as bare literals in
   [../crates/eggbench-core/src/comparison.rs:955](../crates/eggbench-core/src/comparison.rs#L955)
   and [../crates/eggbench-core/src/comparison.rs:982](../crates/eggbench-core/src/comparison.rs#L982).
   They can drift silently. Note also that the write-side filter (`evidence.rs:267`) is *weaker*
   than the validate-side filter: it checks length and control characters but not the
   `secret`/`password`/`token` substrings, so a label carrying one of those words would be recorded
   and then rejected at validation. In practice `route_failure_label` (`dialer.rs:330`) only
   concatenates Eggress `kind`, `stage`, and `protocol` values, so this is latent rather than active.
6. **Mutex-poison handling is inconsistent.** `snapshot` swallows a poisoned lock and returns
   defaults (`evidence.rs:356`), while `begin_invocation` and `invocation_delta` `expect` it
   (`evidence.rs:304`, `evidence.rs:314`). A panic in a recording path would fail the run during
   invocation but yield a zeroed snapshot during evidence assembly.
7. **Three public constants are declared and never referenced** anywhere in the workspace:
   `ROUTE_FAILURE_OTHER` (`mod.rs:47`), `MAX_RETAINED_CHAIN_BYTES` (`mod.rs:51`), and
   `DEFAULT_DIAL_TIMEOUT_MS` (`mod.rs:53`). The CLI hardcodes `Duration::from_secs(30)` at
   [../crates/eggbench-cli/src/workload_registry.rs:491](../crates/eggbench-cli/src/workload_registry.rs#L491)
   rather than using the constant, and the 1024-byte chain bound actually lives in core
   ([plan.rs:2111](../crates/eggbench-core/src/plan.rs#L2111)). `ROUTE_FAILURE_OTHER` is the
   notable one: its doc comment describes a transport-level diagnostic label, but nothing emits
   `"transport_other"` — the `_ =>` arm of `dial_error` maps to `DialErrorKind::Connection`
   (`dialer.rs:348`) and `route_failure_label` always emits a concrete `kind:stage` prefix.
8. **`validate_request` has no in-workspace caller.** It is exported (`mod.rs:115`,
   [`lib.rs:33`](../crates/eggbench-drivers/src/lib.rs#L33)) and correct, but the runtime rejection
   it duplicates is `validate_network_path_contract` at `dialer.rs:148` plus `parse_route_chain`
   at `dialer.rs:164` and `dialer.rs:166`. Hop-option policy must change in `route.rs`, not here.
9. **Startup ordering is asserted, not verifiable here.** The CLI does construct the dialer when
   building the executor
   ([../crates/eggbench-cli/src/workload_registry.rs:487](../crates/eggbench-cli/src/workload_registry.rs#L487)),
   but this module cannot observe *when* the executor is built, so a change to that order would be
   caught by no test here.
10. **`PathDiagnosticsSnapshot` carries two different lifetimes.** The run artifact holds a
    cumulative snapshot (`dialer.rs:283` → `evidence.rs:425`) while the per-invocation object
    holds a delta. Both serialize through the same type, so a consumer must know which lifetime it
    is reading before interpreting `connection_ordinal_count`.

## Related

- [drivers.md](drivers.md) — the same boundary at summary altitude; its "Listener-free path
  ownership" section is otherwise consistent with this code, except for the seed-placement sentence
  flagged in [Review focus](#review-focus).
- [overview.md](overview.md) — system shape, end-to-end flow, the workspace feature matrix, and the
  already-recorded seed-retention drift.
- [drivers-eggstack-http.md](drivers-eggstack-http.md) — the Eggfetch client and its run-scoped
  lifetime, which this document cross-links rather than restates.
- [drivers-catalog-features.md](drivers-catalog-features.md) — the `eggstack-path` feature gate,
  descriptor registration, and the CI proof that default builds do not link Eggress or Eggchaos.
- [core-driver-resolution.md](core-driver-resolution.md) — how the descriptors become a frozen
  `ResolvedNetworkPath`, and the required-capability derivation at
  [resolved.rs:447](../crates/eggbench-core/src/resolved.rs#L447).
- [core-plan-schema.md](core-plan-schema.md) — schema v3 and the `network_path` plan surface.
- [core-qualification-security.md](core-qualification-security.md) — where `network_path.rs` sits
  among the core domain contracts.
- [core-comparison.md](core-comparison.md) — the comparison-time path identity and the second
  implementation of the diagnostics invariants.
- [../docs/eggstack-http.md](../docs/eggstack-http.md) — user-facing route/fault semantics, the
  fault-kind table, and the accepted byte-stream warning.
- [../examples/eggstack-path.json](../examples/eggstack-path.json) — a Direct route with upstream
  latency and downstream slow-close; runs without an external proxy fixture.
- [../examples/eggstack-path-paired-unsupported.json](../examples/eggstack-path-paired-unsupported.json)
  — rejected before startup by [plan.rs:2009](../crates/eggbench-core/src/plan.rs#L2009).
