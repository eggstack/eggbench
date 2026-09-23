//! `eggbench run <plan> <bundle>` command.
//!
//! The command pipeline is:
//! `parse → validate → resolve → doctor/preflight → collect environment +
//! subject snapshot → prepare BundleWriter → LocalSession::prepare →
//! WorkloadExecutor/reset hooks → execute_run → machine/human result`.
//!
//! M003 ships with the deterministic `FakeWorkload` adapter only. Production
//! traffic generators belong to External Oracles / Eggstack Integrations
//! milestones; the CLI fails closed with `unsupported_capability` instead
//! of inventing a production workload.

use crate::envelope::{CliEnvelope, CliOutput, ExitCode, PathBufPayload};
use crate::error::{CliError, CliFailure};
use crate::plan_input::load_plan;
use crate::workload_registry::{BuiltinWorkloadExecutor, WorkloadRegistry};
use crate::{CommandOptions, InputFormat};
use eggbench_core::{DefaultDriverPolicy, DriverCategory, LoadMode, Name, ResolutionOptions};
use eggbench_runner::{
    BundlePreparation, MapSecretProvider, PlatformAdapter, ResetRegistry, RunnerOptions,
    SecretProvider, UnixPlatform, collect_local_environment, prepare_bundle,
};
use std::collections::BTreeMap;
use std::path::Path;
use tokio_util::sync::CancellationToken;

/// Run the experiment end-to-end and produce a finalized bundle.
pub async fn run(
    plan: &Path,
    input_format: Option<InputFormat>,
    bundle: &Path,
    _options: CommandOptions,
) -> Result<CliEnvelope, CliError> {
    let input = load_plan(plan, input_format)?;
    let plan = input.plan;
    let plan_bytes = input.bytes;
    let plan_media_type = match input.format {
        InputFormat::Toml => "application/toml",
        InputFormat::Json => "application/json",
    };

    let platform_label = UnixPlatform.label().to_owned();
    let platform =
        Name::new(platform_label.clone()).unwrap_or_else(|_| Name::new("unknown").unwrap());

    let registry = WorkloadRegistry::with_builtin();
    let driver_inventory = registry.inventory();
    let descriptors: Vec<_> = driver_inventory
        .iter()
        .map(|entry| entry.descriptor.to_descriptor())
        .collect();

    let mut options = ResolutionOptions {
        selections: BTreeMap::default(),
        default_policy: DefaultDriverPolicy::Deterministic,
        platform,
        executable_paths: BTreeMap::default(),
        required_capabilities: BTreeMap::default(),
    };
    let workload_mode = workload_load_mode(&plan);
    let mut caps = BTreeMap::new();
    caps.insert(
        DriverCategory::Workload,
        std::collections::BTreeSet::from([eggbench_core::Capability::LoadMode {
            mode: workload_mode,
        }]),
    );
    options.required_capabilities = caps;

    let resolved =
        eggbench_core::resolve_plan(&plan, &descriptors, &options).map_err(CliError::Resolution)?;

    let managed_executable = match &resolved.subject {
        eggbench_core::Subject::ManagedCommand { argv, .. } => argv.first().cloned(),
        _ => None,
    };
    let resolved_executable_path = if let (eggbench_core::Subject::ManagedCommand { .. }, Some(_)) =
        (&resolved.subject, &managed_executable)
    {
        match resolve_managed_executable_path(&resolved) {
            Ok(path) => Some(path),
            Err(failure) => return Ok(failure_envelope("run", &failure)),
        }
    } else {
        None
    };

    let (environment, subject_snapshot) =
        match collect_local_environment(&resolved, resolved_executable_path.as_deref()) {
            Ok(pair) => pair,
            Err(error) => {
                return Ok(failure_envelope_from_prepare("run", &error.to_string()));
            }
        };

    if let Some(false) = subject_snapshot.declared_digest_matches() {
        let failure = CliFailure::new(
            "subject_digest_mismatch",
            "declared subject digest does not match observed executable digest",
            ExitCode::CapabilityPreflight,
        );
        return Ok(CliEnvelope::fail("run", &failure));
    }

    if let Some(workload) = registry.default_workload() {
        if workload.name != "fake-load" {
            let failure = CliFailure::new(
                "unsupported_workload",
                format!(
                    "workload driver {} is not a production adapter",
                    workload.name
                ),
                ExitCode::CapabilityPreflight,
            );
            return Ok(CliEnvelope::fail("run", &failure));
        }
    } else {
        let failure = CliFailure::new(
            "unsupported_workload",
            "no workload driver is registered",
            ExitCode::CapabilityPreflight,
        );
        return Ok(CliEnvelope::fail("run", &failure));
    }

    let bounds = plan.bounds;

    let secret_provider: std::sync::Arc<dyn SecretProvider> =
        std::sync::Arc::new(MapSecretProvider::empty());
    let runner_options = RunnerOptions {
        workspace_root: std::env::current_dir()
            .map_err(|error| CliError::Internal(error.to_string()))?,
        secrets: secret_provider,
        probes: eggbench_runner::ProbeRegistry::with_builtins(),
        platform: std::sync::Arc::new(UnixPlatform),
    };

    let session = match eggbench_runner::LocalSession::prepare(&resolved, runner_options) {
        Ok(session) => session,
        Err(error) => return Ok(failure_envelope("run", &failure_from_runner_error(error))),
    };

    let writer = match prepare_bundle(&BundlePreparation {
        destination: bundle,
        run_id: eggbench_core::RunId::new(),
        source_plan_bytes: &plan_bytes,
        source_plan_media_type: plan_media_type,
        resolved_plan: &resolved,
        environment: &environment,
        subject_snapshot: &subject_snapshot,
        bounds,
    }) {
        Ok(writer) => writer,
        Err(error) => return Ok(failure_envelope("run", &failure_from_bundle_error(error))),
    };

    let mut fake_workload = eggbench_runner::test_support::FakeWorkload::default();
    fake_workload.delay = std::time::Duration::from_millis(0);
    let mut executor = BuiltinWorkloadExecutor::new(fake_workload);

    let resets = ResetRegistry::default();
    let mut session = session;
    let cancel = CancellationToken::new();
    let outcome = match eggbench_runner::execute_run(
        &mut session,
        &resolved,
        &mut executor,
        &resets,
        writer,
        &cancel,
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(error) => {
            let failure = CliFailure::new("evidence", error.to_string(), ExitCode::EvidenceIo);
            return Ok(CliEnvelope::fail("run", &failure));
        }
    };

    let primary_failure = outcome
        .primary_failure
        .map(|category| format!("{category:?}"));
    let comparison_verdict = outcome
        .manifest
        .comparison_verdict
        .map(|verdict| format!("{verdict:?}"));

    let bundle_payload = PathBufPayload::from_path(&outcome.bundle_path)
        .unwrap_or_else(|| PathBufPayload::from_string(outcome.bundle_path.display().to_string()));

    Ok(CliEnvelope::ok(
        "run",
        CliOutput::Run {
            bundle: bundle_payload,
            bundle_published: true,
            execution_status: format!("{:?}", outcome.execution_status).to_lowercase(),
            measured_trials: outcome.manifest.trials.len(),
            primary_failure,
            comparison_verdict,
        },
    ))
}

fn workload_load_mode(plan: &eggbench_core::ExperimentPlan) -> LoadMode {
    use eggbench_core::Workload;
    match &plan.workload {
        Workload::ClosedLoop { .. } | Workload::FiniteCount { .. } => LoadMode::ClosedLoop,
        Workload::OpenLoop { .. } => LoadMode::OpenLoop,
        Workload::TimeBounded { mode, .. } => *mode,
    }
}

fn failure_envelope(command: &str, failure: &CliFailure) -> CliEnvelope {
    CliEnvelope::fail(command, failure)
}

fn failure_envelope_from_prepare(command: &str, detail: &str) -> CliEnvelope {
    let failure = CliFailure::new("prepare", detail, ExitCode::CapabilityPreflight);
    CliEnvelope::fail(command, &failure)
}

fn failure_from_runner_error(error: eggbench_runner::RunnerError) -> CliFailure {
    use eggbench_runner::RunnerError;
    let category = match &error {
        RunnerError::InvalidPlan { .. } => "invalid_plan",
        RunnerError::UnsupportedPlatform { .. } => "unsupported_platform",
        RunnerError::InvalidWorkingDirectory { .. } => "invalid_working_directory",
        RunnerError::InvalidExecutablePath { .. } => "invalid_executable_path",
        RunnerError::EmptyArgv { .. } => "empty_argv",
        RunnerError::MissingSecret { .. } => "missing_secret",
        RunnerError::UnsupportedService { .. } => "unsupported_service",
        RunnerError::UnsupportedProbe { .. } => "unsupported_probe",
        RunnerError::SpawnFailed { .. } => "spawn_failed",
        RunnerError::ReadinessTimeout { .. } => "readiness_timeout",
        RunnerError::ReadinessFailed { .. } => "readiness_failed",
        RunnerError::Cancelled { .. } => "cancelled",
        RunnerError::CancelledBeforeSpawn => "cancelled_before_spawn",
        RunnerError::ProcessExitedEarly { .. } => "process_exited_early",
    };
    CliFailure::new(category, error.to_string(), ExitCode::CapabilityPreflight)
}

fn failure_from_bundle_error(error: eggbench_core::BundleError) -> CliFailure {
    CliFailure::new("bundle", error.to_string(), ExitCode::EvidenceIo)
}

fn resolve_managed_executable_path(
    resolved: &eggbench_core::ResolvedPlan,
) -> Result<std::path::PathBuf, CliFailure> {
    let argv0 = match &resolved.subject {
        eggbench_core::Subject::ManagedCommand { argv, .. } => argv.first().cloned(),
        _ => None,
    };
    match argv0 {
        Some(value) => Ok(std::path::PathBuf::from(value)),
        None => Err(CliFailure::new(
            "managed_executable_path",
            "no managed executable path",
            ExitCode::CapabilityPreflight,
        )),
    }
}
