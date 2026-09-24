//! Gregg M001b acceptance tests: endpoint policy, preflight, polling,
//! aggregation, retention, and registry behavior.
//!
//! Everything here is gated behind `gregg`. A tiny test-only HTTP/1.1
//! fixture stands in for `greggd`; production code still uses the Gregg
//! wire types and Eggfetch transport.

#![cfg(feature = "gregg")]

use eggbench_core::{RunId, TrialId};
use eggbench_drivers::gregg::{
    GREGG_NDJSON_ARTIFACT, GREGG_PROVENANCE_ARTIFACT, GreggCollector, RequestedHostMetric,
    gregg_telemetry_descriptor, host_metric_names, validate_endpoint,
};
use eggbench_runner::{
    DrainContext, TelemetryCollector as _, TelemetryPreflightContext, TelemetryRegistry,
    TelemetryTrialContext,
};
use gregg_protocol::v2::{
    CpuMetricsV2, HealthResponseV2, MetricCapabilitiesV2, StatusPayloadV2, StatusSnapshotV2,
};
use gregg_protocol::{MemoryMetrics, SystemIdentity};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

// ---- Tiny deterministic HTTP/1.1 fixture (test-only) ----

type Handler = Arc<dyn Fn(&str) -> (u16, Vec<u8>, Duration) + Send + Sync>;

struct FixtureServer {
    addr: SocketAddr,
    requests: Arc<AtomicUsize>,
    _task: tokio::task::JoinHandle<()>,
}

async fn spawn_fixture(handler: Handler) -> FixtureServer {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let requests = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&requests);
    let task = tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let handler = Arc::clone(&handler);
            let counter = Arc::clone(&counter);
            tokio::spawn(async move {
                serve_one(stream, &handler, &counter).await;
            });
        }
    });
    FixtureServer {
        addr,
        requests,
        _task: task,
    }
}

async fn serve_one(stream: tokio::net::TcpStream, handler: &Handler, counter: &AtomicUsize) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = stream;
    let mut buffer = vec![0u8; 8192];
    let mut received = 0usize;
    let path = loop {
        let Ok(count) = stream.read(&mut buffer[received..]).await else {
            return;
        };
        if count == 0 {
            return;
        }
        received += count;
        if let Some(end) = find_header_end(&buffer[..received]) {
            let head = String::from_utf8_lossy(&buffer[..end]).into_owned();
            break head.lines().next().unwrap_or("").to_owned();
        }
        if received >= buffer.len() {
            return;
        }
    };
    // Request line: `GET /path HTTP/1.1`.
    let route = path.split_whitespace().nth(1).unwrap_or("/").to_owned();
    counter.fetch_add(1, Ordering::SeqCst);
    let (status, body, delay) = handler(&route);
    if !delay.is_zero() {
        tokio::time::sleep(delay).await;
    }
    let reason = match status {
        200 => "OK",
        503 => "Service Unavailable",
        _ => "Error",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes()).await;
    let _ = stream.write_all(&body).await;
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| position + 4)
}

/// Build a valid v2 status payload with tunable fields.
fn status_payload(
    observed_at: u64,
    cpu: f32,
    memory_used: u64,
    with_optionals: bool,
) -> StatusPayloadV2 {
    StatusPayloadV2 {
        snapshot: StatusSnapshotV2 {
            schema_version: 2,
            observed_at_unix_ms: observed_at,
            sample_interval_ms: 1000,
            capabilities: MetricCapabilitiesV2::new(false, false, false, false),
            system: SystemIdentity {
                name: "fixture-host".to_owned(),
                hostname: "fixture-host.local".to_owned(),
                os_name: "linux".to_owned(),
                os_version: "6.0".to_owned(),
                kernel_name: "Linux".to_owned(),
                kernel_release: "6.0.0".to_owned(),
                architecture: "x86_64".to_owned(),
            },
            cpu: CpuMetricsV2 {
                logical_cores: 8,
                usage_pct: cpu,
                iowait_pct: None,
            },
            load: None,
            memory: MemoryMetrics {
                used_bytes: memory_used,
                total_bytes: 16_000_000_000,
                usage_pct: 50.0,
            },
            swap: None,
            commit: None,
        },
        drives: None,
        cpu_frequency_hz: with_optionals.then_some(3_400_000_000),
        disk_io: None,
        network: None,
    }
}

fn ready_health(payload: &StatusPayloadV2) -> Vec<u8> {
    // HealthResponseV2 serializes; Ready requires an embedded snapshot.
    let health = HealthResponseV2::ready(payload.snapshot.clone());
    serde_json::to_vec(&health).expect("health serializes")
}

fn trial_context(trial: u32) -> TelemetryTrialContext {
    TelemetryTrialContext {
        run_id: RunId::new(),
        trial_id: TrialId::new(trial).unwrap(),
        cancellation: CancellationToken::new(),
        timeout: Duration::from_secs(10),
    }
}

fn preflight_context() -> TelemetryPreflightContext {
    TelemetryPreflightContext {
        run_id: RunId::new(),
        cancellation: CancellationToken::new(),
        timeout: Duration::from_secs(10),
    }
}

fn requested(names: &[(&str, &str)]) -> Vec<RequestedHostMetric> {
    names
        .iter()
        .map(|(name, unit)| RequestedHostMetric {
            name: (*name).to_owned(),
            unit: (*unit).to_owned(),
        })
        .collect()
}

// ---- Tests ----

#[test]
fn descriptor_carries_exact_protocol_version_and_fields() {
    assert_ne!(eggbench_drivers::gregg::GREGG_PROTOCOL_VERSION, "unknown");
    let descriptor = gregg_telemetry_descriptor();
    assert_eq!(descriptor.name.as_str(), "gregg");
    assert_eq!(descriptor.upstream_name, "gregg-protocol");
    assert_eq!(
        descriptor.upstream_version.as_deref(),
        Some(eggbench_drivers::gregg::GREGG_PROTOCOL_VERSION)
    );
    assert_eq!(
        descriptor.category,
        eggbench_core::DriverCategory::Telemetry
    );
    assert!(!descriptor.external_process);
    assert!(descriptor.default);
    let mut fields: Vec<String> = descriptor
        .capabilities
        .iter()
        .filter_map(|capability| match capability {
            eggbench_core::Capability::TelemetryField { field } => Some(field.as_str().to_owned()),
            _ => None,
        })
        .collect();
    fields.sort();
    let mut expected = host_metric_names();
    expected.sort();
    assert_eq!(fields, expected);
}

#[test]
fn collector_rejects_unknown_metric_names_and_non_loopback() {
    let error = GreggCollector::new("http://127.0.0.1:11310", requested(&[("host_nope", "x")]))
        .expect_err("unknown metric fails");
    assert!(error.contains("unknown gregg metric"));
    let error =
        GreggCollector::new("http://10.0.0.5:11310", vec![]).expect_err("non-loopback fails");
    assert!(error.contains("not_loopback"));
    assert!(validate_endpoint("http://127.0.0.1:11310").is_ok());
}

#[tokio::test]
async fn preflight_succeeds_and_clamps_poll_interval() {
    let payload = status_payload(1000, 12.5, 8_000_000_000, true);
    let health = ready_health(&payload);
    let status = serde_json::to_vec(&payload).expect("status serializes");
    let server = spawn_fixture(Arc::new(move |route| match route {
        "/v2/healthz" => (200, health.clone(), Duration::ZERO),
        "/v2/status" => (200, status.clone(), Duration::ZERO),
        _ => (404, b"{}".to_vec(), Duration::ZERO),
    }))
    .await;
    let endpoint = format!("http://{}", server.addr);
    let mut collector = GreggCollector::new(&endpoint, vec![]).expect("collector");
    let capability = collector
        .preflight(preflight_context())
        .await
        .expect("preflight succeeds");
    // Daemon cadence 1000ms stays within the 250ms..5s clamp.
    assert_eq!(capability.poll_interval, Duration::from_secs(1));
    assert_eq!(
        capability.identity.get("hostname").map(String::as_str),
        Some("fixture-host.local")
    );
    assert!(server.requests.load(Ordering::SeqCst) >= 2);
}

#[tokio::test]
async fn preflight_rejects_warming_daemon() {
    let health = serde_json::to_vec(&HealthResponseV2::warming()).expect("warming");
    let server = spawn_fixture(Arc::new(move |route| match route {
        "/v2/healthz" => (200, health.clone(), Duration::ZERO),
        _ => (404, b"{}".to_vec(), Duration::ZERO),
    }))
    .await;
    let mut collector =
        GreggCollector::new(&format!("http://{}", server.addr), vec![]).expect("collector");
    let error = collector
        .preflight(preflight_context())
        .await
        .expect_err("warming fails preflight");
    assert_eq!(error.category, "health_unavailable");
}

#[tokio::test]
async fn preflight_rejects_unreachable_daemon() {
    let mut collector = GreggCollector::new("http://127.0.0.1:1", vec![]).expect("collector");
    let error = collector
        .preflight(preflight_context())
        .await
        .expect_err("refused connection fails");
    assert_eq!(error.category, "health_unavailable");
}

#[tokio::test]
async fn preflight_rejects_malformed_and_wrong_schema() {
    // Malformed health JSON.
    let server = spawn_fixture(Arc::new(move |route| match route {
        "/v2/healthz" => (200, b"{not json".to_vec(), Duration::ZERO),
        _ => (404, b"{}".to_vec(), Duration::ZERO),
    }))
    .await;
    let mut collector =
        GreggCollector::new(&format!("http://{}", server.addr), vec![]).expect("collector");
    let error = collector
        .preflight(preflight_context())
        .await
        .expect_err("malformed health fails");
    assert_eq!(error.category, "schema_unsupported");

    // Valid JSON with wrong schema version on status.
    let mut payload = status_payload(1000, 10.0, 1_000, false);
    payload.snapshot.schema_version = 99;
    let status = serde_json::to_vec(&payload).expect("status serializes");
    let health = ready_health(&status_payload(1000, 10.0, 1_000, false));
    let server = spawn_fixture(Arc::new(move |route| match route {
        "/v2/healthz" => (200, health.clone(), Duration::ZERO),
        "/v2/status" => (200, status.clone(), Duration::ZERO),
        _ => (404, b"{}".to_vec(), Duration::ZERO),
    }))
    .await;
    let mut collector =
        GreggCollector::new(&format!("http://{}", server.addr), vec![]).expect("collector");
    let error = collector
        .preflight(preflight_context())
        .await
        .expect_err("wrong schema fails");
    assert_eq!(error.category, "schema_unsupported");
}

#[tokio::test]
async fn trial_collects_samples_and_aggregates() {
    // Sequence with changing CPU/memory; the collector polls at 250ms
    // minimum (daemon says 100ms, clamped up).
    let first = status_payload(1000, 10.0, 4_000_000_000, false);
    let second = status_payload(2000, 30.0, 8_000_000_000, false);
    let first_bytes = serde_json::to_vec(&first).expect("status");
    let second_bytes = serde_json::to_vec(&second).expect("status");
    let health = ready_health(&first);
    let counter = Arc::new(AtomicUsize::new(0));
    let handler_counter = Arc::clone(&counter);
    let server = spawn_fixture(Arc::new(move |route| {
        let seen = handler_counter.fetch_add(1, Ordering::SeqCst);
        match route {
            "/v2/healthz" => (200, health.clone(), Duration::ZERO),
            "/v2/status" => {
                // Alternate snapshots so polling observes both values.
                if seen.is_multiple_of(2) {
                    (200, first_bytes.clone(), Duration::ZERO)
                } else {
                    (200, second_bytes.clone(), Duration::ZERO)
                }
            }
            _ => (404, b"{}".to_vec(), Duration::ZERO),
        }
    }))
    .await;
    let mut collector = GreggCollector::new(
        &format!("http://{}", server.addr),
        requested(&[
            ("host_cpu_percent", "percent"),
            ("host_memory_used_bytes", "bytes"),
            ("host_memory_percent", "percent"),
            ("host_cpu_frequency_hz", "hertz"),
        ]),
    )
    .expect("collector");
    collector
        .preflight(preflight_context())
        .await
        .expect("preflight");
    collector
        .start_trial(trial_context(1))
        .await
        .expect("start");
    tokio::time::sleep(Duration::from_millis(700)).await;
    let output = collector.stop_trial(trial_context(1)).await.expect("stop");
    // Two artifacts: NDJSON series + provenance.
    assert_eq!(output.artifacts.len(), 2);
    assert_eq!(output.artifacts[0].name, GREGG_NDJSON_ARTIFACT);
    assert_eq!(output.artifacts[1].name, GREGG_PROVENANCE_ARTIFACT);
    // Every NDJSON line parses as a valid v2 payload.
    let ndjson = &output.artifacts[0].bytes;
    assert!(!ndjson.is_empty());
    for line in ndjson
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let payload: StatusPayloadV2 = serde_json::from_slice(line).expect("ndjson line parses");
        payload.validate().expect("ndjson line validates");
    }
    // Aggregation: CPU mean of {10, 30}, memory max, frequency absent.
    let value = |name: &str| {
        output
            .metrics
            .iter()
            .find(|metric| metric.name == name)
            .unwrap_or_else(|| panic!("metric {name}"))
            .value
    };
    assert!((value("host_cpu_percent") - 20.0).abs() < 1.0);
    assert!((value("host_memory_used_bytes") - 8_000_000_000.0).abs() < 1.0);
    assert!((value("host_memory_percent") - 50.0).abs() < f64::EPSILON);
    // Absent optional frequency stays missing: no observation emitted.
    assert!(
        output
            .metrics
            .iter()
            .all(|metric| metric.name != "host_cpu_frequency_hz")
    );
    // Provenance names the endpoint without credentials.
    let provenance: serde_json::Value =
        serde_json::from_slice(&output.artifacts[1].bytes).expect("provenance json");
    assert_eq!(provenance["source"], "gregg");
    assert_eq!(provenance["endpoint_host"], "127.0.0.1");
    assert_eq!(provenance["wire_schema_version"], 2);
    // Producer attribution for combined normalization.
    for metric in &output.metrics {
        assert_eq!(metric.producer.as_deref(), Some("gregg"));
        assert_eq!(metric.raw_artifacts, vec![GREGG_NDJSON_ARTIFACT.to_owned()]);
    }
    collector
        .drain(DrainContext {
            run_id: RunId::new(),
            cancellation: CancellationToken::new(),
            timeout: Duration::from_secs(5),
        })
        .await
        .expect("drain");
}

#[tokio::test]
async fn explicit_zero_rates_stay_observed() {
    // A daemon reporting measured zero rates must produce observed zero,
    // not missing evidence.
    let mut payload = status_payload(1000, 0.0, 1_000_000, false);
    payload.disk_io = Some(gregg_protocol::v2::DiskIoPayload {
        aggregate_read_bytes_per_sec: 0,
        aggregate_write_bytes_per_sec: 0,
        devices: Vec::new(),
    });
    let status = serde_json::to_vec(&payload).expect("status");
    let health = ready_health(&payload);
    let server = spawn_fixture(Arc::new(move |route| match route {
        "/v2/healthz" => (200, health.clone(), Duration::ZERO),
        "/v2/status" => (200, status.clone(), Duration::ZERO),
        _ => (404, b"{}".to_vec(), Duration::ZERO),
    }))
    .await;
    let mut collector = GreggCollector::new(
        &format!("http://{}", server.addr),
        requested(&[
            ("host_cpu_percent", "percent"),
            ("host_disk_read_bytes_per_sec", "bytes_per_sec"),
        ]),
    )
    .expect("collector");
    collector
        .preflight(preflight_context())
        .await
        .expect("preflight");
    collector
        .start_trial(trial_context(1))
        .await
        .expect("start");
    let output = collector.stop_trial(trial_context(1)).await.expect("stop");
    let value = |name: &str| {
        output
            .metrics
            .iter()
            .find(|metric| metric.name == name)
            .unwrap_or_else(|| panic!("metric {name}"))
            .value
    };
    assert!(value("host_cpu_percent") < f64::EPSILON);
    assert!(value("host_disk_read_bytes_per_sec") < f64::EPSILON);
}

#[tokio::test]
async fn repeated_timestamps_deduplicate() {
    // Every snapshot carries the same daemon timestamp: aggregation runs
    // over one unique sample and a dedup warning is recorded.
    let payload = status_payload(7777, 25.0, 2_000_000, false);
    let status = serde_json::to_vec(&payload).expect("status");
    let health = ready_health(&payload);
    let server = spawn_fixture(Arc::new(move |route| match route {
        "/v2/healthz" => (200, health.clone(), Duration::ZERO),
        "/v2/status" => (200, status.clone(), Duration::ZERO),
        _ => (404, b"{}".to_vec(), Duration::ZERO),
    }))
    .await;
    let mut collector = GreggCollector::new(
        &format!("http://{}", server.addr),
        requested(&[("host_cpu_percent", "percent")]),
    )
    .expect("collector");
    collector
        .preflight(preflight_context())
        .await
        .expect("preflight");
    collector
        .start_trial(trial_context(1))
        .await
        .expect("start");
    tokio::time::sleep(Duration::from_millis(600)).await;
    let output = collector.stop_trial(trial_context(1)).await.expect("stop");
    let cpu = output
        .metrics
        .iter()
        .find(|metric| metric.name == "host_cpu_percent")
        .expect("cpu metric");
    assert!((cpu.value - 25.0).abs() < f64::EPSILON);
    assert!(
        output
            .warnings
            .iter()
            .any(|warning| warning.category == "gregg_timestamps_deduplicated")
    );
}

#[tokio::test]
async fn poll_count_stays_bounded() {
    // Short trial: at most immediate + final + cadence polls + slack.
    // Daemon cadence 1000ms; a ~1.2s window must not exceed 5 polls.
    let payload = status_payload(1000, 10.0, 1_000, false);
    let status = serde_json::to_vec(&payload).expect("status");
    let health = ready_health(&payload);
    let server = spawn_fixture(Arc::new(move |route| match route {
        "/v2/healthz" => (200, health.clone(), Duration::ZERO),
        "/v2/status" => (200, status.clone(), Duration::ZERO),
        _ => (404, b"{}".to_vec(), Duration::ZERO),
    }))
    .await;
    let before = server.requests.load(Ordering::SeqCst);
    let mut collector = GreggCollector::new(
        &format!("http://{}", server.addr),
        requested(&[("host_cpu_percent", "percent")]),
    )
    .expect("collector");
    collector
        .preflight(preflight_context())
        .await
        .expect("preflight");
    let after_preflight = server.requests.load(Ordering::SeqCst);
    collector
        .start_trial(trial_context(1))
        .await
        .expect("start");
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let output = collector.stop_trial(trial_context(1)).await.expect("stop");
    let trial_polls = server.requests.load(Ordering::SeqCst) - after_preflight;
    let _ = before;
    // immediate + ~1 cadence poll + final, with slack for timing jitter.
    assert!(trial_polls <= 5, "bounded poll count, got {trial_polls}");
    assert!(!output.artifacts[0].bytes.is_empty());
}

#[tokio::test]
async fn cancellation_still_returns_captured_samples() {
    // Cancel mid-window: stop returns the samples captured so far and no
    // polling task remains (a second stop fails with no open window).
    let payload = status_payload(1000, 42.0, 1_000, false);
    let status = serde_json::to_vec(&payload).expect("status");
    let health = ready_health(&payload);
    let server = spawn_fixture(Arc::new(move |route| match route {
        "/v2/healthz" => (200, health.clone(), Duration::ZERO),
        "/v2/status" => (200, status.clone(), Duration::ZERO),
        _ => (404, b"{}".to_vec(), Duration::ZERO),
    }))
    .await;
    let mut collector = GreggCollector::new(
        &format!("http://{}", server.addr),
        requested(&[("host_cpu_percent", "percent")]),
    )
    .expect("collector");
    collector
        .preflight(preflight_context())
        .await
        .expect("preflight");
    let cancel = CancellationToken::new();
    collector
        .start_trial(TelemetryTrialContext {
            run_id: RunId::new(),
            trial_id: TrialId::new(1).unwrap(),
            cancellation: cancel.clone(),
            timeout: Duration::from_secs(10),
        })
        .await
        .expect("start");
    tokio::time::sleep(Duration::from_millis(400)).await;
    cancel.cancel();
    let output = collector
        .stop_trial(trial_context(1))
        .await
        .expect("stop returns samples after cancel");
    assert!(
        output
            .metrics
            .iter()
            .any(|metric| metric.name == "host_cpu_percent")
    );
    // No polling task leaks: the window is closed.
    let error = collector
        .stop_trial(trial_context(1))
        .await
        .expect_err("second stop fails");
    assert_eq!(error.category, "polling_failed");
}

#[test]
fn duplicate_telemetry_source_registration_rejected() {
    let mut registry = TelemetryRegistry::new();
    registry
        .register(Box::new(
            GreggCollector::new("http://127.0.0.1:11310", vec![]).expect("collector"),
        ))
        .expect("first registration");
    let error = registry
        .register(Box::new(
            GreggCollector::new("http://127.0.0.1:11311", vec![]).expect("collector"),
        ))
        .expect_err("duplicate rejected");
    assert!(error.contains("gregg"));
}

#[test]
fn host_metric_names_match_plan_vocabulary() {
    // Eight names, stable and distinct from workload metrics.
    let names = host_metric_names();
    assert_eq!(names.len(), 8);
    let mut sorted = names.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), 8);
}
