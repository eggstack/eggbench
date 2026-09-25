//! Eggstack M002 path acceptance and security tests.

#![cfg(feature = "eggstack-path")]

use eggbench_core::{
    DurationMs, Name, PositiveCount, ResolvedDriver, ResolvedNetworkPath, ResolvedStreamFaults,
    RouteMode, RouteRequest, RunId, StreamFaultKind, StreamFaultPlanRequest, StreamFaultRequest,
};
use eggbench_drivers::eggstack::path::{
    build_fault_plan, fault_descriptor, lower_dialer, new_path_diagnostics, route_descriptor,
};
use eggbench_drivers::{EGGSERVE_ORIGIN_SERVICE_TYPE, EggServeOriginAdapter, EggfetchWorkload};
use eggbench_runner::{
    InvocationContext, InvocationKind, ManagedServiceAdapter, ManagedServiceHandle,
    RuntimeBindings, WorkloadExecutor,
};
use eggchaos_core::{ChaosStream, Direction, FaultKind};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

fn resolved_path(
    mode: RouteMode,
    stream_faults: Option<StreamFaultPlanRequest>,
) -> ResolvedNetworkPath {
    let route_driver = route_descriptor();
    let fault_driver = fault_descriptor();
    ResolvedNetworkPath {
        route: RouteRequest {
            driver: route_driver.name.clone(),
            mode,
        },
        route_driver: ResolvedDriver {
            descriptor: route_driver,
            executable_path: None,
        },
        semantics_version: eggbench_core::NETWORK_PATH_SEMANTICS_VERSION.to_owned(),
        stream_faults: stream_faults.map(|request| ResolvedStreamFaults {
            request,
            fault_driver: ResolvedDriver {
                descriptor: fault_driver,
                executable_path: None,
            },
            rng_version: eggbench_core::NETWORK_PATH_RNG_VERSION.to_owned(),
        }),
    }
}

fn fault(id: &str, kind: StreamFaultKind) -> StreamFaultRequest {
    StreamFaultRequest {
        id: Name::new(id).expect("fault id"),
        kind,
    }
}

fn positive(value: u32) -> PositiveCount {
    PositiveCount::new(value).expect("positive count")
}

fn duration(value: u64) -> DurationMs {
    DurationMs::new(value).expect("duration")
}

fn latency_faults() -> StreamFaultPlanRequest {
    StreamFaultPlanRequest {
        driver: fault_descriptor().name,
        upstream: vec![fault(
            "upstream-latency",
            StreamFaultKind::Latency {
                delay_ms: duration(1),
                jitter_ms: duration(1),
                max_buffer_bytes: positive(4096),
            },
        )],
        downstream: Vec::new(),
    }
}

async fn start_origin() -> Box<dyn ManagedServiceHandle> {
    EggServeOriginAdapter::new()
        .start(
            eggbench_runner::ServiceStartRequest {
                service: "origin".to_owned(),
                service_type: EGGSERVE_ORIGIN_SERVICE_TYPE.to_owned(),
                config: [
                    ("path".to_owned(), "/bench".to_owned()),
                    ("body_bytes".to_owned(), "64".to_owned()),
                ]
                .into_iter()
                .collect(),
                grace: Duration::from_secs(5),
            },
            CancellationToken::new(),
        )
        .await
        .expect("EggServe origin starts")
}

fn origin_url(handle: &dyn ManagedServiceHandle) -> String {
    handle
        .bindings()
        .get("origin", "http_url")
        .expect("origin URL")
        .to_owned()
}

async fn start_counting_origin() -> (String, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind counting origin");
    let address = listener.local_addr().expect("origin address");
    let accepted = Arc::new(AtomicUsize::new(0));
    let task_accepted = Arc::clone(&accepted);
    let task = tokio::spawn(async move {
        let Ok((mut stream, _)) = listener.accept().await else {
            return;
        };
        task_accepted.fetch_add(1, Ordering::SeqCst);
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request).await;
        let _ = stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .await;
    });
    (format!("http://{address}/bench"), accepted, task)
}

async fn request_with_path(
    url: &str,
    dialer: Arc<eggbench_drivers::eggstack::path::EggstackPathDialer>,
) {
    let client = eggfetch_core::Client::builder().dialer(dialer).build();
    let mut response = client
        .get(url)
        .expect("valid URL")
        .send_detailed()
        .await
        .expect("path request succeeds");
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(response.bytes().await.expect("body reads").len(), 64);
}

async fn start_eggress_proxies() -> eggress_embed::EggressHandle {
    eggress_embed::EggressService::from_toml_str(
        r#"
version = 1
[[listeners]]
name = "http"
bind = "127.0.0.1:0"
protocols = ["http"]
[[listeners]]
name = "socks"
bind = "127.0.0.1:0"
protocols = ["socks5"]
"#,
    )
    .expect("Eggress fixture config")
    .start()
    .await
    .expect("Eggress fixture starts")
}

#[tokio::test]
async fn direct_http_socks_and_multi_hop_routes_carry_real_http_traffic() {
    let mut origin = start_origin().await;
    let url = origin_url(&*origin);
    let proxy = start_eggress_proxies().await;
    let addresses = proxy.bound_addresses();
    let http = addresses.listener("http").expect("HTTP listener");
    let socks = addresses.listener("socks").expect("SOCKS listener");
    let chains = [
        (format!("http://{http}"), 1_u16),
        (format!("socks5://{socks}"), 1),
        (format!("http://{http}__socks5://{socks}"), 2),
    ];
    for (chain, expected_hops) in chains {
        let resolved = resolved_path(
            RouteMode::ProxyChain {
                chain: chain.clone(),
            },
            None,
        );
        let dialer = Arc::new(
            lower_dialer(
                &resolved,
                None,
                new_path_diagnostics(),
                Duration::from_secs(5),
            )
            .expect("route lowers"),
        );
        request_with_path(&url, Arc::clone(&dialer)).await;
        let evidence = dialer.network_path_evidence();
        evidence.validate().expect("valid route evidence");
        assert_eq!(evidence.configured_hop_count, expected_hops);
        assert_eq!(evidence.redacted_chain.as_deref(), Some(chain.as_str()));
        assert_eq!(
            evidence.diagnostics.max_observed_hop_count,
            u64::from(expected_hops)
        );
        assert!(evidence.diagnostics.successful_dials > 0);
    }
    proxy.shutdown().await.expect("proxy fixture shuts down");
    origin
        .shutdown(Duration::from_secs(5))
        .await
        .expect("origin shuts down");
}

#[tokio::test]
async fn unavailable_requested_proxy_never_falls_back_to_reachable_origin() {
    let (url, accepted, origin_task) = start_counting_origin().await;
    let unavailable = std::net::TcpListener::bind("127.0.0.1:0")
        .expect("reserve port")
        .local_addr()
        .expect("reserved address");
    let resolved = resolved_path(
        RouteMode::ProxyChain {
            chain: format!("http://{unavailable}"),
        },
        None,
    );
    let dialer = Arc::new(
        lower_dialer(
            &resolved,
            None,
            new_path_diagnostics(),
            Duration::from_millis(500),
        )
        .expect("route lowers"),
    );
    let client = eggfetch_core::Client::builder()
        .dialer(Arc::clone(&dialer))
        .build();
    let result = client.get(&url).expect("valid URL").send_detailed().await;
    assert!(result.is_err(), "unavailable proxy must fail the request");
    let evidence = dialer.network_path_evidence();
    evidence.validate().expect("valid failure evidence");
    let serialized = serde_json::to_string(&evidence).expect("evidence serializes");
    let failure_label = evidence
        .diagnostics
        .route_failures
        .keys()
        .next()
        .expect("typed route failure");
    assert!(failure_label.contains(':'));
    assert!(serialized.contains(failure_label));
    assert_eq!(accepted.load(Ordering::SeqCst), 0);
    assert_eq!(evidence.diagnostics.physical_dial_attempts, 1);
    assert_eq!(evidence.diagnostics.successful_dials, 0);
    assert!(!evidence.diagnostics.route_failures.is_empty());
    origin_task.abort();
    let _ = origin_task.await;
}

#[tokio::test]
async fn eggfetch_eggress_eggchaos_eggserve_composition_records_static_faults() {
    let mut origin = start_origin().await;
    let url = origin_url(&*origin);
    let resolved = resolved_path(RouteMode::Direct, Some(latency_faults()));
    let dialer = Arc::new(
        lower_dialer(
            &resolved,
            Some(19),
            new_path_diagnostics(),
            Duration::from_secs(5),
        )
        .expect("path lowers"),
    );
    request_with_path(&url, Arc::clone(&dialer)).await;
    let evidence = dialer.network_path_evidence();
    evidence.validate().expect("valid path evidence");
    assert_eq!(evidence.diagnostics.fault_wrapped_connections, 1);
    assert_eq!(
        evidence
            .stream_faults
            .as_ref()
            .expect("faults")
            .seed_namespace,
        Some(19)
    );
    let json = serde_json::to_string(&evidence).expect("evidence serializes");
    assert!(!json.contains("packet"));
    assert!(!json.contains("datagram"));

    let mut tampered = evidence.clone();
    tampered.eggress_uri_version.clear();
    assert!(tampered.validate().is_err());
    let mut tampered = evidence.clone();
    tampered.chain_config_digest = Some("0".repeat(64));
    assert!(tampered.validate().is_err());
    let mut tampered = evidence;
    tampered.diagnostics.successful_dials += 1;
    assert!(tampered.validate().is_err());

    origin
        .shutdown(Duration::from_secs(5))
        .await
        .expect("origin shuts down");
}

#[tokio::test]
async fn one_client_reuses_faulted_physical_connections_across_invocations() {
    let mut origin = start_origin().await;
    let url = origin_url(&*origin);
    let resolved = resolved_path(RouteMode::Direct, Some(latency_faults()));
    let dialer = Arc::new(
        lower_dialer(
            &resolved,
            Some(23),
            new_path_diagnostics(),
            Duration::from_secs(5),
        )
        .expect("path lowers"),
    );
    let mut workload = EggfetchWorkload::with_path_dialer(Arc::clone(&dialer));
    let mut bindings = RuntimeBindings::new();
    bindings
        .insert("origin", "http_url", url)
        .expect("valid binding");
    let context = |kind| InvocationContext {
        run_id: RunId::new(),
        kind,
        workload: eggbench_core::Workload::FiniteCount {
            target: Name::new("origin").expect("target"),
            requests: positive(20),
            concurrency: positive(4),
        },
        seed: Some(23),
        bindings: bindings.clone(),
        cancellation: CancellationToken::new(),
        timeout: Duration::from_secs(30),
        measurement: eggbench_runner::MeasurementSignal::new(),
    };
    workload
        .execute(context(InvocationKind::Warmup { ordinal: 1 }))
        .await
        .expect("warmup succeeds");
    let output = workload
        .execute(context(InvocationKind::Measured {
            trial_id: eggbench_core::TrialId::new(1).expect("trial id"),
            arm: None,
        }))
        .await
        .expect("measured succeeds");
    assert!(output.error_counts.is_empty());
    let diagnostics = dialer.diagnostics_snapshot();
    assert!(diagnostics.successful_dials > 0);
    assert!(diagnostics.successful_dials < 40);
    let method = output
        .artifacts
        .iter()
        .find(|artifact| artifact.name == "eggfetch-method.json")
        .expect("method evidence");
    let method: serde_json::Value = serde_json::from_slice(&method.bytes).expect("method JSON");
    assert_eq!(
        method["network_path"]["successful_dials"]
            .as_u64()
            .expect("delta dials"),
        0,
        "the idle measured invocation opens no new connection"
    );
    origin
        .shutdown(Duration::from_secs(5))
        .await
        .expect("origin shuts down");
}

#[tokio::test]
async fn no_fault_path_diagnostic_completes_without_overhead_gate() {
    let mut origin = start_origin().await;
    let url = origin_url(&*origin);
    let direct = eggfetch_core::Client::new();
    let started = Instant::now();
    for _ in 0..20 {
        let mut response = direct
            .get(&url)
            .expect("URL")
            .send_detailed()
            .await
            .expect("direct request");
        let _ = response.bytes().await.expect("direct body");
    }
    let direct_elapsed = started.elapsed();

    let resolved = resolved_path(RouteMode::Direct, None);
    let dialer = Arc::new(
        lower_dialer(
            &resolved,
            None,
            new_path_diagnostics(),
            Duration::from_secs(5),
        )
        .expect("direct path lowers"),
    );
    let client = eggfetch_core::Client::builder().dialer(dialer).build();
    let started = Instant::now();
    for _ in 0..20 {
        let mut response = client
            .get(&url)
            .expect("URL")
            .send_detailed()
            .await
            .expect("path request");
        let _ = response.bytes().await.expect("path body");
    }
    let path_elapsed = started.elapsed();
    eprintln!(
        "no-fault diagnostic: direct={direct_elapsed:?} path_direct={path_elapsed:?} ratio={}",
        path_elapsed.as_secs_f64() / direct_elapsed.as_secs_f64().max(f64::EPSILON)
    );
    origin
        .shutdown(Duration::from_secs(5))
        .await
        .expect("origin shuts down");
}

#[tokio::test]
async fn cancellation_drops_in_flight_route_work_without_detached_tasks() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind stalled proxy");
    let proxy = listener.local_addr().expect("proxy address");
    let accepted = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("proxy accepts");
        std::future::pending::<()>().await;
        drop(stream);
    });
    let resolved = resolved_path(
        RouteMode::ProxyChain {
            chain: format!("http://{proxy}"),
        },
        None,
    );
    let dialer = Arc::new(
        lower_dialer(
            &resolved,
            None,
            new_path_diagnostics(),
            Duration::from_secs(30),
        )
        .expect("path lowers"),
    );
    let mut workload = EggfetchWorkload::with_path_dialer(Arc::clone(&dialer));
    let cancel = CancellationToken::new();
    let mut bindings = RuntimeBindings::new();
    bindings
        .insert("origin", "http_url", "http://127.0.0.1:9/bench".to_owned())
        .expect("binding");
    let execution = workload.execute(InvocationContext {
        run_id: RunId::new(),
        kind: InvocationKind::Measured {
            trial_id: eggbench_core::TrialId::new(1).expect("trial"),
            arm: None,
        },
        workload: eggbench_core::Workload::FiniteCount {
            target: Name::new("origin").expect("target"),
            requests: positive(1),
            concurrency: positive(1),
        },
        seed: None,
        bindings,
        cancellation: cancel.clone(),
        timeout: Duration::from_secs(30),
        measurement: eggbench_runner::MeasurementSignal::new(),
    });
    tokio::pin!(execution);
    tokio::select! {
        () = async {
            while dialer.current_ordinal() == 0 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        } => {}
        result = &mut execution => panic!("route unexpectedly completed: {result:?}"),
    }
    cancel.cancel();
    let output = tokio::time::timeout(Duration::from_secs(2), execution)
        .await
        .expect("cancellation completes promptly")
        .expect("workload returns cancellation output");
    assert!(
        output
            .error_counts
            .iter()
            .any(|(category, count)| category == "cancelled" && *count == 1)
    );
    accepted.abort();
    let _ = accepted.await;
}

async fn exercise_fault(kind: StreamFaultKind) -> eggchaos_core::DirectionSummary {
    let (inner, mut peer) = tokio::io::duplex(64 * 1024);
    let plan = build_fault_plan(&[fault("behavior", kind)]).expect("fault lowers");
    let mut stream = ChaosStream::new(inner, plan, 19, "test", 1, Direction::Upstream)
        .expect("fault stream constructs");
    let payload = vec![7_u8; 64];
    let _ = tokio::time::timeout(Duration::from_millis(500), async {
        let _ = stream.write_all(&payload).await;
        let _ = stream.flush().await;
        let _ = stream.shutdown().await;
    })
    .await;
    let mut received = [0_u8; 64];
    let _ = tokio::time::timeout(Duration::from_millis(20), peer.read(&mut received)).await;
    for _ in 0..4 {
        tokio::task::yield_now().await;
    }
    let _ = tokio::time::timeout(Duration::from_millis(20), async {
        loop {
            if stream.summary().activations.iter().any(|count| *count > 0) {
                break;
            }
            if std::future::poll_fn(|cx| stream.poll_termination(cx))
                .await
                .is_some()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await;
    stream.summary()
}

#[tokio::test]
async fn every_supported_fault_kind_activates_deterministically() {
    type BehaviorCheck = fn(&eggchaos_core::DirectionSummary) -> bool;
    let cases: Vec<(StreamFaultKind, BehaviorCheck)> = vec![
        (
            StreamFaultKind::Latency {
                delay_ms: duration(3),
                jitter_ms: duration(1),
                max_buffer_bytes: positive(4096),
            },
            |summary| summary.injected_delay_ms > 0 && summary.bytes_forwarded > 0,
        ),
        (
            StreamFaultKind::Bandwidth {
                bytes_per_second: positive(1000),
                burst_bytes: positive(1),
            },
            |summary| summary.throttled_delay_ms > 0 && summary.bytes_forwarded > 0,
        ),
        (
            StreamFaultKind::Blackhole {
                close_after_ms: None,
            },
            |summary| summary.bytes_discarded > 0 && summary.bytes_forwarded == 0,
        ),
        (
            StreamFaultKind::LimitData {
                bytes: positive(16),
            },
            |summary| summary.bytes_accepted >= 16 && summary.bytes_forwarded <= 16,
        ),
        (
            StreamFaultKind::SlowClose {
                delay_ms: duration(3),
            },
            |summary| summary.bytes_forwarded > 0,
        ),
        (
            StreamFaultKind::Slice {
                average_size: positive(8),
                variation: 0,
                delay_ms: duration(1),
            },
            |summary| summary.slices > 1 && summary.bytes_forwarded == 64,
        ),
        (
            StreamFaultKind::Disconnect {
                after_ms: duration(1),
            },
            |summary| summary.termination.is_some(),
        ),
    ];
    for (index, (kind, assert_behavior)) in cases.into_iter().enumerate() {
        let summary = exercise_fault(kind).await;
        assert!(
            summary.activations[index] > 0,
            "fault {index} did not activate: {summary:?}"
        );
        assert!(
            assert_behavior(&summary),
            "fault {index} behavior was incorrect: {summary:?}"
        );
    }
}

#[tokio::test]
async fn slow_close_delays_shutdown_after_payload_forwarding() {
    let (inner, mut peer) = tokio::io::duplex(4096);
    let plan = build_fault_plan(&[fault(
        "slow-close",
        StreamFaultKind::SlowClose {
            delay_ms: duration(35),
        },
    )])
    .expect("slow-close lowers");
    let mut stream = ChaosStream::new(inner, plan, 19, "slow-close-test", 1, Direction::Upstream)
        .expect("slow-close stream");
    stream.write_all(b"payload").await.expect("payload write");
    stream.flush().await.expect("payload flush");
    let mut received = [0_u8; 7];
    peer.read_exact(&mut received)
        .await
        .expect("payload forward");
    let started = Instant::now();
    tokio::time::timeout(Duration::from_millis(200), stream.shutdown())
        .await
        .expect("slow close completes")
        .expect("slow close succeeds");
    assert!(started.elapsed() >= Duration::from_millis(25));
    assert!(stream.summary().activations.iter().any(|count| *count > 0));
}

#[tokio::test]
async fn production_argument_order_keeps_upstream_and_downstream_faults_distinct() {
    let upstream = build_fault_plan(&[fault(
        "upstream",
        StreamFaultKind::Latency {
            delay_ms: duration(25),
            jitter_ms: duration(1),
            max_buffer_bytes: positive(4096),
        },
    )])
    .expect("upstream lowers");
    let downstream = build_fault_plan(&[fault(
        "downstream",
        StreamFaultKind::Blackhole {
            close_after_ms: None,
        },
    )])
    .expect("downstream lowers");
    let (inner, mut peer) = tokio::io::duplex(4096);
    let mut stream = eggchaos_core::BidirectionalChaosStream::new_live(
        inner,
        eggchaos_core::LivePolicy::new(upstream, 19),
        eggchaos_core::LivePolicy::new(downstream, 19),
        "direction-test",
        1,
    )
    .expect("direction wrapper");
    let started = Instant::now();
    stream.write_all(b"request").await.expect("upstream write");
    stream.flush().await.expect("upstream flush");
    assert!(started.elapsed() >= Duration::from_millis(20));

    peer.write_all(b"response").await.expect("peer write");
    let mut response = [0_u8; 8];
    assert!(
        tokio::time::timeout(Duration::from_millis(30), stream.read_exact(&mut response))
            .await
            .is_err(),
        "downstream blackhole must not deliver response bytes"
    );
}

#[test]
fn every_supported_fault_kind_lowers_with_probability_one() {
    let requests = vec![
        fault(
            "latency",
            StreamFaultKind::Latency {
                delay_ms: duration(1),
                jitter_ms: duration(2),
                max_buffer_bytes: positive(1024),
            },
        ),
        fault(
            "bandwidth",
            StreamFaultKind::Bandwidth {
                bytes_per_second: positive(1024),
                burst_bytes: positive(2048),
            },
        ),
        fault(
            "blackhole",
            StreamFaultKind::Blackhole {
                close_after_ms: None,
            },
        ),
        fault(
            "limit-data",
            StreamFaultKind::LimitData {
                bytes: positive(16),
            },
        ),
        fault(
            "slow-close",
            StreamFaultKind::SlowClose {
                delay_ms: duration(1),
            },
        ),
        fault(
            "slice",
            StreamFaultKind::Slice {
                average_size: positive(16),
                variation: 4,
                delay_ms: duration(1),
            },
        ),
        fault(
            "disconnect",
            StreamFaultKind::Disconnect {
                after_ms: duration(10),
            },
        ),
    ];
    let plan = build_fault_plan(&requests).expect("all faults lower");
    assert_eq!(
        plan,
        build_fault_plan(&requests).expect("same requests lower identically")
    );
    assert_eq!(plan.faults().len(), 7);
    assert!(
        plan.faults()
            .iter()
            .all(|fault| (fault.probability.get() - 1.0).abs() < f64::EPSILON)
    );
    assert!(plan.faults().iter().any(|fault| matches!(
        fault.kind,
        FaultKind::Disconnect(config) if !config.hard_reset
    )));
}

#[test]
fn credentialed_and_extended_routes_fail_before_runtime() {
    let ipv6 = resolved_path(
        RouteMode::ProxyChain {
            chain: "http://[::1]:8080".to_owned(),
        },
        None,
    );
    assert!(lower_dialer(&ipv6, None, new_path_diagnostics(), Duration::from_secs(1),).is_ok());

    for chain in [
        "trojan://secret@proxy.example:443",
        "ssh://proxy.example:22",
        "quic+http://proxy.example:443",
    ] {
        let resolved = resolved_path(
            RouteMode::ProxyChain {
                chain: chain.to_owned(),
            },
            None,
        );
        assert!(
            lower_dialer(
                &resolved,
                None,
                new_path_diagnostics(),
                Duration::from_secs(1),
            )
            .is_err()
        );
    }
}

#[test]
fn invocation_diagnostics_do_not_accumulate_across_invocations() {
    let diagnostics = new_path_diagnostics();
    let before = diagnostics.begin_invocation(0);
    diagnostics.record_dial_attempt();
    diagnostics.record_successful_dial(2, 0);
    diagnostics.record_fault_wrapped_connection();
    let first = diagnostics.invocation_delta(&before);
    assert_eq!(first.physical_dial_attempts, 1);
    assert_eq!(first.successful_dials, 1);
    assert_eq!(first.fault_wrapped_connections, 1);
    assert_eq!(first.connection_ordinal_min, 0);
    assert_eq!(first.connection_ordinal_max, 0);

    let before = diagnostics.begin_invocation(1);
    diagnostics.record_dial_attempt();
    diagnostics.record_successful_dial(3, 1);
    diagnostics.record_fault_wrapper_construction_failure();
    let second = diagnostics.invocation_delta(&before);
    assert_eq!(second.physical_dial_attempts, 1);
    assert_eq!(second.successful_dials, 1);
    assert_eq!(second.fault_wrapped_connections, 0);
    assert_eq!(second.fault_wrapper_construction_failures, 1);
    assert_eq!(second.connection_ordinal_min, 1);
    assert_eq!(second.connection_ordinal_max, 1);

    let run = diagnostics.snapshot();
    assert_eq!(run.physical_dial_attempts, 2);
    assert_eq!(run.successful_dials, 2);
    assert_eq!(run.fault_wrapped_connections, 1);
    assert_eq!(run.fault_wrapper_construction_failures, 1);
}

#[tokio::test]
async fn diagnostic_updates_are_bounded_and_concurrency_safe() {
    let diagnostics = new_path_diagnostics();
    let accepted = Arc::new(AtomicUsize::new(0));
    let mut tasks = Vec::new();
    for ordinal in 0..32_u64 {
        let diagnostics = Arc::clone(&diagnostics);
        let accepted = Arc::clone(&accepted);
        tasks.push(tokio::task::spawn_blocking(move || {
            diagnostics.record_dial_attempt();
            diagnostics.record_successful_dial(2, ordinal);
            accepted.fetch_add(1, Ordering::SeqCst);
        }));
    }
    for task in tasks {
        task.await.expect("counter task");
    }
    for index in 0..64 {
        diagnostics.record_route_failure(format!("kind:stage:{index}"));
    }
    let snapshot = diagnostics.snapshot();
    assert_eq!(snapshot.physical_dial_attempts, 32);
    assert_eq!(snapshot.successful_dials, 32);
    assert_eq!(snapshot.connection_ordinal_min, 0);
    assert_eq!(snapshot.connection_ordinal_max, 31);
    assert_eq!(snapshot.route_failures.len(), 16);
    assert_eq!(snapshot.route_failure_buckets_dropped, 48);
    assert_eq!(accepted.load(Ordering::SeqCst), 32);
}
