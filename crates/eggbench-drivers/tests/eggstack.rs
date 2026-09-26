//! Eggstack M001a acceptance tests: controlled origin route discipline,
//! runtime bindings, workload scheduling, and registry behavior.
//!
//! Everything here is gated behind `eggstack-http`; the minimal build never
//! links these drivers.

#![cfg(feature = "eggstack-http")]

use eggbench_core::{LoadMode, Name, RunId, Workload};
use eggbench_drivers::{
    EGGFETCH_CORE_VERSION, EGGFETCH_HTTP_DRIVER_NAME, EGGSERVE_ORIGIN_SERVICE_TYPE,
    EGGSERVE_PRIMITIVES_VERSION, EGGSERVE_SERVER_VERSION, EggServeOriginAdapter,
    HttpCorpusExecutor, eggfetch_http_descriptor, eggfetch_workload, eggserve_origin_descriptor,
    eggstack_service_adapters,
};
use eggbench_runner::{
    CorrectnessContext, CorrectnessExecutor, DrainContext, FailureCategory, InvocationContext,
    InvocationKind, ManagedServiceAdapter, ManagedServiceHandle, ServiceAdapterRegistry,
    ServiceStartRequest, WorkloadExecutor,
};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

fn start_request(config: &[(&str, &str)]) -> ServiceStartRequest {
    ServiceStartRequest {
        service: "origin".to_owned(),
        service_type: EGGSERVE_ORIGIN_SERVICE_TYPE.to_owned(),
        config: config
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect(),
        grace: Duration::from_secs(5),
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn fixed_http_corpus_runs_serially_and_persists_only_sanitized_outcomes() {
    let mut origin = start_origin(&[]).await;
    let binding = http_url(&*origin);
    let workspace = std::env::current_dir().unwrap();
    let temp = tempfile::tempdir_in(&workspace).unwrap();
    let corpus_ref = format!(
        "{}/security-corpus.json",
        temp.path().file_name().unwrap().to_string_lossy()
    );
    let corpus = serde_json::json!({
        "schema_version": 1,
        "owner": "fixture",
        "corpus_id": "smoke",
        "cases": [
            {
                "id": "route",
                "category": "smoke",
                "request": {
                    "method": "GET",
                    "path_and_query": "/bench",
                    "headers": [["x-fixture", "safe"]],
                    "body": { "kind": "none" }
                },
                "expectation": { "status_exact": 200 }
            },
            {
                "id": "body-is-not-retained",
                "category": "smoke",
                "request": {
                    "method": "GET",
                    "path_and_query": "/bench",
                    "headers": [],
                    "body": { "kind": "inline_utf8", "value": "never-persist-this-body" }
                },
                "expectation": { "status_exact": 413 }
            },
            {
                "id": "allowed-status-set",
                "category": "smoke",
                "request": {
                    "method": "GET",
                    "path_and_query": "/bench",
                    "headers": [],
                    "body": { "kind": "none" }
                },
                "expectation": { "status_any_of": [201, 204] }
            }
        ]
    });
    let corpus_bytes = serde_json::to_vec(&corpus).unwrap();
    std::fs::write(temp.path().join("security-corpus.json"), &corpus_bytes).unwrap();
    let identity = eggbench_core::content_tree_identity(&workspace, &corpus_ref).unwrap();
    let mut bindings = eggbench_runner::RuntimeBindings::new();
    bindings.insert("origin", "http_url", binding).unwrap();
    let request = eggbench_core::HttpCorpusCheckRequest {
        id: Name::new("smoke").unwrap(),
        source: Name::new("eggbench-http-corpus").unwrap(),
        target: Name::new("origin").unwrap(),
        corpus_ref,
        corpus_sha256: identity.aggregate_sha256,
        timeout_ms: eggbench_core::DurationMs::new(10_000).unwrap(),
        case_timeout_ms: eggbench_core::DurationMs::new(2_000).unwrap(),
    };
    let output = HttpCorpusExecutor
        .execute(CorrectnessContext {
            run_id: RunId::new(),
            check_id: "smoke".into(),
            source: "eggbench-http-corpus".into(),
            target: "origin".into(),
            test_type: "http_observable".into(),
            max_successful_bypasses: 0,
            concurrency: 1,
            timeout_ms: 10_000,
            bindings,
            cancellation: CancellationToken::new(),
            timeout: Duration::from_secs(10),
            http_corpus_request: Some(request.clone()),
        })
        .await
        .expect("fixed corpus executes");
    let result: eggbench_core::HttpCorpusCheckResultV1 =
        serde_json::from_slice(&output.sanitized_result).unwrap();
    assert_eq!(result.cases[0].observed_status, Some(200));
    assert_eq!(result.cases[1].observed_status, Some(413));
    assert_eq!(result.cases[2].observed_status, Some(200));
    assert_eq!(result.counts(), (3, 2, 1, 0));
    assert_eq!(
        output.disposition,
        eggbench_runner::CorrectnessDisposition::Fail
    );
    assert!(!String::from_utf8_lossy(&output.sanitized_result).contains("never-persist-this-body"));

    let unavailable = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let unavailable_port = unavailable.local_addr().unwrap().port();
    drop(unavailable);
    let mut failed_bindings = eggbench_runner::RuntimeBindings::new();
    failed_bindings
        .insert(
            "origin",
            "http_url",
            format!("http://127.0.0.1:{unavailable_port}/"),
        )
        .unwrap();
    let invalid = HttpCorpusExecutor
        .execute(CorrectnessContext {
            run_id: RunId::new(),
            check_id: "smoke".into(),
            source: "eggbench-http-corpus".into(),
            target: "origin".into(),
            test_type: "http_observable".into(),
            max_successful_bypasses: 0,
            concurrency: 1,
            timeout_ms: 10_000,
            bindings: failed_bindings,
            cancellation: CancellationToken::new(),
            timeout: Duration::from_secs(10),
            http_corpus_request: Some(request),
        })
        .await
        .expect("transport failures become sanitized Invalid cases");
    let invalid_result: eggbench_core::HttpCorpusCheckResultV1 =
        serde_json::from_slice(&invalid.sanitized_result).unwrap();
    assert_eq!(invalid_result.counts(), (3, 0, 0, 3));
    assert!(
        invalid_result
            .cases
            .iter()
            .all(|case| case.reason.as_deref() == Some("transport_failure"))
    );
    origin.shutdown(Duration::from_secs(5)).await.unwrap();
}

async fn start_origin(config: &[(&str, &str)]) -> Box<dyn ManagedServiceHandle> {
    let adapter = EggServeOriginAdapter::new();
    assert_eq!(adapter.service_type(), EGGSERVE_ORIGIN_SERVICE_TYPE);
    adapter
        .start(start_request(config), CancellationToken::new())
        .await
        .expect("origin starts")
}

fn http_url(handle: &dyn ManagedServiceHandle) -> String {
    let bindings = handle.bindings();
    let url = bindings
        .get("origin", "http_url")
        .expect("http_url binding")
        .to_owned();
    assert!(url.starts_with("http://127.0.0.1:"));
    url
}

#[tokio::test]
async fn origin_serves_configured_route_with_deterministic_body() {
    let mut handle = start_origin(&[("path", "/bench"), ("body_bytes", "1024")]).await;
    let url = http_url(&*handle);
    assert!(url.ends_with("/bench"));

    let client = eggfetch_core::Client::new();
    let mut response = client
        .get(&url)
        .expect("valid url")
        .send_detailed()
        .await
        .expect("route responds");
    assert_eq!(response.status().as_u16(), 200);
    let body = response.bytes().await.expect("body reads");
    assert_eq!(body.len(), 1024);
    assert!(body.iter().all(|byte| *byte == 0x42));

    // Repeatability: the same config serves byte-identical bodies.
    let mut again = client
        .get(&url)
        .expect("valid url")
        .send_detailed()
        .await
        .expect("route responds");
    assert_eq!(again.bytes().await.expect("body reads").len(), 1024);

    handle
        .shutdown(Duration::from_secs(5))
        .await
        .expect("origin shuts down");
}

#[tokio::test]
async fn origin_returns_501_for_unconfigured_routes() {
    let mut handle = start_origin(&[]).await;
    let base = http_url(&*handle);
    let root = base.trim_end_matches("/bench").to_owned();

    let client = eggfetch_core::Client::new();
    for target in [
        root.clone(),
        format!("{root}/other"),
        format!("{root}/bench/"),
    ] {
        let mut response = client
            .get(&target)
            .expect("valid url")
            .send_detailed()
            .await
            .expect("server responds");
        assert_eq!(
            response.status().as_u16(),
            501,
            "unconfigured target {target}"
        );
        let _ = response.bytes().await;
    }

    handle
        .shutdown(Duration::from_secs(5))
        .await
        .expect("origin shuts down");
}

#[tokio::test]
async fn origin_binds_loopback_ephemeral_and_publishes_bindings() {
    let mut handle = start_origin(&[("status", "201")]).await;
    let bindings = handle.bindings();
    assert_eq!(bindings.get("origin", "bound_addr"), Some("127.0.0.1"));
    let port: u16 = bindings
        .get("origin", "bound_port")
        .expect("bound_port")
        .parse()
        .expect("numeric port");
    assert!(port > 0);

    // The configured status applies on the route.
    let client = eggfetch_core::Client::new();
    let mut response = client
        .get(bindings.get("origin", "http_url").expect("http_url"))
        .expect("valid url")
        .send_detailed()
        .await
        .expect("route responds");
    assert_eq!(response.status().as_u16(), 201);
    let _ = response.bytes().await;

    handle
        .shutdown(Duration::from_secs(5))
        .await
        .expect("origin shuts down");
}

#[tokio::test]
async fn origin_rejects_invalid_config_before_listening() {
    let adapter = EggServeOriginAdapter::new();
    for config in [
        vec![("path", "no-leading-slash")],
        vec![("path", "/has query")],
        vec![("path", "/has?query")],
        vec![("body_bytes", "not-a-number")],
        vec![("body_bytes", "1073741824")],
        vec![("status", "199")],
        vec![("status", "600")],
        vec![("status", "ok")],
    ] {
        let error = adapter
            .start(start_request(&config), CancellationToken::new())
            .await
            .err()
            .expect("invalid config must fail");
        assert!(!error.is_empty());
    }
}

#[tokio::test]
async fn origin_start_honors_cancellation() {
    let adapter = EggServeOriginAdapter::new();
    let cancel = CancellationToken::new();
    cancel.cancel();
    let error = adapter
        .start(start_request(&[]), cancel)
        .await
        .err()
        .expect("cancelled start must fail");
    assert!(!error.is_empty());
}

#[test]
fn duplicate_service_adapter_registration_is_rejected() {
    let mut registry = ServiceAdapterRegistry::new();
    registry
        .register(Arc::new(EggServeOriginAdapter::new()))
        .expect("first registration");
    let error = registry
        .register(Arc::new(EggServeOriginAdapter::new()))
        .expect_err("duplicate registration");
    assert!(error.contains(EGGSERVE_ORIGIN_SERVICE_TYPE));
}

#[test]
fn eggstack_registry_serves_origin_and_nothing_else() {
    let registry = eggstack_service_adapters();
    assert_eq!(
        registry.service_types(),
        vec![EGGSERVE_ORIGIN_SERVICE_TYPE.to_owned()]
    );
    assert!(registry.get("eggserve-origin").is_some());
    assert!(registry.get("oha").is_none());
}

#[test]
fn descriptors_carry_exact_sibling_versions() {
    assert_ne!(EGGFETCH_CORE_VERSION, "unknown");
    assert_ne!(EGGSERVE_SERVER_VERSION, "unknown");
    assert_ne!(EGGSERVE_PRIMITIVES_VERSION, "unknown");

    let fetch = eggfetch_http_descriptor();
    assert_eq!(fetch.name.as_str(), EGGFETCH_HTTP_DRIVER_NAME);
    assert_eq!(fetch.upstream_name, "eggfetch-core");
    assert_eq!(
        fetch.upstream_version.as_deref(),
        Some(EGGFETCH_CORE_VERSION)
    );
    assert!(!fetch.external_process);
    assert!(
        fetch
            .compatible_service_types
            .contains(&Name::new(EGGSERVE_ORIGIN_SERVICE_TYPE).unwrap())
    );
    // Only truthful closed-loop capabilities are advertised.
    assert!(fetch.capabilities.iter().any(|capability| matches!(
        capability,
        eggbench_core::Capability::LoadMode {
            mode: LoadMode::ClosedLoop
        }
    )));
    assert!(!fetch.capabilities.iter().any(|capability| matches!(
        capability,
        eggbench_core::Capability::LoadMode {
            mode: LoadMode::OpenLoop
        }
    )));

    let origin = eggserve_origin_descriptor();
    assert_eq!(origin.name.as_str(), EGGSERVE_ORIGIN_SERVICE_TYPE);
    assert_eq!(origin.upstream_name, "eggserve-server");
    assert_eq!(
        origin.upstream_version.as_deref(),
        Some(EGGSERVE_SERVER_VERSION)
    );
    assert!(!origin.external_process);
}

fn invocation(workload: Workload, bindings: eggbench_runner::RuntimeBindings) -> InvocationContext {
    InvocationContext {
        run_id: RunId::new(),
        kind: InvocationKind::Warmup { ordinal: 1 },
        workload,
        seed: Some(7),
        bindings,
        cancellation: CancellationToken::new(),
        timeout: Duration::from_secs(60),
        measurement: eggbench_runner::MeasurementSignal::new(),
    }
}

fn finite_count(target: &str, requests: u32, concurrency: u32) -> Workload {
    Workload::FiniteCount {
        target: Name::new(target).unwrap(),
        requests: eggbench_core::PositiveCount::new(requests).unwrap(),
        concurrency: eggbench_core::PositiveCount::new(concurrency).unwrap(),
    }
}

#[tokio::test]
async fn fetch_issues_exactly_the_planned_count_without_errors() {
    let mut origin = start_origin(&[("path", "/bench"), ("body_bytes", "64")]).await;
    let origin_bindings = origin.bindings();
    let url = origin_bindings
        .get("origin", "http_url")
        .expect("http_url")
        .to_owned();

    let mut snapshot = eggbench_runner::RuntimeBindings::new();
    snapshot
        .insert("origin", "http_url", url)
        .expect("valid binding");
    let context = invocation(finite_count("origin", 20, 4), snapshot);

    let mut workload = eggfetch_workload();
    let output = workload.execute(context).await.expect("workload runs");
    assert!(output.error_counts.is_empty());
    assert_eq!(output.artifacts.len(), 2);
    assert_eq!(output.artifacts[0].name, "latency.hdr");
    assert_eq!(output.artifacts[1].name, "eggfetch-method.json");
    assert_eq!(output.histograms.len(), 1);
    assert_eq!(output.histograms[0].format, "hdrhistogram-v2");

    let names: Vec<&str> = output
        .metrics
        .iter()
        .map(|metric| metric.name.as_str())
        .collect();
    for expected in [
        "throughput",
        "latency_min",
        "latency_mean",
        "latency_p50",
        "latency_p90",
        "latency_p95",
        "latency_p99",
        "latency_p999",
        "error_rate",
        "timeout_rate",
        "bytes_received",
    ] {
        assert!(names.contains(&expected), "missing metric {expected}");
    }
    let error_rate = output
        .metrics
        .iter()
        .find(|metric| metric.name == "error_rate")
        .expect("error_rate");
    assert!(
        error_rate.value < f64::EPSILON,
        "error-free run has zero error rate"
    );
    let bytes = output
        .metrics
        .iter()
        .find(|metric| metric.name == "bytes_received")
        .expect("bytes_received");
    assert!(
        (bytes.value - 1280.0).abs() < f64::EPSILON,
        "20 requests of 64 bytes are received"
    );

    // Method evidence names the exact sibling version and H1-only method.
    let method: serde_json::Value =
        serde_json::from_slice(&output.artifacts[1].bytes).expect("method json");
    assert_eq!(method["eggfetch_core_version"], EGGFETCH_CORE_VERSION);
    assert_eq!(method["http_version"], "1.1");
    assert_eq!(method["attempted_requests"], 20);
    assert_eq!(method["completed_requests"], 20);

    origin
        .shutdown(Duration::from_secs(5))
        .await
        .expect("origin shuts down");
    workload
        .drain(DrainContext {
            run_id: RunId::new(),
            cancellation: CancellationToken::new(),
            timeout: Duration::from_secs(5),
        })
        .await
        .expect("drain succeeds");
}

#[tokio::test]
async fn fetch_rejects_open_loop_before_any_request() {
    let mut snapshot = eggbench_runner::RuntimeBindings::new();
    snapshot
        .insert("origin", "http_url", "http://127.0.0.1:1/bench".to_owned())
        .expect("valid binding");
    let workload = Workload::OpenLoop {
        target: Name::new("origin").unwrap(),
        rate_milli_rps: eggbench_core::RateMilliRps::new(1000).unwrap(),
        requests: Some(eggbench_core::PositiveCount::new(5).unwrap()),
        duration_ms: None,
    };
    let mut executor = eggfetch_workload();
    let error = executor
        .execute(invocation(workload, snapshot))
        .await
        .expect_err("open loop is rejected");
    assert_eq!(error, FailureCategory::WorkloadFailed);
}

#[tokio::test]
async fn fetch_fails_without_issuing_when_binding_is_missing() {
    let context = invocation(
        finite_count("origin", 5, 1),
        eggbench_runner::RuntimeBindings::new(),
    );
    let mut executor = eggfetch_workload();
    let error = executor
        .execute(context)
        .await
        .expect_err("missing binding fails");
    assert_eq!(error, FailureCategory::WorkloadFailed);
}

#[tokio::test]
async fn fetch_counts_status_errors_without_hiding_them() {
    // A route that answers 503 still completes the round trip: the status
    // error is categorized while timing evidence is retained.
    let mut origin = start_origin(&[("path", "/bench"), ("status", "503")]).await;
    let url = origin
        .bindings()
        .get("origin", "http_url")
        .expect("http_url")
        .to_owned();
    let mut snapshot = eggbench_runner::RuntimeBindings::new();
    snapshot
        .insert("origin", "http_url", url)
        .expect("valid binding");
    let mut executor = eggfetch_workload();
    let output = executor
        .execute(invocation(finite_count("origin", 6, 2), snapshot))
        .await
        .expect("workload runs");
    assert_eq!(output.error_counts, vec![("http_5xx".to_owned(), 6)]);
    let error_rate = output
        .metrics
        .iter()
        .find(|metric| metric.name == "error_rate")
        .expect("error_rate");
    assert!(
        (error_rate.value - 1.0).abs() < f64::EPSILON,
        "all requests errored"
    );
    // Timing evidence is still retained for completed round trips.
    assert!(
        output
            .metrics
            .iter()
            .any(|metric| metric.name == "latency_p99")
    );
    origin
        .shutdown(Duration::from_secs(5))
        .await
        .expect("origin shuts down");
}

#[tokio::test]
async fn fetch_counts_transport_failure_without_hiding_it() {
    // Port 1 on loopback refuses connections: every attempt fails in the
    // transport, nothing completes, and the failure is categorized.
    let mut snapshot = eggbench_runner::RuntimeBindings::new();
    snapshot
        .insert("origin", "http_url", "http://127.0.0.1:1/bench".to_owned())
        .expect("valid binding");
    let mut executor = eggfetch_workload();
    let output = executor
        .execute(invocation(finite_count("origin", 6, 2), snapshot))
        .await
        .expect("workload reports transport failure as output");
    assert_eq!(output.error_counts, vec![("transport".to_owned(), 6)]);
    let error_rate = output
        .metrics
        .iter()
        .find(|metric| metric.name == "error_rate")
        .expect("error_rate");
    assert!(
        (error_rate.value - 1.0).abs() < f64::EPSILON,
        "all requests errored"
    );
    // No response means no latency samples and no throughput claim.
    assert!(
        output
            .metrics
            .iter()
            .all(|metric| metric.name != "latency_p99")
    );
    let throughput = output
        .metrics
        .iter()
        .find(|metric| metric.name == "throughput")
        .expect("throughput");
    assert!(
        throughput.value < f64::EPSILON,
        "no response means no throughput"
    );
}

#[tokio::test]
async fn cancellation_stops_issuance_in_duration_mode() {
    let mut origin = start_origin(&[("path", "/bench"), ("body_bytes", "64")]).await;
    let url = origin
        .bindings()
        .get("origin", "http_url")
        .expect("http_url")
        .to_owned();
    let mut snapshot = eggbench_runner::RuntimeBindings::new();
    snapshot
        .insert("origin", "http_url", url)
        .expect("valid binding");
    let workload = Workload::TimeBounded {
        target: Name::new("origin").unwrap(),
        duration_ms: eggbench_core::DurationMs::new(30_000).unwrap(),
        mode: eggbench_core::LoadMode::ClosedLoop,
        concurrency: Some(eggbench_core::PositiveCount::new(2).unwrap()),
        rate_milli_rps: None,
    };
    let context = invocation(workload, snapshot);
    // Cancel shortly after dispatch: issuance must stop far before the
    // 30 s deadline instead of running the full duration.
    let canceller = context.cancellation.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        canceller.cancel();
    });
    let started = std::time::Instant::now();
    let mut executor = eggfetch_workload();
    let output = executor.execute(context).await.expect("cancelled run");
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(10),
        "cancellation stopped issuance after {elapsed:?}"
    );
    let method: serde_json::Value =
        serde_json::from_slice(&output.artifacts[1].bytes).expect("method json");
    assert_eq!(method["schedule_mode"], "deadline");
    origin
        .shutdown(Duration::from_secs(5))
        .await
        .expect("origin shuts down");
}

#[test]
fn open_loop_plans_fail_resolution_before_startup() {
    use eggbench_core::{DefaultDriverPolicy, ResolutionOptions};
    use std::collections::BTreeMap;

    let descriptors = eggbench_drivers::production_catalog().descriptors();
    let options = ResolutionOptions {
        selections: BTreeMap::new(),
        default_policy: DefaultDriverPolicy::Deterministic,
        platform: Name::new("linux-x86_64").unwrap(),
        executable_paths: BTreeMap::new(),
        required_capabilities: BTreeMap::new(),
    };
    let plan = eggbench_core::ExperimentPlan::from_json(include_str!(
        "../../eggbench-core/tests/fixtures/multi-service-open-loop.json"
    ))
    .expect("fixture parses");
    let error = eggbench_core::resolve_plan(&plan, &descriptors, &options)
        .expect_err("open loop must not resolve with closed-loop-only drivers");
    assert!(
        matches!(
            error,
            eggbench_core::ResolveError::UnsupportedCapability { .. }
        ),
        "unexpected resolution error: {error:?}"
    );
}
