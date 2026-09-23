//! `eggbench doctor <plan>` command.

use crate::envelope::CliFailure;
use crate::envelope::{CliEnvelope, CliOutput, DriverSummary, EnvironmentSummary, ExitCode};
use crate::error::CliError;
use crate::plan_input::load_plan;
use crate::{CommandOptions, InputFormat, workload_registry::WorkloadRegistry};
use eggbench_core::{
    DefaultDriverPolicy, DriverCategory, EnvironmentFingerprint, LoadMode, Name, ResolutionOptions,
};
use eggbench_runner::{LocalEnvironmentCollector, PlatformAdapter, UnixPlatform};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Run validate plus driver/capability/environment preflight checks without
/// starting any managed process.
///
/// # Errors
/// Returns [`CliError`] when input reading or plan validation fails.
pub fn run(
    plan: &Path,
    input_format: Option<InputFormat>,
    _options: CommandOptions,
) -> Result<CliEnvelope, CliError> {
    let input = load_plan(plan, input_format)?;
    let plan = input.plan;
    let platform_label = platform_label();
    let platform_supported = platform_support();

    let registry = WorkloadRegistry::with_builtin();
    let driver_inventory = registry.inventory();

    let descriptors: Vec<_> = driver_inventory
        .iter()
        .map(|entry| entry.descriptor.to_descriptor())
        .collect();

    let mut options = ResolutionOptions {
        selections: BTreeMap::default(),
        default_policy: DefaultDriverPolicy::Deterministic,
        platform: Name::new(platform_label).unwrap_or_else(|_| Name::new("unknown").unwrap()),
        executable_paths: BTreeMap::default(),
        required_capabilities: BTreeMap::default(),
    };

    let workload_mode = workload_load_mode(&plan);
    let mut required_capabilities = std::collections::BTreeMap::new();
    required_capabilities.insert(
        DriverCategory::Workload,
        BTreeSet::from([eggbench_core::Capability::LoadMode {
            mode: workload_mode,
        }]),
    );
    options.required_capabilities = required_capabilities;

    let resolved = eggbench_core::resolve_plan(&plan, &descriptors, &options);

    let environment = match LocalEnvironmentCollector.collect() {
        Ok(env) => Some(env),
        Err(error) => {
            return Ok(failure_envelope(format!(
                "environment collection failed: {error}"
            )));
        }
    };

    let drivers: Vec<DriverSummary> = driver_inventory
        .iter()
        .map(|entry| DriverSummary {
            name: entry.descriptor.name.clone(),
            category: format!("{:?}", entry.descriptor.to_descriptor().category),
            default: entry.descriptor.default,
            external_process: entry.descriptor.to_descriptor().external_process,
        })
        .collect();

    let env_fields = environment.as_ref().map_or(Vec::new(), env_field_summaries);

    let envelope = match resolved {
        Ok(_) => CliEnvelope::ok(
            "doctor",
            CliOutput::Doctor {
                resolved: true,
                platform_supported,
                drivers,
                has_workload_driver: registry.has_workload_driver(),
                environment_fields: env_fields,
            },
        ),
        Err(error) => envelope_for_resolution_error(
            &error,
            platform_supported,
            drivers,
            registry.has_workload_driver(),
            env_fields,
        ),
    };
    Ok(envelope)
}

fn envelope_for_resolution_error(
    error: &eggbench_core::ResolveError,
    platform_supported: bool,
    drivers: Vec<DriverSummary>,
    has_workload_driver: bool,
    environment_fields: Vec<EnvironmentSummary>,
) -> CliEnvelope {
    let failure = cli_failure_from_resolve(error);
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
    envelope.result = None;
    let _ = ExitCode::CapabilityPreflight;
    envelope
}

fn cli_failure_from_resolve(error: &eggbench_core::ResolveError) -> CliFailure {
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
    CliFailure::new(category, error.to_string(), ExitCode::CapabilityPreflight)
}

fn failure_envelope(detail: String) -> CliEnvelope {
    let failure = CliFailure::new("internal", detail, ExitCode::Internal);
    let mut envelope = CliEnvelope::fail("doctor", &failure);
    envelope.error = Some(failure.to_payload());
    envelope
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
