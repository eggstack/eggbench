//! Driver capability contracts and deterministic plan resolution.
use crate::{
    EnvironmentPolicy, ExperimentPlan, LoadMode, NETWORK_PATH_RNG_VERSION,
    NETWORK_PATH_SEMANTICS_VERSION, Name, PAIRED_SCHEDULE_V1, PairedArm, PlanError, RouteRequest,
    SchemaVersion, ServiceKind, StreamFaultPlanRequest, Subject, Workload,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

/// Current resolved-plan schema version (v5 retains static service bindings).
pub const RESOLVED_PLAN_SCHEMA_VERSION: SchemaVersion = SchemaVersion(5);
/// Previous resolved-plan schema version, still accepted on read.
pub const RESOLVED_PLAN_SCHEMA_VERSION_4: SchemaVersion = SchemaVersion(4);
/// Previous resolved-plan schema version, still accepted on read.
pub const RESOLVED_PLAN_SCHEMA_VERSION_3: SchemaVersion = SchemaVersion(3);
/// Older resolved-plan schema version, still accepted on read.
pub const RESOLVED_PLAN_SCHEMA_VERSION_2: SchemaVersion = SchemaVersion(2);
/// Oldest resolved-plan schema version, still accepted on read.
pub const RESOLVED_PLAN_SCHEMA_VERSION_1: SchemaVersion = SchemaVersion(1);

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
    /// Security-correctness adapter (Eggstack M004a). Correctness checks
    /// determine whether security behavior meets a predeclared expectation;
    /// they are distinct from workload (statistical trial load) and from
    /// diagnostics (environment/target health).
    Correctness,
    /// Local or future remote execution provider descriptor.
    ExecutionProvider,
    /// Listener-free Eggress TCP route driver (Eggstack M002).
    Route,
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
    /// Driver owns a custom dialer for the HTTP transport (Eggstack M002).
    NetworkPath,
    /// Driver produces a static, deterministic stream-fault plan
    /// (Eggstack M002).
    StreamFaultPlan,
    /// Driver executes one complete immutable `EggReplay` fixture as one
    /// `Eggbench` trial (Eggstack M003a). No `ClosedLoop`/`OpenLoop` claim is
    /// required for this capability.
    SemanticReplay,
    /// Driver executes one diagnostic probe family (Eggstack M003b).
    DiagnosticProbe {
        /// Supported probe family.
        probe: crate::DiagnosticProbe,
    },
    /// Driver executes one Eggsec WAF bypass correctness check
    /// (Eggstack M004a). The family label is `waf_bypass` initially.
    SecurityCheck {
        /// Supported correctness family.
        family: Name,
    },
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
    /// Resolved paired design; present only for paired plans.
    #[serde(default)]
    pub paired: Option<ResolvedPairedDesign>,
    /// Resolved network path for schema-v3 plans; absent when not requested.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::plan::deserialize_present_optional"
    )]
    pub network_path: Option<ResolvedNetworkPath>,
    /// Resolved diagnostics for schema-v5 plans; empty when not requested.
    #[serde(default)]
    pub diagnostics: Vec<crate::DiagnosticRequest>,
    /// Resolved security checks for schema-v6 plans; empty when not requested.
    #[serde(default)]
    pub security_checks: Vec<crate::SecurityCheckRequest>,
    /// Non-fatal resolution diagnostics.
    pub warnings: Vec<ResolutionWarning>,
}

/// Resolved network path: the selected route/fault drivers and the
/// credential-free request they lower from. Re-emitted in evidence as a
/// first-class descriptor, not as a managed service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedNetworkPath {
    /// Original schema-v3 route request (credential-free).
    pub route: RouteRequest,
    /// Selected route descriptor (e.g. `eggress-route`).
    pub route_driver: ResolvedDriver,
    /// Stable route/fault ordering semantics identity.
    pub semantics_version: String,
    /// Original schema-v3 stream-fault request, when present.
    #[serde(default)]
    pub stream_faults: Option<ResolvedStreamFaults>,
}

/// Resolved stream-fault composition.
///
/// Carries the fault driver identity plus the redaction-safe original
/// request so per-invocation evidence can roll it forward without
/// reconstructing the plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedStreamFaults {
    /// Original schema-v3 stream-fault request.
    pub request: StreamFaultPlanRequest,
    /// Selected fault descriptor (e.g. `eggchaos-stream`).
    pub fault_driver: ResolvedDriver,
    /// Stable Eggchaos RNG identity.
    pub rng_version: String,
}

/// Resolved paired baseline/candidate design.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedPairedDesign {
    /// Resolved control arm.
    pub baseline: ResolvedPairedArm,
    /// Resolved candidate arm.
    pub candidate: ResolvedPairedArm,
    /// Schedule identifier (`alternating-baseline-first` in v1).
    pub schedule: String,
    /// Number of pairs (half the measured trial count).
    pub pairs: u32,
}

/// One resolved paired arm.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedPairedArm {
    /// Service receiving load on this arm's trials.
    pub service: Name,
    /// Declared subject identity for this arm's provenance.
    pub subject: Subject,
}

impl ResolvedPairedArm {
    /// Resolve one predeclared arm (arms carry no driver selection of their
    /// own; compatibility is proven against the shared workload driver).
    fn resolve(arm: &PairedArm) -> Self {
        Self {
            service: arm.service.clone(),
            subject: arm.subject.clone(),
        }
    }
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
///
/// # Panics
/// Never panics at runtime; the static correctness family/source names are
/// valid by construction.
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
    match &plan.workload {
        Workload::SemanticReplay { .. } => {
            required
                .entry(DriverCategory::Workload)
                .or_default()
                .insert(Capability::SemanticReplay);
        }
        _ => {
            required
                .entry(DriverCategory::Workload)
                .or_default()
                .insert(Capability::LoadMode {
                    mode: workload_mode(&plan.workload),
                });
        }
    }
    if plan.network_path.is_some() {
        // Network-path requests require a workload driver that owns a
        // custom dialer so route/fault plumbing lives in the executor.
        required
            .entry(DriverCategory::Workload)
            .or_default()
            .insert(Capability::NetworkPath);
        required
            .entry(DriverCategory::Route)
            .or_default()
            .insert(Capability::ProxyRouting);
        if plan
            .network_path
            .as_ref()
            .and_then(|path| path.stream_faults.as_ref())
            .is_some()
        {
            required
                .entry(DriverCategory::Fault)
                .or_default()
                .insert(Capability::StreamFaultPlan);
        }
        // External targets do not own a workload-side dialer; network-path
        // assertions must live with a driver that owns transport.
        if matches!(plan.subject, Subject::External { .. }) {
            return Err(ResolveError::InvalidPlan(PlanError::Validation {
                category: "workload_path_incompatible",
                detail:
                    "network_path is not compatible with a Subject::External experiment in M002"
                        .to_owned(),
            }));
        }
    }
    if !plan.services.is_empty() || matches!(plan.subject, Subject::ManagedCommand { .. }) {
        required.entry(DriverCategory::Service).or_default();
    }
    if let Some(all) = plan.diagnostics.as_deref()
        && !all.is_empty()
    {
        // Each requested probe family must be advertised by the selected
        // Diagnostic driver; the M003b `eggprobe` descriptor claims
        // DNS/TCP/TLS/HTTP only.
        let mut families = BTreeSet::new();
        for request in all {
            for probe in &request.probes {
                families.insert(Capability::DiagnosticProbe { probe: *probe });
            }
        }
        required
            .entry(DriverCategory::Diagnostic)
            .or_default()
            .extend(families);
    }
    if let Some(all) = plan.security_checks.as_deref()
        && !all.is_empty()
    {
        // M004a: every security check is an `eggsec-waf` bypass observation
        // in the initial `waf_bypass` family. Resolution pins the single
        // correctness driver; per-check family filtering happens in the
        // adapter contract validation below.
        required
            .entry(DriverCategory::Correctness)
            .or_default()
            .insert(Capability::SecurityCheck {
                family: crate::Name::new(crate::SECURITY_FAMILY_WAF_BYPASS)
                    .expect("static correctness family"),
            });
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
        let path = descriptor
            .external_process
            .then(|| options.executable_paths.get(&descriptor.name).cloned())
            .flatten();
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

    if let Some(path) = &plan.network_path {
        validate_network_path_driver_contract(path, &drivers)?;
    }

    if plan
        .security_checks
        .as_deref()
        .is_some_and(|all| !all.is_empty())
    {
        validate_correctness_driver_contract(plan, &drivers)?;
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
            let path = descriptor
                .external_process
                .then(|| options.executable_paths.get(&descriptor.name).cloned())
                .flatten();
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

    if let Some(workload_driver) = drivers.get(&DriverCategory::Workload) {
        // The plan target is checked always; paired arm services are checked
        // when a paired design is present so a driver that can only drive one
        // variant fails closed at resolution, never mid-run.
        let mut services_to_check = vec![workload_target(&plan.workload)];
        if let Some(design) = &plan.paired {
            services_to_check.push(&design.baseline.service);
            services_to_check.push(&design.candidate.service);
        }
        for service_name in services_to_check {
            if let Some(target_service) = plan.services.iter().find(|s| &s.name == service_name) {
                let service_type = match &target_service.kind {
                    ServiceKind::Command { .. } => "command".to_owned(),
                    ServiceKind::Named { service_type } => service_type.to_string(),
                };
                let compatible = &workload_driver.descriptor.compatible_service_types;
                if !compatible.is_empty()
                    && !compatible.iter().any(|name| name.as_str() == service_type)
                {
                    return Err(ResolveError::IncompatibleService {
                        driver: workload_driver.descriptor.name.clone(),
                        service_type,
                    });
                }
            }
        }
    }

    let paired = plan.paired.as_ref().map(|design| ResolvedPairedDesign {
        baseline: ResolvedPairedArm::resolve(&design.baseline),
        candidate: ResolvedPairedArm::resolve(&design.candidate),
        schedule: PAIRED_SCHEDULE_V1.to_owned(),
        pairs: plan.trials.measured.get() / 2,
    });

    let network_path = if let Some(network_path) = &plan.network_path {
        let route_descriptor =
            drivers
                .get(&DriverCategory::Route)
                .ok_or(ResolveError::MissingDriver {
                    category: DriverCategory::Route,
                })?;
        if route_descriptor.descriptor.name != network_path.route.driver {
            return Err(ResolveError::MissingDriver {
                category: DriverCategory::Route,
            });
        }
        let stream_faults = network_path.stream_faults.as_ref().map(|request| {
            let fault_descriptor =
                drivers
                    .get(&DriverCategory::Fault)
                    .ok_or(ResolveError::MissingDriver {
                        category: DriverCategory::Fault,
                    })?;
            if fault_descriptor.descriptor.name != request.driver {
                return Err(ResolveError::MissingDriver {
                    category: DriverCategory::Fault,
                });
            }
            Ok::<_, ResolveError>(ResolvedStreamFaults {
                request: request.clone(),
                fault_driver: fault_descriptor.clone(),
                rng_version: NETWORK_PATH_RNG_VERSION.to_owned(),
            })
        });
        Some(ResolvedNetworkPath {
            route: network_path.route.clone(),
            route_driver: route_descriptor.clone(),
            semantics_version: NETWORK_PATH_SEMANTICS_VERSION.to_owned(),
            stream_faults: stream_faults.transpose()?,
        })
    } else {
        None
    };

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
        paired,
        network_path,
        diagnostics: plan.diagnostics.clone().unwrap_or_default(),
        security_checks: plan.security_checks.clone().unwrap_or_default(),
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
        | Workload::TimeBounded { target, .. }
        | Workload::SemanticReplay { target, .. } => target,
    }
}

/// Validate the canonical external Eggsec WAF correctness contract.
///
/// The selected `Correctness` driver must be the external `eggsec-waf`
/// adapter with the `waf_bypass` family capability and a pinned executable
/// path. Upstream tool provenance (observed version plus executable digest)
/// is recorded per run in `security-checks.json`, not in the descriptor.
///
/// # Panics
/// Never panics at runtime; the static correctness names are valid by
/// construction.
///
/// # Errors
/// Returns [`ResolveError`] when no correctness driver is selected or the
/// selected driver violates the contract.
fn validate_correctness_driver_contract(
    plan: &ExperimentPlan,
    drivers: &BTreeMap<DriverCategory, ResolvedDriver>,
) -> Result<(), ResolveError> {
    let invalid = |driver: &Name, detail: String| {
        Err(ResolveError::InvalidPlan(PlanError::Validation {
            category: "unsupported_correctness_driver",
            detail: format!("correctness driver {driver} {detail}"),
        }))
    };
    let correctness = drivers.get(&DriverCategory::Correctness);
    let Some(correctness) = correctness else {
        // Static fallback below.
        let source = plan
            .security_checks
            .as_deref()
            .and_then(|all| all.first())
            .map_or_else(
                || {
                    crate::Name::new(crate::SECURITY_SOURCE_EGGSEC_WAF)
                        .expect("static correctness source")
                },
                |request| request.source.clone(),
            );
        return invalid(&source, "has no selected correctness driver".to_owned());
    };
    let descriptor = &correctness.descriptor;
    let expected_family =
        crate::Name::new(crate::SECURITY_FAMILY_WAF_BYPASS).expect("static correctness family");
    if descriptor.name.as_str() != crate::EGGSEC_WAF_DRIVER_NAME
        || descriptor.adapter_version.is_empty()
        || descriptor.adapter_version.len() > 128
        || descriptor.upstream_name != crate::EGGSEC_UPSTREAM_NAME
        || !descriptor
            .capabilities
            .contains(&Capability::SecurityCheck {
                family: expected_family,
            })
        || !descriptor.external_process
        || correctness.executable_path.is_none()
    {
        return invalid(
            &descriptor.name,
            "does not satisfy the canonical external Eggsec WAF correctness contract".to_owned(),
        );
    }
    if descriptor.category != DriverCategory::Correctness {
        return Err(ResolveError::CategoryMismatch {
            driver: descriptor.name.clone(),
            actual: descriptor.category,
            expected: DriverCategory::Correctness,
        });
    }
    Ok(())
}

fn validate_network_path_driver_contract(
    path: &crate::NetworkPathRequest,
    drivers: &BTreeMap<DriverCategory, ResolvedDriver>,
) -> Result<(), ResolveError> {
    let invalid = |driver: &Name, detail: String| {
        Err(ResolveError::InvalidPlan(PlanError::Validation {
            category: "unsupported_network_path",
            detail: format!("network-path driver {driver} {detail}"),
        }))
    };
    let workload = drivers.get(&DriverCategory::Workload);
    let Some(workload) = workload else {
        return invalid(&path.route.driver, "has no transport workload".to_owned());
    };
    let workload_descriptor = &workload.descriptor;
    if workload_descriptor.name.as_str() != "eggfetch-http"
        || workload_descriptor.adapter_version.is_empty()
        || workload_descriptor.adapter_version.len() > 128
        || workload_descriptor.upstream_name != "eggfetch-core"
        || workload_descriptor
            .upstream_version
            .as_deref()
            .is_none_or(str::is_empty)
        || !workload_descriptor
            .capabilities
            .contains(&Capability::NetworkPath)
        || workload_descriptor.external_process
        || workload.executable_path.is_some()
    {
        return invalid(
            &workload_descriptor.name,
            "must be the native Eggfetch NetworkPath workload".to_owned(),
        );
    }

    let route = drivers.get(&DriverCategory::Route);
    let Some(route) = route else {
        return invalid(
            &path.route.driver,
            "has no selected route driver".to_owned(),
        );
    };
    let route_descriptor = &route.descriptor;
    if path.route.driver != route_descriptor.name {
        return Err(ResolveError::MissingDriver {
            category: DriverCategory::Route,
        });
    }
    if route_descriptor.name.as_str() != "eggress-route"
        || route_descriptor.adapter_version.is_empty()
        || route_descriptor.adapter_version.len() > 128
        || route_descriptor.upstream_name != "eggress-outbound"
        || route_descriptor
            .upstream_version
            .as_deref()
            .is_none_or(str::is_empty)
        || !route_descriptor
            .capabilities
            .contains(&Capability::ProxyRouting)
        || route_descriptor.external_process
        || route.executable_path.is_some()
    {
        return invalid(
            &route_descriptor.name,
            "does not satisfy the canonical native Eggress route contract".to_owned(),
        );
    }

    if let Some(faults) = &path.stream_faults {
        let fault = drivers.get(&DriverCategory::Fault);
        let Some(fault) = fault else {
            return invalid(&faults.driver, "has no selected fault driver".to_owned());
        };
        let fault_descriptor = &fault.descriptor;
        if faults.driver != fault_descriptor.name {
            return Err(ResolveError::MissingDriver {
                category: DriverCategory::Fault,
            });
        }
        if fault_descriptor.name.as_str() != "eggchaos-stream"
            || fault_descriptor.adapter_version.is_empty()
            || fault_descriptor.adapter_version.len() > 128
            || fault_descriptor.upstream_name != "eggchaos-core"
            || fault_descriptor
                .upstream_version
                .as_deref()
                .is_none_or(str::is_empty)
            || !fault_descriptor
                .capabilities
                .contains(&Capability::StreamFaultPlan)
            || fault_descriptor.external_process
            || fault.executable_path.is_some()
        {
            return invalid(
                &fault_descriptor.name,
                "does not satisfy the canonical native Eggchaos stream contract".to_owned(),
            );
        }
    }
    Ok(())
}

#[allow(clippy::match_same_arms)]
fn workload_mode(workload: &Workload) -> LoadMode {
    match workload {
        Workload::OpenLoop { .. } => LoadMode::OpenLoop,
        Workload::TimeBounded { mode, .. } => *mode,
        // SemanticReplay has no load mode; resolution requires the
        // SemanticReplay capability instead. This fallback is unreachable
        // through `resolve_plan` but keeps the helper total for callers
        // that only handle load-model workloads.
        Workload::ClosedLoop { .. }
        | Workload::FiniteCount { .. }
        | Workload::SemanticReplay { .. } => LoadMode::ClosedLoop,
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
            RESOLVED_PLAN_SCHEMA_VERSION
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

    fn paired_plan() -> ExperimentPlan {
        use crate::{PositiveCount, Service, ServiceKind};
        let mut plan = self::plan();
        plan.schema_version = crate::EXPERIMENT_PLAN_SCHEMA_VERSION_2;
        plan.subject = Subject::Label {
            label: name("a-vs-b"),
        };
        let service = |service_name: &str, service_type: &str| Service {
            name: name(service_name),
            kind: ServiceKind::Named {
                service_type: name(service_type),
            },
            lifecycle: crate::Lifecycle::External,
            depends_on: Vec::new(),
            config: std::collections::BTreeMap::new(),
            http_url: None,
            readiness: None,
            shutdown: None,
            working_directory: None,
            log_limit_bytes: 4096,
        };
        plan.services = vec![service("origin-a", "http"), service("origin-b", "http")];
        plan.workload = Workload::FiniteCount {
            target: name("origin-a"),
            requests: PositiveCount::new(100).unwrap(),
            concurrency: PositiveCount::new(5).unwrap(),
        };
        plan.trials.measured = PositiveCount::new(4).unwrap();
        plan.paired = Some(crate::PairedDesign {
            baseline: PairedArm {
                service: name("origin-a"),
                subject: Subject::Label {
                    label: name("variant-a"),
                },
            },
            candidate: PairedArm {
                service: name("origin-b"),
                subject: Subject::Label {
                    label: name("variant-b"),
                },
            },
        });
        plan
    }

    #[test]
    fn paired_resolution_records_schedule_pairs_and_source_version() {
        let mut workload = driver("fake-load", DriverCategory::Workload);
        workload.default = true;
        let mut service = driver("fake-service", DriverCategory::Service);
        service.default = true;
        let resolved = resolve_plan(&paired_plan(), &[workload, service], &options()).unwrap();
        assert_eq!(resolved.schema_version, RESOLVED_PLAN_SCHEMA_VERSION);
        assert_eq!(
            resolved.source_plan_schema_version,
            crate::EXPERIMENT_PLAN_SCHEMA_VERSION_2
        );
        let design = resolved.paired.expect("resolved paired design");
        assert_eq!(design.schedule, crate::PAIRED_SCHEDULE_V1);
        assert_eq!(design.pairs, 2);
        assert_eq!(design.baseline.service.as_str(), "origin-a");
        assert_eq!(design.candidate.service.as_str(), "origin-b");
    }

    #[test]
    fn paired_resolution_fails_closed_when_driver_cannot_drive_one_arm() {
        let mut workload = driver("http-only-load", DriverCategory::Workload);
        workload.default = true;
        workload.compatible_service_types.insert(name("http"));
        let mut service = driver("fake-service", DriverCategory::Service);
        service.default = true;
        // Both arms are http services: compatible.
        resolve_plan(
            &paired_plan(),
            &[workload.clone(), service.clone()],
            &options(),
        )
        .unwrap();
        // Retype the candidate arm: resolution must fail before any trial.
        let mut plan = paired_plan();
        plan.services[1] = crate::Service {
            name: name("origin-b"),
            kind: crate::ServiceKind::Named {
                service_type: name("other"),
            },
            lifecycle: crate::Lifecycle::External,
            depends_on: Vec::new(),
            config: std::collections::BTreeMap::new(),
            http_url: None,
            readiness: None,
            shutdown: None,
            working_directory: None,
            log_limit_bytes: 4096,
        };
        assert!(matches!(
            resolve_plan(&plan, &[workload, service], &options()),
            Err(ResolveError::IncompatibleService { .. })
        ));
    }

    #[test]
    fn semantic_replay_requires_semantic_capability_and_pins_binary() {
        use crate::{EXPERIMENT_PLAN_SCHEMA_VERSION_4, Workload};
        let mut plan =
            super::super::ExperimentPlan::from_json(include_str!("../tests/fixtures/minimal.json"))
                .unwrap();
        plan.schema_version = EXPERIMENT_PLAN_SCHEMA_VERSION_4;
        plan.services = vec![crate::Service {
            name: name("origin"),
            kind: crate::ServiceKind::Named {
                service_type: name("eggserve-origin"),
            },
            lifecycle: crate::Lifecycle::External,
            depends_on: Vec::new(),
            config: std::collections::BTreeMap::new(),
            http_url: None,
            readiness: None,
            shutdown: None,
            working_directory: None,
            log_limit_bytes: 4096,
        }];
        plan.workload = Workload::SemanticReplay {
            target: name("origin"),
            fixture: "fixtures/replay".to_owned(),
        };
        // A ClosedLoop-only driver cannot satisfy SemanticReplay.
        let mut closed = driver("closed-load", DriverCategory::Workload);
        closed.default = true;
        let mut service = driver("fake-service", DriverCategory::Service);
        service.default = true;
        assert!(matches!(
            resolve_plan(&plan, &[closed.clone(), service.clone()], &options()),
            Err(ResolveError::UnsupportedCapability { .. })
        ));
        // A SemanticReplay external driver resolves with an explicit path.
        let mut replay = driver("eggreplay-semantic", DriverCategory::Workload);
        replay.default = true;
        replay.external_process = true;
        replay.capabilities.insert(Capability::SemanticReplay);
        replay.capabilities.insert(Capability::ExternalBinary);
        assert!(matches!(
            resolve_plan(&plan, &[replay.clone(), service.clone()], &options()),
            Err(ResolveError::MissingExecutablePath(_))
        ));
        let mut opts = options();
        opts.executable_paths
            .insert(name("eggreplay-semantic"), "/opt/eggreplay".into());
        let resolved = resolve_plan(&plan, &[replay, service], &opts).unwrap();
        assert_eq!(
            resolved.drivers[&DriverCategory::Workload]
                .executable_path
                .as_deref(),
            Some("/opt/eggreplay")
        );
    }
}
