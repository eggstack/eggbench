//! Local run phase orchestration above the process-owning [`LocalSession`].

use crate::DEFAULT_SUBJECT_LOG_LIMIT_BYTES;
use crate::service::RuntimeBindings;
use crate::{
    CleanupFailure, LifecycleOutcome, LocalSession, stage_lifecycle_logs, stage_lifecycle_metadata,
    stage_runtime_topology,
};
use eggbench_core::{
    ArtifactPath, ArtifactRole, BundleError, BundleManifest, BundleWriter, ComparisonVerdict,
    ExecutionStatus, Name, NormalizationInput, RawHistogramInput, RawMetricObservation,
    ResetPolicy, RunId, SchemaVersion, Sensitivity, TrialDescriptor, TrialExecutionFailure,
    TrialExecutionResult, TrialExecutionStatus, TrialId, Workload, normalize_trial_metrics,
    trial_metrics_path,
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
const TRIAL_RESULT_SCHEMA_VERSION: SchemaVersion = SchemaVersion(1);

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
    reset: Option<ResetBinding>,
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
            match execute_invocation(InvocationRequest {
                executor,
                cancel,
                run_id,
                resolved,
                bindings: session.runtime_bindings(),
                kind: InvocationKind::Warmup { ordinal },
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
            let index = begin_phase(
                &mut state.phases,
                PhaseKind::MeasuredTrial,
                None,
                Some(trial_id),
                origin,
            );
            match execute_invocation(InvocationRequest {
                executor,
                cancel,
                run_id,
                resolved,
                bindings: session.runtime_bindings(),
                kind: InvocationKind::Measured { trial_id },
                limit: config.measurement_timeout,
                origin,
            })
            .await
            {
                InvocationResult::Completed(output, elapsed, start_offset_ns) => {
                    state.workload_entered = true;
                    let result = TrialExecutionResult {
                        schema_version: TRIAL_RESULT_SCHEMA_VERSION,
                        trial_id,
                        measurement_start_offset_ns: start_offset_ns,
                        measurement_elapsed_ns: nanos(elapsed),
                        terminal_status: TrialExecutionStatus::Completed,
                        failure_category: None,
                    };
                    match stage_trial(&mut state.writer, trial_id, &result, output, resolved) {
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
                }
                InvocationResult::Failure(category, elapsed, start_offset_ns) => {
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
                    };
                    match stage_trial(
                        &mut state.writer,
                        trial_id,
                        &result,
                        WorkloadOutput::default(),
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
        limit,
        origin,
    } = request;
    let child = cancel.child_token();
    let context = InvocationContext {
        run_id,
        kind,
        workload: resolved.workload.clone(),
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
    let allowed: BTreeSet<&str> = ["measurement", "warmup", "reset", "drain"]
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
        reset,
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
    if writer.max_artifact_bytes()
        < phase_bytes
            .max(lifecycle_bytes)
            .max(topology_bytes)
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
        InvocationKind::Measured { trial_id } => {
            0x5452_4941_0000_0000_u64 | u64::from(trial_id.get())
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

fn stage_trial(
    writer: &mut BundleWriter,
    id: TrialId,
    result: &TrialExecutionResult,
    output: WorkloadOutput,
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
    let (producer, producer_version) = workload_producer(resolved);
    let input = NormalizationInput {
        trial_id: id,
        metrics: &resolved.metrics,
        terminal_status: result.terminal_status,
        observations: &metrics,
        histograms: &histograms,
        error_counts: &error_counts,
        artifact_map: &artifact_map,
        producer: &producer,
        producer_version: producer_version.as_deref(),
    };
    let normalized = normalize_trial_metrics(&input)?;
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
        schema_version: TRIAL_RESULT_SCHEMA_VERSION,
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
