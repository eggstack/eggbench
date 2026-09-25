//! End-to-end qualification for the M002 phase coordinator.
#![cfg(unix)]

use eggbench_core::{
    ArtifactBounds, ArtifactPath, ArtifactRole, BundleError, BundleReader, BundleWriter,
    DriverCategory, DurationMs, ExecutionStatus, Lifecycle, Name, PositiveCount, ResetPolicy,
    ResolvedPairedArm, ResolvedPairedDesign, ResolvedPlan, RunId, Sensitivity, Service,
    ServiceKind, Subject, TrialArm, TrialExecutionResult, TrialPolicy, Workload,
};
use eggbench_runner::test_support::FakeWorkload;
use eggbench_runner::{
    CorrectnessRegistry, DiagnosticRegistry, DrainContext, FailureCategory, InvocationContext,
    InvocationKind, LocalSession, MapSecretProvider, OrchestrationError, PhaseEvent, PhaseKind,
    PlatformAdapter, PlatformSupport, ResetContext, ResetHook, ResetRegistry, RunEvidenceArtifact,
    RunnerOptions, ServiceAdapterRegistry, TelemetryRegistry, UnixPlatform, WorkloadArtifact,
    WorkloadExecutor, WorkloadOutput, execute_run,
};
use std::{
    collections::BTreeMap,
    future::Future,
    io::{BufReader, Read},
    path::Path,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio_util::sync::CancellationToken;

fn name(text: &str) -> Name {
    Name::new(text).unwrap()
}

fn plan() -> ResolvedPlan {
    let mut timeouts = BTreeMap::new();
    timeouts.insert(name("measurement"), DurationMs::new(500).unwrap());
    timeouts.insert(name("drain"), DurationMs::new(500).unwrap());
    ResolvedPlan {
        schema_version: eggbench_core::RESOLVED_PLAN_SCHEMA_VERSION,
        source_plan_schema_version: eggbench_core::EXPERIMENT_PLAN_SCHEMA_VERSION,
        experiment: name("orchestration"),
        drivers: BTreeMap::new(),
        subject: Subject::Label {
            label: name("subject"),
        },
        topology: Vec::new(),
        workload: Workload::FiniteCount {
            target: name("app"),
            requests: PositiveCount::new(4).unwrap(),
            concurrency: PositiveCount::new(1).unwrap(),
        },
        trials: TrialPolicy {
            measured: PositiveCount::new(2).unwrap(),
            warmup: 1,
            cooldown_ms: Some(DurationMs::new(1).unwrap()),
            reset: ResetPolicy::None,
            timeouts,
        },
        telemetry: Vec::new(),
        defaults: eggbench_core::ResolvedDefaults {
            platform: name("test-platform"),
            warmup_trials: 1,
            measured_trials: 2,
        },
        environment_policy: eggbench_core::EnvironmentPolicy::StrictSameTestbed,
        metrics: Vec::new(),
        artifact_bounds: ArtifactBounds {
            artifact_count: PositiveCount::new(64).unwrap(),
            artifact_bytes: 1024 * 1024,
            total_bytes: 4 * 1024 * 1024,
        },
        seed: Some(42),
        paired: None,
        network_path: None,
        diagnostics: Vec::new(),
        security_checks: Vec::new(),
        warnings: Vec::new(),
    }
}

fn runner_options(root: &Path) -> RunnerOptions {
    RunnerOptions {
        workspace_root: root.to_path_buf(),
        secrets: Arc::new(MapSecretProvider::empty()),
        probes: eggbench_runner::ProbeRegistry::with_builtins(),
        platform: Arc::new(UnixPlatform),
        service_adapters: ServiceAdapterRegistry::new(),
    }
}

#[derive(Debug)]
struct CancelOnTerminate(CancellationToken);
impl PlatformAdapter for CancelOnTerminate {
    fn support(&self) -> PlatformSupport {
        UnixPlatform.support()
    }
    fn label(&self) -> &'static str {
        "cancel-on-terminate-test"
    }
    fn is_alive(&self, pid: u32) -> bool {
        UnixPlatform.is_alive(pid)
    }
    fn terminate_group(&self, pid: u32) -> Result<(), String> {
        self.0.cancel();
        UnixPlatform.terminate_group(pid)
    }
    fn kill_group(&self, pid: u32) -> Result<(), String> {
        UnixPlatform.kill_group(pid)
    }
}

fn writer(root: &Path) -> BundleWriter {
    writer_with_bounds(root, 64, 1024 * 1024, 4 * 1024 * 1024)
}

fn writer_with_bounds(
    root: &Path,
    artifact_count: u32,
    artifact_bytes: u64,
    total_bytes: u64,
) -> BundleWriter {
    let path = root.join("out.eggb");
    let mut writer = BundleWriter::create(
        &path,
        RunId::new(),
        ArtifactBounds {
            artifact_count: PositiveCount::new(artifact_count).unwrap(),
            artifact_bytes,
            total_bytes,
        },
    )
    .unwrap();
    for (file, role, content) in [
        ("plan.json", ArtifactRole::ExperimentPlan, br"{}".as_slice()),
        (
            "resolved-plan.json",
            ArtifactRole::ResolvedPlan,
            br"{}".as_slice(),
        ),
        (
            "environment.json",
            ArtifactRole::EnvironmentFingerprint,
            br"{}".as_slice(),
        ),
    ] {
        writer
            .add_artifact(
                ArtifactPath::new(file).unwrap(),
                role,
                "application/json",
                Sensitivity::Public,
                content,
            )
            .unwrap();
    }
    writer
}

fn attach_network_path(resolved: &mut ResolvedPlan) {
    let path: eggbench_core::ResolvedNetworkPath = serde_json::from_value(serde_json::json!({
        "route": {
            "driver": "eggress-route",
            "mode": { "kind": "direct" }
        },
        "route_driver": {
            "descriptor": {
                "name": "eggress-route",
                "adapter_version": "0.1.0",
                "upstream_name": "eggress-outbound",
                "upstream_version": "1.0.10",
                "category": "route",
                "capabilities": [{ "kind": "proxy_routing" }],
                "supported_platforms": [],
                "machine_output_schema": null,
                "external_process": false,
                "default": true,
                "compatible_service_types": []
            },
            "executable_path": null
        },
        "semantics_version": "route-first-fault-second-v1"
    }))
    .expect("resolved network path");
    resolved.source_plan_schema_version = eggbench_core::EXPERIMENT_PLAN_SCHEMA_VERSION_3;
    let workload: eggbench_core::ResolvedDriver = serde_json::from_value(serde_json::json!({
        "descriptor": {
            "name": "eggfetch-http",
            "adapter_version": "0.1.0",
            "upstream_name": "eggfetch-core",
            "upstream_version": "0.2.0",
            "category": "workload",
            "capabilities": [
                { "kind": "network_path" },
                { "kind": "load_mode", "mode": "closed_loop" }
            ],
            "supported_platforms": [],
            "machine_output_schema": null,
            "external_process": false,
            "default": true,
            "compatible_service_types": []
        },
        "executable_path": null
    }))
    .expect("path workload");
    resolved.drivers.insert(DriverCategory::Workload, workload);
    resolved
        .drivers
        .insert(DriverCategory::Route, path.route_driver.clone());
    resolved.network_path = Some(path);
}

#[derive(serde::Serialize)]
struct TestNetworkPathEvidence {
    schema_version: u32,
    route: serde_json::Value,
    route_driver: serde_json::Value,
    eggress_outbound_version: serde_json::Value,
    semantics: serde_json::Value,
    diagnostics: serde_json::Value,
    policy_mode: serde_json::Value,
}

impl eggbench_runner::RunEvidenceContract for TestNetworkPathEvidence {
    const SCHEMA_VERSION: eggbench_core::SchemaVersion = eggbench_core::SchemaVersion(1);

    fn validate_contract(&self) -> Result<(), BundleError> {
        Ok(())
    }
}

struct EvidenceWorkload {
    emit: bool,
    executions: Arc<AtomicUsize>,
    drained: Arc<AtomicBool>,
}

impl WorkloadExecutor for EvidenceWorkload {
    fn execute<'a>(
        &'a mut self,
        _context: InvocationContext,
    ) -> Pin<Box<dyn Future<Output = Result<WorkloadOutput, FailureCategory>> + Send + 'a>> {
        self.executions.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(WorkloadOutput::default()) })
    }

    fn drain<'a>(
        &'a mut self,
        _context: DrainContext,
    ) -> Pin<Box<dyn Future<Output = Result<(), FailureCategory>> + Send + 'a>> {
        self.drained.store(true, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }

    fn run_evidence(&mut self) -> Result<Option<RunEvidenceArtifact>, BundleError> {
        self.emit
            .then(|| {
                RunEvidenceArtifact::from_contract(
                    "network-path.json",
                    name("network-path"),
                    "application/json",
                    Sensitivity::Redacted,
                    &TestNetworkPathEvidence {
                        schema_version: 1,
                        route: serde_json::json!({
                            "driver": "eggress-route",
                            "mode": { "kind": "direct" }
                        }),
                        route_driver: serde_json::json!({
                            "name": "eggress-route",
                            "adapter_version": "0.1.0",
                            "upstream_name": "eggress-outbound",
                            "upstream_version": "1.0.10"
                        }),
                        eggress_outbound_version: serde_json::json!("1.0.10"),
                        semantics: serde_json::json!({
                            "ordering_version": "route-first-fault-second-v1",
                            "ordering": "route_first_fault_second",
                            "fault_layer": "user_space_stream",
                            "upstream": "client_to_target",
                            "downstream": "target_to_client"
                        }),
                        diagnostics: serde_json::json!({
                            "physical_dial_attempts": 0,
                            "successful_dials": 0,
                            "fault_wrapped_connections": 0,
                            "fault_wrapper_construction_failures": 0,
                            "route_failure_buckets_dropped": 0,
                            "route_failures": {},
                            "hop_count_distribution": {},
                            "max_observed_hop_count": 0,
                            "connection_ordinal_min": 0,
                            "connection_ordinal_max": 0,
                            "connection_ordinal_count": 0
                        }),
                        policy_mode: serde_json::json!("static"),
                    },
                )
            })
            .transpose()
    }
}

#[derive(Clone)]
struct FakeReset(Arc<Mutex<Vec<String>>>);
impl ResetHook for FakeReset {
    fn reset<'a>(
        &'a self,
        context: ResetContext,
    ) -> Pin<Box<dyn Future<Output = Result<(), FailureCategory>> + Send + 'a>> {
        Box::pin(async move {
            self.0.lock().unwrap().push(context.target.to_string());
            Ok(())
        })
    }
}

struct PendingReset;
impl ResetHook for PendingReset {
    fn reset<'a>(
        &'a self,
        context: ResetContext,
    ) -> Pin<Box<dyn Future<Output = Result<(), FailureCategory>> + Send + 'a>> {
        Box::pin(async move {
            context.cancellation.cancelled().await;
            Err(FailureCategory::Cancelled)
        })
    }
}

struct FailedReset;
impl ResetHook for FailedReset {
    fn reset<'a>(
        &'a self,
        _context: ResetContext,
    ) -> Pin<Box<dyn Future<Output = Result<(), FailureCategory>> + Send + 'a>> {
        Box::pin(async { Err(FailureCategory::ResetFailed) })
    }
}

#[tokio::test]
async fn run_evidence_is_staged_after_drain_and_before_finalization() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    attach_network_path(&mut resolved);
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let executions = Arc::new(AtomicUsize::new(0));
    let drained = Arc::new(AtomicBool::new(false));
    let mut workload = EvidenceWorkload {
        emit: true,
        executions: Arc::clone(&executions),
        drained: Arc::clone(&drained),
    };
    let outcome = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer(temp.path()),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(executions.load(Ordering::SeqCst), 3);
    assert!(drained.load(Ordering::SeqCst));
    let record = outcome
        .manifest
        .artifacts
        .iter()
        .find(|artifact| artifact.path.as_str() == "network-path.json")
        .expect("path evidence");
    assert!(matches!(
        &record.role,
        ArtifactRole::Other { label } if label.as_str() == "network-path"
    ));
    assert_eq!(record.media_type, "application/json");
    assert_eq!(record.sensitivity, Sensitivity::Redacted);
    BundleReader::open(&outcome.bundle_path)
        .unwrap()
        .verify()
        .unwrap();
}

#[tokio::test]
async fn path_plan_without_run_evidence_fails_closed() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    attach_network_path(&mut resolved);
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = EvidenceWorkload {
        emit: false,
        executions: Arc::new(AtomicUsize::new(0)),
        drained: Arc::new(AtomicBool::new(false)),
    };
    let error = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer(temp.path()),
        &CancellationToken::new(),
    )
    .await
    .expect_err("missing required path evidence must fail");
    assert!(matches!(error, OrchestrationError::Evidence { .. }));
    assert!(!temp.path().join("out.eggb").exists());
}

#[tokio::test]
async fn path_evidence_capacity_is_reserved_before_workload_entry() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    attach_network_path(&mut resolved);
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let executions = Arc::new(AtomicUsize::new(0));
    let mut workload = EvidenceWorkload {
        emit: true,
        executions: Arc::clone(&executions),
        drained: Arc::new(AtomicBool::new(false)),
    };
    let error = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer_with_bounds(temp.path(), 64, 64 * 1024, 1024 * 1024),
        &CancellationToken::new(),
    )
    .await
    .expect_err("undersized path artifact bound must fail");
    assert!(matches!(error, OrchestrationError::Preflight(_)));
    assert_eq!(executions.load(Ordering::SeqCst), 0);
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // Keep this end-to-end evidence flow in one qualification scenario.
async fn schedules_warmup_trials_reset_cooldown_and_finalizes_separate_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    resolved.trials.reset = ResetPolicy::Reference {
        reference: name("reset-app"),
    };
    resolved
        .trials
        .timeouts
        .insert(name("reset"), DurationMs::new(500).unwrap());
    let targets = Arc::new(Mutex::new(Vec::new()));
    let mut resets = ResetRegistry::default();
    resets.register(name("reset-app"), Arc::new(FakeReset(Arc::clone(&targets))));
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    workload.delay = Duration::from_millis(10);
    let result = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &resets,
        &mut TelemetryRegistry::new(),
        writer(temp.path()),
        &CancellationToken::new(),
    )
    .await
    .unwrap();

    assert_eq!(result.execution_status, ExecutionStatus::Completed);
    assert_eq!(result.manifest.comparison_verdict, None);
    assert_eq!(
        result
            .manifest
            .trials
            .iter()
            .map(|trial| trial.id.get())
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(targets.lock().unwrap().as_slice(), &["reset-app"]);
    assert_eq!(
        workload.invocations,
        vec![
            InvocationKind::Warmup { ordinal: 1 },
            InvocationKind::Measured {
                trial_id: eggbench_core::TrialId::new(1).unwrap(),
                arm: None,
            },
            InvocationKind::Measured {
                trial_id: eggbench_core::TrialId::new(2).unwrap(),
                arm: None,
            }
        ]
    );
    assert!(workload.drained);
    assert_eq!(
        result
            .phases
            .iter()
            .map(|event| event.phase)
            .collect::<Vec<_>>(),
        vec![
            PhaseKind::StartupReadiness,
            PhaseKind::Warmup,
            PhaseKind::MeasuredTrial,
            PhaseKind::Reset,
            PhaseKind::Cooldown,
            PhaseKind::MeasuredTrial,
            PhaseKind::Drain,
            PhaseKind::Teardown,
            PhaseKind::Finalization
        ]
    );

    let bundle = BundleReader::open(&result.bundle_path).unwrap();
    bundle.verify().unwrap();
    let manifest = bundle.manifest();
    assert_eq!(manifest.trials.len(), 2);
    assert!(
        manifest
            .artifacts
            .iter()
            .any(|item| item.path.as_str() == "warmups/001/result.json"
                && item.role != ArtifactRole::TrialResult)
    );
    assert!(
        manifest
            .artifacts
            .iter()
            .any(|item| item.path.as_str() == "runner-phases.json")
    );
    let json: eggbench_core::TrialExecutionResult = serde_json::from_slice(
        &std::fs::read(result.bundle_path.join("trials/001/result.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(json.schema_version, eggbench_core::SchemaVersion(2));
    assert_eq!(
        json.terminal_status,
        eggbench_core::TrialExecutionStatus::Completed
    );
    // Unpaired runs stage schema-v2 results with absent arm/pair tags.
    assert_eq!(json.arm, None);
    assert_eq!(json.pair_id, None);
    assert!(json.measurement_elapsed_ns >= 5_000_000);
    let second: eggbench_core::TrialExecutionResult = serde_json::from_slice(
        &std::fs::read(result.bundle_path.join("trials/002/result.json")).unwrap(),
    )
    .unwrap();
    assert!(
        second.measurement_start_offset_ns
            > json.measurement_start_offset_ns + json.measurement_elapsed_ns
    );
}

#[tokio::test]
async fn later_trial_failure_preserves_earlier_trial_and_still_drains() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    resolved.trials.warmup = 0;
    resolved.trials.cooldown_ms = None;
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    workload.fail_on = Some(2);
    let result = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer(temp.path()),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.manifest.trials.len(), 2);
    assert_eq!(workload.invocations.len(), 2);
    assert!(workload.drained);
    let first: eggbench_core::TrialExecutionResult = serde_json::from_slice(
        &std::fs::read(result.bundle_path.join("trials/001/result.json")).unwrap(),
    )
    .unwrap();
    let second: eggbench_core::TrialExecutionResult = serde_json::from_slice(
        &std::fs::read(result.bundle_path.join("trials/002/result.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        first.terminal_status,
        eggbench_core::TrialExecutionStatus::Completed
    );
    assert_eq!(
        second.terminal_status,
        eggbench_core::TrialExecutionStatus::Failed
    );
}

#[tokio::test]
async fn cancellation_during_measurement_records_entered_trial_and_runs_cleanup() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    resolved.trials.warmup = 0;
    resolved.trials.cooldown_ms = None;
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    workload.pending_on = Some(1);
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        trigger.cancel();
    });
    let result = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer(temp.path()),
        &cancel,
    )
    .await
    .unwrap();
    assert_eq!(result.execution_status, ExecutionStatus::Cancelled);
    assert_eq!(result.manifest.trials.len(), 1);
    assert!(workload.drained);
    let trial: eggbench_core::TrialExecutionResult = serde_json::from_slice(
        &std::fs::read(result.bundle_path.join("trials/001/result.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        trial.terminal_status,
        eggbench_core::TrialExecutionStatus::Cancelled
    );
}

#[tokio::test]
async fn cancellation_during_warmup_records_diagnostic_and_skips_trials() {
    let temp = tempfile::tempdir().unwrap();
    let resolved = plan();
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    workload.pending_on = Some(1);
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        trigger.cancel();
    });
    let result = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer(temp.path()),
        &cancel,
    )
    .await
    .unwrap();
    assert_eq!(result.execution_status, ExecutionStatus::Cancelled);
    assert!(result.manifest.trials.is_empty());
    assert!(
        result
            .manifest
            .artifacts
            .iter()
            .any(|artifact| artifact.path.as_str() == "warmups/001/result.json")
    );
    assert!(workload.drained);
}

#[tokio::test]
async fn cancellation_during_teardown_does_not_interrupt_service_cleanup() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    resolved.trials.warmup = 0;
    resolved.trials.cooldown_ms = None;
    resolved.topology.push(Service {
        name: name("sleeper"),
        kind: ServiceKind::Command {
            argv: vec![
                env!("CARGO_BIN_EXE_eggbench-child-fixture").to_owned(),
                "sleep".to_owned(),
                "30000".to_owned(),
            ],
        },
        lifecycle: Lifecycle::Managed,
        depends_on: Vec::new(),
        config: BTreeMap::new(),
        readiness: None,
        shutdown: None,
        working_directory: None,
        log_limit_bytes: 1024,
    });
    let cancel = CancellationToken::new();
    let platform: Arc<dyn PlatformAdapter> = Arc::new(CancelOnTerminate(cancel.clone()));
    let mut options = runner_options(temp.path());
    options.platform = platform;
    let mut session = LocalSession::prepare(&resolved, options).unwrap();
    let mut workload = FakeWorkload::default();
    let result = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer(temp.path()),
        &cancel,
    )
    .await
    .unwrap();
    assert_eq!(result.execution_status, ExecutionStatus::Cancelled);
    assert!(!session.is_running());
    assert!(workload.drained);
    assert!(result.cleanup_failures.is_empty());
}

#[tokio::test]
async fn missing_reset_hook_fails_preflight_before_start_or_drain() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    resolved.trials.reset = ResetPolicy::Service {
        service: name("app"),
    };
    resolved
        .trials
        .timeouts
        .insert(name("reset"), DurationMs::new(500).unwrap());
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    let result = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer(temp.path()),
        &CancellationToken::new(),
    )
    .await;
    assert!(matches!(
        result,
        Err(eggbench_runner::OrchestrationError::Preflight(_))
    ));
    assert!(!workload.drained);
    assert!(!session.is_running());
}

#[tokio::test]
async fn cancellation_before_startup_does_not_fabricate_a_trial() {
    let temp = tempfile::tempdir().unwrap();
    let resolved = plan();
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    let cancel = CancellationToken::new();
    cancel.cancel();
    let result = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer(temp.path()),
        &cancel,
    )
    .await
    .unwrap();
    assert_eq!(result.execution_status, ExecutionStatus::Cancelled);
    assert!(result.manifest.trials.is_empty());
    assert!(workload.invocations.is_empty());
    assert!(workload.drained);
    assert_eq!(result.phases[0].phase, PhaseKind::StartupReadiness);
    assert_eq!(
        result.phases[0].outcome,
        Some(eggbench_runner::PhaseOutcome::Cancelled)
    );
}

#[tokio::test]
async fn warmup_failure_stops_experiment_without_fabricating_a_trial() {
    let temp = tempfile::tempdir().unwrap();
    let resolved = plan();
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    workload.fail_on = Some(1);
    let result = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer(temp.path()),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert!(result.manifest.trials.is_empty());
    assert!(workload.drained);
    assert!(
        result
            .manifest
            .artifacts
            .iter()
            .any(|artifact| artifact.path.as_str() == "warmups/001/result.json")
    );
}

#[tokio::test]
async fn reset_failure_keeps_completed_trial_and_stops_before_next_trial() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    resolved.trials.warmup = 0;
    resolved.trials.reset = ResetPolicy::Reference {
        reference: name("reset-app"),
    };
    resolved
        .trials
        .timeouts
        .insert(name("reset"), DurationMs::new(500).unwrap());
    let mut resets = ResetRegistry::default();
    resets.register(name("reset-app"), Arc::new(FailedReset));
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    let result = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &resets,
        &mut TelemetryRegistry::new(),
        writer(temp.path()),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.manifest.trials.len(), 1);
    assert_eq!(result.primary_failure, Some(FailureCategory::ResetFailed));
    assert!(workload.drained);
}

#[tokio::test]
async fn cancellation_during_reset_never_enters_next_trial() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    resolved.trials.warmup = 0;
    resolved.trials.reset = ResetPolicy::Reference {
        reference: name("reset-app"),
    };
    resolved
        .trials
        .timeouts
        .insert(name("reset"), DurationMs::new(500).unwrap());
    let mut resets = ResetRegistry::default();
    resets.register(name("reset-app"), Arc::new(PendingReset));
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        trigger.cancel();
    });
    let result = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &resets,
        &mut TelemetryRegistry::new(),
        writer(temp.path()),
        &cancel,
    )
    .await
    .unwrap();
    assert_eq!(result.execution_status, ExecutionStatus::Cancelled);
    assert_eq!(result.manifest.trials.len(), 1);
    assert_eq!(workload.invocations.len(), 1);
    assert!(workload.drained);
}

#[tokio::test]
async fn cancellation_during_cooldown_prevents_next_trial_and_still_drains() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    resolved.trials.warmup = 0;
    resolved.trials.cooldown_ms = Some(DurationMs::new(10_000).unwrap());
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        trigger.cancel();
    });
    let result = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer(temp.path()),
        &cancel,
    )
    .await
    .unwrap();
    assert_eq!(result.execution_status, ExecutionStatus::Cancelled);
    assert_eq!(result.manifest.trials.len(), 1);
    assert!(workload.drained);
}

#[tokio::test]
async fn cancellation_during_drain_finishes_cleanup_and_marks_run_cancelled() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    resolved.trials.warmup = 0;
    resolved.trials.cooldown_ms = None;
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    workload.drain_delay = Duration::from_millis(40);
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        trigger.cancel();
    });
    let result = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer(temp.path()),
        &cancel,
    )
    .await
    .unwrap();
    assert_eq!(result.execution_status, ExecutionStatus::Cancelled);
    assert_eq!(result.manifest.trials.len(), 2);
    assert!(workload.drained);
}

#[tokio::test]
async fn unknown_timeout_key_is_rejected_before_startup() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    resolved
        .trials
        .timeouts
        .insert(name("unrecognized"), DurationMs::new(1).unwrap());
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    let result = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer(temp.path()),
        &CancellationToken::new(),
    )
    .await;
    assert!(matches!(
        result,
        Err(eggbench_runner::OrchestrationError::Preflight(_))
    ));
    assert!(workload.invocations.is_empty());
    assert!(!workload.drained);
}

#[tokio::test]
async fn insufficient_evidence_bounds_fail_preflight_before_startup() {
    let temp = tempfile::tempdir().unwrap();
    let resolved = plan();
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    let result = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer_with_bounds(temp.path(), 8, 512, 4 * 1024),
        &CancellationToken::new(),
    )
    .await;
    assert!(matches!(
        result,
        Err(eggbench_runner::OrchestrationError::Preflight(_))
    ));
    assert!(session.events().is_empty());
    assert!(workload.invocations.is_empty());
    assert!(!workload.drained);
}

#[tokio::test]
async fn measured_timeout_records_timed_out_trial_and_drains() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    resolved.trials.warmup = 0;
    resolved.trials.cooldown_ms = None;
    resolved
        .trials
        .timeouts
        .insert(name("measurement"), DurationMs::new(20).unwrap());
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    workload.pending_on = Some(1);
    let result = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer(temp.path()),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.manifest.trials.len(), 1);
    assert!(workload.drained);
    let trial: eggbench_core::TrialExecutionResult = serde_json::from_slice(
        &std::fs::read(result.bundle_path.join("trials/001/result.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        trial.terminal_status,
        eggbench_core::TrialExecutionStatus::TimedOut
    );
    assert_eq!(
        trial.failure_category,
        Some(eggbench_core::TrialExecutionFailure::TimedOut)
    );
}

#[tokio::test]
async fn warmup_timeout_stops_before_any_measured_trial() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    resolved
        .trials
        .timeouts
        .insert(name("warmup"), DurationMs::new(20).unwrap());
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    workload.pending_on = Some(1);
    let result = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer(temp.path()),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert!(result.manifest.trials.is_empty());
    assert_eq!(
        workload.invocations,
        vec![InvocationKind::Warmup { ordinal: 1 }]
    );
    assert!(workload.drained);
}

#[tokio::test]
async fn reset_timeout_retains_completed_trial_and_stops_next_trial() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    resolved.trials.warmup = 0;
    resolved.trials.reset = ResetPolicy::Reference {
        reference: name("reset-app"),
    };
    resolved
        .trials
        .timeouts
        .insert(name("reset"), DurationMs::new(20).unwrap());
    let mut resets = ResetRegistry::default();
    resets.register(name("reset-app"), Arc::new(PendingReset));
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    let result = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &resets,
        &mut TelemetryRegistry::new(),
        writer(temp.path()),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.primary_failure, Some(FailureCategory::TimedOut));
    assert_eq!(result.manifest.trials.len(), 1);
    assert!(workload.drained);
}

#[tokio::test]
async fn drain_failure_changes_only_otherwise_completed_run_to_failed() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    resolved.trials.warmup = 0;
    resolved.trials.cooldown_ms = None;
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    workload.drain_fails = true;
    let result = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer(temp.path()),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.primary_failure, Some(FailureCategory::DrainFailed));
    assert_eq!(result.manifest.trials.len(), 2);
    assert!(
        result
            .phases
            .iter()
            .any(|event| event.phase == PhaseKind::Teardown && event.outcome.is_some())
    );
}

#[tokio::test]
async fn drain_timeout_still_tears_down_and_preserves_trial_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    resolved.trials.warmup = 0;
    resolved.trials.cooldown_ms = None;
    resolved
        .trials
        .timeouts
        .insert(name("drain"), DurationMs::new(20).unwrap());
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    workload.drain_delay = Duration::from_millis(50);
    let result = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer(temp.path()),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.primary_failure, Some(FailureCategory::DrainFailed));
    assert_eq!(result.manifest.trials.len(), 2);
    assert!(
        result
            .phases
            .iter()
            .any(|event| event.phase == PhaseKind::Teardown && event.outcome.is_some())
    );
}

/// Topological fixture that owns a real child process so the cleanup tests prove
/// `LocalSession::shutdown` actually reclaims managed descendants when the run aborts
/// on an evidence error.
fn sleeper_service() -> Service {
    Service {
        name: name("sleeper"),
        kind: ServiceKind::Command {
            argv: vec![
                env!("CARGO_BIN_EXE_eggbench-child-fixture").to_owned(),
                "sleep".to_owned(),
                "30000".to_owned(),
            ],
        },
        lifecycle: Lifecycle::Managed,
        depends_on: Vec::new(),
        config: BTreeMap::new(),
        readiness: None,
        shutdown: None,
        working_directory: None,
        log_limit_bytes: 1024,
    }
}

#[tokio::test]
async fn unsafe_workload_artifact_name_after_measured_invocation_drains_and_tears_down() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    resolved.trials.warmup = 0;
    resolved.trials.cooldown_ms = None;
    resolved.topology.push(sleeper_service());
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    workload.artifacts_by_invocation = vec![Some(vec![WorkloadArtifact {
        name: "../escape".to_owned(),
        media_type: "application/octet-stream".to_owned(),
        bytes: b"x".to_vec(),
    }])];
    let error = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer(temp.path()),
        &CancellationToken::new(),
    )
    .await
    .expect_err("unsafe artifact name must surface as an OrchestrationError");
    let OrchestrationError::Evidence { source, cleanup } = &error else {
        panic!("expected OrchestrationError::Evidence, got {error:?}");
    };
    assert!(
        matches!(
            source,
            BundleError::InvalidManifest("unsafe workload artifact name")
        ),
        "evidence source must be the artifact-safety cause: {source:?}"
    );
    assert!(cleanup.is_empty(), "no cleanup failure was injected");
    assert!(
        workload.drained,
        "drain must be attempted after a measured invocation"
    );
    assert!(!session.is_running(), "managed services must be torn down");
    assert!(
        !temp.path().join("out.eggb").exists(),
        "failed evidence staging must not publish the final bundle"
    );
}

#[tokio::test]
async fn too_many_workload_artifacts_after_measured_invocation_drains_and_tears_down() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    resolved.trials.warmup = 0;
    resolved.trials.cooldown_ms = None;
    resolved.topology.push(sleeper_service());
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    let overflow = (0..=256)
        .map(|i| WorkloadArtifact {
            name: format!("artifact-{i:03}.bin"),
            media_type: "application/octet-stream".to_owned(),
            bytes: vec![0_u8; 4],
        })
        .collect::<Vec<_>>();
    workload.artifacts_by_invocation = vec![Some(overflow)];
    let error = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer(temp.path()),
        &CancellationToken::new(),
    )
    .await
    .expect_err("artifact-count overflow must surface as an OrchestrationError");
    let OrchestrationError::Evidence { source, .. } = &error else {
        panic!("expected OrchestrationError::Evidence, got {error:?}");
    };
    assert!(
        matches!(
            source,
            BundleError::BoundExceeded("workload artifact count")
        ),
        "evidence source must be the artifact-count overflow: {source:?}"
    );
    assert!(
        workload.drained,
        "drain must be attempted after the measured invocation"
    );
    assert!(!session.is_running(), "managed services must be torn down");
}

#[tokio::test]
async fn dynamic_workload_byte_overflow_after_preflight_passes_drains_and_tears_down() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    resolved.trials.warmup = 0;
    resolved.trials.cooldown_ms = None;
    resolved.topology.push(sleeper_service());
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    // The mandatory runner preflight reserves room for the small fixed-size runner
    // artifacts, but cannot know this large dynamic workload payload. The writer
    // has a per-artifact cap that the payload will exceed.
    workload.artifacts_by_invocation = vec![Some(vec![WorkloadArtifact {
        name: "overflow.bin".to_owned(),
        media_type: "application/octet-stream".to_owned(),
        bytes: vec![0xAB; 12 * 1024],
    }])];
    let error = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer_with_bounds(temp.path(), 32, 8 * 1024, 32 * 1024),
        &CancellationToken::new(),
    )
    .await
    .expect_err("dynamic byte overflow must surface as an OrchestrationError");
    let OrchestrationError::Evidence { source, .. } = &error else {
        panic!("expected OrchestrationError::Evidence, got {error:?}");
    };
    assert!(
        matches!(source, BundleError::BoundExceeded(_)),
        "evidence source must be the byte-bound overflow: {source:?}"
    );
    assert!(
        workload.drained,
        "drain must be attempted after the measured invocation"
    );
    assert!(!session.is_running(), "managed services must be torn down");
}

#[tokio::test]
async fn warmup_staging_failure_skips_measured_trials_and_still_drains() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    resolved.trials.warmup = 1;
    resolved.trials.cooldown_ms = None;
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    workload.artifacts_by_invocation = vec![Some(vec![WorkloadArtifact {
        name: "../escape".to_owned(),
        media_type: "application/octet-stream".to_owned(),
        bytes: b"x".to_vec(),
    }])];
    let error = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer(temp.path()),
        &CancellationToken::new(),
    )
    .await
    .expect_err("warmup staging failure must surface as an OrchestrationError");
    let OrchestrationError::Evidence { source, .. } = &error else {
        panic!("expected OrchestrationError::Evidence, got {error:?}");
    };
    assert!(
        matches!(
            source,
            BundleError::InvalidManifest("unsafe workload artifact name")
        ),
        "evidence source must be the warmup staging cause: {source:?}"
    );
    assert_eq!(workload.invocations.len(), 1, "only the warmup entered");
    assert!(
        workload.drained,
        "drain must run after a warmup-staging failure"
    );
    assert!(
        workload
            .invocations
            .first()
            .is_some_and(|kind| matches!(kind, InvocationKind::Warmup { ordinal: 1 })),
        "no measured trial may begin after a warmup staging failure"
    );
}

#[tokio::test]
async fn evidence_error_with_drain_failure_still_tears_down() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    resolved.trials.warmup = 0;
    resolved.trials.cooldown_ms = None;
    resolved.topology.push(sleeper_service());
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    workload.drain_fails = true;
    workload.artifacts_by_invocation = vec![Some(vec![WorkloadArtifact {
        name: "../escape".to_owned(),
        media_type: "application/octet-stream".to_owned(),
        bytes: b"x".to_vec(),
    }])];
    let error = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer(temp.path()),
        &CancellationToken::new(),
    )
    .await
    .expect_err("drain failure after evidence error must still surface");
    let OrchestrationError::Evidence { source, cleanup } = &error else {
        panic!("expected OrchestrationError::Evidence, got {error:?}");
    };
    assert!(
        matches!(
            source,
            BundleError::InvalidManifest("unsafe workload artifact name")
        ),
        "evidence source must remain primary: {source:?}"
    );
    assert!(
        workload.drained,
        "drain must be attempted even though it is configured to fail"
    );
    assert!(
        !session.is_running(),
        "teardown must still run after evidence+drain failure"
    );
    assert!(
        cleanup.is_empty(),
        "no teardown failure was injected; cleanup diagnostics must remain empty"
    );
}

#[tokio::test]
async fn evidence_error_with_teardown_failure_preserves_primary_cause() {
    let cancel = CancellationToken::new();
    let platform: Arc<dyn PlatformAdapter> = Arc::new(FailKillOnStartup);
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    resolved.trials.warmup = 0;
    resolved.trials.cooldown_ms = None;
    resolved.topology.push(sleeper_service());
    let mut options = runner_options(temp.path());
    options.platform = platform;
    let mut session = LocalSession::prepare(&resolved, options).unwrap();
    session.startup(&cancel).await.unwrap();
    let mut workload = FakeWorkload::default();
    workload.artifacts_by_invocation = vec![Some(vec![WorkloadArtifact {
        name: "../escape".to_owned(),
        media_type: "application/octet-stream".to_owned(),
        bytes: b"x".to_vec(),
    }])];
    let error = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer(temp.path()),
        &CancellationToken::new(),
    )
    .await
    .expect_err("evidence error with teardown failure must surface");
    let OrchestrationError::Evidence { source, cleanup } = &error else {
        panic!("expected OrchestrationError::Evidence, got {error:?}");
    };
    assert!(
        matches!(
            source,
            BundleError::InvalidManifest("unsafe workload artifact name")
        ),
        "primary evidence cause must be preserved when teardown also fails: {source:?}"
    );
    assert!(
        !cleanup.is_empty(),
        "teardown failure must be reported as secondary cleanup"
    );
    assert_eq!(cleanup[0].service, "sleeper");
    assert!(workload.drained, "drain must still attempt to run");
}

/// Forces a teardown failure without affecting drain behavior. Lets a test prove that
/// even when teardown reports a cleanup problem, the primary evidence error remains
/// primary.
#[derive(Debug, Clone, Copy)]
struct FailKillOnStartup;
impl PlatformAdapter for FailKillOnStartup {
    fn support(&self) -> PlatformSupport {
        UnixPlatform.support()
    }
    fn label(&self) -> &'static str {
        "fail-kill-on-startup"
    }
    fn is_alive(&self, pid: u32) -> bool {
        UnixPlatform.is_alive(pid)
    }
    fn terminate_group(&self, _pid: u32) -> Result<(), String> {
        Err("injected terminate failure".to_owned())
    }
    fn kill_group(&self, _pid: u32) -> Result<(), String> {
        Err("injected kill failure".to_owned())
    }
}

#[tokio::test]
async fn failed_evidence_staging_does_not_publish_final_bundle() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    resolved.trials.warmup = 0;
    resolved.trials.cooldown_ms = None;
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    workload.artifacts_by_invocation = vec![Some(vec![WorkloadArtifact {
        name: "../escape".to_owned(),
        media_type: "application/octet-stream".to_owned(),
        bytes: b"x".to_vec(),
    }])];
    let error = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer(temp.path()),
        &CancellationToken::new(),
    )
    .await
    .expect_err("evidence error must surface");
    assert!(matches!(error, OrchestrationError::Evidence { .. }));
    assert!(
        !temp.path().join("out.eggb").exists(),
        "failed evidence staging must leave the final path absent"
    );
}

#[tokio::test]
async fn persisted_phase_vector_equals_returned_phase_vector_on_success() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    resolved.trials.warmup = 0;
    resolved.trials.cooldown_ms = None;
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    workload.delay = Duration::from_millis(5);
    let result = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer(temp.path()),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(result.execution_status, ExecutionStatus::Completed);
    assert_eq!(result.manifest.trials.len(), 2);
    let bundle = BundleReader::open(&result.bundle_path).unwrap();
    bundle.verify().unwrap();
    let persisted_path = ArtifactPath::new("runner-phases.json").unwrap();
    let bytes = {
        let mut reader = BufReader::new(bundle.open_artifact(&persisted_path).unwrap());
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).unwrap();
        buf
    };
    let persisted: Vec<PhaseEvent> = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        persisted, result.phases,
        "persisted runner-phases.json must equal RunOutcome.phases"
    );
    assert!(
        persisted.iter().all(|event| event.outcome.is_some()),
        "every persisted event must be terminal"
    );
    assert_eq!(
        persisted
            .iter()
            .filter(|event| event.phase == PhaseKind::Finalization)
            .count(),
        1,
        "exactly one finalization event must be persisted"
    );
    let finalization = persisted
        .iter()
        .find(|event| event.phase == PhaseKind::Finalization)
        .expect("finalization event must exist");
    assert_eq!(
        finalization.outcome,
        Some(eggbench_runner::PhaseOutcome::Completed)
    );
    assert!(
        finalization.elapsed_ns.is_some(),
        "finalization must carry one terminal duration"
    );
}

#[tokio::test]
async fn finalization_event_is_terminalized_exactly_once() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    resolved.trials.warmup = 0;
    resolved.trials.cooldown_ms = None;
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    let result = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer(temp.path()),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(
        result
            .phases
            .iter()
            .filter(|event| event.phase == PhaseKind::Finalization)
            .count(),
        1,
        "exactly one finalization event must be in the returned phase vector"
    );
    let finalization = result
        .phases
        .iter()
        .find(|event| event.phase == PhaseKind::Finalization)
        .expect("finalization event must exist");
    assert_eq!(
        finalization.outcome,
        Some(eggbench_runner::PhaseOutcome::Completed)
    );
    assert!(
        finalization.elapsed_ns.is_some(),
        "finalization must have one terminal duration"
    );
}

// ---- Named-service cleanup invariants under failure ----

use eggbench_runner::{
    BoxFuture, ManagedServiceAdapter, ManagedServiceHandle, RuntimeBindings, ServiceStartRequest,
};
use tokio::sync::Mutex as AsyncMutex;

/// Minimal adapter double: records start/stop, optionally fails shutdown.
#[derive(Debug)]
struct StopRecorder {
    events: Arc<AsyncMutex<Vec<String>>>,
    shutdown_error: Option<String>,
}

impl ManagedServiceAdapter for StopRecorder {
    fn service_type(&self) -> &'static str {
        "fake-svc"
    }

    fn start(
        &self,
        request: ServiceStartRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'_, Result<Box<dyn ManagedServiceHandle>, String>> {
        Box::pin(async move {
            if cancel.is_cancelled() {
                return Err("cancelled".to_owned());
            }
            self.events
                .lock()
                .await
                .push(format!("start:{}", request.service));
            Ok(Box::new(StopHandle {
                identity: request.service,
                events: Arc::clone(&self.events),
                shutdown_error: self.shutdown_error.clone(),
            }) as Box<dyn ManagedServiceHandle>)
        })
    }
}

#[derive(Debug)]
struct StopHandle {
    identity: String,
    events: Arc<AsyncMutex<Vec<String>>>,
    shutdown_error: Option<String>,
}

impl ManagedServiceHandle for StopHandle {
    fn bindings(&self) -> RuntimeBindings {
        RuntimeBindings::new()
    }

    fn shutdown(&mut self, _grace: Duration) -> BoxFuture<'_, Result<(), String>> {
        Box::pin(async move {
            self.events
                .lock()
                .await
                .push(format!("stop:{}", self.identity));
            match &self.shutdown_error {
                Some(reason) => Err(reason.clone()),
                None => Ok(()),
            }
        })
    }
}

fn named_service(service: &str) -> Service {
    Service {
        name: name(service),
        kind: ServiceKind::Named {
            service_type: name("fake-svc"),
        },
        lifecycle: Lifecycle::Managed,
        depends_on: Vec::new(),
        config: BTreeMap::new(),
        readiness: None,
        shutdown: None,
        working_directory: None,
        log_limit_bytes: 4096,
    }
}

fn options_with_recorder(
    root: &Path,
    recorder: StopRecorder,
) -> (RunnerOptions, Arc<AsyncMutex<Vec<String>>>) {
    let events = Arc::clone(&recorder.events);
    let mut registry = ServiceAdapterRegistry::new();
    registry.register(Arc::new(recorder)).expect("test adapter");
    (
        RunnerOptions {
            workspace_root: root.to_path_buf(),
            secrets: Arc::new(MapSecretProvider::empty()),
            probes: eggbench_runner::ProbeRegistry::with_builtins(),
            platform: Arc::new(UnixPlatform),
            service_adapters: registry,
        },
        events,
    )
}

#[tokio::test]
async fn evidence_staging_failure_after_startup_still_cleans_up_adapter() {
    let temp = tempfile::tempdir().unwrap();
    let events = Arc::new(AsyncMutex::new(Vec::new()));
    let (runner_options, events) = options_with_recorder(
        temp.path(),
        StopRecorder {
            events: Arc::clone(&events),
            shutdown_error: None,
        },
    );
    let mut resolved = plan();
    resolved.topology = vec![named_service("origin")];
    let mut session = LocalSession::prepare(&resolved, runner_options).unwrap();
    // An unsafe workload artifact name fails staging after the adapter has
    // started; teardown must still own the adapter exactly once.
    let mut workload = FakeWorkload::default();
    workload.artifacts_by_invocation = vec![Some(vec![WorkloadArtifact {
        name: "evil/x".to_owned(),
        media_type: "application/octet-stream".to_owned(),
        bytes: vec![1],
    }])];
    let error = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer(temp.path()),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(error, OrchestrationError::Evidence { .. }),
        "staging failure is the evidence cause, got {error:?}"
    );
    assert_eq!(
        events.lock().await.as_slice(),
        ["start:origin", "stop:origin"]
    );
    // Nothing remains owned after the failed run.
    let shutdown = session.shutdown().await;
    assert!(shutdown.stopped_order.is_empty());
    assert!(shutdown.failures.is_empty());
}

#[tokio::test]
async fn adapter_shutdown_failure_does_not_overwrite_workload_failure() {
    let temp = tempfile::tempdir().unwrap();
    let events = Arc::new(AsyncMutex::new(Vec::new()));
    let (runner_options, _) = options_with_recorder(
        temp.path(),
        StopRecorder {
            events: Arc::clone(&events),
            shutdown_error: Some("adapter stop failed".to_owned()),
        },
    );
    let mut resolved = plan();
    resolved.topology = vec![named_service("origin")];
    let mut session = LocalSession::prepare(&resolved, runner_options).unwrap();
    let mut workload = FakeWorkload::default();
    workload.fail_on = Some(1);
    let outcome = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer(temp.path()),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    // The workload failure stays primary; the teardown failure is attached
    // as cleanup evidence without rewriting it.
    assert_eq!(
        outcome.primary_failure,
        Some(FailureCategory::WorkloadFailed)
    );
    assert_eq!(outcome.cleanup_failures.len(), 1);
    assert_eq!(outcome.cleanup_failures[0].service, "origin");
    assert_eq!(
        events.lock().await.as_slice(),
        ["start:origin", "stop:origin"]
    );
}

fn paired_plan() -> ResolvedPlan {
    let mut resolved = plan();
    resolved.trials.measured = PositiveCount::new(4).unwrap();
    resolved.trials.warmup = 2;
    resolved.defaults.measured_trials = 4;
    resolved.defaults.warmup_trials = 2;
    resolved.subject = Subject::Label {
        label: name("a-vs-b"),
    };
    resolved.paired = Some(ResolvedPairedDesign {
        baseline: ResolvedPairedArm {
            service: name("origin-a"),
            subject: Subject::Label {
                label: name("variant-a"),
            },
        },
        candidate: ResolvedPairedArm {
            service: name("origin-b"),
            subject: Subject::Label {
                label: name("variant-b"),
            },
        },
        schedule: eggbench_core::PAIRED_SCHEDULE_V1.to_owned(),
        pairs: 2,
    });
    resolved
}

fn read_trial_result(bundle: &std::path::Path, result: &ArtifactPath) -> TrialExecutionResult {
    let reader = BundleReader::open(bundle).unwrap();
    let mut file = reader.open_artifact(result).unwrap();
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut file, &mut bytes).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn paired_schedule_alternates_arms_targets_and_pair_identities() {
    let temp = tempfile::tempdir().unwrap();
    let resolved = paired_plan();
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    let outcome = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer(temp.path()),
        &CancellationToken::new(),
    )
    .await
    .unwrap();

    assert_eq!(outcome.execution_status, ExecutionStatus::Completed);
    // Warmups alternate arms round-robin; measured trials strictly alternate
    // baseline/candidate with arm tags.
    assert_eq!(
        workload.invocations,
        vec![
            InvocationKind::Warmup { ordinal: 1 },
            InvocationKind::Warmup { ordinal: 2 },
            InvocationKind::Measured {
                trial_id: eggbench_core::TrialId::new(1).unwrap(),
                arm: Some(TrialArm::Baseline),
            },
            InvocationKind::Measured {
                trial_id: eggbench_core::TrialId::new(2).unwrap(),
                arm: Some(TrialArm::Candidate),
            },
            InvocationKind::Measured {
                trial_id: eggbench_core::TrialId::new(3).unwrap(),
                arm: Some(TrialArm::Baseline),
            },
            InvocationKind::Measured {
                trial_id: eggbench_core::TrialId::new(4).unwrap(),
                arm: Some(TrialArm::Candidate),
            },
        ]
    );
    // Every invocation directed load at the scheduled arm's service while the
    // resolved load shape stayed identical (the fake ignores bindings, so the
    // recorded target is the proof of per-trial override).
    assert_eq!(
        workload.workload_targets.as_slice(),
        &[
            "origin-a", "origin-b", "origin-a", "origin-b", "origin-a", "origin-b"
        ]
    );
    // Staged evidence carries arm and pair identities in execution order.
    let bundle = temp.path().join("out.eggb");
    let expected = [
        (Some(TrialArm::Baseline), Some(1)),
        (Some(TrialArm::Candidate), Some(1)),
        (Some(TrialArm::Baseline), Some(2)),
        (Some(TrialArm::Candidate), Some(2)),
    ];
    assert_eq!(outcome.manifest.trials.len(), 4);
    for (descriptor, (arm, pair_id)) in outcome.manifest.trials.iter().zip(expected) {
        let result = read_trial_result(&bundle, &descriptor.result);
        assert_eq!(result.arm, arm, "trial {}", descriptor.id.get());
        assert_eq!(result.pair_id, pair_id, "trial {}", descriptor.id.get());
        assert_eq!(result.schema_version, eggbench_core::SchemaVersion(2));
    }
    // Arm-namespaced seeds differ between the two trials of a pair even
    // though both derive from the same plan seed, and across pairs too.
    let seeds = &workload.invocation_seeds;
    assert_eq!(seeds.len(), 6);
    let measured: Vec<u64> = seeds[2..].iter().map(|seed| seed.unwrap()).collect();
    assert_eq!(measured.len(), 4);
    let unique: std::collections::BTreeSet<u64> = measured.iter().copied().collect();
    assert_eq!(
        unique.len(),
        4,
        "every paired trial gets its own seed stream"
    );
}

#[tokio::test]
async fn paired_run_with_odd_measured_count_fails_before_startup() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = paired_plan();
    resolved.trials.measured = PositiveCount::new(3).unwrap();
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    let error = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer(temp.path()),
        &CancellationToken::new(),
    )
    .await
    .expect_err("odd paired trial count must fail preflight");
    assert!(matches!(error, OrchestrationError::Preflight(_)));
    assert!(workload.invocations.is_empty());
}

#[tokio::test]
async fn paired_odd_warmup_count_alternates_deterministically() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = paired_plan();
    resolved.trials.measured = PositiveCount::new(2).unwrap();
    resolved.trials.warmup = 1;
    resolved.defaults.measured_trials = 2;
    resolved.defaults.warmup_trials = 1;
    resolved.paired.as_mut().unwrap().pairs = 1;
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    let outcome = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer(temp.path()),
        &CancellationToken::new(),
    )
    .await
    .unwrap();

    assert_eq!(outcome.execution_status, ExecutionStatus::Completed);
    // The single warmup takes the baseline arm; measured trials alternate.
    assert_eq!(
        workload.invocations,
        vec![
            InvocationKind::Warmup { ordinal: 1 },
            InvocationKind::Measured {
                trial_id: eggbench_core::TrialId::new(1).unwrap(),
                arm: Some(TrialArm::Baseline),
            },
            InvocationKind::Measured {
                trial_id: eggbench_core::TrialId::new(2).unwrap(),
                arm: Some(TrialArm::Candidate),
            },
        ]
    );
    assert_eq!(
        workload.workload_targets.as_slice(),
        &["origin-a", "origin-a", "origin-b"]
    );
}

// Eggstack M003b: one-shot pre/post workload diagnostics run outside every
// measured interval through the sibling-neutral DiagnosticRegistry seam.
// These tests use FakeDiagnosticExecutor (no external binary) to prove
// lifecycle ordering, required/optional policy, failure precedence, and
// evidence staging; tool-contract parsing is covered by the Eggprobe
// adapter unit tests.

use eggbench_core::{DiagnosticPhase, DiagnosticProbe, DiagnosticRequest};
use eggbench_runner::{FakeDiagnosticExecutor, execute_run_with_diagnostics};

fn diagnostic_request(id: &str, phase: DiagnosticPhase, required: bool) -> DiagnosticRequest {
    DiagnosticRequest {
        id: name(id),
        source: name("eggprobe"),
        phase,
        target: name("app"),
        probes: vec![DiagnosticProbe::Tcp],
        required,
        timeout_ms: DurationMs::new(5000).unwrap(),
    }
}

/// Managed `sleep` service so post-workload diagnostics have live services.
/// Post diagnostics run after drain while services are still alive; with no
/// started services the post slot is skipped by design.
fn managed_sleep_service() -> Service {
    Service {
        name: name("app"),
        kind: ServiceKind::Command {
            argv: vec![
                env!("CARGO_BIN_EXE_eggbench-child-fixture").to_owned(),
                "sleep".to_owned(),
                "30000".to_owned(),
            ],
        },
        lifecycle: Lifecycle::Managed,
        depends_on: Vec::new(),
        config: BTreeMap::new(),
        readiness: None,
        shutdown: None,
        working_directory: None,
        log_limit_bytes: 4096,
    }
}

fn diagnostic_registry_for(negative_ids: &[&str], fail_ids: &[&str]) -> DiagnosticRegistry {
    let mut fake = FakeDiagnosticExecutor::new("eggprobe");
    fake.negative_ids = negative_ids.iter().map(|id| (*id).to_owned()).collect();
    fake.fail_ids = fail_ids.iter().map(|id| (*id).to_owned()).collect();
    let mut registry = DiagnosticRegistry::new();
    registry.register(Box::new(fake));
    registry
}

fn phase_kinds(outcome: &eggbench_runner::RunOutcome) -> Vec<PhaseKind> {
    outcome.phases.iter().map(|event| event.phase).collect()
}

fn read_diagnostics_index(outcome: &eggbench_runner::RunOutcome) -> serde_json::Value {
    let reader = BundleReader::open(&outcome.bundle_path).unwrap();
    reader.verify().unwrap();
    let record = outcome
        .manifest
        .artifacts
        .iter()
        .find(|artifact| artifact.path.as_str() == "diagnostics.json")
        .expect("diagnostics index staged");
    let mut file = reader.open_artifact(&record.path).unwrap();
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn pre_and_post_diagnostics_run_around_workload_and_stage_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    resolved.topology = vec![managed_sleep_service()];
    resolved.diagnostics = vec![
        diagnostic_request("pre-check", DiagnosticPhase::PreWorkload, true),
        diagnostic_request("post-check", DiagnosticPhase::PostWorkload, true),
        diagnostic_request("both-check", DiagnosticPhase::Both, false),
    ];
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    let mut diagnostics = diagnostic_registry_for(&[], &[]);
    let outcome = execute_run_with_diagnostics(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        &mut diagnostics,
        &mut CorrectnessRegistry::new(),
        writer(temp.path()),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(outcome.execution_status, ExecutionStatus::Completed);
    // Pre diagnostics run after readiness/before warmups; post diagnostics
    // run after drain/before teardown. Both executes twice.
    let kinds = phase_kinds(&outcome);
    let first_pre = kinds
        .iter()
        .position(|kind| *kind == PhaseKind::DiagnosticsPre)
        .unwrap();
    let first_warmup = kinds
        .iter()
        .position(|kind| *kind == PhaseKind::Warmup)
        .unwrap();
    let last_trial = kinds
        .iter()
        .rposition(|kind| *kind == PhaseKind::MeasuredTrial)
        .unwrap_or(first_warmup);
    let first_post = kinds
        .iter()
        .position(|kind| *kind == PhaseKind::DiagnosticsPost)
        .unwrap();
    let teardown = kinds
        .iter()
        .position(|kind| *kind == PhaseKind::Teardown)
        .unwrap();
    assert!(first_pre < first_warmup, "pre before warmups: {kinds:?}");
    assert!(last_trial < first_post, "post after trials: {kinds:?}");
    assert!(first_post < teardown, "post before teardown: {kinds:?}");
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == PhaseKind::DiagnosticsPre)
            .count(),
        2,
        "pre + both-pre: {kinds:?}"
    );
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == PhaseKind::DiagnosticsPost)
            .count(),
        2,
        "post + both-post: {kinds:?}"
    );
    // Run-level index carries four executions in plan order with provenance.
    let index = read_diagnostics_index(&outcome);
    assert_eq!(index["schema_version"], 1);
    assert_eq!(index["driver"], "eggprobe");
    assert_eq!(index["machine_schema"], "0.3");
    let executions = index["executions"].as_array().unwrap();
    assert_eq!(executions.len(), 4);
    assert_eq!(executions[0]["id"], "pre-check");
    assert_eq!(executions[0]["phase"], "pre_workload");
    assert_eq!(executions[3]["id"], "both-check");
    assert_eq!(executions[3]["phase"], "post_workload");
    for execution in executions {
        assert_eq!(execution["disposition"], "positive");
        assert!(!execution["artifact_sha256"].as_str().unwrap().is_empty());
    }
}

#[tokio::test]
async fn required_pre_negative_invalidates_without_workload() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    resolved.diagnostics = vec![diagnostic_request(
        "pre-check",
        DiagnosticPhase::PreWorkload,
        true,
    )];
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    let mut diagnostics = diagnostic_registry_for(&["pre-check"], &[]);
    let outcome = execute_run_with_diagnostics(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        &mut diagnostics,
        &mut CorrectnessRegistry::new(),
        writer(temp.path()),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(outcome.execution_status, ExecutionStatus::Invalid);
    assert_eq!(
        outcome.primary_failure,
        Some(FailureCategory::DiagnosticFailed)
    );
    assert!(
        workload.invocations.is_empty(),
        "no warmup/measured workload began"
    );
    // Teardown still runs through the common cleanup tail.
    assert!(phase_kinds(&outcome).contains(&PhaseKind::Teardown));
    let index = read_diagnostics_index(&outcome);
    assert_eq!(index["executions"][0]["disposition"], "negative");
}

#[tokio::test]
async fn optional_pre_negative_continues_with_warning_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    resolved.diagnostics = vec![diagnostic_request(
        "pre-check",
        DiagnosticPhase::PreWorkload,
        false,
    )];
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    let mut diagnostics = diagnostic_registry_for(&["pre-check"], &[]);
    let outcome = execute_run_with_diagnostics(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        &mut diagnostics,
        &mut CorrectnessRegistry::new(),
        writer(temp.path()),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(outcome.execution_status, ExecutionStatus::Completed);
    assert!(!workload.invocations.is_empty(), "workload continued");
    let index = read_diagnostics_index(&outcome);
    assert_eq!(index["executions"][0]["disposition"], "negative");
}

#[tokio::test]
async fn required_post_negative_invalidates_completed_run() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    resolved.topology = vec![managed_sleep_service()];
    resolved.diagnostics = vec![diagnostic_request(
        "post-check",
        DiagnosticPhase::PostWorkload,
        true,
    )];
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    let mut diagnostics = diagnostic_registry_for(&["post-check"], &[]);
    let outcome = execute_run_with_diagnostics(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        &mut diagnostics,
        &mut CorrectnessRegistry::new(),
        writer(temp.path()),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(outcome.execution_status, ExecutionStatus::Invalid);
    assert_eq!(
        outcome.primary_failure,
        Some(FailureCategory::DiagnosticFailed)
    );
    assert!(!workload.invocations.is_empty(), "workload ran before post");
}

#[tokio::test]
async fn post_negative_never_masks_workload_failure() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    resolved.topology = vec![managed_sleep_service()];
    resolved.diagnostics = vec![diagnostic_request(
        "post-check",
        DiagnosticPhase::PostWorkload,
        true,
    )];
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    workload.fail_on = Some(1);
    let mut diagnostics = diagnostic_registry_for(&["post-check"], &[]);
    let outcome = execute_run_with_diagnostics(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        &mut diagnostics,
        &mut CorrectnessRegistry::new(),
        writer(temp.path()),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(outcome.execution_status, ExecutionStatus::Failed);
    assert_eq!(
        outcome.primary_failure,
        Some(FailureCategory::WorkloadFailed)
    );
    // Post diagnostics still ran as failure context.
    let index = read_diagnostics_index(&outcome);
    assert_eq!(index["executions"][0]["disposition"], "negative");
}

#[tokio::test]
async fn cancelled_run_skips_diagnostics_and_still_tears_down() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    resolved.diagnostics = vec![
        diagnostic_request("pre-check", DiagnosticPhase::PreWorkload, true),
        diagnostic_request("post-check", DiagnosticPhase::PostWorkload, true),
    ];
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    let mut diagnostics = diagnostic_registry_for(&[], &[]);
    let cancel = CancellationToken::new();
    cancel.cancel();
    let outcome = execute_run_with_diagnostics(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        &mut diagnostics,
        &mut CorrectnessRegistry::new(),
        writer(temp.path()),
        &cancel,
    )
    .await
    .unwrap();
    assert_eq!(outcome.execution_status, ExecutionStatus::Cancelled);
    assert!(workload.invocations.is_empty());
    // No diagnostic executed; nothing is staged and teardown is not faked.
    assert!(
        outcome
            .manifest
            .artifacts
            .iter()
            .all(|artifact| artifact.path.as_str() != "diagnostics.json"),
        "no diagnostics index without executions"
    );
}

#[tokio::test]
async fn diagnostic_evidence_never_enters_trial_metrics() {
    use eggbench_core::{Aggregation, RawMetricObservation};
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    resolved.diagnostics = vec![diagnostic_request(
        "pre-check",
        DiagnosticPhase::PreWorkload,
        true,
    )];
    resolved.metrics = vec![eggbench_core::MetricRequest {
        name: name("latency_p99"),
        unit: name("ms"),
        direction: eggbench_core::MetricDirection::LowerIsBetter,
        intent: eggbench_core::MetricIntent::Primary,
        gate: None,
    }];
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    workload.metrics_by_invocation = vec![
        None,
        Some(vec![RawMetricObservation {
            name: "latency_p99".to_owned(),
            unit: "ms".to_owned(),
            value: 3.0,
            aggregation: Aggregation::Direct,
            source_field: None,
            producer: None,
            producer_version: None,
            raw_artifacts: Vec::new(),
        }]),
        Some(vec![RawMetricObservation {
            name: "latency_p99".to_owned(),
            unit: "ms".to_owned(),
            value: 4.0,
            aggregation: Aggregation::Direct,
            source_field: None,
            producer: None,
            producer_version: None,
            raw_artifacts: Vec::new(),
        }]),
    ];
    let mut diagnostics = diagnostic_registry_for(&[], &[]);
    let outcome = execute_run_with_diagnostics(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        &mut diagnostics,
        &mut CorrectnessRegistry::new(),
        writer(temp.path()),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(outcome.execution_status, ExecutionStatus::Completed);
    let reader = BundleReader::open(&outcome.bundle_path).unwrap();
    for descriptor in &outcome.manifest.trials {
        let metrics = reader.trial_metrics(descriptor.id).unwrap().unwrap();
        let names: Vec<&str> = metrics
            .observations
            .iter()
            .map(|observation| observation.name.as_str())
            .collect();
        assert_eq!(
            names,
            vec!["latency_p99"],
            "only workload metrics: {names:?}"
        );
    }
    // Diagnostic evidence exists alongside, never inside, trial metrics.
    let index = read_diagnostics_index(&outcome);
    assert_eq!(index["executions"].as_array().unwrap().len(), 1);
}

// ---- Eggstack M004a security-correctness lifecycle tests ----
//
// These tests use FakeCorrectnessExecutor (no external binary) to prove
// lifecycle placement, fail-continues semantics, operational-failure
// precedence, and evidence staging; tool-contract parsing is covered by the
// Eggsec adapter unit tests.

use eggbench_core::{EggsecWafTestType, SecurityCheckRequest};
use eggbench_runner::{CorrectnessRegistry as M004aCorrectnessRegistry, FakeCorrectnessExecutor};

fn security_request(id: &str) -> SecurityCheckRequest {
    SecurityCheckRequest {
        id: name(id),
        source: name("eggsec-waf"),
        target: name("app"),
        test_type: EggsecWafTestType::Sqli,
        max_successful_bypasses: 0,
        concurrency: PositiveCount::new(2).unwrap(),
        timeout_ms: DurationMs::new(5_000).unwrap(),
    }
}

fn correctness_registry_for(fail_ids: &[&str], invalid_ids: &[&str]) -> M004aCorrectnessRegistry {
    let mut fake = FakeCorrectnessExecutor::new("eggsec-waf");
    fake.fail_ids = fail_ids.iter().map(|id| (*id).to_owned()).collect();
    fake.invalid_ids = invalid_ids.iter().map(|id| (*id).to_owned()).collect();
    let mut registry = M004aCorrectnessRegistry::new();
    registry.register(Box::new(fake));
    registry
}

fn read_security_index(outcome: &eggbench_runner::RunOutcome) -> serde_json::Value {
    let reader = BundleReader::open(&outcome.bundle_path).unwrap();
    reader.verify().unwrap();
    let record = outcome
        .manifest
        .artifacts
        .iter()
        .find(|artifact| artifact.path.as_str() == "security-checks.json")
        .expect("security index staged");
    let mut file = reader.open_artifact(&record.path).unwrap();
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn correctness_runs_after_readiness_before_warmups_and_stages_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    resolved.topology = vec![managed_sleep_service()];
    resolved.security_checks = vec![security_request("waf-sqli")];
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    let mut diagnostics = DiagnosticRegistry::new();
    let mut correctness = correctness_registry_for(&[], &[]);
    let outcome = execute_run_with_diagnostics(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        &mut diagnostics,
        &mut correctness,
        writer(temp.path()),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(outcome.execution_status, ExecutionStatus::Completed);
    // Correctness runs after readiness/before warmups.
    let kinds = phase_kinds(&outcome);
    let readiness = kinds
        .iter()
        .position(|kind| *kind == PhaseKind::StartupReadiness)
        .unwrap();
    let correctness_pos = kinds
        .iter()
        .position(|kind| *kind == PhaseKind::CorrectnessChecks)
        .unwrap();
    let first_warmup = kinds
        .iter()
        .position(|kind| *kind == PhaseKind::Warmup)
        .unwrap();
    assert!(
        readiness < correctness_pos,
        "correctness after readiness: {kinds:?}"
    );
    assert!(
        correctness_pos < first_warmup,
        "correctness before warmups: {kinds:?}"
    );
    // Run-level index carries one pass execution with provenance.
    let index = read_security_index(&outcome);
    assert_eq!(index["schema_version"], 1);
    assert_eq!(index["driver"], "eggsec-waf");
    assert_eq!(index["operation"], "waf --json --bypass");
    let checks = index["checks"].as_array().unwrap();
    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0]["id"], "waf-sqli");
    assert_eq!(checks[0]["disposition"], "pass");
    assert!(!index["executable_sha256"].as_str().unwrap().is_empty());
    assert!(!index["scope_sha256"].as_str().unwrap().is_empty());
}

#[tokio::test]
async fn correctness_fail_still_permits_warmups_and_trials() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    resolved.topology = vec![managed_sleep_service()];
    resolved.security_checks = vec![security_request("waf-sqli")];
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    let mut diagnostics = DiagnosticRegistry::new();
    let mut correctness = correctness_registry_for(&["waf-sqli"], &[]);
    let outcome = execute_run_with_diagnostics(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        &mut diagnostics,
        &mut correctness,
        writer(temp.path()),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    // A valid Fail observation never fails execution and never suppresses
    // performance trials.
    assert_eq!(outcome.execution_status, ExecutionStatus::Completed);
    assert!(outcome.primary_failure.is_none());
    assert_eq!(outcome.manifest.trials.len(), 2);
    let kinds = phase_kinds(&outcome);
    assert!(kinds.contains(&PhaseKind::Warmup));
    assert!(kinds.contains(&PhaseKind::MeasuredTrial));
    let index = read_security_index(&outcome);
    assert_eq!(
        index["checks"].as_array().unwrap()[0]["disposition"],
        "fail"
    );
}

#[tokio::test]
async fn correctness_operational_failure_invalidates_and_still_cleans_up() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    resolved.topology = vec![managed_sleep_service()];
    resolved.security_checks = vec![security_request("waf-sqli")];
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    let mut diagnostics = DiagnosticRegistry::new();
    let mut correctness = correctness_registry_for(&[], &["waf-sqli"]);
    let outcome = execute_run_with_diagnostics(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        &mut diagnostics,
        &mut correctness,
        writer(temp.path()),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    // Operational correctness failure is an execution-validity problem:
    // workload is skipped and cleanup still runs through the common tail.
    assert_eq!(outcome.execution_status, ExecutionStatus::Invalid);
    assert_eq!(
        outcome.primary_failure,
        Some(FailureCategory::CorrectnessFailed)
    );
    assert!(outcome.manifest.trials.is_empty());
    assert!(!session.is_running(), "managed services must be torn down");
    let kinds = phase_kinds(&outcome);
    assert!(kinds.contains(&PhaseKind::CorrectnessChecks));
    assert!(kinds.contains(&PhaseKind::Teardown));
    assert!(!kinds.contains(&PhaseKind::Warmup));
}

#[tokio::test]
async fn correctness_runs_between_pre_diagnostics_and_warmups() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    resolved.topology = vec![managed_sleep_service()];
    resolved.diagnostics = vec![diagnostic_request(
        "pre-check",
        DiagnosticPhase::PreWorkload,
        false,
    )];
    resolved.security_checks = vec![security_request("waf-sqli")];
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    let mut diagnostics = diagnostic_registry_for(&[], &[]);
    let mut correctness = correctness_registry_for(&[], &[]);
    let outcome = execute_run_with_diagnostics(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        &mut diagnostics,
        &mut correctness,
        writer(temp.path()),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(outcome.execution_status, ExecutionStatus::Completed);
    let kinds = phase_kinds(&outcome);
    let pre = kinds
        .iter()
        .position(|kind| *kind == PhaseKind::DiagnosticsPre)
        .unwrap();
    let correctness_pos = kinds
        .iter()
        .position(|kind| *kind == PhaseKind::CorrectnessChecks)
        .unwrap();
    let warmup = kinds
        .iter()
        .position(|kind| *kind == PhaseKind::Warmup)
        .unwrap();
    assert!(
        pre < correctness_pos,
        "correctness after pre diagnostics: {kinds:?}"
    );
    assert!(
        correctness_pos < warmup,
        "correctness before warmups: {kinds:?}"
    );
}

#[tokio::test]
async fn correctness_fail_leaves_trial_metrics_untouched() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    resolved.security_checks = vec![security_request("waf-sqli")];
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    let mut diagnostics = DiagnosticRegistry::new();
    let mut correctness = correctness_registry_for(&["waf-sqli"], &[]);
    let outcome = execute_run_with_diagnostics(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        &mut diagnostics,
        &mut correctness,
        writer(temp.path()),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(outcome.execution_status, ExecutionStatus::Completed);
    // Security observations never enter TrialMetrics: measured trials carry
    // no security-derived observations.
    let reader = BundleReader::open(&outcome.bundle_path).unwrap();
    for descriptor in &outcome.manifest.trials {
        let metrics = reader.trial_metrics(descriptor.id).unwrap().unwrap();
        for observation in &metrics.observations {
            assert!(
                !observation.name.as_str().contains("bypass")
                    && !observation.name.as_str().contains("security")
                    && !observation.name.as_str().contains("waf"),
                "no security metric in trials: {}",
                observation.name.as_str()
            );
        }
    }
}

#[tokio::test]
async fn security_evidence_never_leaks_unrelated_secrets() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan();
    // Secret-like sentinel in unrelated plan content (staged legitimately
    // in the resolved plan): no security artifact may echo it.
    let sentinel = "EGGBENCH_SENTINEL_SECRET_9f8e7d6c5b4a";
    resolved.topology = vec![Service {
        name: name("app"),
        kind: ServiceKind::Named {
            service_type: name("http"),
        },
        lifecycle: Lifecycle::External,
        depends_on: Vec::new(),
        config: [(String::from("note"), sentinel.to_owned())]
            .into_iter()
            .collect(),
        readiness: None,
        shutdown: None,
        working_directory: None,
        log_limit_bytes: 4096,
    }];
    resolved.security_checks = vec![security_request("waf-sqli")];
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    let mut diagnostics = DiagnosticRegistry::new();
    let mut correctness = correctness_registry_for(&["waf-sqli"], &[]);
    let outcome = execute_run_with_diagnostics(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        &mut diagnostics,
        &mut correctness,
        writer(temp.path()),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(outcome.execution_status, ExecutionStatus::Completed);
    let reader = BundleReader::open(&outcome.bundle_path).unwrap();
    for artifact in &outcome.manifest.artifacts {
        if artifact.path.as_str() == "security-checks.json"
            || artifact.path.as_str().starts_with("security/")
        {
            let mut file = reader.open_artifact(&artifact.path).unwrap();
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).unwrap();
            let text = String::from_utf8_lossy(&bytes);
            assert!(
                !text.contains(sentinel),
                "security artifact {} leaks unrelated secret",
                artifact.path.as_str()
            );
        }
    }
}
