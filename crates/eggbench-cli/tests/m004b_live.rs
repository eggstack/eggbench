//! Eggstack M004b live combined-verdict qualification: Case B.
//!
//! A qualification-only reflecting managed service (NOT production code)
//! serves the permissive echo contract against loopback while the REAL
//! `eggsec` binary (resolved from `PATH`) executes the bounded WAF check.
//! The test proves through the real lifecycle, real evidence, and real
//! comparison: execution `Completed`, performance verdict `Pass`,
//! correctness `Fail`, final combined `Fail`.
//!
//! The test skips gracefully (with a stderr note) when the `eggsec`
//! binary is absent. The M004b harness
//! (`scripts/qualification/m004b-combined/`) builds the exact pinned
//! Eggsec revision and runs this suite with the binary on `PATH`.
//!
//! The echo adapter is deliberately minimal and qualification-only: it
//! echoes the request target with 200 for every path except `/` (404) so
//! Eggsec-declared bypasses are observable. It contains no scanner logic.

use eggbench_core::{
    ArtifactBounds, ArtifactPath, ArtifactRole, BundleWriter, DefaultDriverPolicy, DriverCategory,
    EnvironmentFingerprint, ExecutionStatus, Name, PositiveCount, ResolutionOptions, ResolvedPlan,
    RunId, Sensitivity,
};
use eggbench_drivers::{EGGSEC_DRIVER_NAME, EggsecWafExecutor};
use eggbench_runner::{
    BoxFuture, CorrectnessRegistry, LocalSession, MapSecretProvider, ProbeRegistry, ResetRegistry,
    RunnerOptions, RuntimeBindings, ServiceAdapterRegistry, ServiceStartRequest, TelemetryRegistry,
    UnixPlatform, execute_run_with_diagnostics,
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

fn name(value: &str) -> Name {
    Name::new(value).unwrap()
}

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

/// Qualification-only reflecting service adapter.
///
/// Binds 127.0.0.1:0, publishes the `http_url` runtime binding, answers
/// 404 for `/` and 200 with an echo body otherwise. Shutdown aborts the
/// accept loop.
struct EchoAdapter;

impl eggbench_runner::ManagedServiceAdapter for EchoAdapter {
    fn service_type(&self) -> &'static str {
        "echo-permissive-qual"
    }

    fn start(
        &self,
        _request: ServiceStartRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'_, Result<Box<dyn eggbench_runner::ManagedServiceHandle>, String>> {
        Box::pin(async move {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .map_err(|error| format!("echo bind failed: {error}"))?;
            let port = listener
                .local_addr()
                .map_err(|error| format!("echo addr failed: {error}"))?
                .port();
            let task = tokio::spawn(async move {
                loop {
                    if cancel.is_cancelled() {
                        break;
                    }
                    let Ok((socket, _)) = listener.accept().await else {
                        break;
                    };
                    tokio::spawn(async move {
                        use tokio::io::{AsyncReadExt, AsyncWriteExt};
                        let mut socket = socket;
                        let mut buffer = vec![0_u8; 8192];
                        let Ok(read) = socket.read(&mut buffer).await else {
                            return;
                        };
                        let head = String::from_utf8_lossy(&buffer[..read]);
                        let path = head
                            .lines()
                            .next()
                            .and_then(|line| line.split_whitespace().nth(1))
                            .unwrap_or("/");
                        let (status, body) = if path == "/" {
                            ("404 Not Found", b"not found".to_vec())
                        } else {
                            ("200 OK", format!("echo:{path}").into_bytes())
                        };
                        let response = format!(
                            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        );
                        let _ = socket.write_all(response.as_bytes()).await;
                        let _ = socket.write_all(&body).await;
                    });
                }
            });
            let mut bindings = RuntimeBindings::new();
            bindings
                .insert("echo", "http_url", format!("http://127.0.0.1:{port}/"))
                .map_err(|error| format!("echo binding failed: {error}"))?;
            Ok(Box::new(EchoHandle { bindings, task })
                as Box<dyn eggbench_runner::ManagedServiceHandle>)
        })
    }
}

struct EchoHandle {
    bindings: RuntimeBindings,
    task: tokio::task::JoinHandle<()>,
}

impl eggbench_runner::ManagedServiceHandle for EchoHandle {
    fn bindings(&self) -> RuntimeBindings {
        self.bindings.clone()
    }

    fn shutdown(&mut self, _grace: Duration) -> BoxFuture<'_, Result<(), String>> {
        Box::pin(async move {
            self.task.abort();
            Ok(())
        })
    }
}

fn case_b_plan() -> eggbench_core::ExperimentPlan {
    let raw = serde_json::json!({
        "schema_version": 6,
        "experiment": "m004b-case-b",
        "subject": {"kind": "label", "label": "m004b-case-b"},
        "services": [{
            "name": "echo",
            "kind": {"kind": "named", "service_type": "echo-permissive-qual"},
            "lifecycle": "managed",
            "depends_on": [],
            "config": {},
            "readiness": null,
            "shutdown": null,
            "working_directory": null,
            "log_limit_bytes": 65536,
        }],
        "workload": {"kind": "finite_count", "target": "echo", "requests": 4, "concurrency": 1},
        "trials": {"measured": 2, "warmup": 0, "cooldown_ms": null,
                   "reset": {"kind": "none"},
                   "timeouts": {"measurement": 5000, "drain": 5000}},
        "telemetry": [],
        "metrics": [{
            "name": "latency_p99", "unit": "ms",
            "direction": {"kind": "lower_is_better"},
            "intent": "primary",
            "gate": {"kind": "absolute", "value": 60000.0},
        }],
        "environment_policy": {"kind": "strict_same_testbed"},
        "seed": 11,
        "diagnostics": [],
        "security_checks": [{
            "id": "waf-sqli", "source": "eggsec-waf", "target": "echo",
            "test_type": "sqli", "max_successful_bypasses": 0,
            "concurrency": 2, "timeout_ms": 60000,
        }],
        "bounds": {"artifact_count": 256, "artifact_bytes": 16_777_216, "total_bytes": 268_435_456},
    });
    eggbench_core::ExperimentPlan::from_json(&raw.to_string()).expect("case-b plan validates")
}

fn fake_descriptor(
    descriptor_name: &str,
    category: DriverCategory,
    capabilities: BTreeSet<eggbench_core::Capability>,
) -> eggbench_core::DriverDescriptor {
    eggbench_core::DriverDescriptor {
        name: name(descriptor_name),
        adapter_version: env!("CARGO_PKG_VERSION").to_owned(),
        upstream_name: descriptor_name.to_owned(),
        upstream_version: None,
        category,
        capabilities,
        supported_platforms: BTreeSet::new(),
        machine_output_schema: None,
        external_process: false,
        default: false,
        compatible_service_types: BTreeSet::new(),
    }
}

fn latency_fake(values: &[f64]) -> eggbench_runner::test_support::FakeWorkload {
    let mut fake = eggbench_runner::test_support::FakeWorkload::default();
    fake.metrics_by_invocation = values
        .iter()
        .map(|value| {
            Some(vec![eggbench_core::RawMetricObservation {
                name: "latency_p99".to_owned(),
                unit: "ms".to_owned(),
                value: *value,
                aggregation: eggbench_core::Aggregation::Percentile { basis_points: 9900 },
                source_field: Some("fake.p99".to_owned()),
                producer: None,
                producer_version: None,
                raw_artifacts: Vec::new(),
            }])
        })
        .collect();
    fake
}

/// M004b Case B: security Fail + performance Pass through the real
/// lifecycle with the real pinned Eggsec binary.
///
/// Expected: execution `Completed` (valid observation, threshold
/// exceeded — not an execution failure), performance verdict `Pass`,
/// correctness `Fail`, final combined `Fail`.
#[allow(clippy::too_many_lines)] // One end-to-end live qualification script.
#[tokio::test]
async fn case_b_security_fail_with_performance_pass_combines_to_fail() {
    if !binary_present("eggsec") {
        eprintln!("skipping live Case B test: eggsec binary not installed");
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let plan = case_b_plan();

    // Resolution against the fake workload/service drivers plus the real
    // external eggsec-waf descriptor with its pinned executable path.
    let mut capabilities = BTreeSet::new();
    capabilities.insert(eggbench_core::Capability::LoadMode {
        mode: eggbench_core::LoadMode::ClosedLoop,
    });
    let descriptors = vec![
        fake_descriptor("fake-load", DriverCategory::Workload, capabilities),
        fake_descriptor("fake-service", DriverCategory::Service, BTreeSet::new()),
        eggbench_drivers::eggsec_descriptor(),
    ];
    let eggsec_name = name(EGGSEC_DRIVER_NAME);
    let eggsec_path = eggbench_drivers::executable_path_for(&eggsec_name)
        .expect("eggsec resolves when the binary is installed");
    let mut selections = BTreeMap::new();
    selections.insert(DriverCategory::Workload, name("fake-load"));
    selections.insert(DriverCategory::Service, name("fake-service"));
    selections.insert(DriverCategory::Correctness, eggsec_name.clone());
    let mut executable_paths = BTreeMap::new();
    executable_paths.insert(eggsec_name, eggsec_path);
    let options = ResolutionOptions {
        selections,
        default_policy: DefaultDriverPolicy::Deterministic,
        platform: name("linux-x86_64"),
        executable_paths,
        required_capabilities: BTreeMap::new(),
    };
    let resolved: ResolvedPlan =
        eggbench_core::resolve_plan(&plan, &descriptors, &options).expect("case-b resolves");

    let mut adapters = ServiceAdapterRegistry::new();
    adapters
        .register(Arc::new(EchoAdapter))
        .expect("register echo adapter");
    let runner_options = RunnerOptions {
        workspace_root: temp.path().to_path_buf(),
        secrets: Arc::new(MapSecretProvider::empty()),
        probes: ProbeRegistry::with_builtins(),
        platform: Arc::new(UnixPlatform),
        service_adapters: adapters,
    };
    let mut session = LocalSession::prepare(&resolved, runner_options).expect("session prepares");

    let bounds = ArtifactBounds {
        artifact_count: PositiveCount::new(256).unwrap(),
        artifact_bytes: 16 * 1024 * 1024,
        total_bytes: 256 * 1024 * 1024,
    };
    let mut writer = BundleWriter::create(temp.path().join("case-b.eggb"), RunId::new(), bounds)
        .expect("writer creates");
    for (file, role, content) in [
        (
            "plan.json",
            ArtifactRole::ExperimentPlan,
            plan.to_json().expect("plan serializes").into_bytes(),
        ),
        (
            "resolved-plan.json",
            ArtifactRole::ResolvedPlan,
            serde_json::to_vec(&resolved).expect("resolved serializes"),
        ),
        (
            "environment.json",
            ArtifactRole::EnvironmentFingerprint,
            serde_json::to_vec(&EnvironmentFingerprint::new(BTreeMap::new()))
                .expect("environment serializes"),
        ),
    ] {
        writer
            .add_artifact(
                ArtifactPath::new(file).unwrap(),
                role,
                "application/json",
                Sensitivity::Redacted,
                content.as_slice(),
            )
            .expect("stage bundle artifact");
    }

    let mut workload = latency_fake(&[10.0, 10.0]);
    let mut diagnostics = eggbench_runner::DiagnosticRegistry::new();
    let eggsec_executable = eggbench_drivers::EggsecWafExecutor::resolve()
        .expect("eggsec resolves when the binary is installed");
    let mut correctness = CorrectnessRegistry::new();
    correctness.register(Box::new(EggsecWafExecutor::from_resolved(
        eggsec_executable,
        temp.path().join("scope"),
    )));
    let mut telemetry = TelemetryRegistry::new();
    let cancel = CancellationToken::new();
    let outcome = execute_run_with_diagnostics(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut telemetry,
        &mut diagnostics,
        &mut correctness,
        writer,
        &cancel,
    )
    .await
    .expect("run finalizes");
    assert_eq!(outcome.execution_status, ExecutionStatus::Completed);
    assert_eq!(outcome.manifest.trials.len(), 2);

    // Comparison through the production loader: recomputed evidence,
    // independent gates, conservative combination.
    let candidate =
        eggbench_core::load_candidate_bundle(&outcome.bundle_path).expect("candidate loads");
    let receipt = eggbench_core::compare(
        &eggbench_core::ComparisonRequest {
            candidate: &candidate,
            baseline: None,
        },
        &eggbench_core::ComparisonOptions::default(),
    );
    assert_eq!(
        receipt.performance_verdict,
        Some(eggbench_core::AggregateVerdict::Pass),
        "performance passes while security fails"
    );
    let section = receipt.correctness.expect("correctness section");
    assert_eq!(
        section.policy_id, "eggbench.security-correctness.v1",
        "immutable policy identifier"
    );
    assert_eq!(
        section.aggregate_verdict,
        eggbench_core::AggregateVerdict::Fail,
        "correctness fails on Eggsec-declared bypasses"
    );
    assert!(
        section.checks.iter().any(|check| {
            check.disposition == eggbench_core::CorrectnessDisposition::Fail
                && check.evidence_path.as_str() == "security/waf-sqli.json"
        }),
        "per-check Fail record with evidence path"
    );
    assert_eq!(
        receipt.aggregate_verdict,
        Some(eggbench_core::AggregateVerdict::Fail),
        "performance pass never overrides security failure"
    );
    // Execution status is untouched by the security failure.
    assert_eq!(outcome.execution_status, ExecutionStatus::Completed);

    // The production CLI surfaces the combined verdict: comparison-fail
    // exit 6 with a security-specific human detail, and typed v3 sections
    // in machine JSON.
    let presented = eggbench_cli::execute(
        eggbench_cli::Command::Compare {
            baseline: None,
            candidate: outcome.bundle_path.clone(),
            alias: None,
            absolute_only: true,
            paired: false,
            output: None,
            seed: None,
        },
        eggbench_cli::CommandOptions::human(),
    )
    .await;
    assert_eq!(presented.exit_code, eggbench_cli::ExitCode::ComparisonFail);
    assert_eq!(presented.exit_code.code(), 6);
    let body: serde_json::Value = serde_json::to_value(&presented.envelope).unwrap();
    assert_eq!(body["result"]["aggregate_verdict"], "fail");
    assert_eq!(body["result"]["receipt"]["performance_verdict"], "pass");
    assert_eq!(
        body["result"]["receipt"]["correctness"]["aggregate_verdict"],
        "fail"
    );
    assert_eq!(
        body["result"]["receipt"]["correctness"]["policy_id"],
        "eggbench.security-correctness.v1"
    );
}
