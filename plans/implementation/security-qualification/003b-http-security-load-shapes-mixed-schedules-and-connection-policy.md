# Security Qualification M003b — HTTP Security Load Shapes, Mixed Schedules, and Connection Policy

Status: authored; blocked on M003a closure.

Research baseline: `eggstack/eggbench@1eb1bfed4edbc6c4af4f193fa71dc82235ad927e`.

Prerequisite:

- `plans/implementation/security-qualification/003a-qualification-method-binding-and-runtime-binding-resolution.md`

## 1. Objective

Add a reusable, deterministic HTTP security-performance workload shape capable of:

- owner-authored method/path/query/header/body requests;
- exact benign/malicious mixtures;
- request-body campaigns;
- pooled versus fresh-connection execution;
- expected HTTP outcome accounting that does not misclassify a correct WAF block as a transport failure.

This must remain a benchmark/workload primitive. Eggbench does not classify attacks, generate payloads, or infer security expectations.

## 2. Research findings

Current native `eggfetch-http` behavior is intentionally narrow:

- HTTP GET only;
- target URL comes directly from the target service's `http_url`;
- no arbitrary method/header/body request template;
- one Eggfetch client is retained for the invocation, so warmups establish pooled connection state;
- closed-loop finite/count/time-bounded load only.

The existing fixed HTTP correctness corpus already has the needed owner-authored request vocabulary:

- method;
- path/query;
- bounded headers;
- inline or bounded file-backed body;
- expected externally observable HTTP status;
- immutable corpus/content identity.

Current SynVoid owner export at `dbowm91/synvoid@30e68af8f6e79ce0fe07f0c1871f1d0caa6be6f2` now includes a 15-case v1 corpus, including a benign JSON POST body, and records a neutral User-Agent transport requirement. Unsupported smuggling/hop-by-hop/binary/multipart/internal-only fixtures remain explicitly excluded.

Current upstream oha supports arbitrary method/headers, inline/file bodies, URL files, JSON output, explicit HTTP versions, and `--disable-keepalive`. Eggbench's oha adapter exposes only the current simple URL/load geometry. Therefore independent body/churn qualification can be added later by extending Eggbench's adapter; no upstream oha change is required.

## 3. New workload contract

Add a new plan-schema workload form rather than overloading the existing GET-only variants.

Recommended shape:

~~~json
{
  "kind": "http_corpus",
  "target": "synvoid",
  "corpus_ref": "corpus.json",
  "corpus_sha256": "...",
  "schedule": [
    {"case_id": "benign_json_body", "count": 800},
    {"case_id": "xss_percent_encoded", "count": 200}
  ],
  "concurrency": 8,
  "connection_policy": "pooled"
}
~~~

Exact field spelling may follow existing naming conventions.

Requirements:

- workspace-confined corpus loading reuses the existing qualification corpus/path hashing implementation;
- corpus digest is validated before network execution;
- case IDs must exist and be unique within the schedule;
- counts are positive and bounded;
- total requests are bounded;
- schedule is finite for M003 v1;
- request methods/headers/bodies are taken from the immutable owner-authored corpus;
- no absolute target URL comes from the corpus;
- target authority comes only from runtime bindings;
- forbidden headers remain rejected by the corpus contract;
- transport-required neutral defaults may be supplied by a reviewed profile/driver option only when they are comparison-critical and recorded.

Do not add malformed/raw HTTP or request-smuggling support here.

## 4. Deterministic exact mixture scheduling

Do not implement probabilistic per-request sampling for qualification.

For every measured trial:

1. materialize the exact requested multiset;
2. derive a deterministic permutation from the experiment seed + stable trial ordinal + schedule identity;
3. dispatch that exact permutation under the requested concurrency;
4. retain a schedule digest and per-case dispatch counts in method evidence.

Baseline and candidate using the same plan/seed must see the same case multiset and permutation.

Warmup scheduling must be explicit and reproducible. It may reuse the same schedule or a bounded deterministic prefix, but the policy must be versioned and recorded.

## 5. Expected-outcome accounting

A security workload cannot use generic HTTP status failure semantics.

For each completed response classify independently:

- transport/protocol failure;
- HTTP response received;
- expected owner-authored observable outcome matched;
- expected outcome mismatched.

At minimum normalize:

- throughput;
- latency mean/p50/p95/p99 where the current driver supports them;
- transport_error_rate;
- expected_outcome_mismatch_rate;
- total expected-block/pass counts;
- per-status distribution as bounded evidence.

Do not call an expected 403 a transport error or generic workload failure.

Performance metrics and security correctness remain separate gate families:

- performance gates may use throughput/latency/transport error;
- owner-outcome mismatches feed correctness disposition/evidence;
- a fast run with outcome mismatches must not pass the combined qualification.

Reuse the existing correctness policy machinery where possible rather than inventing a third verdict family.

## 6. Connection policy

Add a comparison-critical HTTP connection policy:

- `pooled` — current warm client/pool behavior;
- `fresh_per_request` — every request obtains a fresh physical client/connection according to the driver contract.

Requirements:

- do not implement churn by injecting `Connection: close` as a generic header;
- do not claim H2/H3 fresh-stream behavior is equivalent to fresh TCP/TLS connection behavior;
- native Eggfetch implementation must make physical connection semantics explicit in method evidence;
- connection policy participates in comparability identity;
- changing policy between baseline and candidate makes strict comparison invalid.

M003 v1 may remain cleartext H1 for the SynVoid live profile.

## 7. Native Eggfetch implementation

Extend the native Eggfetch workload driver to execute `http_corpus`.

Requirements:

- use the runtime target authority plus corpus relative request target;
- support owner-authored method, allowed headers, inline body and bounded body file;
- consume the full response body;
- preserve existing cancellation/timeouts;
- record latency from dispatch through body consumption;
- retain no raw security payload artifact beyond already-authorized immutable input identity;
- method evidence records corpus SHA, schedule SHA, connection policy, neutral/default headers, request count, exact Eggfetch version, and target binding identity.

Do not silently fall back to simple GET execution.

## 8. Independent oha extension

Extend the existing oha adapter only for capabilities necessary to independently exercise M003 request/body/churn shapes.

Initial supported subset:

- one corpus case repeated;
- method;
- safe headers;
- inline/file body;
- pooled/default or `--disable-keepalive`;
- finite count and closed-loop concurrency;
- JSON output.

Mixed multi-case scheduling through oha is optional in M003b; native Eggfetch remains the canonical exact-mixture executor unless oha can reproduce the exact sequence deterministically.

Raw oha JSON remains retained beside normalized metrics.

No external-tool option may bypass target confinement or executable provenance.

## 9. Controlled-origin support

Extend the controlled EggServe origin only as needed to support deterministic request-body/per-route assertions.

Prefer one generic bounded request echo/acceptance fixture over a security-aware origin.

The origin must not decide whether a payload is malicious.

## 10. Safety

Security-performance load remains local/private by default.

For any security-owned corpus workload:

- target must resolve through existing local/private confinement policy where required;
- request counts/concurrency/body sizes remain bounded;
- ordinary CI uses small finite campaigns;
- no flood/stress raw-socket behavior is added;
- no credentials, cookies, authorization, proxy authorization, or arbitrary Host override enter the corpus workload v1.

## 11. Tests

Required deterministic tests include:

1. corpus digest mismatch fails before I/O;
2. schedule references unknown case fail;
3. exact case counts are preserved;
4. same seed/trial ordinal yields same sequence;
5. different trial ordinal yields deterministic but distinct permutation when possible;
6. POST JSON body reaches controlled origin intact;
7. expected 403 counts as expected outcome, not transport error;
8. unexpected 200 for an expected block yields correctness failure;
9. pooled method retains connection reuse;
10. fresh-per-request produces fresh physical connections in an instrumented local fixture;
11. baseline/candidate connection-policy mismatch is incomparable;
12. raw payload bytes are not copied into dynamic method/result artifacts;
13. oha body/churn argv construction is exact and bounded;
14. existing GET workload behavior remains unchanged.

## 12. Verification

Use the full workspace/MSRV/four-lane matrix from M003a plus focused live local tests for:

- native pooled corpus load;
- native fresh-connection corpus load;
- POST/body corpus case;
- mixed expected-pass/expected-block corpus;
- oha single-case body/churn oracle where available.

## 13. Acceptance criteria

M003b closes only when:

1. a versioned HTTP corpus workload exists;
2. exact deterministic mixtures are reproducible;
3. body-bearing requests execute through native Eggfetch;
4. expected block/pass outcomes are not conflated with transport failures;
5. pooled/fresh connection policy is explicit and comparison-critical;
6. method evidence binds corpus/schedule/connection policy;
7. at least one independent external tool can exercise body or churn semantics;
8. no scanner/payload-generation semantics move into Eggbench;
9. old GET workloads remain compatible;
10. hosted CI and focused live qualification are green.

## 14. Stop conditions

Stop and re-plan if:

- exact mixed scheduling requires random sampling whose realized mixture differs run-to-run;
- owner expectations would need to be inferred by Eggbench;
- connection churn can only be approximated with hop-by-hop headers;
- request-smuggling/raw malformed HTTP becomes necessary for closure;
- supporting bodies requires retaining unbounded payload/result artifacts.

## 15. Handoff

Blocked on M003a closure.

M003c depends on this plan because target telemetry qualification must be exercised under the richer workload shapes that create queue/event-loop/offload pressure.
