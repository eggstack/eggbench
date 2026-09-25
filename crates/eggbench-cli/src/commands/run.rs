//! `eggbench run <plan> <bundle>` command.
//!
//! The command pipeline is:
//! `parse → validate → resolve → doctor/preflight → collect environment +
//! subject snapshot → prepare BundleWriter → LocalSession::prepare →
//! WorkloadExecutor/reset hooks → execute_run → machine/human result`.
//!
//! Production `eggbench` resolves against the production catalog: without
//! installed external tools or the `eggstack-http` feature, resolution or
//! executor construction fails before managed startup with a stable
//! `missing_driver`/`unsupported_workload` category; with the feature the
//! registered `EggServe`/`Eggfetch` adapters execute a real loopback run.
//! External-process drivers additionally probe their tool version in
//! preflight. Deterministic qualification uses
//! [`run_with_qualification`] with an explicitly injected `FakeWorkload`;
//! that path is never used by `main.rs` and is not reachable through any
//! public production flag.

use crate::envelope::{CliOutput, ExitCode, PathBufPayload, PresentedCommandResult};
use crate::error::{CliError, CliFailure};
use crate::plan_input::{PlanInput, load_plan};
use crate::workload_registry::{
    BuiltinWorkloadExecutor, ProductionRuntime, QualificationRuntime, WorkloadRuntime,
    production_service_adapters, production_telemetry_registry, production_workload_executor,
};
use crate::{CommandOptions, InputFormat};
use eggbench_core::{
    DefaultDriverPolicy, DriverCategory, DriverDescriptor, ExecutionStatus, LoadMode, Name,
    ResolutionOptions, ResolvedPlan,
};
use eggbench_runner::{
    BundlePreparation, MapSecretProvider, PlatformAdapter, ResetRegistry, RunnerOptions,
    SecretProvider, ServiceAdapterRegistry, UnixPlatform, WorkloadExecutor,
    collect_local_environment, prepare_bundle,
};
use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

/// Validate the explicit `--workload-driver` selection.
///
/// A malformed name is a usage failure before any plan I/O; an unknown but
/// well-formed name resolves explicitly and fails later with
/// `missing_driver` when no catalog driver matches.
fn parse_workload_driver(
    workload_driver: Option<&str>,
) -> Result<Option<Name>, Box<PresentedCommandResult>> {
    workload_driver
        .map(|name| {
            Name::new(name).map_err(|_| {
                Box::new(PresentedCommandResult::failure(
                    "run",
                    &CliFailure::new(
                        "usage",
                        format!("invalid --workload-driver name {name:?}"),
                        ExitCode::ParseValidation,
                    ),
                ))
            })
        })
        .transpose()
}

/// Production run: resolve against the production catalog and execute with
/// the registered adapters.
///
/// Resolution, executor construction, and external-tool version probing all
/// fail before environment/bundle preparation, managed startup, workload
/// invocation, or bundle publication. With `eggstack-http`, the resolved
/// workload driver selects its executor and the registered service adapters
/// join the session; unsupported drivers fail closed with
/// `unsupported_workload` before startup.
pub async fn run(
    plan: &Path,
    input_format: Option<InputFormat>,
    bundle: &Path,
    workload_driver: Option<&str>,
    _options: CommandOptions,
) -> Result<PresentedCommandResult, CliError> {
    let driver_selection = match parse_workload_driver(workload_driver) {
        Ok(selection) => selection,
        Err(presented) => return Ok(*presented),
    };
    let runtime = ProductionRuntime::new();
    let descriptors = runtime.driver_descriptors();

    let input = load_plan(plan, input_format)?;
    if input.plan.network_path.is_some() && !cfg!(feature = "eggstack-path") {
        return Ok(PresentedCommandResult::failure(
            "run",
            &CliFailure::new(
                "unsupported_network_path",
                "network_path requires the eggstack-path feature",
                ExitCode::CapabilityPreflight,
            ),
        ));
    }
    if input.plan.network_path.is_some()
        && driver_selection
            .as_ref()
            .is_some_and(eggbench_drivers::is_external_workload)
    {
        return Ok(PresentedCommandResult::failure(
            "run",
            &CliFailure::new(
                "workload_path_incompatible",
                "external workload drivers cannot own an Eggbench network path",
                ExitCode::CapabilityPreflight,
            ),
        ));
    }
    let options = resolution_options(platform_name()?, &input.plan, driver_selection.as_ref());
    let resolved = eggbench_core::resolve_plan(&input.plan, &descriptors, &options)
        .map_err(CliError::Resolution)?;

    // Defense in depth: even if resolution ever succeeded without a workload
    // adapter, production must still fail before startup.
    let workload_driver = resolved
        .drivers
        .get(&eggbench_core::DriverCategory::Workload);
    let executor_result = match workload_driver {
        Some(driver) => production_workload_executor(&driver.descriptor.name, Some(&resolved)),
        None => Err("no workload driver is registered".to_owned()),
    };
    let mut executor = match executor_result {
        Ok(executor) => executor,
        Err(message) => {
            let category = if resolved.network_path.is_some() {
                if message.contains("missing_fault_seed") {
                    "missing_fault_seed"
                } else if message.contains("credential") {
                    "route_credentials_not_supported"
                } else if message.contains("fault") {
                    "invalid_fault_plan"
                } else {
                    "invalid_route"
                }
            } else {
                "unsupported_workload"
            };
            let failure = CliFailure::new(category, message, ExitCode::CapabilityPreflight);
            return Ok(PresentedCommandResult::failure("run", &failure));
        }
    };

    // External-process preflight: resolve, probe, and version-check the tool
    // before any managed startup. Executors self-probe on first execution as
    // well, so this gate only moves the failure earlier, never wider.
    if let Some(driver) = workload_driver
        && eggbench_drivers::is_external_workload(&driver.descriptor.name)
        && driver.descriptor.name.as_str() != eggbench_drivers::EGGREPLAY_DRIVER_NAME
    {
        let probe_cancel = CancellationToken::new();
        if let Err(error) =
            eggbench_drivers::probe_external_workload(&driver.descriptor.name, &probe_cancel).await
        {
            let failure = CliFailure::new(
                "external_tool",
                error.to_string(),
                ExitCode::CapabilityPreflight,
            );
            return Ok(PresentedCommandResult::failure("run", &failure));
        }
    }

    // M003a semantic-replay preflight: trusted binary resolution, version
    // probe, workspace-confined fixture identity, and `eggreplay validate`
    // machine-contract verification before any managed startup.
    if let eggbench_core::Workload::SemanticReplay { fixture, .. } = &resolved.workload {
        let workspace_root =
            std::env::current_dir().map_err(|error| CliError::Internal(error.to_string()))?;
        let preflight_cancel = CancellationToken::new();
        if let Err(error) =
            eggbench_drivers::preflight_semantic_replay(&workspace_root, fixture, &preflight_cancel)
                .await
        {
            let category = if error.to_string().contains("fixture")
                || error.to_string().contains("validate")
                || error.to_string().contains("envelope")
                || error.to_string().contains("schema")
            {
                "invalid_fixture"
            } else {
                "external_tool"
            };
            let failure =
                CliFailure::new(category, error.to_string(), ExitCode::CapabilityPreflight);
            return Ok(PresentedCommandResult::failure("run", &failure));
        }
    }

    // M003b diagnostic preflight: trusted binary resolution, version
    // probe, and schema-0.3 compatibility handshake before any managed
    // startup when the plan requests diagnostics.
    let mut diagnostic_registry = eggbench_runner::DiagnosticRegistry::new();
    if !resolved.diagnostics.is_empty() {
        let preflight_cancel = CancellationToken::new();
        match eggbench_drivers::preflight_eggprobe(&preflight_cancel).await {
            Ok((executable, probed, _proof)) => {
                diagnostic_registry.register(Box::new(
                    eggbench_drivers::EggProbeExecutor::from_resolved(executable, probed.version),
                ));
            }
            Err(error) => {
                let detail = error.to_string();
                let category = if detail.contains("diagnostic_contract_unsupported") {
                    "diagnostic_contract_unsupported"
                } else {
                    "external_tool"
                };
                let failure = CliFailure::new(category, detail, ExitCode::CapabilityPreflight);
                return Ok(PresentedCommandResult::failure("run", &failure));
            }
        }
    }

    run_impl(
        RunPlan { input, bundle },
        &descriptors,
        &mut *executor,
        production_service_adapters(),
        driver_selection.as_ref(),
        Some(resolved),
        &mut diagnostic_registry,
        wait_for_ctrl_c(),
    )
    .await
}

/// Qualification-only run with an explicitly injected fake workload.
///
/// Not used by `main.rs`. Tests and qualification harnesses supply the fake
/// executor behavior (success, failure, pending-until-cancelled) and the
/// matching fake descriptors. No public CLI flag selects this path.
pub async fn run_with_qualification(
    plan: &Path,
    input_format: Option<InputFormat>,
    bundle: &Path,
    _options: CommandOptions,
    fake: eggbench_runner::test_support::FakeWorkload,
    signal: impl Future<Output = ()> + Send + 'static,
) -> Result<PresentedCommandResult, CliError> {
    let runtime = QualificationRuntime::new();
    let mut descriptors = runtime.driver_descriptors();
    // Plans that declare services (including paired arm services) require a
    // Service-category driver at resolution; the fake claims no capabilities
    // and external-lifecycle services spawn nothing.
    descriptors.push(QualificationRuntime::service_descriptor());
    let mut executor = QualificationRuntime::workload_executor(fake);
    let input = load_plan(plan, input_format)?;
    let mut diagnostic_registry = eggbench_runner::DiagnosticRegistry::new();
    run_impl(
        RunPlan { input, bundle },
        &descriptors,
        &mut executor,
        ServiceAdapterRegistry::new(),
        None,
        None,
        &mut diagnostic_registry,
        signal,
    )
    .await
}

/// Parsed plan input shared by production and qualification run paths.
struct RunPlan<'a> {
    input: PlanInput,
    bundle: &'a Path,
}

#[allow(clippy::too_many_arguments)] // The diagnostic registry joins the established run context.
async fn run_impl(
    input: RunPlan<'_>,
    descriptors: &[DriverDescriptor],
    executor: &mut dyn WorkloadExecutor,
    service_adapters: ServiceAdapterRegistry,
    workload_driver: Option<&Name>,
    pre_resolved: Option<ResolvedPlan>,
    diagnostics: &mut eggbench_runner::DiagnosticRegistry,
    signal: impl Future<Output = ()> + Send + 'static,
) -> Result<PresentedCommandResult, CliError> {
    let plan = input.input.plan;
    let plan_bytes = input.input.bytes;
    let plan_media_type = match input.input.format {
        InputFormat::Toml => "application/toml",
        InputFormat::Json => "application/json",
    };

    let resolved = if let Some(resolved) = pre_resolved {
        resolved
    } else {
        let platform = platform_name()?;
        let options = resolution_options(platform, &plan, workload_driver);
        eggbench_core::resolve_plan(&plan, descriptors, &options).map_err(CliError::Resolution)?
    };

    let resolved_executable_path = match &resolved.subject {
        eggbench_core::Subject::ManagedCommand { .. } => {
            match resolve_managed_executable_path(&resolved) {
                Ok(path) => Some(path),
                Err(failure) => {
                    return Ok(PresentedCommandResult::failure("run", &failure));
                }
            }
        }
        _ => None,
    };

    let (environment, subject_snapshot) =
        match collect_local_environment(&resolved, resolved_executable_path.as_deref()) {
            Ok(pair) => pair,
            Err(error) => {
                return Ok(PresentedCommandResult::failure(
                    "run",
                    &CliFailure::new("prepare", error.to_string(), ExitCode::CapabilityPreflight),
                ));
            }
        };

    if let Some(false) = subject_snapshot.declared_digest_matches() {
        let failure = CliFailure::new(
            "subject_digest_mismatch",
            "declared subject digest does not match observed executable digest",
            ExitCode::CapabilityPreflight,
        );
        return Ok(PresentedCommandResult::failure("run", &failure));
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
        service_adapters,
    };

    let session = match eggbench_runner::LocalSession::prepare(&resolved, runner_options) {
        Ok(session) => session,
        Err(error) => {
            return Ok(PresentedCommandResult::failure(
                "run",
                &failure_from_runner_error(error),
            ));
        }
    };

    let writer = match prepare_bundle(&BundlePreparation {
        destination: input.bundle,
        run_id: eggbench_core::RunId::new(),
        source_plan_bytes: &plan_bytes,
        source_plan_media_type: plan_media_type,
        resolved_plan: &resolved,
        environment: &environment,
        subject_snapshot: &subject_snapshot,
        bounds,
    }) {
        Ok(writer) => writer,
        Err(error) => {
            return Ok(PresentedCommandResult::failure(
                "run",
                &failure_from_bundle_error(error),
            ));
        }
    };

    let resets = ResetRegistry::default();
    // Telemetry collectors construct from the resolved plan before managed
    // startup: required misconfiguration fails closed here, optional gaps
    // warn and normalize as missing downstream.
    let (mut telemetry_registry, telemetry_warnings) =
        match production_telemetry_registry(&resolved) {
            Ok(pair) => pair,
            Err(reason) => {
                return Ok(PresentedCommandResult::failure(
                    "run",
                    &CliFailure::new("telemetry", reason, ExitCode::CapabilityPreflight),
                ));
            }
        };
    let mut session = session;
    let cancel = CancellationToken::new();
    // One OS Ctrl-C signal requests cancellation through the existing M002
    // token; drain/teardown remain authoritative. The listener is aborted and
    // joined after completion so no detached task remains.
    let signal_handle = spawn_signal_forwarder(cancel.clone(), signal);
    let outcome = eggbench_runner::execute_run_with_diagnostics(
        &mut session,
        &resolved,
        executor,
        &resets,
        &mut telemetry_registry,
        diagnostics,
        writer,
        &cancel,
    )
    .await;
    signal_handle.abort();
    let _ = signal_handle.await;

    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(error) => {
            let failure = CliFailure::new("evidence", error.to_string(), ExitCode::EvidenceIo);
            return Ok(PresentedCommandResult::failure("run", &failure));
        }
    };

    let mut presented = presented_run_outcome(&outcome);
    for (category, detail) in telemetry_warnings {
        presented.envelope = presented.envelope.with_warning(category, detail);
    }
    Ok(presented)
}

/// Forward one signal future into the M002 cancellation token.
///
/// The future resolves when Ctrl-C (or a deterministic test trigger) fires.
/// Cleanup/drain semantics stay inside `execute_run`; this only requests
/// cancellation and never terminates the process directly.
pub async fn forward_signal(cancel: CancellationToken, signal: impl Future<Output = ()>) {
    signal.await;
    cancel.cancel();
}

fn spawn_signal_forwarder(
    cancel: CancellationToken,
    signal: impl Future<Output = ()> + Send + 'static,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        forward_signal(cancel, signal).await;
    })
}

/// Real Ctrl-C listener used by future production drivers.
///
/// Currently only qualification harnesses reach `execute_run`, but the seam
/// is provided so a real adapter reuses the same cancellation wiring without
/// bypassing M002 cleanup.
#[allow(dead_code)]
pub async fn wait_for_ctrl_c() {
    let _ = tokio::signal::ctrl_c().await;
}

fn presented_run_outcome(outcome: &eggbench_runner::RunOutcome) -> PresentedCommandResult {
    let primary_failure = outcome
        .primary_failure
        .map(|category| format!("{category:?}"));
    let comparison_verdict = outcome
        .manifest
        .comparison_verdict
        .map(|verdict| format!("{verdict:?}"));

    let bundle_payload = PathBufPayload::from_path(&outcome.bundle_path)
        .unwrap_or_else(|| PathBufPayload::from_string(outcome.bundle_path.display().to_string()));

    let run = CliOutput::Run {
        bundle: bundle_payload,
        bundle_published: true,
        execution_status: format!("{:?}", outcome.execution_status).to_lowercase(),
        measured_trials: outcome.manifest.trials.len(),
        primary_failure,
        comparison_verdict,
    };

    match outcome.execution_status {
        ExecutionStatus::Completed => PresentedCommandResult::success("run", run),
        ExecutionStatus::Failed | ExecutionStatus::Cancelled | ExecutionStatus::Invalid => {
            PresentedCommandResult::run_non_success(
                "run",
                run,
                format!(
                    "run finalized with execution status {:?}",
                    outcome.execution_status
                ),
            )
        }
    }
}

fn resolution_options(
    platform: Name,
    plan: &eggbench_core::ExperimentPlan,
    workload_driver: Option<&Name>,
) -> ResolutionOptions {
    let mut options = ResolutionOptions {
        selections: BTreeMap::default(),
        default_policy: DefaultDriverPolicy::Deterministic,
        platform,
        executable_paths: BTreeMap::default(),
        required_capabilities: BTreeMap::default(),
    };
    if let Some(driver) = workload_driver {
        options
            .selections
            .insert(DriverCategory::Workload, driver.clone());
        // External-process drivers pin their resolved binary into
        // resolution; when the binary is missing the path stays absent and
        // resolution reports MissingExecutablePath before any startup.
        if eggbench_drivers::is_external_workload(driver)
            && let Some(path) = eggbench_drivers::executable_path_for(driver)
        {
            options.executable_paths.insert(driver.clone(), path);
        }
    }
    let mut caps = BTreeMap::new();
    if matches!(
        plan.workload,
        eggbench_core::Workload::SemanticReplay { .. }
    ) {
        caps.insert(
            DriverCategory::Workload,
            std::collections::BTreeSet::from([eggbench_core::Capability::SemanticReplay]),
        );
    } else {
        let workload_mode = workload_load_mode(plan);
        caps.insert(
            DriverCategory::Workload,
            std::collections::BTreeSet::from([eggbench_core::Capability::LoadMode {
                mode: workload_mode,
            }]),
        );
    }
    options.required_capabilities = caps;
    // M003b: pin the resolved eggprobe binary so diagnostic resolution
    // records the exact executable before any managed startup. When the
    // binary is missing the path stays absent and resolution reports
    // MissingExecutablePath before startup.
    if plan
        .diagnostics
        .as_deref()
        .is_some_and(|all| !all.is_empty())
        && let Some(path) = eggbench_drivers::executable_path_for(&eggprobe_name())
    {
        options.executable_paths.insert(eggprobe_name(), path);
    }
    options
}

/// Canonical `eggprobe` diagnostic driver name.
fn eggprobe_name() -> Name {
    Name::new(eggbench_drivers::EGGPROBE_DRIVER_NAME).expect("static driver name")
}

/// Resolve the platform label without a masked `unknown` fallback.
///
/// An invalid platform label is a caller-visible internal defect, not a value
/// to silently coerce.
fn platform_name() -> Result<Name, CliError> {
    let label = UnixPlatform.label().to_owned();
    Name::new(label.clone())
        .map_err(|error| CliError::Internal(format!("invalid platform label {label:?}: {error}")))
}

#[allow(clippy::match_same_arms)]
fn workload_load_mode(plan: &eggbench_core::ExperimentPlan) -> LoadMode {
    use eggbench_core::Workload;
    match &plan.workload {
        // SemanticReplay never reaches here: the caller selects the
        // SemanticReplay capability instead. The fallback keeps the helper
        // total for exhaustive matching.
        Workload::ClosedLoop { .. }
        | Workload::FiniteCount { .. }
        | Workload::SemanticReplay { .. } => LoadMode::ClosedLoop,
        Workload::OpenLoop { .. } => LoadMode::OpenLoop,
        Workload::TimeBounded { mode, .. } => *mode,
    }
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
) -> Result<PathBuf, CliFailure> {
    let argv0 = match &resolved.subject {
        eggbench_core::Subject::ManagedCommand { argv, .. } => argv.first().cloned(),
        _ => None,
    };
    match argv0 {
        Some(value) => Ok(PathBuf::from(value)),
        None => Err(CliFailure::new(
            "managed_executable_path",
            "no managed executable path",
            ExitCode::CapabilityPreflight,
        )),
    }
}

#[allow(dead_code)]
fn builtin_executor_for_tests(
    fake: eggbench_runner::test_support::FakeWorkload,
) -> BuiltinWorkloadExecutor {
    BuiltinWorkloadExecutor::new(fake)
}
