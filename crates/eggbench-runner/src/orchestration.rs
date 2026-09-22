//! Local run phase orchestration above the process-owning [`LocalSession`].

use crate::DEFAULT_SUBJECT_LOG_LIMIT_BYTES;
use crate::{
    CleanupFailure, LifecycleOutcome, LocalSession, stage_lifecycle_logs, stage_lifecycle_metadata,
};
use eggbench_core::{
    ArtifactPath, ArtifactRole, BundleError, BundleManifest, BundleWriter, ComparisonVerdict,
    ExecutionStatus, Name, ResetPolicy, RunId, SchemaVersion, Sensitivity, TrialDescriptor,
    TrialExecutionFailure, TrialExecutionResult, TrialExecutionStatus, TrialId, Workload,
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

/// Non-metric result from a workload invocation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkloadOutput {
    /// Optional diagnostic files, bounded during bundle staging.
    pub artifacts: Vec<WorkloadArtifact>,
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
    /// Evidence serialization and bundle publication.
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
    /// Evidence could not be truthfully finalized.
    #[error(transparent)]
    Evidence(#[from] BundleError),
}

/// Run resolved work through startup, warmups, measured trials, and cleanup.
///
/// `writer` must already contain source plan, resolved plan, and environment artifacts.
/// Operational workload/reset/cleanup failures are returned in a finalized [`RunOutcome`].
///
/// # Errors
/// Returns an error for preflight or evidence finalization failures.
#[allow(clippy::too_many_lines)] // Keep the canonical phase transition order auditable in one place.
pub async fn execute_run<E: WorkloadExecutor + ?Sized>(
    session: &mut LocalSession,
    resolved: &eggbench_core::ResolvedPlan,
    executor: &mut E,
    resets: &ResetRegistry,
    mut writer: BundleWriter,
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
    let mut phases = Vec::with_capacity(phase_bound);
    let mut status = ExecutionStatus::Completed;
    let mut primary_failure = None;
    let mut cleanup_failures = Vec::new();
    let mut trials = Vec::new();
    let mut started = Vec::new();

    if cancel.is_cancelled() {
        let index = begin_phase(&mut phases, PhaseKind::StartupReadiness, None, None, origin);
        status = ExecutionStatus::Cancelled;
        primary_failure = Some(FailureCategory::Cancelled);
        finish_phase(
            &mut phases,
            index,
            origin,
            PhaseOutcome::Cancelled,
            primary_failure,
        );
    } else {
        let index = begin_phase(&mut phases, PhaseKind::StartupReadiness, None, None, origin);
        match session.startup(cancel).await {
            Ok(report) => {
                started = report.started;
                finish_phase(&mut phases, index, origin, PhaseOutcome::Completed, None);
            }
            Err(error) => {
                started = session
                    .events()
                    .iter()
                    .filter(|event| event.kind == crate::LifecycleEventKind::Spawned)
                    .map(|event| event.identity.clone())
                    .collect();
                status = if cancel.is_cancelled() {
                    ExecutionStatus::Cancelled
                } else {
                    ExecutionStatus::Failed
                };
                primary_failure = Some(if cancel.is_cancelled() {
                    FailureCategory::Cancelled
                } else {
                    FailureCategory::StartupFailed
                });
                cleanup_failures.extend_from_slice(error.cleanup());
                finish_phase(
                    &mut phases,
                    index,
                    origin,
                    if cancel.is_cancelled() {
                        PhaseOutcome::Cancelled
                    } else {
                        PhaseOutcome::Failed
                    },
                    primary_failure,
                );
            }
        }
    }

    if status == ExecutionStatus::Completed {
        for ordinal in 1..=warmup_count {
            let index = begin_phase(&mut phases, PhaseKind::Warmup, Some(ordinal), None, origin);
            match execute_invocation(
                executor,
                cancel,
                run_id,
                resolved,
                InvocationKind::Warmup { ordinal },
                config.warmup_timeout,
                origin,
            )
            .await
            {
                InvocationResult::Completed(output, elapsed, _) => {
                    finish_phase(&mut phases, index, origin, PhaseOutcome::Completed, None);
                    stage_warmup(&mut writer, ordinal, elapsed, None, output)?;
                }
                InvocationResult::Failure(category, elapsed, _) => {
                    let category = workload_failure(category);
                    status = status_for(category);
                    primary_failure = Some(category);
                    finish_phase(
                        &mut phases,
                        index,
                        origin,
                        outcome_for(category),
                        Some(category),
                    );
                    stage_warmup(
                        &mut writer,
                        ordinal,
                        elapsed,
                        Some(category),
                        WorkloadOutput::default(),
                    )?;
                    break;
                }
            }
        }
    }

    if status == ExecutionStatus::Completed {
        for number in 1..=trial_count {
            if cancel.is_cancelled() {
                status = ExecutionStatus::Cancelled;
                primary_failure = Some(FailureCategory::Cancelled);
                break;
            }
            let trial_id = TrialId::new(number)?;
            let index = begin_phase(
                &mut phases,
                PhaseKind::MeasuredTrial,
                None,
                Some(trial_id),
                origin,
            );
            match execute_invocation(
                executor,
                cancel,
                run_id,
                resolved,
                InvocationKind::Measured { trial_id },
                config.measurement_timeout,
                origin,
            )
            .await
            {
                InvocationResult::Completed(output, elapsed, start_offset_ns) => {
                    finish_phase(&mut phases, index, origin, PhaseOutcome::Completed, None);
                    let result = TrialExecutionResult {
                        schema_version: TRIAL_RESULT_SCHEMA_VERSION,
                        trial_id,
                        measurement_start_offset_ns: start_offset_ns,
                        measurement_elapsed_ns: nanos(elapsed),
                        terminal_status: TrialExecutionStatus::Completed,
                        failure_category: None,
                    };
                    let (result_path, artifacts) =
                        stage_trial(&mut writer, trial_id, &result, output)?;
                    trials.push(TrialDescriptor {
                        id: trial_id,
                        result: result_path,
                        artifacts,
                    });
                }
                InvocationResult::Failure(category, elapsed, start_offset_ns) => {
                    let category = workload_failure(category);
                    status = status_for(category);
                    primary_failure = Some(category);
                    finish_phase(
                        &mut phases,
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
                    let (result_path, artifacts) =
                        stage_trial(&mut writer, trial_id, &result, WorkloadOutput::default())?;
                    trials.push(TrialDescriptor {
                        id: trial_id,
                        result: result_path,
                        artifacts,
                    });
                    break;
                }
            }
            if number < trial_count && status == ExecutionStatus::Completed {
                if let Some(reset) = config.reset.as_ref() {
                    let index =
                        begin_phase(&mut phases, PhaseKind::Reset, None, Some(trial_id), origin);
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
                            finish_phase(&mut phases, index, origin, PhaseOutcome::Completed, None);
                        }
                        Err(category) => {
                            token.cancel();
                            let category = match category {
                                FailureCategory::Cancelled | FailureCategory::TimedOut => category,
                                _ => FailureCategory::ResetFailed,
                            };
                            status = status_for(category);
                            primary_failure = Some(category);
                            finish_phase(
                                &mut phases,
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
                        &mut phases,
                        PhaseKind::Cooldown,
                        None,
                        Some(trial_id),
                        origin,
                    );
                    let sleep = tokio::time::sleep(Duration::from_millis(cooldown.get()));
                    tokio::pin!(sleep);
                    tokio::select! { () = cancel.cancelled() => { status = ExecutionStatus::Cancelled; primary_failure = Some(FailureCategory::Cancelled); finish_phase(&mut phases, index, origin, PhaseOutcome::Cancelled, primary_failure); break; }, () = &mut sleep => finish_phase(&mut phases, index, origin, PhaseOutcome::Completed, None) }
                }
            }
        }
    }

    let index = begin_phase(&mut phases, PhaseKind::Drain, None, None, origin);
    let drain_context = DrainContext {
        run_id,
        cancellation: cancel.clone(),
        timeout: config.drain_timeout,
    };
    match timeout(config.drain_timeout, executor.drain(drain_context)).await {
        Ok(Ok(())) => finish_phase(&mut phases, index, origin, PhaseOutcome::Completed, None),
        Ok(Err(category)) => {
            if status == ExecutionStatus::Completed {
                if category == FailureCategory::Cancelled {
                    status = ExecutionStatus::Cancelled;
                    primary_failure = Some(FailureCategory::Cancelled);
                } else {
                    status = ExecutionStatus::Failed;
                    primary_failure = Some(FailureCategory::DrainFailed);
                }
            }
            finish_phase(
                &mut phases,
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
            if status == ExecutionStatus::Completed {
                status = ExecutionStatus::Failed;
                primary_failure = Some(FailureCategory::DrainFailed);
            }
            finish_phase(
                &mut phases,
                index,
                origin,
                PhaseOutcome::TimedOut,
                Some(FailureCategory::TimedOut),
            );
        }
    }
    if cancel.is_cancelled() && status == ExecutionStatus::Completed {
        status = ExecutionStatus::Cancelled;
        primary_failure = Some(FailureCategory::Cancelled);
    }
    let index = begin_phase(&mut phases, PhaseKind::Teardown, None, None, origin);
    let shutdown = session.shutdown().await;
    let stopped_order = shutdown.stopped_order;
    cleanup_failures.extend(shutdown.failures);
    if cancel.is_cancelled() && status == ExecutionStatus::Completed {
        status = ExecutionStatus::Cancelled;
        primary_failure = Some(FailureCategory::Cancelled);
    }
    if cleanup_failures.is_empty() {
        finish_phase(&mut phases, index, origin, PhaseOutcome::Completed, None);
    } else {
        if status == ExecutionStatus::Completed {
            status = ExecutionStatus::Failed;
            primary_failure = Some(FailureCategory::TeardownFailed);
        }
        finish_phase(
            &mut phases,
            index,
            origin,
            PhaseOutcome::Failed,
            Some(FailureCategory::TeardownFailed),
        );
    }

    let lifecycle = LifecycleOutcome {
        started,
        stopped_order,
        cleanup: cleanup_failures.clone(),
    };
    stage_lifecycle_logs(session, &mut writer).await?;
    stage_lifecycle_metadata(session, &lifecycle, &mut writer)?;

    let final_index = begin_phase(&mut phases, PhaseKind::Finalization, None, None, origin);
    finish_phase(
        &mut phases,
        final_index,
        origin,
        PhaseOutcome::Completed,
        None,
    );
    stage_phase_artifacts(&mut writer, &phases)?;
    // Finalization event covers phase artifact staging; manifest publication follows it.
    let path = writer.final_path().to_path_buf();
    let bundle = writer.finalize(
        status,
        None::<ComparisonVerdict>,
        resolved.subject.clone(),
        resolved
            .drivers
            .values()
            .map(|driver| driver.descriptor.clone())
            .collect(),
        trials,
        None,
        None,
    )?;
    finish_phase(
        &mut phases,
        final_index,
        origin,
        PhaseOutcome::Completed,
        None,
    );
    Ok(RunOutcome {
        execution_status: status,
        primary_failure,
        cleanup_failures,
        phases,
        bundle_path: path,
        manifest: bundle.manifest().clone(),
    })
}

enum InvocationResult {
    Completed(WorkloadOutput, Duration, u64),
    Failure(FailureCategory, Duration, u64),
}

async fn execute_invocation<E: WorkloadExecutor + ?Sized>(
    executor: &mut E,
    cancel: &CancellationToken,
    run_id: RunId,
    resolved: &eggbench_core::ResolvedPlan,
    kind: InvocationKind,
    limit: Duration,
    origin: Instant,
) -> InvocationResult {
    let child = cancel.child_token();
    let context = InvocationContext {
        run_id,
        kind,
        workload: resolved.workload.clone(),
        seed: resolved.seed.map(|seed| derive_seed(seed, kind)),
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
    let required_count = warmups
        .checked_add(measured)
        .and_then(|count| count.checked_add(2)) // phase timeline and lifecycle metadata
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
    let lifecycle_bytes = u64::try_from(identities.len())
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
        ))?;
    if writer.max_artifact_bytes() < phase_bytes.max(lifecycle_bytes).max(512) {
        return Err(OrchestrationError::Preflight(
            "per-artifact bound cannot hold mandatory runner metadata",
        ));
    }

    let mut required_bytes = phase_bytes
        .checked_add(lifecycle_bytes)
        .and_then(|bytes| bytes.checked_add(u64::try_from(warmups).ok()?.checked_mul(512)?))
        .and_then(|bytes| bytes.checked_add(u64::try_from(measured).ok()?.checked_mul(512)?))
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
    let artifacts = stage_workload_artifacts(writer, &base, output, true)?;
    Ok((result_path, artifacts))
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
                Ok(WorkloadOutput::default())
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
