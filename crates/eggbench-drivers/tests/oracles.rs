//! External Oracles M002 acceptance tests: oha/h2load/iperf3 adapters.
//!
//! Parser/argv matrices live next to the adapters as unit tests. This suite
//! covers descriptors, preflight helpers, and live loopback execution.
//! Live tests skip gracefully (with a stderr note) when the tool binary is
//! absent; the missing-binary path itself is a first-class capability error
//! exercised through resolution.

use eggbench_core::{DurationMs, Name, PositiveCount, RunId, Workload};
use eggbench_drivers::{
    H2LOAD_DRIVER_NAME, IPERF3_DRIVER_NAME, OHA_DRIVER_NAME, h2load_descriptor, iperf3_descriptor,
    is_external_workload, oha_descriptor, probe_external_workload,
};
use eggbench_drivers::{H2loadWorkload, Iperf3Workload, OhaWorkload};
use eggbench_runner::{
    FailureCategory, InvocationContext, InvocationKind, RuntimeBindings, WorkloadExecutor,
};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

fn binary_present(binary: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|dir| {
            let candidate = dir.join(binary);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                candidate.is_file()
                    && candidate
                        .metadata()
                        .is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
            }
            #[cfg(not(unix))]
            {
                candidate.is_file()
            }
        })
    })
}

fn require_tool(binary: &str) -> bool {
    if binary_present(binary) {
        true
    } else {
        eprintln!("skipping live {binary} test: binary not installed");
        false
    }
}

fn find_free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("loopback bind")
        .local_addr()
        .expect("local addr")
        .port()
}

fn wait_for_tcp(port: u16) {
    for _ in 0..50 {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("server on port {port} never became reachable");
}

/// Guard that kills the child server on drop.
struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn spawn_python_http(port: u16) -> ChildGuard {
    let child = Command::new("python3")
        .args([
            "-m",
            "http.server",
            &port.to_string(),
            "--bind",
            "127.0.0.1",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn python http.server");
    wait_for_tcp(port);
    ChildGuard(child)
}

fn spawn_iperf_server(port: u16) -> ChildGuard {
    // No `-1` single-test mode: the TCP readiness probe below would consume
    // the single shot. The guard kills the persistent server after the test.
    let child = Command::new("iperf3")
        .args(["-s", "-B", "127.0.0.1", "-p", &port.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn iperf3 server");
    wait_for_tcp(port);
    ChildGuard(child)
}

fn bindings_for(url: &str) -> RuntimeBindings {
    let mut bindings = RuntimeBindings::new();
    bindings
        .insert("target", "http_url", url.to_owned())
        .expect("binding inserts");
    bindings
}

fn invocation(workload: Workload, url: &str, timeout: Duration) -> InvocationContext {
    InvocationContext {
        run_id: RunId::new(),
        kind: InvocationKind::Warmup { ordinal: 1 },
        workload,
        seed: Some(7),
        bindings: bindings_for(url),
        cancellation: CancellationToken::new(),
        timeout,
        measurement: eggbench_runner::MeasurementSignal::new(),
    }
}

fn finite_count(requests: u32, concurrency: u32) -> Workload {
    Workload::FiniteCount {
        target: Name::new("target").unwrap(),
        requests: PositiveCount::new(requests).unwrap(),
        concurrency: PositiveCount::new(concurrency).unwrap(),
    }
}

fn closed_duration(duration_ms: u64, concurrency: u32) -> Workload {
    Workload::ClosedLoop {
        target: Name::new("target").unwrap(),
        concurrency: PositiveCount::new(concurrency).unwrap(),
        requests: None,
        duration_ms: Some(DurationMs::new(duration_ms).unwrap()),
    }
}

fn metric(output: &eggbench_runner::WorkloadOutput, name: &str) -> f64 {
    output
        .metrics
        .iter()
        .find(|m| m.name == name)
        .unwrap_or_else(|| panic!("metric {name} present"))
        .value
}

// ---- Descriptors and preflight ----

#[test]
fn oracle_descriptors_carry_external_capability_matrices() {
    use eggbench_core::{Capability, DriverCategory, LoadMode};
    let oha = oha_descriptor();
    assert_eq!(oha.name.as_str(), OHA_DRIVER_NAME);
    assert_eq!(oha.category, DriverCategory::Workload);
    assert!(oha.external_process);
    assert!(!oha.default);
    assert!(oha.machine_output_schema.is_some());
    assert!(oha.capabilities.contains(&Capability::ExternalBinary));
    assert!(oha.capabilities.contains(&Capability::LoadMode {
        mode: LoadMode::ClosedLoop
    }));
    assert!(oha.capabilities.contains(&Capability::LoadMode {
        mode: LoadMode::OpenLoop
    }));
    assert!(oha.capabilities.contains(&Capability::CorrectedLatency));

    let h2load = h2load_descriptor();
    assert_eq!(h2load.name.as_str(), H2LOAD_DRIVER_NAME);
    assert!(h2load.external_process);
    assert!(h2load.capabilities.contains(&Capability::LoadMode {
        mode: LoadMode::ClosedLoop
    }));
    assert!(!h2load.capabilities.contains(&Capability::LoadMode {
        mode: LoadMode::OpenLoop
    }));

    let iperf3 = iperf3_descriptor();
    assert_eq!(iperf3.name.as_str(), IPERF3_DRIVER_NAME);
    assert!(iperf3.external_process);
    assert!(iperf3.capabilities.contains(&Capability::LoadMode {
        mode: LoadMode::ClosedLoop
    }));
    assert!(!iperf3.capabilities.contains(&Capability::LoadMode {
        mode: LoadMode::OpenLoop
    }));
}

#[test]
fn external_workload_names_are_known() {
    for tool in [OHA_DRIVER_NAME, H2LOAD_DRIVER_NAME, IPERF3_DRIVER_NAME] {
        assert!(is_external_workload(&Name::new(tool).unwrap()));
    }
    assert!(!is_external_workload(&Name::new("eggfetch-http").unwrap()));
}

#[tokio::test]
async fn preflight_rejects_unknown_driver_name() {
    let error = probe_external_workload(
        &Name::new("no-such-tool").unwrap(),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(
        error.category(),
        eggbench_drivers::ErrorCategory::BinaryNotFound
    );
}

// ---- oha live ----

#[tokio::test]
async fn oha_reports_parity_metrics_against_loopback() {
    if !require_tool("oha") || !require_tool("python3") {
        return;
    }
    let port = find_free_port();
    let _server = spawn_python_http(port);
    let url = format!("http://127.0.0.1:{port}/");

    let executable = OhaWorkload::resolve().expect("oha resolves");
    let probed = OhaWorkload::probe(&executable, &CancellationToken::new())
        .await
        .expect("oha probes");
    let mut workload = OhaWorkload::new(executable, probed.version).expect("version floor");
    let output = workload
        .execute(invocation(
            finite_count(20, 2),
            &url,
            Duration::from_secs(60),
        ))
        .await
        .expect("oha trial completes");
    assert!(metric(&output, "throughput") > 0.0);
    assert_eq!(metric(&output, "error_rate").to_bits(), 0.0_f64.to_bits());
    assert!(metric(&output, "latency_mean") > 0.0);
    assert!(metric(&output, "latency_p99") >= metric(&output, "latency_p50"));
    assert!(output.error_counts.is_empty());
    assert!(output.artifacts.iter().any(|a| a.name == "stdout.raw"));
    assert!(
        output
            .artifacts
            .iter()
            .any(|a| a.name == "command-metadata.json")
    );
    assert!(output.artifacts.iter().any(|a| a.name == "oha-status.json"));
    workload
        .drain(eggbench_runner::DrainContext {
            run_id: RunId::new(),
            cancellation: CancellationToken::new(),
            timeout: Duration::from_secs(5),
        })
        .await
        .expect("drain");
}

#[tokio::test]
async fn oha_surfaces_unreachable_target_as_full_error_rate() {
    if !require_tool("oha") {
        return;
    }
    // oha exits 0 with every request failed: the trial completes and the
    // failure is carried by error_rate plus error counts, never hidden.
    let port = find_free_port();
    let url = format!("http://127.0.0.1:{port}/");
    let executable = OhaWorkload::resolve().expect("oha resolves");
    let mut workload = OhaWorkload::from_resolved(executable);
    let output = workload
        .execute(invocation(
            finite_count(5, 1),
            &url,
            Duration::from_secs(60),
        ))
        .await
        .expect("unreachable trial still completes");
    assert_eq!(metric(&output, "error_rate").to_bits(), 1.0_f64.to_bits());
    assert!(!output.error_counts.is_empty());
    assert!(!output.metrics.iter().any(|m| m.name == "latency_min"));
}

// ---- h2load live ----

#[tokio::test]
async fn h2load_reports_parity_metrics_against_loopback() {
    if !require_tool("h2load") || !require_tool("python3") {
        return;
    }
    let port = find_free_port();
    let _server = spawn_python_http(port);
    let url = format!("http://127.0.0.1:{port}/");

    let executable = H2loadWorkload::resolve().expect("h2load resolves");
    let probed = H2loadWorkload::probe(&executable, &CancellationToken::new())
        .await
        .expect("h2load probes");
    let mut workload = H2loadWorkload::new(executable, probed.version).expect("version floor");
    let output = workload
        .execute(invocation(
            finite_count(20, 2),
            &url,
            Duration::from_secs(60),
        ))
        .await
        .expect("h2load trial completes");
    assert!(metric(&output, "throughput") > 0.0);
    assert_eq!(metric(&output, "error_rate").to_bits(), 0.0_f64.to_bits());
    assert!(metric(&output, "latency_mean") >= 0.0);
    assert!(output.error_counts.is_empty());
    assert!(
        output
            .artifacts
            .iter()
            .any(|a| a.name == "h2load-status.json")
    );
}

#[tokio::test]
async fn h2load_surfaces_failures_as_counts_not_exit_status() {
    if !require_tool("h2load") {
        return;
    }
    let port = find_free_port();
    let url = format!("http://127.0.0.1:{port}/");
    let executable = H2loadWorkload::resolve().expect("h2load resolves");
    let mut workload = H2loadWorkload::from_resolved(executable);
    let output = workload
        .execute(invocation(
            finite_count(5, 1),
            &url,
            Duration::from_secs(60),
        ))
        .await
        .expect("failed trial still completes");
    assert_eq!(metric(&output, "error_rate").to_bits(), 1.0_f64.to_bits());
    assert!(
        output
            .error_counts
            .iter()
            .any(|(category, _)| category == "h2load:failed")
    );
}

// ---- iperf3 live ----

#[tokio::test]
async fn iperf3_measures_loopback_throughput() {
    if !require_tool("iperf3") {
        return;
    }
    let port = find_free_port();
    let server = spawn_iperf_server(port);
    // The -s -1 server exits after one test; hold the guard until done.
    let url = format!("http://127.0.0.1:{port}/");

    let executable = Iperf3Workload::resolve().expect("iperf3 resolves");
    let probed = Iperf3Workload::probe(&executable, &CancellationToken::new())
        .await
        .expect("iperf3 probes");
    let mut workload = Iperf3Workload::new(executable, probed.version).expect("version floor");
    let output = workload
        .execute(invocation(
            closed_duration(2000, 1),
            &url,
            Duration::from_secs(120),
        ))
        .await
        .expect("iperf3 trial completes");
    assert!(metric(&output, "bits_per_sec_received") > 0.0);
    assert!(metric(&output, "bytes_received") > 0.0);
    assert!(
        output
            .artifacts
            .iter()
            .any(|a| a.name == "command-metadata.json")
    );
    drop(server);
}

#[tokio::test]
async fn iperf3_refused_server_is_trial_failure() {
    if !require_tool("iperf3") {
        return;
    }
    let port = find_free_port();
    let url = format!("http://127.0.0.1:{port}/");
    let executable = Iperf3Workload::resolve().expect("iperf3 resolves");
    let mut workload = Iperf3Workload::from_resolved(executable);
    let error = workload
        .execute(invocation(
            closed_duration(1000, 1),
            &url,
            Duration::from_secs(60),
        ))
        .await
        .unwrap_err();
    assert_eq!(error, FailureCategory::WorkloadFailed);
}

// ---- Cancellation ----

#[tokio::test]
async fn oracle_execution_honors_cancellation() {
    if !require_tool("oha") || !require_tool("python3") {
        return;
    }
    let port = find_free_port();
    let _server = spawn_python_http(port);
    let url = format!("http://127.0.0.1:{port}/");
    let executable = OhaWorkload::resolve().expect("oha resolves");
    let mut workload = OhaWorkload::from_resolved(executable);
    let cancel = CancellationToken::new();
    cancel.cancel();
    let context = InvocationContext {
        run_id: RunId::new(),
        kind: InvocationKind::Warmup { ordinal: 1 },
        workload: finite_count(10_000, 2),
        seed: None,
        bindings: bindings_for(&url),
        cancellation: cancel,
        timeout: Duration::from_secs(60),
        measurement: eggbench_runner::MeasurementSignal::new(),
    };
    let error = workload.execute(context).await.unwrap_err();
    assert_eq!(error, FailureCategory::Cancelled);
}

// ---- Raw evidence determinism ----

#[tokio::test]
async fn oracle_raw_stdout_matches_retained_json() {
    if !require_tool("oha") || !require_tool("python3") {
        return;
    }
    let port = find_free_port();
    let _server = spawn_python_http(port);
    let url = format!("http://127.0.0.1:{port}/");
    let executable = OhaWorkload::resolve().expect("oha resolves");
    let mut workload = OhaWorkload::from_resolved(executable);
    let output = workload
        .execute(invocation(
            finite_count(5, 1),
            &url,
            Duration::from_secs(60),
        ))
        .await
        .expect("oha trial completes");
    let raw = output
        .artifacts
        .iter()
        .find(|a| a.name == "stdout.raw")
        .expect("raw retained");
    let mut parsed: serde_json::Value =
        serde_json::from_slice(&raw.bytes).expect("raw is the tool JSON");
    assert!(parsed.get("summary").is_some());
    // Status artifact restates the same distribution object.
    let status = output
        .artifacts
        .iter()
        .find(|a| a.name == "oha-status.json")
        .expect("status retained");
    let status_json: serde_json::Value =
        serde_json::from_slice(&status.bytes).expect("status parses");
    assert_eq!(
        status_json,
        parsed
            .get_mut("statusCodeDistribution")
            .expect("distribution present")
            .take()
    );
}
