//! Driver capability contracts and deterministic plan resolution.
use crate::{
    EnvironmentPolicy, ExperimentPlan, LoadMode, Name, PlanError, SchemaVersion, ServiceKind,
    Subject, Workload,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

/// Current resolved-plan schema version.
pub const RESOLVED_PLAN_SCHEMA_VERSION: SchemaVersion = SchemaVersion(1);

/// Independent adapter category; categories do not share a universal driver trait.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriverCategory {
    /// Subject/service lifecycle adapter.
    Service,
    /// Load generation adapter.
    Workload,
    /// Host or service telemetry adapter.
    Telemetry,
    /// Fault injection adapter.
    Fault,
    /// Diagnostic collection adapter.
    Diagnostic,
    /// Local or future remote execution provider descriptor.
    ExecutionProvider,
}

/// HTTP protocol version capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HttpVersion {
    /// HTTP/1.0.
    Http10,
    /// HTTP/1.1.
    Http11,
    /// HTTP/2.
    Http2,
    /// HTTP/3.
    Http3,
}

/// Structured capability vocabulary for initial drivers.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Capability {
    /// Supports an HTTP version.
    HttpVersion {
        /// Protocol version.
        version: HttpVersion,
    },
    /// Supports a load model.
    LoadMode {
        /// Supported mode.
        mode: LoadMode,
    },
    /// Corrects coordinated omission in latency measurements.
    CorrectedLatency,
    /// Supports explicit proxy or route selection.
    ProxyRouting,
    /// Supports a named fault family.
    FaultFamily {
        /// Fault family identifier.
        family: Name,
    },
    /// Emits a named telemetry field.
    TelemetryField {
        /// Field identifier.
        field: Name,
    },
    /// Requires/uses an external binary.
    ExternalBinary,
}

/// Stable identity and advertised capabilities of one adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriverDescriptor {
    /// Canonical Eggbench driver name.
    pub name: Name,
    /// Version of the Eggbench adapter contract implementation.
    pub adapter_version: String,
    /// Upstream tool or library name.
    pub upstream_name: String,
    /// Upstream version or revision, if known.
    pub upstream_version: Option<String>,
    /// One stable category.
    pub category: DriverCategory,
    /// Advertised semantics.
    pub capabilities: BTreeSet<Capability>,
    /// Empty means portable; otherwise explicit supported platform labels.
    #[serde(default)]
    pub supported_platforms: BTreeSet<Name>,
    /// Optional machine-output schema version.
    pub machine_output_schema: Option<SchemaVersion>,
    /// Whether the adapter is backed by an external process.
    pub external_process: bool,
    /// Whether this is the deterministic default candidate for its category.
    #[serde(default)]
    pub default: bool,
    /// Service type names that a workload driver can target; empty means no restriction.
    #[serde(default)]
    pub compatible_service_types: BTreeSet<Name>,
}

/// Explicit resolution policy. Default selection uses a unique marked default, or a sole candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DefaultDriverPolicy {
    /// Do not infer choices; every needed category must be selected explicitly.
    ExplicitOnly,
    /// Select one marked default or the only candidate in a category.
    Deterministic,
}

/// Inputs available before any side effect; paths are supplied by the caller, never discovered here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolutionOptions {
    /// Explicit canonical driver name per category.
    #[serde(default)]
    pub selections: BTreeMap<DriverCategory, Name>,
    /// Default selection behavior.
    pub default_policy: DefaultDriverPolicy,
    /// Current platform label, such as `linux-x86_64`.
    pub platform: Name,
    /// Caller-provided paths to known external executables, keyed by driver name.
    #[serde(default)]
    pub executable_paths: BTreeMap<Name, String>,
    /// Additional explicit capability requirements per category.
    #[serde(default)]
    pub required_capabilities: BTreeMap<DriverCategory, BTreeSet<Capability>>,
}

/// Selected driver plus the exact version/capabilities frozen into evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedDriver {
    /// Adapter descriptor snapshot.
    pub descriptor: DriverDescriptor,
    /// Non-secret executable path, if supplied for an external adapter.
    pub executable_path: Option<String>,
}

/// Warning that does not prevent pre-execution resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResolutionWarning {
    /// Optional telemetry was unavailable and omitted.
    OptionalTelemetryOmitted {
        /// Requested source.
        source: Name,
        /// Missing field names.
        missing_fields: Vec<Name>,
    },
}

/// Serializable, redaction-safe output of resolving one symbolic plan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedPlan {
    /// Resolved contract schema version.
    pub schema_version: SchemaVersion,
    /// Source plan schema version.
    pub source_plan_schema_version: SchemaVersion,
    /// Source experiment identity.
    pub experiment: Name,
    /// Exact driver choices by independent category.
    pub drivers: BTreeMap<DriverCategory, ResolvedDriver>,
    /// Subject identity/lifecycle request retained for reproducibility.
    pub subject: Subject,
    /// Normalized topology and workload intent.
    pub topology: Vec<crate::Service>,
    /// Resolved load request.
    pub workload: Workload,
    /// Trial, warmup, reset, and timeout request.
    pub trials: crate::TrialPolicy,
    /// All telemetry requests, including explicitly optional omissions.
    pub telemetry: Vec<crate::TelemetryRequest>,
    /// Resolved non-secret defaults and policy.
    pub defaults: ResolvedDefaults,
    /// Environment/testbed policy request.
    pub environment_policy: EnvironmentPolicy,
    /// Metric/comparison request, without computed verdicts.
    pub metrics: Vec<crate::MetricRequest>,
    /// Artifact registration bounds requested by the plan.
    pub artifact_bounds: crate::ArtifactBounds,
    /// Deterministic seed.
    pub seed: Option<u64>,
    /// Non-fatal resolution diagnostics.
    pub warnings: Vec<ResolutionWarning>,
}

/// Defaults frozen when the plan was resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedDefaults {
    /// Platform label used for compatibility checks.
    pub platform: Name,
    /// Number of warmups.
    pub warmup_trials: u32,
    /// Number of measured trials.
    pub measured_trials: u32,
}

/// Fail-closed resolution errors.
#[derive(Debug, Error)]
pub enum ResolveError {
    /// The symbolic plan failed its own validation.
    #[error(transparent)]
    InvalidPlan(#[from] PlanError),
    /// No suitable driver was available.
    #[error("missing required {category:?} driver")]
    MissingDriver {
        /// Required category.
        category: DriverCategory,
    },
    /// Explicit selection named a driver from another category.
    #[error("driver {driver} has category {actual:?}, expected {expected:?}")]
    CategoryMismatch {
        /// Driver name.
        driver: Name,
        /// Actual category.
        actual: DriverCategory,
        /// Required category.
        expected: DriverCategory,
    },
    /// Multiple candidates prevented deterministic defaulting.
    #[error("ambiguous {category:?} driver selection: {candidates:?}")]
    AmbiguousSelection {
        /// Required category.
        category: DriverCategory,
        /// Candidate names, in stable order.
        candidates: Vec<Name>,
    },
    /// A required capability is absent.
    #[error("driver {driver} lacks required capability {capability:?}")]
    UnsupportedCapability {
        /// Selected driver.
        driver: Name,
        /// Required capability.
        capability: Capability,
    },
    /// Driver cannot run on the declared platform.
    #[error("driver {driver} does not support platform {platform}")]
    UnsupportedPlatform {
        /// Driver name.
        driver: Name,
        /// Platform label.
        platform: Name,
    },
    /// An external-process driver has no caller-supplied executable path.
    #[error("external driver {0} has no resolved executable path")]
    MissingExecutablePath(Name),
    /// Workload driver is not compatible with target service type.
    #[error("workload driver {driver} is incompatible with service type {service_type}")]
    IncompatibleService {
        /// Workload driver.
        driver: Name,
        /// Service type.
        service_type: String,
    },
    /// A registry contains duplicate driver identities.
    #[error("duplicate driver identity {0}")]
    DuplicateDriver(Name),
}

/// Resolve a validated plan against an in-memory descriptor list, without performing I/O.
///
/// # Errors
/// Returns [`ResolveError`] on invalid input, selection ambiguity, platform incompatibility,
/// or any missing requested semantic capability.
#[allow(clippy::too_many_lines)] // Resolution is intentionally one side-effect-free validation pass.
pub fn resolve_plan(
    plan: &ExperimentPlan,
    descriptors: &[DriverDescriptor],
    options: &ResolutionOptions,
) -> Result<ResolvedPlan, ResolveError> {
    plan.validate()?;
    let mut registry = BTreeMap::<Name, &DriverDescriptor>::new();
    for descriptor in descriptors {
        if registry
            .insert(descriptor.name.clone(), descriptor)
            .is_some()
        {
            return Err(ResolveError::DuplicateDriver(descriptor.name.clone()));
        }
    }

    let mut required = BTreeMap::<DriverCategory, BTreeSet<Capability>>::new();
    required
        .entry(DriverCategory::Workload)
        .or_default()
        .insert(Capability::LoadMode {
            mode: workload_mode(&plan.workload),
        });
    if !plan.services.is_empty() || matches!(plan.subject, Subject::ManagedCommand { .. }) {
        required.entry(DriverCategory::Service).or_default();
    }
    for (category, capabilities) in &options.required_capabilities {
        required
            .entry(*category)
            .or_default()
            .extend(capabilities.iter().cloned());
    }

    let mut warnings = Vec::new();
    let mut drivers = BTreeMap::new();
    for (category, capabilities) in &required {
        let descriptor = select_driver(*category, &registry, options)?;
        validate_driver(descriptor, capabilities, options)?;
        let path = options.executable_paths.get(&descriptor.name).cloned();
        if descriptor.external_process && path.as_deref().is_none_or(str::is_empty) {
            return Err(ResolveError::MissingExecutablePath(descriptor.name.clone()));
        }
        drivers.insert(
            *category,
            ResolvedDriver {
                descriptor: descriptor.clone(),
                executable_path: path,
            },
        );
    }

    if !plan.telemetry.is_empty() {
        let candidates: Vec<_> = registry
            .values()
            .copied()
            .filter(|d| d.category == DriverCategory::Telemetry)
            .collect();
        if candidates.is_empty() {
            if plan.telemetry.iter().any(|request| request.required) {
                return Err(ResolveError::MissingDriver {
                    category: DriverCategory::Telemetry,
                });
            }
            for telemetry in &plan.telemetry {
                warnings.push(ResolutionWarning::OptionalTelemetryOmitted {
                    source: telemetry.source.clone(),
                    missing_fields: telemetry.fields.clone(),
                });
            }
        } else {
            let descriptor = select_driver(DriverCategory::Telemetry, &registry, options)?;
            validate_driver(descriptor, &BTreeSet::new(), options)?;
            let path = options.executable_paths.get(&descriptor.name).cloned();
            if descriptor.external_process && path.as_deref().is_none_or(str::is_empty) {
                return Err(ResolveError::MissingExecutablePath(descriptor.name.clone()));
            }
            for telemetry in &plan.telemetry {
                let missing: Vec<_> = telemetry
                    .fields
                    .iter()
                    .filter(|field| {
                        !descriptor
                            .capabilities
                            .contains(&Capability::TelemetryField {
                                field: (*field).clone(),
                            })
                    })
                    .cloned()
                    .collect();
                if !missing.is_empty() && telemetry.required {
                    return Err(ResolveError::UnsupportedCapability {
                        driver: descriptor.name.clone(),
                        capability: Capability::TelemetryField {
                            field: missing[0].clone(),
                        },
                    });
                }
                if !missing.is_empty() {
                    warnings.push(ResolutionWarning::OptionalTelemetryOmitted {
                        source: telemetry.source.clone(),
                        missing_fields: missing,
                    });
                }
            }
            drivers.insert(
                DriverCategory::Telemetry,
                ResolvedDriver {
                    descriptor: descriptor.clone(),
                    executable_path: path.clone(),
                },
            );
        }
    }

    if let Some(target_service) = plan
        .services
        .iter()
        .find(|s| &s.name == workload_target(&plan.workload))
        && let Some(workload_driver) = drivers.get(&DriverCategory::Workload)
    {
        let service_type = match &target_service.kind {
            ServiceKind::Command { .. } => "command".to_owned(),
            ServiceKind::Named { service_type } => service_type.to_string(),
        };
        let compatible = &workload_driver.descriptor.compatible_service_types;
        if !compatible.is_empty() && !compatible.iter().any(|name| name.as_str() == service_type) {
            return Err(ResolveError::IncompatibleService {
                driver: workload_driver.descriptor.name.clone(),
                service_type,
            });
        }
    }

    Ok(ResolvedPlan {
        schema_version: RESOLVED_PLAN_SCHEMA_VERSION,
        source_plan_schema_version: plan.schema_version,
        experiment: plan.experiment.clone(),
        drivers,
        subject: plan.subject.clone(),
        topology: plan.services.clone(),
        workload: plan.workload.clone(),
        trials: plan.trials.clone(),
        telemetry: plan.telemetry.clone(),
        defaults: ResolvedDefaults {
            platform: options.platform.clone(),
            warmup_trials: plan.trials.warmup,
            measured_trials: plan.trials.measured.get(),
        },
        environment_policy: plan.environment_policy,
        metrics: plan.metrics.clone(),
        artifact_bounds: plan.bounds,
        seed: plan.seed,
        warnings,
    })
}

fn select_driver<'a>(
    category: DriverCategory,
    registry: &BTreeMap<Name, &'a DriverDescriptor>,
    options: &ResolutionOptions,
) -> Result<&'a DriverDescriptor, ResolveError> {
    if let Some(name) = options.selections.get(&category) {
        let descriptor = registry
            .get(name)
            .copied()
            .ok_or(ResolveError::MissingDriver { category })?;
        if descriptor.category != category {
            return Err(ResolveError::CategoryMismatch {
                driver: name.clone(),
                actual: descriptor.category,
                expected: category,
            });
        }
        return Ok(descriptor);
    }
    let candidates: Vec<_> = registry
        .values()
        .copied()
        .filter(|d| d.category == category)
        .collect();
    if options.default_policy == DefaultDriverPolicy::ExplicitOnly {
        return Err(ResolveError::MissingDriver { category });
    }
    let defaults: Vec<_> = candidates.iter().copied().filter(|d| d.default).collect();
    match defaults.as_slice() {
        [only] => Ok(only),
        [] => match candidates.as_slice() {
            [only] => Ok(only),
            [] => Err(ResolveError::MissingDriver { category }),
            many => Err(ResolveError::AmbiguousSelection {
                category,
                candidates: many.iter().map(|d| d.name.clone()).collect(),
            }),
        },
        many => Err(ResolveError::AmbiguousSelection {
            category,
            candidates: many.iter().map(|d| d.name.clone()).collect(),
        }),
    }
}

fn validate_driver(
    descriptor: &DriverDescriptor,
    required: &BTreeSet<Capability>,
    options: &ResolutionOptions,
) -> Result<(), ResolveError> {
    if !descriptor.supported_platforms.is_empty()
        && !descriptor.supported_platforms.contains(&options.platform)
    {
        return Err(ResolveError::UnsupportedPlatform {
            driver: descriptor.name.clone(),
            platform: options.platform.clone(),
        });
    }
    for capability in required {
        if !descriptor.capabilities.contains(capability) {
            return Err(ResolveError::UnsupportedCapability {
                driver: descriptor.name.clone(),
                capability: capability.clone(),
            });
        }
    }
    if descriptor.external_process
        && !descriptor
            .capabilities
            .contains(&Capability::ExternalBinary)
    {
        return Err(ResolveError::UnsupportedCapability {
            driver: descriptor.name.clone(),
            capability: Capability::ExternalBinary,
        });
    }
    Ok(())
}

fn workload_target(workload: &Workload) -> &Name {
    match workload {
        Workload::ClosedLoop { target, .. }
        | Workload::OpenLoop { target, .. }
        | Workload::FiniteCount { target, .. }
        | Workload::TimeBounded { target, .. } => target,
    }
}

fn workload_mode(workload: &Workload) -> LoadMode {
    match workload {
        Workload::OpenLoop { .. } => LoadMode::OpenLoop,
        Workload::TimeBounded { mode, .. } => *mode,
        Workload::ClosedLoop { .. } | Workload::FiniteCount { .. } => LoadMode::ClosedLoop,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ExperimentPlan, Name};

    fn name(value: &str) -> Name {
        Name::new(value).unwrap()
    }
    fn plan() -> ExperimentPlan {
        ExperimentPlan::from_json(include_str!("../tests/fixtures/minimal.json")).unwrap()
    }
    fn options() -> ResolutionOptions {
        ResolutionOptions {
            selections: BTreeMap::new(),
            default_policy: DefaultDriverPolicy::Deterministic,
            platform: name("linux-x86_64"),
            executable_paths: BTreeMap::new(),
            required_capabilities: BTreeMap::new(),
        }
    }
    fn driver(name_text: &str, category: DriverCategory) -> DriverDescriptor {
        let mut capabilities = BTreeSet::new();
        if category == DriverCategory::Workload {
            capabilities.insert(Capability::LoadMode {
                mode: LoadMode::ClosedLoop,
            });
        }
        DriverDescriptor {
            name: name(name_text),
            adapter_version: "1.2.3".into(),
            upstream_name: format!("{name_text}-upstream"),
            upstream_version: Some("9.8".into()),
            category,
            capabilities,
            supported_platforms: BTreeSet::new(),
            machine_output_schema: Some(SchemaVersion(1)),
            external_process: false,
            default: false,
            compatible_service_types: BTreeSet::new(),
        }
    }

    #[test]
    fn exact_selection_and_deterministic_default_keep_provenance() {
        let mut workload = driver("fake-load", DriverCategory::Workload);
        workload.default = true;
        let resolved = resolve_plan(&plan(), &[workload.clone()], &options()).unwrap();
        assert_eq!(
            resolved.drivers[&DriverCategory::Workload].descriptor,
            workload
        );
        let json = serde_json::to_string(&resolved).unwrap();
        let round: ResolvedPlan = serde_json::from_str(&json).unwrap();
        assert_eq!(resolved, round);
        assert!(json.contains("9.8"));

        let mut selected = options();
        selected
            .selections
            .insert(DriverCategory::Workload, name("fake-load"));
        assert_eq!(
            resolve_plan(&plan(), &[workload], &selected)
                .unwrap()
                .schema_version,
            SchemaVersion(1)
        );
    }

    #[test]
    fn capability_matrix_and_resolved_snapshot_fixtures_are_current() {
        let descriptors: Vec<DriverDescriptor> =
            serde_json::from_str(include_str!("../tests/fixtures/driver-capabilities.json"))
                .unwrap();
        let mut opts = options();
        opts.required_capabilities
            .entry(DriverCategory::Workload)
            .or_default()
            .insert(Capability::LoadMode {
                mode: LoadMode::ClosedLoop,
            });
        let resolved = resolve_plan(&plan(), &descriptors, &opts).unwrap();
        let actual = serde_json::to_value(resolved).unwrap();
        let expected: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/sample-resolved-plan.json"))
                .unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn resolution_fails_closed_on_missing_ambiguous_wrong_category_capability_and_platform() {
        assert!(matches!(
            resolve_plan(&plan(), &[], &options()),
            Err(ResolveError::MissingDriver {
                category: DriverCategory::Workload
            })
        ));
        let a = driver("a", DriverCategory::Workload);
        let b = driver("b", DriverCategory::Workload);
        assert!(matches!(
            resolve_plan(&plan(), &[a.clone(), b], &options()),
            Err(ResolveError::AmbiguousSelection { .. })
        ));
        let mut wrong = options();
        wrong
            .selections
            .insert(DriverCategory::Workload, name("wrong"));
        assert!(matches!(
            resolve_plan(&plan(), &[driver("wrong", DriverCategory::Service)], &wrong),
            Err(ResolveError::CategoryMismatch { .. })
        ));
        let mut needs = options();
        needs
            .required_capabilities
            .entry(DriverCategory::Workload)
            .or_default()
            .insert(Capability::HttpVersion {
                version: HttpVersion::Http2,
            });
        assert!(matches!(
            resolve_plan(&plan(), std::slice::from_ref(&a), &needs),
            Err(ResolveError::UnsupportedCapability { .. })
        ));
        let mut platform = a;
        platform.default = true;
        platform.supported_platforms.insert(name("windows-x86_64"));
        assert!(matches!(
            resolve_plan(&plan(), &[platform], &options()),
            Err(ResolveError::UnsupportedPlatform { .. })
        ));
    }

    #[test]
    fn optional_telemetry_warns_and_required_telemetry_fails() {
        let mut p = plan();
        p.telemetry.push(crate::TelemetryRequest {
            source: name("host"),
            fields: vec![name("cpu"), name("memory")],
            required: false,
        });
        let mut workload = driver("load", DriverCategory::Workload);
        workload.default = true;
        let resolved = resolve_plan(&p, &[workload.clone()], &options()).unwrap();
        assert_eq!(resolved.warnings.len(), 1);
        p.telemetry[0].required = true;
        assert!(matches!(
            resolve_plan(&p, &[workload], &options()),
            Err(ResolveError::MissingDriver {
                category: DriverCategory::Telemetry
            })
        ));
    }

    #[test]
    fn required_telemetry_capability_fails_before_resolution() {
        let mut p = plan();
        p.telemetry.push(crate::TelemetryRequest {
            source: name("host"),
            fields: vec![name("cpu")],
            required: true,
        });
        let mut workload = driver("load", DriverCategory::Workload);
        workload.default = true;
        let mut telemetry = driver("host", DriverCategory::Telemetry);
        telemetry.default = true;
        assert!(matches!(
            resolve_plan(&p, &[workload, telemetry], &options()),
            Err(ResolveError::UnsupportedCapability { .. })
        ));
    }

    #[test]
    fn compatible_service_relation_is_checked() {
        let p = ExperimentPlan::from_json(include_str!(
            "../tests/fixtures/multi-service-open-loop.json"
        ))
        .unwrap();
        let mut service = driver("service", DriverCategory::Service);
        service.default = true;
        let mut workload = driver("load", DriverCategory::Workload);
        workload.default = true;
        workload.capabilities.insert(Capability::LoadMode {
            mode: LoadMode::OpenLoop,
        });
        workload.compatible_service_types.insert(name("wrong-type"));
        assert!(matches!(
            resolve_plan(&p, &[service, workload], &options()),
            Err(ResolveError::IncompatibleService { .. })
        ));
    }

    #[test]
    fn external_driver_requires_explicit_binary_capability_and_path() {
        let mut workload = driver("external-load", DriverCategory::Workload);
        workload.default = true;
        workload.external_process = true;
        workload.capabilities.insert(Capability::ExternalBinary);
        assert!(matches!(
            resolve_plan(&plan(), &[workload.clone()], &options()),
            Err(ResolveError::MissingExecutablePath(_))
        ));
        let mut opts = options();
        opts.executable_paths
            .insert(name("external-load"), "/opt/tools/loadgen".into());
        let resolved = resolve_plan(&plan(), &[workload], &opts).unwrap();
        assert_eq!(
            resolved.drivers[&DriverCategory::Workload]
                .executable_path
                .as_deref(),
            Some("/opt/tools/loadgen")
        );
    }
}
