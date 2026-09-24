//! `eggbench doctor <plan>` command.

use crate::envelope::{CliEnvelope, CliOutput, DriverSummary, EnvironmentSummary, ExitCode};
use crate::envelope::{CliFailure, PresentedCommandResult};
use crate::error::CliError;
use crate::plan_input::load_plan;
use crate::{
    CommandOptions, InputFormat,
    workload_registry::{
        ProductionRuntime, WorkloadRegistry, WorkloadRuntime, gregg_endpoint_config_error,
    },
};
use eggbench_core::{
    DefaultDriverPolicy, DriverCategory, EnvironmentFingerprint, LoadMode, Name, ResolutionOptions,
    ResolveError,
};
use eggbench_runner::{LocalEnvironmentCollector, PlatformAdapter, UnixPlatform};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Run validate plus driver/capability/environment preflight checks without
/// starting any managed process.
///
/// Resolves against the full production catalog (all driver categories),
/// so service and telemetry descriptors participate exactly as in `run`.
/// Without compiled adapters the command completes truthfully reporting
/// `has_workload_driver=false`.
pub fn run(
    plan: &Path,
    input_format: Option<InputFormat>,
    workload_driver: Option<&str>,
    _options: CommandOptions,
) -> Result<PresentedCommandResult, CliError> {
    let workload_driver = match parse_workload_driver(workload_driver) {
        Ok(selection) => selection,
        Err(presented) => return Ok(presented),
    };
    let runtime = ProductionRuntime::new();
    run_with_descriptors(
        plan,
        input_format,
        &runtime.driver_descriptors(),
        workload_driver.as_ref(),
    )
}

/// Validate the explicit `--workload-driver` selection.
///
/// A malformed name is a usage failure before any plan I/O; an unknown but
/// well-formed name resolves explicitly and surfaces as `missing_driver`.
fn parse_workload_driver(
    workload_driver: Option<&str>,
) -> Result<Option<Name>, PresentedCommandResult> {
    workload_driver
        .map(|name| {
            Name::new(name).map_err(|_| {
                PresentedCommandResult::failure(
                    "doctor",
                    &CliFailure::new(
                        "usage",
                        format!("invalid --workload-driver name {name:?}"),
                        ExitCode::ParseValidation,
                    ),
                )
            })
        })
        .transpose()
}

/// Test/qualification seam with an explicit driver inventory.
///
/// Production calls [`run`]; qualification tests inject the fake inventory to
/// prove resolution succeeds only with an explicit non-production runtime.
pub fn run_with_registry(
    plan: &Path,
    input_format: Option<InputFormat>,
    inventory: &[crate::workload_registry::DriverInventoryEntry],
) -> Result<PresentedCommandResult, CliError> {
    let descriptors: Vec<_> = inventory
        .iter()
        .map(|entry| entry.descriptor.to_descriptor())
        .collect();
    run_with_descriptors(plan, input_format, &descriptors, None)
}

/// Shared doctor flow over explicit canonical descriptors.
fn run_with_descriptors(
    plan: &Path,
    input_format: Option<InputFormat>,
    descriptors: &[eggbench_core::DriverDescriptor],
    workload_driver: Option<&Name>,
) -> Result<PresentedCommandResult, CliError> {
    let input = load_plan(plan, input_format)?;
    let plan = input.plan;
    let platform_label = platform_label();
    let platform_supported = platform_support();

    let platform = Name::new(platform_label.clone()).map_err(|error| {
        CliError::Internal(format!(
            "invalid platform label {platform_label:?}: {error}"
        ))
    })?;
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
        if eggbench_drivers::is_external_workload(driver)
            && let Some(path) = eggbench_drivers::executable_path_for(driver)
        {
            options.executable_paths.insert(driver.clone(), path);
        }
    }

    let workload_mode = workload_load_mode(&plan);
    let mut required_capabilities = std::collections::BTreeMap::new();
    required_capabilities.insert(
        DriverCategory::Workload,
        BTreeSet::from([eggbench_core::Capability::LoadMode {
            mode: workload_mode,
        }]),
    );
    options.required_capabilities = required_capabilities;

    let resolved = eggbench_core::resolve_plan(&plan, descriptors, &options);

    let environment = match LocalEnvironmentCollector.collect() {
        Ok(env) => Some(env),
        Err(error) => {
            let failure = CliFailure::new(
                "environment",
                format!("environment collection failed: {error}"),
                ExitCode::Internal,
            );
            return Ok(PresentedCommandResult::failure("doctor", &failure));
        }
    };

    let drivers: Vec<DriverSummary> = {
        let mut summaries: Vec<DriverSummary> = descriptors
            .iter()
            .map(|descriptor| {
                let mut capabilities: Vec<String> = descriptor
                    .capabilities
                    .iter()
                    .map(|capability| format!("{capability:?}"))
                    .collect();
                capabilities.sort();
                DriverSummary {
                    name: descriptor.name.as_str().to_owned(),
                    category: format!("{:?}", descriptor.category),
                    default: descriptor.default,
                    external_process: descriptor.external_process,
                    adapter_version: descriptor.adapter_version.clone(),
                    upstream_name: descriptor.upstream_name.clone(),
                    upstream_version: descriptor.upstream_version.clone(),
                    capabilities,
                    binary_present: eggbench_drivers::external_binary_present(&descriptor.name),
                }
            })
            .collect();
        summaries.sort_by(|left, right| left.name.cmp(&right.name));
        summaries
    };

    let has_workload_driver = descriptors
        .iter()
        .any(|descriptor| descriptor.category == DriverCategory::Workload);
    let env_fields = environment.as_ref().map_or(Vec::new(), env_field_summaries);

    // Config-syntax validation for declared Gregg endpoints (no network:
    // live probing stays in `run` preflight).
    if resolved.is_ok()
        && let Some(reason) = gregg_endpoint_config_error(&plan)
    {
        let failure = CliFailure::new("telemetry_config", reason, ExitCode::CapabilityPreflight);
        let mut envelope = CliEnvelope::ok(
            "doctor",
            CliOutput::Doctor {
                resolved: false,
                platform_supported,
                drivers: drivers.clone(),
                has_workload_driver,
                environment_fields: env_fields.clone(),
            },
        );
        envelope.ok = false;
        envelope.error = Some(failure.to_payload());
        return Ok(PresentedCommandResult {
            envelope,
            exit_code: failure.exit_code,
        });
    }

    match resolved {
        Ok(_) => Ok(PresentedCommandResult::success(
            "doctor",
            CliOutput::Doctor {
                resolved: true,
                platform_supported,
                drivers,
                has_workload_driver,
                environment_fields: env_fields,
            },
        )),
        Err(error) => Ok(envelope_for_resolution_error(
            &error,
            platform_supported,
            drivers,
            has_workload_driver,
            env_fields,
        )),
    }
}

fn envelope_for_resolution_error(
    error: &ResolveError,
    platform_supported: bool,
    drivers: Vec<DriverSummary>,
    has_workload_driver: bool,
    environment_fields: Vec<EnvironmentSummary>,
) -> PresentedCommandResult {
    let failure = cli_failure_from_resolve(error);
    // Retain the doctor payload (including has_workload_driver) alongside the
    // error so the failure stays truthful instead of discarding diagnostics.
    let mut envelope = CliEnvelope::ok(
        "doctor",
        CliOutput::Doctor {
            resolved: false,
            platform_supported,
            drivers,
            has_workload_driver,
            environment_fields,
        },
    );
    envelope.ok = false;
    envelope.error = Some(failure.to_payload());
    PresentedCommandResult {
        envelope,
        exit_code: failure.exit_code,
    }
}

fn cli_failure_from_resolve(error: &ResolveError) -> CliFailure {
    use eggbench_core::ResolveError;
    let category = match error {
        ResolveError::InvalidPlan(_) => "plan_validation",
        ResolveError::MissingDriver { .. } => "missing_driver",
        ResolveError::CategoryMismatch { .. } => "category_mismatch",
        ResolveError::AmbiguousSelection { .. } => "ambiguous_selection",
        ResolveError::UnsupportedCapability { .. } => "unsupported_capability",
        ResolveError::UnsupportedPlatform { .. } => "unsupported_platform",
        ResolveError::MissingExecutablePath(_) => "missing_executable_path",
        ResolveError::IncompatibleService { .. } => "incompatible_service",
        ResolveError::DuplicateDriver(_) => "duplicate_driver",
    };
    let exit_code = match error {
        ResolveError::InvalidPlan(_) => ExitCode::ParseValidation,
        _ => ExitCode::CapabilityPreflight,
    };
    CliFailure::new(category, error.to_string(), exit_code)
}

fn workload_load_mode(plan: &eggbench_core::ExperimentPlan) -> LoadMode {
    use eggbench_core::Workload;
    match &plan.workload {
        Workload::ClosedLoop { .. } | Workload::FiniteCount { .. } => LoadMode::ClosedLoop,
        Workload::OpenLoop { .. } => LoadMode::OpenLoop,
        Workload::TimeBounded { mode, .. } => *mode,
    }
}

fn platform_label() -> String {
    UnixPlatform.label().to_owned()
}

fn platform_support() -> bool {
    matches!(
        UnixPlatform.support(),
        eggbench_runner::PlatformSupport::Supported
    )
}

fn env_field_summaries(env: &EnvironmentFingerprint) -> Vec<EnvironmentSummary> {
    env.fields
        .iter()
        .map(|(name, field)| EnvironmentSummary {
            name: name.as_str().to_owned(),
            value: field.value.clone(),
            class: format!("{:?}", field.class),
        })
        .collect()
}

#[allow(dead_code)]
fn production_registry_for_docs() -> WorkloadRegistry {
    WorkloadRegistry::production()
}
