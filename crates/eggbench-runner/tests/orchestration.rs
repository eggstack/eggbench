//! End-to-end qualification for the M002 phase coordinator.
#![cfg(unix)]

use eggbench_core::{
    ArtifactBounds, ArtifactPath, ArtifactRole, BundleReader, BundleWriter, DurationMs,
    ExecutionStatus, Lifecycle, Name, PositiveCount, ResetPolicy, ResolvedPlan, RunId, Sensitivity,
    Service, ServiceKind, Subject, TrialPolicy, Workload,
};
use eggbench_runner::test_support::FakeWorkload;
use eggbench_runner::{
    FailureCategory, InvocationKind, LocalSession, MapSecretProvider, PhaseKind, PlatformAdapter,
    PlatformSupport, ResetContext, ResetHook, ResetRegistry, RunnerOptions, UnixPlatform,
    execute_run,
};
use std::{
    collections::BTreeMap,
    future::Future,
    path::Path,
    pin::Pin,
    sync::{Arc, Mutex},
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
        warnings: Vec::new(),
    }
}

fn runner_options(root: &Path) -> RunnerOptions {
    RunnerOptions {
        workspace_root: root.to_path_buf(),
        secrets: Arc::new(MapSecretProvider::empty()),
        probes: eggbench_runner::ProbeRegistry::with_builtins(),
        platform: Arc::new(UnixPlatform),
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
                trial_id: eggbench_core::TrialId::new(1).unwrap()
            },
            InvocationKind::Measured {
                trial_id: eggbench_core::TrialId::new(2).unwrap()
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
    assert_eq!(json.schema_version, eggbench_core::SchemaVersion(1));
    assert_eq!(
        json.terminal_status,
        eggbench_core::TrialExecutionStatus::Completed
    );
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
