//! Local run phase orchestration above the process-owning [`LocalSession`].

use crate::DEFAULT_SUBJECT_LOG_LIMIT_BYTES;
use crate::service::RuntimeBindings;
use crate::telemetry::{
    TelemetryError, TelemetryOutput, TelemetryPreflightContext, TelemetryRegistry,
    TelemetryTrialContext,
};
use crate::{
    CleanupFailure, LifecycleOutcome, LocalSession, stage_lifecycle_logs, stage_lifecycle_metadata,
    stage_runtime_topology,
};
use eggbench_core::{
    ArtifactPath, ArtifactRole, BundleError, BundleManifest, BundleWriter, ComparisonVerdict,
    ExecutionStatus, MetricWarning, Name, NormalizationInput, RawHistogramInput,
    RawMetricObservation, ResetPolicy, RunId, SchemaVersion, Sensitivity, TrialArm,
    TrialDescriptor, TrialExecutionFailure, TrialExecutionResult, TrialExecutionStatus, TrialId,
    Workload, normalize_trial_metrics, trial_metrics_path,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    path::PathBuf,
    pin::Pin,
    sync::Arc,
    time::{Duration, Instant},
};
use thiserror::Error;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

const MAX_PHASE_EVENTS: usize = 10_000;
const MAX_WORKLOAD_ARTIFACTS_PER_INVOCATION: usize = 256;
const TRIAL_RESULT_SCHEMA_VERSION: SchemaVersion = SchemaVersion(2);
/// Warmup record schema version (warmup records carry no arm/pair tags).
const WARMUP_RECORD_SCHEMA_VERSION: SchemaVersion = SchemaVersion(1);

/// Invocation identity, without comparison or metric semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InvocationKind {
    /// Untimed warmup invocation.
    Warmup {
        /// Ordinal in the warmup namespace, starting at one.
        ordinal: u32,
    },
    /// One measured repetition.
    Measured {
        /// Stable trial identity.
        trial_id: TrialId,
        /// Paired arm measured by this trial; `None` for unpaired runs.
        arm: Option<TrialArm>,
    },
}

/// Context passed to one workload invocation.
#[derive(Debug, Clone)]
pub struct InvocationContext {
    /// Bundle/run identity.
    pub run_id: RunId,
    /// Warmup or measured invocation identity.
    pub kind: InvocationKind,
    /// Workload intent frozen by plan resolution.
    pub workload: Workload,
    /// Deterministic invocation-specific seed, when the plan has a seed.
    pub seed: Option<u64>,
    /// Startup-established runtime bindings snapshot. Every invocation of one
    /// run receives the same snapshot; workload drivers must treat it as
    /// read-only and never mutate topology bindings through it.
    pub bindings: RuntimeBindings,
    /// Cancellation token for this invocation.
    pub cancellation: CancellationToken,
    /// Runner safety deadline.
    pub timeout: Duration,
}

/// Context passed to workload cleanup.
#[derive(Debug, Clone)]
pub struct DrainContext {
    /// Bundle/run identity.
    pub run_id: RunId,
    /// Cancellation token is informational; cleanup must proceed even when cancelled.
    pub cancellation: CancellationToken,
    /// Independent drain safety bound.
    pub timeout: Duration,
}

/// Optional bounded diagnostic file returned by a workload adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadArtifact {
    /// File name (must be one safe path component).
    pub name: String,
    /// Media type.
    pub media_type: String,
    /// Artifact bytes, staged after the measured interval.
    pub bytes: Vec<u8>,
}

/// Result from a workload invocation: diagnostic files plus protocol-neutral
/// raw metric inputs for post-measurement normalization.
///
/// Raw observations are driver output to be validated, never normalized
/// evidence. Normalization runs after the measured interval ends and stages a
/// separate deterministic `metrics.json` artifact per measured trial.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorkloadOutput {
    /// Optional diagnostic files, bounded during bundle staging.
    pub artifacts: Vec<WorkloadArtifact>,
    /// Raw metric observations for post-measurement normalization.
    /// Unrequested names never become gate-eligible normalized metrics.
    pub metrics: Vec<RawMetricObservation>,
    /// Raw histogram inputs referencing same-invocation artifacts.
    pub histograms: Vec<RawHistogramInput>,
    /// Raw error-category counts `(category, count)`.
    pub error_counts: Vec<(String, u64)>,
}

/// Redaction-safe operation failure category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureCategory {
    /// Adapter returned an operational failure.
    WorkloadFailed,
    /// Runner safety timeout expired.
    TimedOut,
    /// User cancellation stopped experimental work.
    Cancelled,
    /// Reset target failed.
    ResetFailed,
    /// Workload cleanup failed.
    DrainFailed,
    /// Managed service startup failed.
    StartupFailed,
    /// Managed service cleanup failed.
    TeardownFailed,
    /// Trial telemetry collection failed outside measured workload timing.
    TelemetryFailed,
}

/// Object-safe asynchronous workload adapter.
pub trait WorkloadExecutor: Send {
    /// Execute one warmup or measured workload invocation.
    fn execute<'a>(
        &'a mut self,
        context: InvocationContext,
    ) -> Pin<Box<dyn Future<Output = Result<WorkloadOutput, FailureCategory>> + Send + 'a>>;

    /// Drain driver-owned tasks/resources after experimental work.
    fn drain<'a>(
        &'a mut self,
        context: DrainContext,
    ) -> Pin<Box<dyn Future<Output = Result<(), FailureCategory>> + Send + 'a>>;
}

/// Context passed to a registered reset hook.
#[derive(Debug, Clone)]
pub struct ResetContext {
    /// Bundle/run identity.
    pub run_id: RunId,
    /// Reset target.
    pub target: Name,
    /// Cancellation token for this experimental operation.
    pub cancellation: CancellationToken,
    /// Reset safety bound.
    pub timeout: Duration,
}

/// Object-safe asynchronous reset adapter.
pub trait ResetHook: Send + Sync {
    /// Reset one registered service or reference.
    fn reset<'a>(
        &'a self,
        context: ResetContext,
    ) -> Pin<Box<dyn Future<Output = Result<(), FailureCategory>> + Send + 'a>>;
}

struct ResetBinding {
    target: Name,
    timeout: Duration,
    hook: Arc<dyn ResetHook>,
}

struct PhaseConfig {
    measurement_timeout: Duration,
    warmup_timeout: Duration,
    drain_timeout: Duration,
    telemetry_timeout: Duration,
    reset: Option<ResetBinding>,
}

/// Per-run telemetry disposition established by preflight.
#[derive(Debug, Default)]
struct TelemetryPlan {
    /// Sources whose preflight succeeded; started/stopped around each trial.
    active: Vec<String>,
    /// Sources disabled for the run with explicit warning detail.
    disabled: Vec<DisabledTelemetry>,
}

/// One telemetry source disabled for the run (optional preflight failure).
#[derive(Debug, Clone)]
struct DisabledTelemetry {
    source: String,
    reason: String,
}

/// Per-trial telemetry inputs for evidence staging.
struct TrialTelemetry<'a> {
    /// Captured outputs keyed by source, in stop order.
    outputs: &'a [(String, TelemetryOutput)],
    /// Explicit warnings (for example disabled-collector notices).
    warnings: &'a [MetricWarning],
}

/// Maximum artifacts one collector may stage per measured trial.
const MAX_TELEMETRY_ARTIFACTS_PER_TRIAL: usize = 8;

/// Open the telemetry window for one measured trial, outside the workload
/// timer. Returns the sources whose window opened, for matched stop calls.
/// On any start failure, already-opened windows are stopped best-effort so
/// no polling task leaks, and the initiating error is returned.
async fn start_trial_telemetry(
    telemetry: &mut TelemetryRegistry,
    plan: &TelemetryPlan,
    run_id: RunId,
    trial_id: TrialId,
    cancel: &CancellationToken,
    timeout_limit: Duration,
) -> Result<Vec<String>, TelemetryError> {
    let mut started = Vec::new();
    for source in &plan.active {
        let Some(collector) = telemetry.get_mut(source.as_str()) else {
            continue;
        };
        let context = TelemetryTrialContext {
            run_id,
            trial_id,
            cancellation: cancel.child_token(),
            timeout: timeout_limit,
        };
        let outcome = tokio::select! {
            () = cancel.cancelled() => Err(TelemetryError::new(
                "collector_cancelled",
                "telemetry start cancelled",
            )),
            started_outcome = timeout(timeout_limit, collector.start_trial(context)) => {
                match started_outcome {
                    Ok(result) => result,
                    Err(_) => Err(TelemetryError::new(
                        "polling_timeout",
                        "telemetry start timed out",
                    )),
                }
            }
        };
        match outcome {
            Ok(()) => started.push(source.clone()),
            Err(error) => {
                // Best-effort stop for already-opened windows so no polling
                // task leaks; outputs are discarded, error is primary.
                for open in &started {
                    if let Some(collector) = telemetry.get_mut(open.as_str()) {
                        let context = TelemetryTrialContext {
                            run_id,
                            trial_id,
                            cancellation: cancel.child_token(),
                            timeout: timeout_limit,
                        };
                        let _ = timeout(timeout_limit, collector.stop_trial(context)).await;
                    }
                }
                return Err(error);
            }
        }
    }
    Ok(started)
}

/// Close the telemetry window after the captured workload elapsed.
///
/// Stop is attempted for every started source even when the workload failed
/// or was cancelled. Failures become cleanup evidence; captured outputs are
/// returned alongside.
async fn stop_trial_telemetry(
    telemetry: &mut TelemetryRegistry,
    started: &[String],
    run_id: RunId,
    trial_id: TrialId,
    cancel: &CancellationToken,
    timeout_limit: Duration,
) -> (Vec<(String, TelemetryOutput)>, Vec<CleanupFailure>) {
    let mut outputs = Vec::new();
    let mut failures = Vec::new();
    for source in started {
        let Some(collector) = telemetry.get_mut(source.as_str()) else {
            continue;
        };
        let context = TelemetryTrialContext {
            run_id,
            trial_id,
            cancellation: cancel.child_token(),
            timeout: timeout_limit,
        };
        // Stop runs even under cancellation; the collector races internally
        // and the outer bound keeps teardown authoritative.
        match timeout(timeout_limit, collector.stop_trial(context)).await {
            Ok(Ok(output)) => outputs.push((source.clone(), output)),
            Ok(Err(error)) => {
                failures.push(CleanupFailure::new(source.clone(), error.to_string()));
            }
            Err(_) => {
                failures.push(CleanupFailure::new(
                    source.clone(),
                    "telemetry stop timed out",
                ));
            }
        }
    }
    (outputs, failures)
}

/// Build per-trial warnings for collectors disabled at preflight.
fn disabled_telemetry_warnings(plan: &TelemetryPlan) -> Vec<MetricWarning> {
    plan.disabled
        .iter()
        .map(|disabled| MetricWarning {
            category: "telemetry_disabled".to_owned(),
            detail: format!(
                "telemetry source {} disabled: {}",
                disabled.source, disabled.reason
            )
            .chars()
            .take(512)
            .collect(),
        })
        .collect()
}

/// Explicit registry of reset capabilities.
#[derive(Default)]
pub struct ResetRegistry {
    hooks: BTreeMap<Name, Arc<dyn ResetHook>>,
}

impl ResetRegistry {
    /// Register or replace a reset hook for a named target.
    pub fn register(&mut self, target: Name, hook: Arc<dyn ResetHook>) {
        self.hooks.insert(target, hook);
    }

    fn get(&self, target: &Name) -> Option<&Arc<dyn ResetHook>> {
        self.hooks.get(target)
    }
}

/// State-machine phase vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhaseKind {
    /// Local service startup and readiness.
    StartupReadiness,
    /// One diagnostic warmup invocation.
    Warmup,
    /// One measured workload invocation.
    MeasuredTrial,
    /// Explicit adapter reset between measured trials.
    Reset,
    /// Configured post-reset stabilization delay.
    Cooldown,
    /// Workload-adapter cleanup.
    Drain,
    /// Managed-service teardown.
    Teardown,
    /// Runner-owned evidence staging prior to immutable bundle publication.
    ///
    /// This phase covers staging of `runner-phases.json` and any other
    /// runner-owned artifacts required before the bundle is atomically
    /// published. The immutable publication step itself is not representable
    /// inside the bundle it publishes and is therefore outside this phase.
    Finalization,
}

/// Terminal result for a phase event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhaseOutcome {
    /// Phase completed successfully.
    Completed,
    /// Phase failed.
    Failed,
    /// Phase timed out.
    TimedOut,
    /// Experimental phase was cancelled.
    Cancelled,
}

/// Bounded, run-relative phase timeline event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhaseEvent {
    /// Stable event order, starting at zero.
    pub sequence: u64,
    /// Phase identity.
    pub phase: PhaseKind,
    /// Warmup ordinal where applicable.
    pub warmup_ordinal: Option<u32>,
    /// Measured trial identity where applicable.
    pub trial_id: Option<TrialId>,
    /// Offset from the run monotonic origin, in nanoseconds.
    pub start_offset_ns: u64,
    /// Phase duration when complete, in nanoseconds.
    pub elapsed_ns: Option<u64>,
    /// Terminal result; `None` only while the event is active in memory.
    pub outcome: Option<PhaseOutcome>,
    /// Redaction-safe failure category.
    pub failure_category: Option<FailureCategory>,
}

/// Completed high-level run result and finalized bundle.
#[derive(Debug, Clone)]
pub struct RunOutcome {
    /// Truthful lifecycle status; comparison is always absent in M002.
    pub execution_status: ExecutionStatus,
    /// Primary redaction-safe failure, if any.
    pub primary_failure: Option<FailureCategory>,
    /// Cleanup failures from managed services.
    pub cleanup_failures: Vec<CleanupFailure>,
    /// Run-relative state-machine event stream.
    pub phases: Vec<PhaseEvent>,
    /// Finalized bundle path.
    pub bundle_path: PathBuf,
    /// Verified immutable manifest.
    pub manifest: BundleManifest,
}

/// Preflight, orchestration, or evidence-finalization error.
#[derive(Debug, Error)]
pub enum OrchestrationError {
    /// M002 configuration cannot be executed safely.
    #[error("orchestration preflight failed: {0}")]
    Preflight(&'static str),
    /// Evidence could not be truthfully finalized; the staged tree is incomplete and the
    /// final bundle path was not published.
    ///
    /// `cleanup` carries any secondary drain or teardown failures observed while attempting
    /// mandatory cleanup after the primary evidence error. The primary error is always the
    /// `source` bundle error; cleanup diagnostics never replace it.
    #[error("evidence finalization failed: {source}")]
    Evidence {
        /// Primary staging or finalization error.
        source: BundleError,
        /// Secondary cleanup diagnostics collected during mandatory drain/teardown.
        cleanup: Vec<CleanupFailure>,
    },
}

impl OrchestrationError {
    /// Inspect the primary source bundle error, if this is an evidence error.
    #[must_use]
    pub fn source(&self) -> Option<&BundleError> {
        match self {
            Self::Evidence { source, .. } => Some(source),
            Self::Preflight(_) => None,
        }
    }

    /// Inspect secondary cleanup diagnostics collected during mandatory drain/teardown.
    #[must_use]
    pub fn cleanup(&self) -> &[CleanupFailure] {
        match self {
            Self::Evidence { cleanup, .. } => cleanup,
            Self::Preflight(_) => &[],
        }
    }
}

/// Internal orchestration state for the cleanup-aware execution contract.
///
/// Tracks whether managed startup created owned processes, whether the workload
/// executor was entered, and any evidence-staging error encountered during the
/// experimental phase. Cleanup decisions at the bottom of [`execute_run`]
/// derive from these flags so that drain and teardown cannot be bypassed by an
/// evidence error propagating via `?`.
struct RunState {
    /// Bundle writer carrying all staged evidence up to the finalization tail.
    writer: BundleWriter,
    /// Run-relative state-machine event stream.
    phases: Vec<PhaseEvent>,
    /// Truthful lifecycle status; comparison is always absent in M002.
    status: ExecutionStatus,
    /// Primary redaction-safe failure, if any.
    primary_failure: Option<FailureCategory>,
    /// Cleanup failures from managed services and post-start failure paths.
    cleanup_failures: Vec<CleanupFailure>,
    /// Completed measured-trial descriptors ready for the manifest.
    trials: Vec<TrialDescriptor>,
    /// Identities in spawn order.
    started: Vec<String>,
    /// Whether managed startup created at least one owned process.
    services_started: bool,
    /// Whether the workload executor was entered for any invocation.
    workload_entered: bool,
    /// Primary evidence-staging failure encountered during the experimental phase,
    /// if any. Preserved across the mandatory cleanup tail.
    staging_error: Option<BundleError>,
}

impl RunState {
    fn new(writer: BundleWriter, phases: Vec<PhaseEvent>) -> Self {
        Self {
            writer,
            phases,
            status: ExecutionStatus::Completed,
            primary_failure: None,
            cleanup_failures: Vec::new(),
            trials: Vec::new(),
            started: Vec::new(),
            services_started: false,
            workload_entered: false,
            staging_error: None,
        }
    }
}

/// Run resolved work through startup, warmups, measured trials, and cleanup.
///
/// `writer` must already contain source plan, resolved plan, and environment artifacts.
/// Operational workload/reset/cleanup failures are returned in a finalized [`RunOutcome`].
///
/// # Errors
/// Returns an error for preflight or evidence finalization failures.
///
/// Any [`OrchestrationError::Evidence`] returned after managed startup carries the primary
/// evidence failure plus any secondary cleanup failures observed while attempting mandatory
/// drain and `LocalSession::shutdown`; the primary cause is never replaced by cleanup
/// diagnostics. No failed evidence publication is represented as a valid finalized bundle.
#[allow(clippy::too_many_lines)] // Keep the canonical phase transition order auditable in one place.
pub async fn execute_run<E: WorkloadExecutor + ?Sized>(
    session: &mut LocalSession,
    resolved: &eggbench_core::ResolvedPlan,
    executor: &mut E,
    resets: &ResetRegistry,
    telemetry: &mut TelemetryRegistry,
    writer: BundleWriter,
    cancel: &CancellationToken,
) -> Result<RunOutcome, OrchestrationError> {
    let config = preflight(resolved, resets)?;
    let trial_count = resolved.trials.measured.get();
    let warmup_count = resolved.trials.warmup;
    let phase_bound = usize::try_from(warmup_count)
        .ok()
        .and_then(|warmups| {
            usize::try_from(trial_count)
                .ok()
                .and_then(|trials| warmups.checked_add(trials.checked_mul(3)?)?.checked_add(2))
        })
        .ok_or(OrchestrationError::Preflight("phase event bound overflow"))?;
    if phase_bound > MAX_PHASE_EVENTS {
        return Err(OrchestrationError::Preflight(
            "phase event bound exceeds runner limit",
        ));
    }
    preflight_evidence_capacity(&writer, session, resolved, phase_bound)?;
    let run_id = writer.run_id();
    let origin = Instant::now();
    let mut state = RunState::new(writer, Vec::with_capacity(phase_bound));

    // ---- Telemetry preflight ----
    // Before managed startup: validate every requested backend. Required
    // failures prevent measurement; optional failures disable the collector
    // for the run with an explicit per-trial warning (never zeroes).
    let telemetry_plan = if cancel.is_cancelled() {
        TelemetryPlan::default()
    } else {
        preflight_telemetry(telemetry, resolved, run_id, &config, cancel).await?
    };

    // ---- Startup / readiness ----
    if cancel.is_cancelled() {
        let index = begin_phase(
            &mut state.phases,
            PhaseKind::StartupReadiness,
            None,
            None,
            origin,
        );
        state.status = ExecutionStatus::Cancelled;
        state.primary_failure = Some(FailureCategory::Cancelled);
        finish_phase(
            &mut state.phases,
            index,
            origin,
            PhaseOutcome::Cancelled,
            state.primary_failure,
        );
    } else {
        let index = begin_phase(
            &mut state.phases,
            PhaseKind::StartupReadiness,
            None,
            None,
            origin,
        );
        match session.startup(cancel).await {
            Ok(report) => {
                state.started = report.started;
                state.services_started = !state.started.is_empty();
                finish_phase(
                    &mut state.phases,
                    index,
                    origin,
                    PhaseOutcome::Completed,
                    None,
                );
            }
            Err(error) => {
                state.started = session
                    .events()
                    .iter()
                    .filter(|event| event.kind == crate::LifecycleEventKind::Spawned)
                    .map(|event| event.identity.clone())
                    .collect();
                state.status = if cancel.is_cancelled() {
                    ExecutionStatus::Cancelled
                } else {
                    ExecutionStatus::Failed
                };
                state.primary_failure = Some(if cancel.is_cancelled() {
                    FailureCategory::Cancelled
                } else {
                    FailureCategory::StartupFailed
                });
                state.cleanup_failures.extend_from_slice(error.cleanup());
                finish_phase(
                    &mut state.phases,
                    index,
                    origin,
                    if cancel.is_cancelled() {
                        PhaseOutcome::Cancelled
                    } else {
                        PhaseOutcome::Failed
                    },
                    state.primary_failure,
                );
            }
        }
    }

    // ---- Warmups ----
    if state.status == ExecutionStatus::Completed && state.staging_error.is_none() {
        for ordinal in 1..=warmup_count {
            let index = begin_phase(
                &mut state.phases,
                PhaseKind::Warmup,
                Some(ordinal),
                None,
                origin,
            );
            // Paired warmups alternate arms round-robin so each variant is
            // warm before its first measured trial; they carry no pair id.
            let workload_override = if resolved.paired.is_some() {
                Some(effective_workload(resolved, warmup_arm(ordinal)))
            } else {
                None
            };
            match execute_invocation(InvocationRequest {
                executor,
                cancel,
                run_id,
                resolved,
                bindings: session.runtime_bindings(),
                kind: InvocationKind::Warmup { ordinal },
                workload_override,
                limit: config.warmup_timeout,
                origin,
            })
            .await
            {
                InvocationResult::Completed(output, elapsed, _) => {
                    state.workload_entered = true;
                    match stage_warmup(&mut state.writer, ordinal, elapsed, None, output) {
                        Ok(()) => {
                            finish_phase(
                                &mut state.phases,
                                index,
                                origin,
                                PhaseOutcome::Completed,
                                None,
                            );
                        }
                        Err(error) => {
                            state.staging_error = Some(error);
                            state.status = ExecutionStatus::Failed;
                            finish_phase(
                                &mut state.phases,
                                index,
                                origin,
                                PhaseOutcome::Failed,
                                None,
                            );
                            break;
                        }
                    }
                }
                InvocationResult::Failure(category, elapsed, _) => {
                    state.workload_entered = true;
                    let category = workload_failure(category);
                    state.status = status_for(category);
                    state.primary_failure = Some(category);
                    finish_phase(
                        &mut state.phases,
                        index,
                        origin,
                        outcome_for(category),
                        Some(category),
                    );
                    if let Err(error) = stage_warmup(
                        &mut state.writer,
                        ordinal,
                        elapsed,
                        Some(category),
                        WorkloadOutput::default(),
                    ) {
                        // Preserve the workload failure as primary while recording the
                        // staging error as the evidence-cause.
                        state.staging_error = Some(error);
                    }
                    break;
                }
            }
        }
    }

    // ---- Measured trials ----
    if state.status == ExecutionStatus::Completed && state.staging_error.is_none() {
        for number in 1..=trial_count {
            if cancel.is_cancelled() {
                state.status = ExecutionStatus::Cancelled;
                state.primary_failure = Some(FailureCategory::Cancelled);
                break;
            }
            let trial_id = match TrialId::new(number) {
                Ok(id) => id,
                Err(error) => {
                    state.staging_error = Some(error);
                    break;
                }
            };
            // Paired schedule: alternating arms starting with baseline; the
            // pair identity counts consecutive baseline/candidate trials.
            let (arm, pair_id) = match &resolved.paired {
                Some(_) => {
                    let (arm, pair) = paired_assignment(number);
                    (Some(arm), Some(pair))
                }
                None => (None, None),
            };
            let workload_override = arm.map(|arm| effective_workload(resolved, arm));
            let index = begin_phase(
                &mut state.phases,
                PhaseKind::MeasuredTrial,
                None,
                Some(trial_id),
                origin,
            );
            let telemetry_warnings = disabled_telemetry_warnings(&telemetry_plan);
            let no_telemetry = TrialTelemetry {
                outputs: &[],
                warnings: &telemetry_warnings,
            };
            // Telemetry opens its window before the workload timer starts.
            // The trial fails without measurement when start fails; the
            // timer never started and no workload ran. Already-opened
            // windows were stopped inside the helper; no cleanup remains.
            let Ok(started_sources) = start_trial_telemetry(
                telemetry,
                &telemetry_plan,
                run_id,
                trial_id,
                cancel,
                config.telemetry_timeout,
            )
            .await
            else {
                state.status = ExecutionStatus::Failed;
                state.primary_failure = Some(FailureCategory::TelemetryFailed);
                finish_phase(
                    &mut state.phases,
                    index,
                    origin,
                    PhaseOutcome::Failed,
                    Some(FailureCategory::TelemetryFailed),
                );
                let result = TrialExecutionResult {
                    schema_version: TRIAL_RESULT_SCHEMA_VERSION,
                    trial_id,
                    measurement_start_offset_ns: nanos(origin.elapsed()),
                    measurement_elapsed_ns: 0,
                    terminal_status: TrialExecutionStatus::Failed,
                    failure_category: Some(TrialExecutionFailure::TelemetryFailed),
                    arm,
                    pair_id,
                };
                match stage_trial(
                    &mut state.writer,
                    trial_id,
                    &result,
                    WorkloadOutput::default(),
                    &no_telemetry,
                    resolved,
                ) {
                    Ok((result_path, artifacts)) => {
                        state.trials.push(TrialDescriptor {
                            id: trial_id,
                            result: result_path,
                            artifacts,
                        });
                    }
                    Err(stage_error) => {
                        state.staging_error = Some(stage_error);
                    }
                }
                break;
            };
            match execute_invocation(InvocationRequest {
                executor,
                cancel,
                run_id,
                resolved,
                bindings: session.runtime_bindings(),
                kind: InvocationKind::Measured { trial_id, arm },
                workload_override,
                limit: config.measurement_timeout,
                origin,
            })
            .await
            {
                InvocationResult::Completed(output, elapsed, start_offset_ns) => {
                    state.workload_entered = true;
                    // Telemetry closes after the captured elapsed, even on
                    // later failure paths below.
                    let (telemetry_outputs, telemetry_cleanup) = stop_trial_telemetry(
                        telemetry,
                        &started_sources,
                        run_id,
                        trial_id,
                        cancel,
                        config.telemetry_timeout,
                    )
                    .await;
                    if telemetry_cleanup.is_empty() {
                        let trial_telemetry = TrialTelemetry {
                            outputs: &telemetry_outputs,
                            warnings: &telemetry_warnings,
                        };
                        let result = TrialExecutionResult {
                            schema_version: TRIAL_RESULT_SCHEMA_VERSION,
                            trial_id,
                            measurement_start_offset_ns: start_offset_ns,
                            measurement_elapsed_ns: nanos(elapsed),
                            terminal_status: TrialExecutionStatus::Completed,
                            failure_category: None,
                            arm,
                            pair_id,
                        };
                        match stage_trial(
                            &mut state.writer,
                            trial_id,
                            &result,
                            output,
                            &trial_telemetry,
                            resolved,
                        ) {
                            Ok((result_path, artifacts)) => {
                                finish_phase(
                                    &mut state.phases,
                                    index,
                                    origin,
                                    PhaseOutcome::Completed,
                                    None,
                                );
                                state.trials.push(TrialDescriptor {
                                    id: trial_id,
                                    result: result_path,
                                    artifacts,
                                });
                            }
                            Err(error) => {
                                state.staging_error = Some(error);
                                state.status = ExecutionStatus::Failed;
                                finish_phase(
                                    &mut state.phases,
                                    index,
                                    origin,
                                    PhaseOutcome::Failed,
                                    None,
                                );
                                break;
                            }
                        }
                    } else {
                        // Telemetry stop failed after workload success: the
                        // trial fails with telemetry as primary and the stop
                        // failures attached as cleanup diagnostics.
                        state.cleanup_failures.extend(telemetry_cleanup);
                        state.status = ExecutionStatus::Failed;
                        state.primary_failure = Some(FailureCategory::TelemetryFailed);
                        finish_phase(
                            &mut state.phases,
                            index,
                            origin,
                            PhaseOutcome::Failed,
                            Some(FailureCategory::TelemetryFailed),
                        );
                        let result = TrialExecutionResult {
                            schema_version: TRIAL_RESULT_SCHEMA_VERSION,
                            trial_id,
                            measurement_start_offset_ns: start_offset_ns,
                            measurement_elapsed_ns: nanos(elapsed),
                            terminal_status: TrialExecutionStatus::Failed,
                            failure_category: Some(TrialExecutionFailure::TelemetryFailed),
                            arm,
                            pair_id,
                        };
                        match stage_trial(
                            &mut state.writer,
                            trial_id,
                            &result,
                            WorkloadOutput::default(),
                            &no_telemetry,
                            resolved,
                        ) {
                            Ok((result_path, artifacts)) => {
                                state.trials.push(TrialDescriptor {
                                    id: trial_id,
                                    result: result_path,
                                    artifacts,
                                });
                            }
                            Err(error) => {
                                state.staging_error = Some(error);
                            }
                        }
                        break;
                    }
                }
                InvocationResult::Failure(category, elapsed, start_offset_ns) => {
                    state.workload_entered = true;
                    // Telemetry still closes after workload failure or
                    // cancellation when its window opened; stop failures
                    // attach as cleanup without rewriting the primary cause.
                    let (_, telemetry_cleanup) = stop_trial_telemetry(
                        telemetry,
                        &started_sources,
                        run_id,
                        trial_id,
                        cancel,
                        config.telemetry_timeout,
                    )
                    .await;
                    state.cleanup_failures.extend(telemetry_cleanup);
                    let category = workload_failure(category);
                    state.status = status_for(category);
                    state.primary_failure = Some(category);
                    finish_phase(
                        &mut state.phases,
                        index,
                        origin,
                        outcome_for(category),
                        Some(category),
                    );
                    let terminal_status = match category {
                        FailureCategory::TimedOut => TrialExecutionStatus::TimedOut,
                        FailureCategory::Cancelled => TrialExecutionStatus::Cancelled,
                        _ => TrialExecutionStatus::Failed,
                    };
                    let result = TrialExecutionResult {
                        schema_version: TRIAL_RESULT_SCHEMA_VERSION,
                        trial_id,
                        measurement_start_offset_ns: start_offset_ns,
                        measurement_elapsed_ns: nanos(elapsed),
                        terminal_status,
                        failure_category: Some(match category {
                            FailureCategory::TimedOut => TrialExecutionFailure::TimedOut,
                            FailureCategory::Cancelled => TrialExecutionFailure::Cancelled,
                            _ => TrialExecutionFailure::WorkloadFailed,
                        }),
                        arm,
                        pair_id,
                    };
                    match stage_trial(
                        &mut state.writer,
                        trial_id,
                        &result,
                        WorkloadOutput::default(),
                        &no_telemetry,
                        resolved,
                    ) {
                        Ok((result_path, artifacts)) => {
                            state.trials.push(TrialDescriptor {
                                id: trial_id,
                                result: result_path,
                                artifacts,
                            });
                        }
                        Err(error) => {
                            // Preserve the workload failure as primary while recording the
                            // staging error as the evidence-cause. We cannot include this
                            // trial descriptor in the manifest because the result artifact
                            // could not be staged.
                            state.staging_error = Some(error);
                        }
                    }
                    break;
                }
            }
            if number < trial_count
                && state.status == ExecutionStatus::Completed
                && state.staging_error.is_none()
            {
                if let Some(reset) = config.reset.as_ref() {
                    let index = begin_phase(
                        &mut state.phases,
                        PhaseKind::Reset,
                        None,
                        Some(trial_id),
                        origin,
                    );
                    let token = cancel.child_token();
                    let context = ResetContext {
                        run_id,
                        target: reset.target.clone(),
                        cancellation: token.clone(),
                        timeout: reset.timeout,
                    };
                    let result = tokio::select! { () = cancel.cancelled() => Err(FailureCategory::Cancelled), timed = timeout(context.timeout, reset.hook.reset(context)) => match timed { Ok(result) => result, Err(_) => Err(FailureCategory::TimedOut) } };
                    match result {
                        Ok(()) => {
                            finish_phase(
                                &mut state.phases,
                                index,
                                origin,
                                PhaseOutcome::Completed,
                                None,
                            );
                        }
                        Err(category) => {
                            token.cancel();
                            let category = match category {
                                FailureCategory::Cancelled | FailureCategory::TimedOut => category,
                                _ => FailureCategory::ResetFailed,
                            };
                            state.status = status_for(category);
                            state.primary_failure = Some(category);
                            finish_phase(
                                &mut state.phases,
                                index,
                                origin,
                                outcome_for(category),
                                Some(category),
                            );
                            break;
                        }
                    }
                }
                if let Some(cooldown) = resolved.trials.cooldown_ms {
                    let index = begin_phase(
                        &mut state.phases,
                        PhaseKind::Cooldown,
                        None,
                        Some(trial_id),
                        origin,
                    );
                    let sleep = tokio::time::sleep(Duration::from_millis(cooldown.get()));
                    tokio::pin!(sleep);
                    tokio::select! { () = cancel.cancelled() => { state.status = ExecutionStatus::Cancelled; state.primary_failure = Some(FailureCategory::Cancelled); finish_phase(&mut state.phases, index, origin, PhaseOutcome::Cancelled, state.primary_failure); break; }, () = &mut sleep => finish_phase(&mut state.phases, index, origin, PhaseOutcome::Completed, None) }
                }
            }
        }
    }

    // ===========================================================
    // Mandatory cleanup tail. Every code path that reaches here
    // runs workload drain (if entered) and managed-service
    // teardown (if startup created processes) before any
    // evidence/finalization disposition.
    // ===========================================================

    // ---- Workload drain ----
    // Always call drain to preserve existing M002 contract; the workload adapter is the
    // canonical owner of its cleanup hook and is expected to be idempotent when no
    // invocation was entered.
    {
        let index = begin_phase(&mut state.phases, PhaseKind::Drain, None, None, origin);
        let drain_context = DrainContext {
            run_id,
            cancellation: cancel.clone(),
            timeout: config.drain_timeout,
        };
        match timeout(config.drain_timeout, executor.drain(drain_context)).await {
            Ok(Ok(())) => finish_phase(
                &mut state.phases,
                index,
                origin,
                PhaseOutcome::Completed,
                None,
            ),
            Ok(Err(category)) => {
                if state.status == ExecutionStatus::Completed {
                    if category == FailureCategory::Cancelled {
                        state.status = ExecutionStatus::Cancelled;
                        state.primary_failure = Some(FailureCategory::Cancelled);
                    } else {
                        state.status = ExecutionStatus::Failed;
                        state.primary_failure = Some(FailureCategory::DrainFailed);
                    }
                }
                finish_phase(
                    &mut state.phases,
                    index,
                    origin,
                    PhaseOutcome::Failed,
                    Some(if category == FailureCategory::Cancelled {
                        FailureCategory::Cancelled
                    } else {
                        FailureCategory::DrainFailed
                    }),
                );
            }
            Err(_) => {
                if state.status == ExecutionStatus::Completed {
                    state.status = ExecutionStatus::Failed;
                    state.primary_failure = Some(FailureCategory::DrainFailed);
                }
                finish_phase(
                    &mut state.phases,
                    index,
                    origin,
                    PhaseOutcome::TimedOut,
                    Some(FailureCategory::TimedOut),
                );
            }
        }
        if cancel.is_cancelled() && state.status == ExecutionStatus::Completed {
            state.status = ExecutionStatus::Cancelled;
            state.primary_failure = Some(FailureCategory::Cancelled);
        }
    }

    // ---- Telemetry drain ----
    // Drain every preflight-active collector after the workload drain. A
    // telemetry drain failure follows the same precedence as workload
    // drain: it becomes primary only when no earlier failure stands, and
    // service teardown still runs afterwards.
    if !telemetry_plan.active.is_empty() {
        let index = begin_phase(&mut state.phases, PhaseKind::Drain, None, None, origin);
        let mut telemetry_drain_failed = false;
        for source in &telemetry_plan.active {
            let Some(collector) = telemetry.get_mut(source.as_str()) else {
                continue;
            };
            let drain_context = DrainContext {
                run_id,
                cancellation: cancel.clone(),
                timeout: config.drain_timeout,
            };
            if timeout(config.drain_timeout, collector.drain(drain_context))
                .await
                .is_ok_and(|result| result.is_ok())
            {
            } else {
                telemetry_drain_failed = true;
                state.cleanup_failures.push(CleanupFailure::new(
                    source.clone(),
                    "telemetry drain failed",
                ));
            }
        }
        if telemetry_drain_failed {
            if state.status == ExecutionStatus::Completed {
                state.status = ExecutionStatus::Failed;
                state.primary_failure = Some(FailureCategory::TelemetryFailed);
            }
            finish_phase(
                &mut state.phases,
                index,
                origin,
                PhaseOutcome::Failed,
                Some(FailureCategory::TelemetryFailed),
            );
        } else {
            finish_phase(
                &mut state.phases,
                index,
                origin,
                PhaseOutcome::Completed,
                None,
            );
        }
        if cancel.is_cancelled() && state.status == ExecutionStatus::Completed {
            state.status = ExecutionStatus::Cancelled;
            state.primary_failure = Some(FailureCategory::Cancelled);
        }
    }

    // ---- Managed-service teardown ----
    let teardown_index = begin_phase(&mut state.phases, PhaseKind::Teardown, None, None, origin);
    let stopped_order = if state.services_started {
        let shutdown = session.shutdown().await;
        state.cleanup_failures.extend(shutdown.failures);
        if cancel.is_cancelled() && state.status == ExecutionStatus::Completed {
            state.status = ExecutionStatus::Cancelled;
            state.primary_failure = Some(FailureCategory::Cancelled);
        }
        if state.cleanup_failures.is_empty() {
            finish_phase(
                &mut state.phases,
                teardown_index,
                origin,
                PhaseOutcome::Completed,
                None,
            );
        } else {
            if state.status == ExecutionStatus::Completed {
                state.status = ExecutionStatus::Failed;
                state.primary_failure = Some(FailureCategory::TeardownFailed);
            }
            finish_phase(
                &mut state.phases,
                teardown_index,
                origin,
                PhaseOutcome::Failed,
                Some(FailureCategory::TeardownFailed),
            );
        }
        shutdown.stopped_order
    } else {
        finish_phase(
            &mut state.phases,
            teardown_index,
            origin,
            PhaseOutcome::Completed,
            None,
        );
        Vec::new()
    };

    // ---- Lifecycle evidence staging ----
    let lifecycle = LifecycleOutcome {
        started: std::mem::take(&mut state.started),
        stopped_order,
        cleanup: state.cleanup_failures.clone(),
    };
    let lifecycle_logs = stage_lifecycle_logs(session, &mut state.writer).await;
    if let Err(error) = lifecycle_logs {
        if state.staging_error.is_none() {
            state.staging_error = Some(error);
        }
    } else if state.staging_error.is_none()
        && let Err(error) = stage_lifecycle_metadata(session, &lifecycle, &mut state.writer)
    {
        state.staging_error = Some(error);
    }
    // Runtime-topology evidence stages from retained session state after
    // teardown, so topology staging failure still preserves the evidence
    // cause without rewriting cleanup or workload outcomes.
    if state.staging_error.is_none()
        && let Err(error) = stage_runtime_topology(session, &mut state.writer)
    {
        state.staging_error = Some(error);
    }

    // ===========================================================
    // Finalization phase: the in-memory finalization event is
    // finished exactly once. `runner-phases.json` is staged from
    // that terminal state and is not mutated again. Immutable
    // bundle publication follows; its result is the
    // evidence-cause when it fails.
    // ===========================================================

    let final_index = begin_phase(
        &mut state.phases,
        PhaseKind::Finalization,
        None,
        None,
        origin,
    );
    finish_phase(
        &mut state.phases,
        final_index,
        origin,
        PhaseOutcome::Completed,
        None,
    );
    if state.staging_error.is_none()
        && let Err(error) = stage_phase_artifacts(&mut state.writer, &state.phases)
    {
        state.staging_error = Some(error);
    }

    if let Some(source) = state.staging_error.take() {
        return Err(OrchestrationError::Evidence {
            source,
            cleanup: state.cleanup_failures,
        });
    }

    let path = state.writer.final_path().to_path_buf();
    let bundle = match state.writer.finalize(
        state.status,
        None::<ComparisonVerdict>,
        resolved.subject.clone(),
        resolved
            .drivers
            .values()
            .map(|driver| driver.descriptor.clone())
            .collect(),
        state.trials,
        None,
        None,
    ) {
        Ok(bundle) => bundle,
        Err(source) => {
            return Err(OrchestrationError::Evidence {
                source,
                cleanup: state.cleanup_failures,
            });
        }
    };

    Ok(RunOutcome {
        execution_status: state.status,
        primary_failure: state.primary_failure,
        cleanup_failures: state.cleanup_failures,
        phases: state.phases,
        bundle_path: path,
        manifest: bundle.manifest().clone(),
    })
}

enum InvocationResult {
    Completed(WorkloadOutput, Duration, u64),
    Failure(FailureCategory, Duration, u64),
}

/// Explicit parameters for one workload invocation.
struct InvocationRequest<'a, E: ?Sized> {
    executor: &'a mut E,
    cancel: &'a CancellationToken,
    run_id: RunId,
    resolved: &'a eggbench_core::ResolvedPlan,
    bindings: &'a RuntimeBindings,
    kind: InvocationKind,
    /// Effective workload for this invocation. Paired trials direct load at
    /// one arm's service while keeping the resolved load shape; `None`
    /// selects the resolved workload unchanged.
    workload_override: Option<Workload>,
    limit: Duration,
    origin: Instant,
}

async fn execute_invocation<E: WorkloadExecutor + ?Sized>(
    request: InvocationRequest<'_, E>,
) -> InvocationResult {
    let InvocationRequest {
        executor,
        cancel,
        run_id,
        resolved,
        bindings,
        kind,
        workload_override,
        limit,
        origin,
    } = request;
    let child = cancel.child_token();
    let context = InvocationContext {
        run_id,
        kind,
        workload: workload_override.unwrap_or_else(|| resolved.workload.clone()),
        seed: resolved.seed.map(|seed| derive_seed(seed, kind)),
        bindings: bindings.clone(),
        cancellation: child.clone(),
        timeout: limit,
    };
    let start = Instant::now();
    let start_offset_ns = nanos(start.duration_since(origin));
    let result = tokio::select! {
        () = cancel.cancelled() => Err(FailureCategory::Cancelled),
        result = timeout(limit, executor.execute(context)) => match result { Ok(result) => result, Err(_) => Err(FailureCategory::TimedOut) },
    };
    child.cancel();
    let elapsed = start.elapsed();
    match result {
        Ok(output) => InvocationResult::Completed(output, elapsed, start_offset_ns),
        Err(category) => InvocationResult::Failure(category, elapsed, start_offset_ns),
    }
}

fn preflight(
    resolved: &eggbench_core::ResolvedPlan,
    resets: &ResetRegistry,
) -> Result<PhaseConfig, OrchestrationError> {
    // Plan validation already enforces even measured counts for paired runs;
    // re-check here so hand-built resolved plans fail before managed startup.
    if resolved.paired.is_some() {
        let measured = resolved.trials.measured.get();
        if measured < 2 || !measured.is_multiple_of(2) {
            return Err(OrchestrationError::Preflight(
                "paired run requires an even measured trial count of at least 2",
            ));
        }
    }
    let allowed: BTreeSet<&str> = ["measurement", "warmup", "reset", "drain", "telemetry"]
        .into_iter()
        .collect();
    if resolved
        .trials
        .timeouts
        .keys()
        .any(|key| !allowed.contains(key.as_str()))
    {
        return Err(OrchestrationError::Preflight("unknown M002 timeout key"));
    }
    let get = |key: &str| {
        resolved
            .trials
            .timeouts
            .get(&Name::new(key).expect("fixed timeout key"))
            .map(|ms| Duration::from_millis(ms.get()))
    };
    let measurement = get("measurement").ok_or(OrchestrationError::Preflight(
        "measurement timeout is required",
    ))?;
    let warmup = get("warmup").unwrap_or(measurement);
    let drain = get("drain").ok_or(OrchestrationError::Preflight("drain timeout is required"))?;
    // Telemetry exchanges are bounded but outside workload timing; absent an
    // explicit key they share the measurement bound.
    let telemetry_timeout = get("telemetry").unwrap_or(measurement);
    if measurement.is_zero() || warmup.is_zero() || drain.is_zero() {
        return Err(OrchestrationError::Preflight(
            "timeouts must be greater than zero",
        ));
    }
    let target = match &resolved.trials.reset {
        ResetPolicy::None => None,
        ResetPolicy::Service { service } => Some(service.clone()),
        ResetPolicy::Reference { reference } => Some(reference.clone()),
    };
    let reset = if let Some(target) = target {
        let reset_timeout =
            get("reset").ok_or(OrchestrationError::Preflight("reset timeout is required"))?;
        if reset_timeout.is_zero() {
            return Err(OrchestrationError::Preflight(
                "reset timeout must be greater than zero",
            ));
        }
        let hook = resets
            .get(&target)
            .cloned()
            .ok_or(OrchestrationError::Preflight(
                "required reset hook is not registered",
            ))?;
        Some(ResetBinding {
            target,
            timeout: reset_timeout,
            hook,
        })
    } else {
        None
    };
    Ok(PhaseConfig {
        measurement_timeout: measurement,
        warmup_timeout: warmup,
        drain_timeout: drain,
        telemetry_timeout,
        reset,
    })
}

/// Preflight every telemetry source requested by the resolved plan.
///
/// Runs before managed startup. A required source whose collector is
/// missing or whose preflight fails prevents measurement with a preflight
/// error. An optional source failure disables the collector for the run;
/// its requested metrics later normalize as missing with an explicit
/// warning, never as fabricated zeroes.
async fn preflight_telemetry(
    telemetry: &mut TelemetryRegistry,
    resolved: &eggbench_core::ResolvedPlan,
    run_id: RunId,
    config: &PhaseConfig,
    cancel: &CancellationToken,
) -> Result<TelemetryPlan, OrchestrationError> {
    let mut plan = TelemetryPlan::default();
    for request in &resolved.telemetry {
        let source = request.source.as_str();
        let Some(collector) = telemetry.get_mut(source) else {
            if request.required {
                return Err(OrchestrationError::Preflight(
                    "required telemetry collector is not registered",
                ));
            }
            plan.disabled.push(DisabledTelemetry {
                source: source.to_owned(),
                reason: "collector not registered".to_owned(),
            });
            continue;
        };
        let context = TelemetryPreflightContext {
            run_id,
            cancellation: cancel.child_token(),
            timeout: config.telemetry_timeout,
        };
        let outcome = tokio::select! {
            () = cancel.cancelled() => None,
            probed = timeout(config.telemetry_timeout, collector.preflight(context)) => Some(probed),
        };
        // Cancellation races preflight; the startup section reports the
        // Cancelled outcome. Leave collectors undispositioned.
        let Some(probed) = outcome else {
            return Ok(TelemetryPlan::default());
        };
        match probed {
            Ok(Ok(_)) => plan.active.push(source.to_owned()),
            Ok(Err(error)) => {
                if request.required {
                    return Err(OrchestrationError::Preflight(
                        "required telemetry preflight failed",
                    ));
                }
                plan.disabled.push(DisabledTelemetry {
                    source: source.to_owned(),
                    reason: error.to_string(),
                });
            }
            Err(_) => {
                if request.required {
                    return Err(OrchestrationError::Preflight(
                        "required telemetry preflight timed out",
                    ));
                }
                plan.disabled.push(DisabledTelemetry {
                    source: source.to_owned(),
                    reason: "telemetry preflight timed out".to_owned(),
                });
            }
        }
    }
    Ok(plan)
}

/// Per-artifact byte estimate for one telemetry artifact (for example the
/// bounded `gregg.ndjson` trial series).
const TELEMETRY_ARTIFACT_BYTE_ESTIMATE: u64 = 262_144;

/// Per-trial telemetry artifact slots across all requested sources.
fn telemetry_artifact_count(
    measured: usize,
    resolved: &eggbench_core::ResolvedPlan,
) -> Result<usize, OrchestrationError> {
    measured
        .checked_mul(resolved.telemetry.len())
        .and_then(|count| count.checked_mul(MAX_TELEMETRY_ARTIFACTS_PER_TRIAL))
        .ok_or(OrchestrationError::Preflight(
            "telemetry artifact count overflow",
        ))
}

/// Bounded per-trial telemetry byte estimate across requested sources.
struct TelemetryByteEstimate {
    /// Per-artifact floor enforced when telemetry is requested.
    floor: u64,
    /// Total bytes per measured trial.
    per_trial: u64,
}

fn telemetry_byte_estimate(
    resolved: &eggbench_core::ResolvedPlan,
) -> Result<TelemetryByteEstimate, OrchestrationError> {
    if resolved.telemetry.is_empty() {
        return Ok(TelemetryByteEstimate {
            floor: 0,
            per_trial: 0,
        });
    }
    let per_trial = u64::try_from(resolved.telemetry.len())
        .ok()
        .and_then(|sources| {
            sources
                .checked_mul(MAX_TELEMETRY_ARTIFACTS_PER_TRIAL as u64)?
                .checked_mul(TELEMETRY_ARTIFACT_BYTE_ESTIMATE)
        })
        .ok_or(OrchestrationError::Preflight(
            "telemetry artifact byte bound overflow",
        ))?;
    Ok(TelemetryByteEstimate {
        floor: TELEMETRY_ARTIFACT_BYTE_ESTIMATE,
        per_trial,
    })
}

/// Byte bound for the `lifecycle/lifecycle.json` evidence artifact.
fn lifecycle_byte_bound(identities: &[String]) -> Result<u64, OrchestrationError> {
    u64::try_from(identities.len())
        .ok()
        .and_then(|count| count.checked_mul(512))
        .and_then(|bytes| {
            u64::try_from(identities.iter().map(String::len).sum::<usize>())
                .ok()
                .and_then(|identity_bytes| identity_bytes.checked_mul(8))
                .and_then(|identity_bytes| bytes.checked_add(identity_bytes))
        })
        .and_then(|bytes| bytes.checked_add(2_048))
        .ok_or(OrchestrationError::Preflight(
            "lifecycle artifact bound overflow",
        ))
}

/// Byte bound for the `lifecycle/runtime-topology.json` evidence artifact:
/// one entry per managed identity plus the startup-established bindings.
fn topology_byte_bound(identities: &[String]) -> Result<u64, OrchestrationError> {
    u64::try_from(identities.len())
        .ok()
        .and_then(|count| count.checked_mul(1_024))
        .and_then(|bytes| {
            u64::try_from(identities.iter().map(String::len).sum::<usize>())
                .ok()
                .and_then(|identity_bytes| identity_bytes.checked_mul(8))
                .and_then(|identity_bytes| bytes.checked_add(identity_bytes))
        })
        .and_then(|bytes| bytes.checked_add(2_048))
        .ok_or(OrchestrationError::Preflight(
            "runtime-topology artifact bound overflow",
        ))
}

fn preflight_evidence_capacity(
    writer: &BundleWriter,
    session: &LocalSession,
    resolved: &eggbench_core::ResolvedPlan,
    phase_bound: usize,
) -> Result<(), OrchestrationError> {
    let identities = session.spawn_order();
    let warmups = usize::try_from(resolved.trials.warmup)
        .map_err(|_| OrchestrationError::Preflight("warmup count overflow"))?;
    let measured = usize::try_from(resolved.trials.measured.get())
        .map_err(|_| OrchestrationError::Preflight("trial count overflow"))?;
    let log_artifact_count = identities
        .len()
        .checked_mul(2)
        .ok_or(OrchestrationError::Preflight("log artifact count overflow"))?;
    // Each measured trial stages `result.json` plus normalized `metrics.json`.
    // Requested telemetry adds per-trial collector artifacts on top.
    let telemetry_artifact_count = telemetry_artifact_count(measured, resolved)?;
    let required_count = warmups
        .checked_add(
            measured
                .checked_mul(2)
                .ok_or(OrchestrationError::Preflight(
                    "evidence artifact count overflow",
                ))?,
        )
        .and_then(|count| count.checked_add(3)) // phase timeline, lifecycle metadata, runtime topology
        .and_then(|count| count.checked_add(log_artifact_count))
        .and_then(|count| count.checked_add(telemetry_artifact_count))
        .ok_or(OrchestrationError::Preflight(
            "evidence artifact count overflow",
        ))?;
    if writer.remaining_artifact_count() < required_count {
        return Err(OrchestrationError::Preflight(
            "bundle artifact-count bound cannot hold mandatory runner evidence",
        ));
    }

    let phase_bytes = u64::try_from(phase_bound)
        .ok()
        .and_then(|count| count.checked_mul(256))
        .and_then(|bytes| bytes.checked_add(2_048))
        .ok_or(OrchestrationError::Preflight(
            "phase artifact bound overflow",
        ))?;
    let lifecycle_bytes = lifecycle_byte_bound(&identities)?;
    // Runtime-topology evidence carries one entry per identity plus the
    // startup-established non-secret bindings.
    let topology_bytes = topology_byte_bound(&identities)?;
    let telemetry_bytes = telemetry_byte_estimate(resolved)?;
    if writer.max_artifact_bytes()
        < phase_bytes
            .max(lifecycle_bytes)
            .max(topology_bytes)
            .max(telemetry_bytes.floor)
            .max(512)
    {
        return Err(OrchestrationError::Preflight(
            "per-artifact bound cannot hold mandatory runner metadata",
        ));
    }

    let mut required_bytes = phase_bytes
        .checked_add(lifecycle_bytes)
        .and_then(|bytes| bytes.checked_add(topology_bytes))
        .and_then(|bytes| bytes.checked_add(u64::try_from(warmups).ok()?.checked_mul(512)?))
        .and_then(|bytes| {
            // `result.json` plus normalized `metrics.json` per measured trial.
            bytes.checked_add(u64::try_from(measured).ok()?.checked_mul(1_024)?)
        })
        .and_then(|bytes| {
            bytes.checked_add(
                u64::try_from(measured)
                    .ok()?
                    .checked_mul(telemetry_bytes.per_trial)?,
            )
        })
        .ok_or(OrchestrationError::Preflight(
            "runner artifact byte bound overflow",
        ))?;
    for identity in identities {
        let log_limit = if identity == "subject" {
            DEFAULT_SUBJECT_LOG_LIMIT_BYTES
        } else {
            resolved
                .topology
                .iter()
                .find(|service| service.name.as_str() == identity)
                .map_or(0, |service| service.log_limit_bytes)
        };
        if log_limit > writer.max_artifact_bytes() {
            return Err(OrchestrationError::Preflight(
                "per-artifact bound is smaller than a declared service log cap",
            ));
        }
        required_bytes = required_bytes
            .checked_add(
                log_limit
                    .checked_mul(2)
                    .ok_or(OrchestrationError::Preflight(
                        "service log byte bound overflow",
                    ))?,
            )
            .ok_or(OrchestrationError::Preflight(
                "runner artifact byte bound overflow",
            ))?;
    }
    if writer.remaining_total_bytes() < required_bytes {
        return Err(OrchestrationError::Preflight(
            "bundle byte bound cannot hold mandatory runner evidence",
        ));
    }
    Ok(())
}

fn begin_phase(
    events: &mut Vec<PhaseEvent>,
    phase: PhaseKind,
    warmup_ordinal: Option<u32>,
    trial_id: Option<TrialId>,
    origin: Instant,
) -> usize {
    let index = events.len();
    events.push(PhaseEvent {
        sequence: u64::try_from(index).unwrap_or(u64::MAX),
        phase,
        warmup_ordinal,
        trial_id,
        start_offset_ns: nanos(origin.elapsed()),
        elapsed_ns: None,
        outcome: None,
        failure_category: None,
    });
    index
}
fn finish_phase(
    events: &mut [PhaseEvent],
    index: usize,
    origin: Instant,
    outcome: PhaseOutcome,
    failure: Option<FailureCategory>,
) {
    let event = &mut events[index];
    event.elapsed_ns = Some(nanos(origin.elapsed()).saturating_sub(event.start_offset_ns));
    event.outcome = Some(outcome);
    event.failure_category = failure;
}
fn nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}
fn status_for(category: FailureCategory) -> ExecutionStatus {
    if category == FailureCategory::Cancelled {
        ExecutionStatus::Cancelled
    } else {
        ExecutionStatus::Failed
    }
}

fn workload_failure(category: FailureCategory) -> FailureCategory {
    match category {
        FailureCategory::Cancelled | FailureCategory::TimedOut => category,
        _ => FailureCategory::WorkloadFailed,
    }
}
fn outcome_for(category: FailureCategory) -> PhaseOutcome {
    match category {
        FailureCategory::Cancelled => PhaseOutcome::Cancelled,
        FailureCategory::TimedOut => PhaseOutcome::TimedOut,
        _ => PhaseOutcome::Failed,
    }
}
fn derive_seed(seed: u64, kind: InvocationKind) -> u64 {
    let namespace = match kind {
        InvocationKind::Warmup { ordinal } => 0x5741_524d_0000_0000_u64 | u64::from(ordinal),
        InvocationKind::Measured { trial_id, arm } => {
            let arm_bit = match arm {
                Some(TrialArm::Candidate) => 0x0000_0001_0000_0000_u64,
                Some(TrialArm::Baseline) | None => 0,
            };
            0x5452_4941_0000_0000_u64 | arm_bit | u64::from(trial_id.get())
        }
    };
    mix64(seed ^ namespace)
}
fn mix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

/// Paired arm and pair identity for measured trial `number` (1-based) under
/// the v1 alternating schedule: odd trials measure baseline, even trials
/// measure candidate; pair identities count consecutive baseline/candidate
/// trials starting at one.
fn paired_assignment(number: u32) -> (TrialArm, u32) {
    let arm = if number.is_multiple_of(2) {
        TrialArm::Candidate
    } else {
        TrialArm::Baseline
    };
    (arm, (number + 1) / 2)
}

/// Warmup arm for `ordinal` (1-based) under a paired run: round-robin
/// starting with baseline. Warmups carry no pair identity.
fn warmup_arm(ordinal: u32) -> TrialArm {
    if ordinal.is_multiple_of(2) {
        TrialArm::Candidate
    } else {
        TrialArm::Baseline
    }
}

/// Effective workload for one invocation of a paired run: the resolved load
/// shape directed at the arm's service. Unpaired runs use the resolved
/// workload unchanged.
fn effective_workload(resolved: &eggbench_core::ResolvedPlan, arm: TrialArm) -> Workload {
    match &resolved.paired {
        Some(design) => {
            let service = match arm {
                TrialArm::Baseline => &design.baseline.service,
                TrialArm::Candidate => &design.candidate.service,
            };
            resolved.workload.with_target(service.clone())
        }
        None => resolved.workload.clone(),
    }
}

fn stage_trial(
    writer: &mut BundleWriter,
    id: TrialId,
    result: &TrialExecutionResult,
    output: WorkloadOutput,
    telemetry: &TrialTelemetry<'_>,
    resolved: &eggbench_core::ResolvedPlan,
) -> Result<(ArtifactPath, Vec<ArtifactPath>), BundleError> {
    let base = format!("trials/{:03}", id.get());
    let bytes = serde_json::to_vec_pretty(&result)
        .map_err(|error| BundleError::ManifestParse(error.to_string()))?;
    let result_path = ArtifactPath::new(format!("{base}/result.json"))?;
    writer.add_artifact(
        result_path.clone(),
        ArtifactRole::TrialResult,
        "application/json",
        Sensitivity::Public,
        bytes.as_slice(),
    )?;
    // Capture raw artifact names before staging consumes the output so metric
    // provenance can resolve same-trial references without trusting driver paths.
    let artifact_names: Vec<String> = output
        .artifacts
        .iter()
        .map(|artifact| artifact.name.clone())
        .collect();
    let metrics = output.metrics.clone();
    let histograms = output.histograms.clone();
    let error_counts = output.error_counts.clone();
    let mut artifacts = stage_workload_artifacts(writer, &base, output, true)?;
    let mut artifact_map = BTreeMap::new();
    for (name, path) in artifact_names.into_iter().zip(artifacts.iter().cloned()) {
        artifact_map.insert(name, path);
    }
    // Telemetry artifacts stage under a separate namespace; a safe-name
    // collision with workload artifacts fails closed rather than silently
    // shadowing provenance references.
    let mut telemetry_observations = Vec::new();
    let mut telemetry_warnings: Vec<MetricWarning> = Vec::new();
    for (collector_index, (source, toutput)) in telemetry.outputs.iter().enumerate() {
        if toutput.artifacts.len() > MAX_TELEMETRY_ARTIFACTS_PER_TRIAL {
            return Err(BundleError::BoundExceeded("telemetry artifact count"));
        }
        for (artifact_index, artifact) in toutput.artifacts.iter().enumerate() {
            validate_artifact_name(&artifact.name)?;
            // Collector/source labels never enter the path: indices keep
            // staging deterministic even for adversarial labels.
            let path = ArtifactPath::new(format!(
                "{base}/telemetry/{collector_index:02}-{artifact_index:02}-{}",
                artifact.name,
            ))?;
            writer.add_artifact(
                path.clone(),
                ArtifactRole::TrialArtifact,
                artifact.media_type.clone(),
                Sensitivity::Redacted,
                artifact.bytes.as_slice(),
            )?;
            artifacts.push(path.clone());
            if artifact_map.insert(artifact.name.clone(), path).is_some() {
                return Err(BundleError::InvalidManifest(
                    "telemetry/workload artifact name collision",
                ));
            }
        }
        let _ = source;
        telemetry_observations.extend(toutput.metrics.iter().cloned());
        telemetry_warnings.extend(toutput.warnings.iter().cloned());
    }
    telemetry_warnings.extend(telemetry.warnings.iter().cloned());
    let mut combined = metrics;
    combined.extend(telemetry_observations);
    let (producer, producer_version) = workload_producer(resolved);
    let input = NormalizationInput {
        trial_id: id,
        metrics: &resolved.metrics,
        terminal_status: result.terminal_status,
        observations: &combined,
        histograms: &histograms,
        error_counts: &error_counts,
        artifact_map: &artifact_map,
        producer: &producer,
        producer_version: producer_version.as_deref(),
    };
    let mut normalized = normalize_trial_metrics(&input)?;
    // Telemetry warnings append deterministically after normalization's own
    // warnings; the bound is re-checked so overflow still fails closed.
    normalized.warnings.extend(telemetry_warnings);
    normalized.validate()?;
    let metrics_bytes = normalized.to_json_bytes()?;
    let metrics_path = trial_metrics_path(id)?;
    writer.add_artifact(
        metrics_path.clone(),
        ArtifactRole::TrialArtifact,
        "application/json",
        Sensitivity::Public,
        metrics_bytes.as_slice(),
    )?;
    artifacts.push(metrics_path);
    Ok((result_path, artifacts))
}

/// Validate one driver-supplied artifact name: a single safe path component.
fn validate_artifact_name(name: &str) -> Result<(), BundleError> {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name == "." || name == ".." {
        return Err(BundleError::InvalidManifest(
            "unsafe telemetry artifact name",
        ));
    }
    Ok(())
}

/// Resolve the workload producer label/version from the resolved driver
/// inventory. Falls back to the `unknown-workload` label only when no
/// workload driver was resolved; provenance always names the source.
fn workload_producer(resolved: &eggbench_core::ResolvedPlan) -> (String, Option<String>) {
    use eggbench_core::DriverCategory;
    if let Some(driver) = resolved.drivers.get(&DriverCategory::Workload) {
        (
            driver.descriptor.name.as_str().to_owned(),
            Some(driver.descriptor.adapter_version.clone()),
        )
    } else {
        ("unknown-workload".to_owned(), None)
    }
}
fn stage_warmup(
    writer: &mut BundleWriter,
    ordinal: u32,
    elapsed: Duration,
    failure: Option<FailureCategory>,
    output: WorkloadOutput,
) -> Result<(), BundleError> {
    #[derive(Serialize)]
    struct WarmupRecord {
        schema_version: SchemaVersion,
        ordinal: u32,
        elapsed_ns: u64,
        status: &'static str,
        failure_category: Option<FailureCategory>,
    }
    let record = WarmupRecord {
        schema_version: WARMUP_RECORD_SCHEMA_VERSION,
        ordinal,
        elapsed_ns: nanos(elapsed),
        status: if failure.is_some() {
            "failed"
        } else {
            "completed"
        },
        failure_category: failure,
    };
    let bytes = serde_json::to_vec_pretty(&record)
        .map_err(|error| BundleError::ManifestParse(error.to_string()))?;
    let path = ArtifactPath::new(format!("warmups/{ordinal:03}/result.json"))?;
    writer.add_artifact(
        path,
        ArtifactRole::Other {
            label: Name::new("warmup").expect("static role"),
        },
        "application/json",
        Sensitivity::Redacted,
        bytes.as_slice(),
    )?;
    let _ = stage_workload_artifacts(writer, &format!("warmups/{ordinal:03}"), output, false)?;
    Ok(())
}
fn stage_workload_artifacts(
    writer: &mut BundleWriter,
    base: &str,
    output: WorkloadOutput,
    measured: bool,
) -> Result<Vec<ArtifactPath>, BundleError> {
    if output.artifacts.len() > MAX_WORKLOAD_ARTIFACTS_PER_INVOCATION {
        return Err(BundleError::BoundExceeded("workload artifact count"));
    }
    let mut staged = Vec::new();
    for (index, artifact) in output.artifacts.into_iter().enumerate() {
        if artifact.name.is_empty()
            || artifact.name.contains('/')
            || artifact.name.contains('\\')
            || artifact.name == "."
            || artifact.name == ".."
        {
            return Err(BundleError::InvalidManifest(
                "unsafe workload artifact name",
            ));
        }
        let path = ArtifactPath::new(format!(
            "{base}/artifacts/{:03}-{}",
            index + 1,
            artifact.name
        ))?;
        let role = if measured {
            ArtifactRole::TrialArtifact
        } else {
            ArtifactRole::Other {
                label: Name::new("warmup_artifact").expect("static role"),
            }
        };
        writer.add_artifact(
            path.clone(),
            role,
            artifact.media_type,
            Sensitivity::Redacted,
            artifact.bytes.as_slice(),
        )?;
        if measured {
            staged.push(path);
        }
    }
    Ok(staged)
}
fn stage_phase_artifacts(
    writer: &mut BundleWriter,
    phases: &[PhaseEvent],
) -> Result<(), BundleError> {
    let bytes = serde_json::to_vec_pretty(phases)
        .map_err(|error| BundleError::ManifestParse(error.to_string()))?;
    let path = ArtifactPath::new("runner-phases.json")?;
    writer.add_artifact(
        path,
        ArtifactRole::Other {
            label: Name::new("runner_phases").expect("static role"),
        },
        "application/json",
        Sensitivity::Redacted,
        bytes.as_slice(),
    )?;
    Ok(())
}

/// A deterministic workload adapter for state-machine qualification.
#[derive(Debug, Clone)]
pub struct FakeWorkload {
    /// Delay before successful invocations.
    pub delay: Duration,
    /// Invocation ordinal that fails, if any (warmups then trials; one-based).
    pub fail_on: Option<u32>,
    /// Invocation ordinal that intentionally remains pending until cancelled/timed out.
    pub pending_on: Option<u32>,
    /// Delay/failure behavior for drain.
    pub drain_delay: Duration,
    /// Whether drain returns a failure.
    pub drain_fails: bool,
    /// Per-invocation artifacts to return alongside the workload output. Indexed by
    /// invocation ordinal (warmups first, then measured trials). An entry of `Some(artifacts)`
    /// overrides the default empty output; `None` returns an empty [`WorkloadOutput`].
    ///
    /// Use this to exercise evidence-staging paths that respond to adversarial artifact
    /// names, counts, or byte sizes.
    pub artifacts_by_invocation: Vec<Option<Vec<WorkloadArtifact>>>,
    /// Per-invocation raw metric observations for normalization qualification.
    /// Indexed like `artifacts_by_invocation`; `None` means no observations.
    pub metrics_by_invocation: Vec<Option<Vec<RawMetricObservation>>>,
    /// Per-invocation raw histogram inputs. Indexed like `artifacts_by_invocation`.
    pub histograms_by_invocation: Vec<Option<Vec<RawHistogramInput>>>,
    /// Per-invocation raw error-category counts. Indexed like `artifacts_by_invocation`.
    pub error_counts_by_invocation: Vec<Option<Vec<(String, u64)>>>,
    invocation_count: u32,
    /// Invocation kinds in execution order.
    pub invocations: Vec<InvocationKind>,
    /// Effective workload target per invocation, in execution order.
    pub workload_targets: Vec<String>,
    /// Derived invocation seed per invocation, in execution order.
    pub invocation_seeds: Vec<Option<u64>>,
    /// Whether drain was entered.
    pub drained: bool,
}
impl Default for FakeWorkload {
    fn default() -> Self {
        Self {
            delay: Duration::ZERO,
            fail_on: None,
            pending_on: None,
            drain_delay: Duration::ZERO,
            drain_fails: false,
            artifacts_by_invocation: Vec::new(),
            metrics_by_invocation: Vec::new(),
            histograms_by_invocation: Vec::new(),
            error_counts_by_invocation: Vec::new(),
            invocation_count: 0,
            invocations: Vec::new(),
            workload_targets: Vec::new(),
            invocation_seeds: Vec::new(),
            drained: false,
        }
    }
}
impl WorkloadExecutor for FakeWorkload {
    fn execute<'a>(
        &'a mut self,
        context: InvocationContext,
    ) -> Pin<Box<dyn Future<Output = Result<WorkloadOutput, FailureCategory>> + Send + 'a>> {
        Box::pin(async move {
            self.invocation_count += 1;
            self.invocations.push(context.kind);
            self.workload_targets
                .push(context.workload.target().as_str().to_owned());
            self.invocation_seeds.push(context.seed);
            if self.pending_on == Some(self.invocation_count) {
                context.cancellation.cancelled().await;
                return Err(FailureCategory::Cancelled);
            }
            tokio::time::sleep(self.delay).await;
            if self.fail_on == Some(self.invocation_count) {
                Err(FailureCategory::WorkloadFailed)
            } else {
                let index = self.invocation_count as usize - 1;
                let artifacts = self
                    .artifacts_by_invocation
                    .get(index)
                    .and_then(Clone::clone)
                    .unwrap_or_default();
                let metrics = self
                    .metrics_by_invocation
                    .get(index)
                    .and_then(Clone::clone)
                    .unwrap_or_default();
                let histograms = self
                    .histograms_by_invocation
                    .get(index)
                    .and_then(Clone::clone)
                    .unwrap_or_default();
                let error_counts = self
                    .error_counts_by_invocation
                    .get(index)
                    .and_then(Clone::clone)
                    .unwrap_or_default();
                Ok(WorkloadOutput {
                    artifacts,
                    metrics,
                    histograms,
                    error_counts,
                })
            }
        })
    }
    fn drain<'a>(
        &'a mut self,
        _context: DrainContext,
    ) -> Pin<Box<dyn Future<Output = Result<(), FailureCategory>> + Send + 'a>> {
        Box::pin(async move {
            self.drained = true;
            tokio::time::sleep(self.drain_delay).await;
            if self.drain_fails {
                Err(FailureCategory::DrainFailed)
            } else {
                Ok(())
            }
        })
    }
}
