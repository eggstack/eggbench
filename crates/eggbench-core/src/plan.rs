use crate::{
    BasisPoints, DurationMs, EXPERIMENT_PLAN_SCHEMA_VERSION, EXPERIMENT_PLAN_SCHEMA_VERSION_2,
    EXPERIMENT_PLAN_SCHEMA_VERSION_3, EXPERIMENT_PLAN_SCHEMA_VERSION_4, Name, NetworkPathRequest,
    PositiveCount, RateMilliRps, RouteMode, SchemaVersion, SecretRef,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

/// Complete declarative experiment request. Unknown fields are rejected for schema v1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperimentPlan {
    /// Explicit input schema version.
    pub schema_version: SchemaVersion,
    /// Stable experiment identity.
    pub experiment: Name,
    /// Subject being evaluated.
    pub subject: Subject,
    /// Service topology.
    #[serde(default)]
    pub services: Vec<Service>,
    /// Workload intent.
    pub workload: Workload,
    /// Trial and warmup policy.
    pub trials: TrialPolicy,
    /// Requested telemetry.
    #[serde(default)]
    pub telemetry: Vec<TelemetryRequest>,
    /// Metric and gate requests.
    #[serde(default)]
    pub metrics: Vec<MetricRequest>,
    /// Comparison/testbed policy.
    pub environment_policy: EnvironmentPolicy,
    /// Optional deterministic random seed.
    pub seed: Option<u64>,
    /// Optional paired baseline/candidate design (schema v2 only).
    #[serde(default)]
    pub paired: Option<PairedDesign>,
    /// Optional first-class listener-free network path (schema v3 only).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present_optional"
    )]
    pub network_path: Option<NetworkPathRequest>,
    /// Upper bounds for later evidence creation.
    pub bounds: ArtifactBounds,
}

/// Subject declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Subject {
    /// Process managed by a future runner.
    ManagedCommand {
        /// Program and arguments.
        argv: Vec<String>,
        /// Non-secret environment references.
        environment: BTreeMap<String, SecretRef>,
        /// Optional source revision hint.
        revision: Option<String>,
        /// Optional source digest hint.
        digest: Option<String>,
    },
    /// Target managed outside Eggbench.
    External {
        /// Named target used by workloads.
        target: Name,
        /// Optional source revision hint.
        revision: Option<String>,
        /// Optional digest hint.
        digest: Option<String>,
    },
    /// Opaque label with no implied lifecycle.
    Label {
        /// Subject label.
        label: Name,
    },
}

/// Service request within a topology.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Service {
    /// Stable service name.
    pub name: Name,
    /// Declarative service kind.
    pub kind: ServiceKind,
    /// Lifecycle intent.
    pub lifecycle: Lifecycle,
    /// Dependencies by service name.
    #[serde(default)]
    pub depends_on: Vec<Name>,
    /// Opaque non-secret config values.
    #[serde(default)]
    pub config: BTreeMap<String, String>,
    /// Optional readiness request.
    pub readiness: Option<Readiness>,
    /// Optional shutdown request.
    pub shutdown: Option<Shutdown>,
    /// Optional working directory intent.
    pub working_directory: Option<String>,
    /// Maximum bytes retained for each log.
    pub log_limit_bytes: u64,
}

/// Declarative service kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ServiceKind {
    /// Command-like process.
    Command {
        /// Program and arguments.
        argv: Vec<String>,
    },
    /// Named service type interpreted by a future adapter.
    Named {
        /// Stable type label.
        service_type: Name,
    },
}
/// Managed/external lifecycle intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lifecycle {
    /// Runner starts and stops this service.
    Managed,
    /// Service exists outside the experiment runner.
    External,
}
/// Readiness request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Readiness {
    /// Delay then consider ready.
    Delay {
        /// Delay duration.
        after_ms: DurationMs,
    },
    /// Future adapter-defined named readiness probe.
    Probe {
        /// Probe identifier.
        probe: Name,
        /// Timeout.
        timeout_ms: DurationMs,
    },
}
/// Shutdown request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shutdown {
    /// Graceful drain allowance.
    pub grace_ms: DurationMs,
    /// Optional signal/protocol label.
    pub method: Option<Name>,
}

/// Load pattern.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Workload {
    /// Closed-loop concurrent clients.
    ClosedLoop {
        /// Destination service or external target.
        target: Name,
        /// Concurrent workers.
        concurrency: PositiveCount,
        /// Completion count, mutually exclusive with duration.
        requests: Option<PositiveCount>,
        /// Time bound, mutually exclusive with requests.
        duration_ms: Option<DurationMs>,
    },
    /// Open-loop offered request rate.
    OpenLoop {
        /// Destination.
        target: Name,
        /// Offered rate in milli-rps.
        rate_milli_rps: RateMilliRps,
        /// Completion count, mutually exclusive with duration.
        requests: Option<PositiveCount>,
        /// Time bound.
        duration_ms: Option<DurationMs>,
    },
    /// Finite completion workload.
    FiniteCount {
        /// Destination.
        target: Name,
        /// Number of operations.
        requests: PositiveCount,
        /// Concurrency.
        concurrency: PositiveCount,
    },
    /// Time-bounded workload.
    TimeBounded {
        /// Destination.
        target: Name,
        /// Duration.
        duration_ms: DurationMs,
        /// Closed/open loop behavior.
        mode: LoadMode,
        /// Concurrency for closed loop, offered rate for open loop.
        concurrency: Option<PositiveCount>,
        /// Open-loop offered rate.
        rate_milli_rps: Option<RateMilliRps>,
    },
    /// Semantic replay workload (schema v4): one complete immutable
    /// `EggReplay` fixture replay equals one Eggbench trial observation.
    SemanticReplay {
        /// Destination service or external target publishing an HTTP binding.
        target: Name,
        /// Relative workspace path to the immutable `.eggr` fixture directory.
        fixture: String,
    },
}
/// Explicit load model discriminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoadMode {
    /// Fixed concurrency.
    ClosedLoop,
    /// Fixed offered rate.
    OpenLoop,
}

/// Trial policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrialPolicy {
    /// Number of measured independent trials.
    pub measured: PositiveCount,
    /// Number of warmups.
    pub warmup: u32,
    /// Delay between measured trials.
    pub cooldown_ms: Option<DurationMs>,
    /// Reset policy between trials.
    pub reset: ResetPolicy,
    /// Phase timeouts.
    #[serde(default)]
    pub timeouts: BTreeMap<Name, DurationMs>,
}
/// Reset between trials.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResetPolicy {
    /// No reset requested.
    None,
    /// Reinitialize a named service.
    Service {
        /// Service to reset.
        service: Name,
    },
    /// Reset through a named external reference.
    Reference {
        /// Reset adapter reference.
        reference: Name,
    },
}

/// Requested telemetry field set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TelemetryRequest {
    /// Source/driver category label.
    pub source: Name,
    /// Requested metric fields.
    pub fields: Vec<Name>,
    /// Whether absence invalidates resolution later.
    pub required: bool,
}
/// Metric semantics used by later comparison.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricRequest {
    /// Stable metric name.
    pub name: Name,
    /// Unit label.
    pub unit: Name,
    /// Direction for interpretation.
    pub direction: MetricDirection,
    /// Primary gate eligibility.
    pub intent: MetricIntent,
    /// Optional budget.
    pub gate: Option<Gate>,
}
/// Metric direction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MetricDirection {
    /// Higher is better.
    HigherIsBetter,
    /// Lower is better.
    LowerIsBetter,
    /// Values should fall within inclusive bounds.
    TargetRange {
        /// Inclusive minimum.
        min: f64,
        /// Inclusive maximum.
        max: f64,
    },
    /// No direction.
    Informational,
}
/// Metric role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricIntent {
    /// Metric may gate acceptance.
    Primary,
    /// Diagnostic only.
    Diagnostic,
}
/// Absolute or relative budget request; no statistical calculation is performed here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Gate {
    /// Maximum/minimum absolute metric value.
    Absolute {
        /// Budget value in metric's declared unit.
        value: f64,
    },
    /// Relative regression budget.
    RelativeRegression {
        /// Allowed regression as basis points.
        allowance: BasisPoints,
    },
    /// Later trial-level statistical comparison request.
    StatisticalRelative {
        /// Allowed regression.
        allowance: BasisPoints,
        /// Minimum comparison trials.
        min_trials: PositiveCount,
    },
}

/// Paired baseline/candidate experiment design (plan schema v2).
///
/// Both arms are services that stay live for the whole run; the runner
/// alternates which service receives load. Arm subjects are provenance
/// declarations only: the runner never launches or digests them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PairedDesign {
    /// Control arm; also the nominal plan workload target (see validation).
    pub baseline: PairedArm,
    /// Candidate arm under qualification.
    pub candidate: PairedArm,
}

/// One arm of a paired design.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PairedArm {
    /// Declared service that receives load on this arm's trials.
    pub service: Name,
    /// Declared subject identity for this arm's provenance.
    pub subject: Subject,
}

/// Testbed comparison policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EnvironmentPolicy {
    /// Require same testbed.
    StrictSameTestbed,
    /// Permit mismatch with warning/descriptive output.
    WarnOnMismatch,
    /// Explicitly descriptive cross-testbed mode.
    CrossTestbedDescriptive,
}
/// Artifact registration bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactBounds {
    /// Maximum retained artifact count.
    pub artifact_count: PositiveCount,
    /// Maximum bytes per artifact.
    pub artifact_bytes: u64,
    /// Maximum total bytes.
    pub total_bytes: u64,
}

/// Stable parse, schema, and validation failure categories.
#[derive(Debug, Error)]
pub enum PlanError {
    /// Input syntax/schema decoding failed.
    #[error("{format} parse/schema error: {message}")]
    Parse {
        /// Format label.
        format: &'static str,
        /// Human-readable parser context.
        message: String,
    },
    /// Unsupported schema version.
    #[error("unsupported experiment schema version {0}")]
    UnsupportedVersion(u32),
    /// Validation error category and contextual detail.
    #[error("{category}: {detail}")]
    Validation {
        /// Stable machine-readable category.
        category: &'static str,
        /// Human-readable context.
        detail: String,
    },
}

pub(crate) fn deserialize_present_optional<'de, D, T>(
    deserializer: D,
) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

fn raw_faults_invalid(value: Option<&serde_json::Value>) -> bool {
    let Some(value) = value else {
        return false;
    };
    let Some(faults) = value.as_object() else {
        return true;
    };
    if faults
        .get("driver")
        .and_then(serde_json::Value::as_str)
        .is_none_or(str::is_empty)
    {
        return true;
    }
    ["upstream", "downstream"].iter().any(|direction| {
        let Some(requests) = faults.get(*direction).and_then(serde_json::Value::as_array) else {
            return true;
        };
        requests.iter().any(raw_fault_request_invalid)
    })
}

fn raw_fault_request_invalid(value: &serde_json::Value) -> bool {
    let Some(request) = value.as_object() else {
        return true;
    };
    if request
        .get("id")
        .and_then(serde_json::Value::as_str)
        .is_none_or(str::is_empty)
    {
        return true;
    }
    let Some(kind) = request.get("kind").and_then(serde_json::Value::as_object) else {
        return true;
    };
    let positive = |key: &str| {
        kind.get(key)
            .and_then(serde_json::Value::as_u64)
            .is_some_and(|value| (1..=1_000_000).contains(&value))
    };
    let duration = |key: &str| {
        kind.get(key)
            .and_then(serde_json::Value::as_u64)
            .is_some_and(|value| (1..=31_536_000_000).contains(&value))
    };
    match kind.get("kind").and_then(serde_json::Value::as_str) {
        Some("latency") => {
            !(duration("delay_ms") && duration("jitter_ms") && positive("max_buffer_bytes"))
        }
        Some("bandwidth") => !(positive("bytes_per_second") && positive("burst_bytes")),
        Some("blackhole") => kind
            .get("close_after_ms")
            .is_some_and(|value| !value.is_null() && duration_value_invalid(value)),
        Some("limit_data") => !positive("bytes"),
        Some("slow_close") => !duration("delay_ms"),
        Some("slice") => {
            let average = kind
                .get("average_size")
                .and_then(serde_json::Value::as_u64)
                .filter(|value| (1..=1_000_000).contains(value));
            let variation = kind.get("variation").and_then(serde_json::Value::as_u64);
            !(average.is_some()
                && duration("delay_ms")
                && variation.is_some_and(|value| value < average.unwrap_or(0)))
        }
        Some("disconnect") => !duration("after_ms"),
        _ => true,
    }
}

fn duration_value_invalid(value: &serde_json::Value) -> bool {
    value
        .as_u64()
        .is_none_or(|value| !(1..=31_536_000_000).contains(&value))
}

fn raw_route_invalid(value: Option<&serde_json::Value>) -> bool {
    let Some(route) = value.and_then(serde_json::Value::as_object) else {
        return true;
    };
    let driver_invalid = route
        .get("driver")
        .and_then(serde_json::Value::as_str)
        .is_none_or(str::is_empty);
    let mode_invalid = route
        .get("mode")
        .and_then(serde_json::Value::as_object)
        .is_none_or(|mode| {
            !matches!(
                mode.get("kind").and_then(serde_json::Value::as_str),
                Some("direct" | "proxy_chain")
            ) || (mode.get("kind").and_then(serde_json::Value::as_str) == Some("proxy_chain")
                && mode
                    .get("chain")
                    .and_then(serde_json::Value::as_str)
                    .is_none_or(str::is_empty))
        });
    driver_invalid || mode_invalid
}

fn map_deserialize_error(
    format: &'static str,
    input: &str,
    error: impl std::fmt::Display,
) -> PlanError {
    let message = error.to_string();
    if message.contains("network_path.stream_faults")
        || serde_json::from_str::<serde_json::Value>(input)
            .ok()
            .as_ref()
            .and_then(|value| value.get("network_path"))
            .is_some_and(|path| raw_faults_invalid(path.get("stream_faults")))
    {
        return PlanError::Validation {
            category: "invalid_fault_plan",
            detail: message,
        };
    }
    if message.contains("network_path")
        || serde_json::from_str::<serde_json::Value>(input)
            .ok()
            .as_ref()
            .and_then(|value| value.get("network_path"))
            .is_some_and(|path| raw_route_invalid(path.get("route")))
    {
        return PlanError::Validation {
            category: "invalid_route",
            detail: message,
        };
    }
    PlanError::Parse { format, message }
}

impl ExperimentPlan {
    /// Parse JSON, enforce schema version, and validate semantics.
    ///
    /// # Errors
    /// Returns a parse, unsupported-version, or semantic validation error.
    pub fn from_json(input: &str) -> Result<Self, PlanError> {
        let plan: Self = serde_json::from_str(input)
            .map_err(|error| map_deserialize_error("JSON", input, error))?;
        plan.validate()?;
        Ok(plan)
    }
    /// Parse TOML, enforce schema version, and validate semantics.
    ///
    /// # Errors
    /// Returns a parse, unsupported-version, or semantic validation error.
    pub fn from_toml(input: &str) -> Result<Self, PlanError> {
        let plan: Self =
            toml::from_str(input).map_err(|error| map_deserialize_error("TOML", input, error))?;
        plan.validate()?;
        Ok(plan)
    }
    /// Canonical deterministic JSON (struct fields and ordered maps only).
    ///
    /// # Errors
    /// Returns an unsupported-version or semantic validation error.
    pub fn to_json(&self) -> Result<String, PlanError> {
        self.validate()?;
        serde_json::to_string_pretty(self).map_err(|e| PlanError::Parse {
            format: "JSON",
            message: e.to_string(),
        })
    }
    /// Human-edited TOML representation.
    ///
    /// # Errors
    /// Returns an unsupported-version or semantic validation error.
    pub fn to_toml(&self) -> Result<String, PlanError> {
        self.validate()?;
        toml::to_string_pretty(self).map_err(|e| PlanError::Parse {
            format: "TOML",
            message: e.to_string(),
        })
    }
    /// Validate cross-field semantics and references.
    ///
    /// # Errors
    /// Returns an unsupported-version or categorized semantic validation error.
    #[allow(clippy::too_many_lines)] // Keep the complete invariant gate in one reviewable pass.
    pub fn validate(&self) -> Result<(), PlanError> {
        if self.schema_version != EXPERIMENT_PLAN_SCHEMA_VERSION
            && self.schema_version != EXPERIMENT_PLAN_SCHEMA_VERSION_2
            && self.schema_version != EXPERIMENT_PLAN_SCHEMA_VERSION_3
            && self.schema_version != EXPERIMENT_PLAN_SCHEMA_VERSION_4
        {
            return Err(PlanError::UnsupportedVersion(self.schema_version.0));
        }
        // SemanticReplay is a schema-v4 workload; earlier schemas fail closed
        // rather than silently accepting future semantics.
        if matches!(self.workload, Workload::SemanticReplay { .. })
            && self.schema_version != EXPERIMENT_PLAN_SCHEMA_VERSION_4
        {
            return invalid(
                "unsupported_option",
                format!(
                    "SemanticReplay workload requires schema version 4 (got {})",
                    self.schema_version.0
                ),
            );
        }
        // M003a: SemanticReplay never composes with network_path; the
        // replay uses `--route direct` explicitly and diagnostics bypass the
        // benchmark path by design. This specific incompatibility is checked
        // before the generic schema-version gate so callers see the stable
        // workload_path_incompatible category.
        if matches!(self.workload, Workload::SemanticReplay { .. }) && self.network_path.is_some() {
            return invalid(
                "workload_path_incompatible",
                "SemanticReplay workload is incompatible with network_path in M003a",
            );
        }
        // Schema-v3 is the only schema where `network_path` is permitted.
        if self.network_path.is_some() && self.schema_version != EXPERIMENT_PLAN_SCHEMA_VERSION_3 {
            return invalid(
                "unsupported_option",
                format!(
                    "network_path requires schema version 3 (got {})",
                    self.schema_version.0
                ),
            );
        }
        if let Some(network_path) = &self.network_path {
            validate_network_path_contract(network_path)?;
            validate_path_service_configs(&self.services)?;
            if matches!(self.subject, Subject::External { .. }) {
                return invalid(
                    "workload_path_incompatible",
                    "network_path requires a transport-owning workload and is incompatible with an external subject",
                );
            }
            let fault_count = network_path
                .stream_faults
                .as_ref()
                .map_or(0, |faults| faults.upstream.len() + faults.downstream.len());
            if fault_count > 0 && self.seed.is_none() {
                return invalid(
                    "missing_fault_seed",
                    "network_path stream faults require an explicit experiment seed",
                );
            }
        }
        if let Subject::ManagedCommand { argv, .. } = &self.subject
            && (argv.is_empty() || argv[0].trim().is_empty())
        {
            return invalid(
                "invalid_bound",
                "managed command argv must contain a non-empty program",
            );
        }
        let mut services = BTreeMap::new();
        if self.services.len() > 256 {
            return invalid("invalid_bound", "at most 256 services are allowed");
        }
        for service in &self.services {
            if services.insert(service.name.clone(), service).is_some() {
                return invalid(
                    "duplicate_identity",
                    format!("duplicate service {}", service.name),
                );
            }
            if service.log_limit_bytes > self.bounds.artifact_bytes {
                return invalid(
                    "invalid_bound",
                    format!(
                        "service {} log limit exceeds per-artifact bound",
                        service.name
                    ),
                );
            }
            if let ServiceKind::Command { argv } = &service.kind
                && service.lifecycle == Lifecycle::Managed
                && (argv.is_empty() || argv[0].trim().is_empty())
            {
                return invalid(
                    "contradictory_configuration",
                    format!("managed command service {} needs a program", service.name),
                );
            }
            if service.lifecycle == Lifecycle::External
                && matches!(service.kind, ServiceKind::Command { .. })
                && service.shutdown.is_some()
            {
                return invalid(
                    "contradictory_configuration",
                    format!(
                        "external service {} cannot request managed shutdown",
                        service.name
                    ),
                );
            }
        }
        let target = workload_target(&self.workload);
        if !services.contains_key(target) {
            let is_external =
                matches!(self.subject, Subject::External { target: ref ext, .. } if ext == target);
            if !is_external {
                return invalid(
                    "missing_reference",
                    format!(
                        "workload target {target} is neither a declared service nor the subject's external target"
                    ),
                );
            }
        }
        for service in &self.services {
            for dep in &service.depends_on {
                if dep == &service.name {
                    return invalid(
                        "contradictory_configuration",
                        format!("service {} depends on itself", service.name),
                    );
                }
                if !services.contains_key(dep) {
                    return invalid(
                        "missing_reference",
                        format!("service {} depends on unknown service {dep}", service.name),
                    );
                }
            }
        }
        ensure_acyclic(&self.services)?;
        validate_workload(&self.workload)?;
        validate_paired(self, &services)?;
        if self.trials.warmup > 1_000 {
            return invalid("invalid_bound", "warmup count exceeds 1000");
        }
        if let ResetPolicy::Service { service } = &self.trials.reset
            && !services.contains_key(service)
        {
            return invalid(
                "missing_reference",
                format!("reset references unknown service {service}"),
            );
        }
        if self.metrics.len() > 256 || self.telemetry.len() > 128 {
            return invalid(
                "invalid_bound",
                "metric or telemetry request count exceeds bound",
            );
        }
        let mut metric_names = BTreeSet::new();
        for metric in &self.metrics {
            if !metric_names.insert(&metric.name) {
                return invalid(
                    "duplicate_identity",
                    format!("duplicate metric {}", metric.name),
                );
            }
            if let MetricDirection::TargetRange { min, max } = metric.direction
                && (!min.is_finite() || !max.is_finite() || min > max)
            {
                return invalid(
                    "invalid_bound",
                    format!("metric {} has invalid target range", metric.name),
                );
            }
            if let Some(gate) = &metric.gate {
                if metric.intent == MetricIntent::Diagnostic {
                    return invalid(
                        "contradictory_configuration",
                        format!("diagnostic metric {} cannot have a gate", metric.name),
                    );
                }
                match gate {
                    Gate::Absolute { value } if !value.is_finite() => {
                        return invalid(
                            "invalid_bound",
                            format!("metric {} has non-finite absolute gate", metric.name),
                        );
                    }
                    _ => {}
                }
                if matches!(metric.direction, MetricDirection::Informational) {
                    return invalid(
                        "contradictory_configuration",
                        format!("informational metric {} cannot be gated", metric.name),
                    );
                }
                // M003a: semantic mismatch counts are correctness quantities,
                // not smooth performance quantities for ratio/bootstrap
                // inference. Only absolute gates are supported.
                if metric.name.as_str() == "semantic_findings"
                    && matches!(self.workload, Workload::SemanticReplay { .. })
                    && matches!(
                        gate,
                        Gate::RelativeRegression { .. } | Gate::StatisticalRelative { .. }
                    )
                {
                    return invalid(
                        "unsupported_gate",
                        "semantic_findings supports only absolute gates in M003a",
                    );
                }
            }
        }
        if self.bounds.artifact_bytes == 0
            || self.bounds.total_bytes == 0
            || self.bounds.total_bytes < self.bounds.artifact_bytes
        {
            return invalid(
                "invalid_bound",
                "artifact bounds must be positive and total_bytes must be at least artifact_bytes",
            );
        }
        Ok(())
    }
}

impl Workload {
    /// Destination service or external target of this workload.
    #[must_use]
    pub fn target(&self) -> &Name {
        workload_target(self)
    }
    /// Clone this workload with the destination service replaced.
    ///
    /// The runner uses this to direct one trial at a paired arm's service
    /// while keeping the load shape identical across arms.
    #[must_use]
    pub fn with_target(&self, target: Name) -> Self {
        match self {
            Self::ClosedLoop {
                concurrency,
                requests,
                duration_ms,
                ..
            } => Self::ClosedLoop {
                target,
                concurrency: *concurrency,
                requests: *requests,
                duration_ms: *duration_ms,
            },
            Self::OpenLoop {
                rate_milli_rps,
                requests,
                duration_ms,
                ..
            } => Self::OpenLoop {
                target,
                rate_milli_rps: *rate_milli_rps,
                requests: *requests,
                duration_ms: *duration_ms,
            },
            Self::FiniteCount {
                requests,
                concurrency,
                ..
            } => Self::FiniteCount {
                target,
                requests: *requests,
                concurrency: *concurrency,
            },
            Self::TimeBounded {
                duration_ms,
                mode,
                concurrency,
                rate_milli_rps,
                ..
            } => Self::TimeBounded {
                target,
                duration_ms: *duration_ms,
                mode: *mode,
                concurrency: *concurrency,
                rate_milli_rps: *rate_milli_rps,
            },
            Self::SemanticReplay { fixture, .. } => Self::SemanticReplay {
                target,
                fixture: fixture.clone(),
            },
        }
    }
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
fn validate_workload(w: &Workload) -> Result<(), PlanError> {
    match w {
        Workload::ClosedLoop {
            requests,
            duration_ms,
            ..
        }
        | Workload::OpenLoop {
            requests,
            duration_ms,
            ..
        } => {
            if requests.is_some() == duration_ms.is_some() {
                invalid(
                    "contradictory_configuration",
                    "exactly one of requests or duration_ms must be set",
                )
            } else {
                Ok(())
            }
        }
        Workload::TimeBounded {
            mode,
            concurrency,
            rate_milli_rps,
            ..
        } => match (mode, concurrency.is_some(), rate_milli_rps.is_some()) {
            (LoadMode::ClosedLoop, true, false) | (LoadMode::OpenLoop, false, true) => Ok(()),
            _ => invalid(
                "contradictory_configuration",
                "closed-loop requires only concurrency; open-loop requires only rate_milli_rps",
            ),
        },
        Workload::FiniteCount { .. } => Ok(()),
        Workload::SemanticReplay { fixture, .. } => validate_semantic_fixture_path(fixture),
    }
}

/// Validate the M003a semantic-replay fixture path: relative, workspace
/// confined by construction, bounded, and free of absolute/traversal/control
/// characters. Filesystem existence and symlink-escape checks happen in
/// driver preflight against `RunnerOptions.workspace_root`.
fn validate_semantic_fixture_path(fixture: &str) -> Result<(), PlanError> {
    if fixture.is_empty() || fixture.len() > 512 {
        return invalid(
            "invalid_fixture",
            "SemanticReplay fixture path must be 1..=512 bytes",
        );
    }
    if fixture.contains('\0') || fixture.chars().any(char::is_control) {
        return invalid(
            "invalid_fixture",
            "SemanticReplay fixture path must not contain NUL or control characters",
        );
    }
    if fixture.starts_with('/') || fixture.starts_with('\\') {
        return invalid(
            "invalid_fixture",
            "SemanticReplay fixture must be a relative workspace path",
        );
    }
    // Reject Windows drive prefixes and UNC-style leading separators.
    if fixture.len() >= 2 && fixture.as_bytes()[1] == b':' {
        return invalid(
            "invalid_fixture",
            "SemanticReplay fixture must be a relative workspace path",
        );
    }
    let mut depth = 0usize;
    for component in fixture.split('/') {
        if component.is_empty() || component == "." {
            return invalid(
                "invalid_fixture",
                "SemanticReplay fixture path contains an empty or dot component",
            );
        }
        if component == ".." {
            return invalid(
                "invalid_fixture",
                "SemanticReplay fixture must not contain parent traversal",
            );
        }
        if component.len() > 128 {
            return invalid(
                "invalid_fixture",
                "SemanticReplay fixture path component exceeds 128 bytes",
            );
        }
        depth += 1;
        if depth > 16 {
            return invalid(
                "invalid_fixture",
                "SemanticReplay fixture path exceeds 16 components",
            );
        }
    }
    if fixture.contains('\\') {
        return invalid(
            "invalid_fixture",
            "SemanticReplay fixture path must use forward slashes",
        );
    }
    Ok(())
}
fn validate_paired(
    plan: &ExperimentPlan,
    services: &BTreeMap<Name, &Service>,
) -> Result<(), PlanError> {
    let Some(paired) = &plan.paired else {
        return Ok(());
    };
    if plan.schema_version == EXPERIMENT_PLAN_SCHEMA_VERSION {
        return Err(PlanError::UnsupportedVersion(plan.schema_version.0));
    }
    if plan.network_path.is_some() {
        return invalid(
            "paired_network_path_not_supported",
            "network_path is not supported together with paired experiments in M002",
        );
    }
    if !matches!(plan.subject, Subject::Label { .. }) {
        return invalid(
            "contradictory_configuration",
            "paired experiments require a label subject naming the comparison",
        );
    }
    let measured = plan.trials.measured.get();
    if measured < 2 || !measured.is_multiple_of(2) {
        return invalid(
            "invalid_bound",
            "paired experiments require an even measured trial count of at least 2",
        );
    }
    for (arm_name, arm) in [
        ("baseline", &paired.baseline),
        ("candidate", &paired.candidate),
    ] {
        if !services.contains_key(&arm.service) {
            return invalid(
                "missing_reference",
                format!(
                    "paired {arm_name} arm references unknown service {}",
                    arm.service
                ),
            );
        }
        if matches!(arm.subject, Subject::ManagedCommand { .. }) {
            return invalid(
                "unsupported_option",
                format!(
                    "paired {arm_name} arm subject must be a label or external identity, not a managed command"
                ),
            );
        }
    }
    if paired.baseline.service == paired.candidate.service {
        return invalid(
            "contradictory_configuration",
            "paired arms must target distinct services",
        );
    }
    if workload_target(&plan.workload) != &paired.baseline.service {
        return invalid(
            "contradictory_configuration",
            "paired plan workload target must equal the baseline arm service",
        );
    }
    Ok(())
}
fn ensure_acyclic(services: &[Service]) -> Result<(), PlanError> {
    fn visit(
        name: &Name,
        map: &BTreeMap<Name, &Service>,
        temporary: &mut BTreeSet<Name>,
        permanent: &mut BTreeSet<Name>,
    ) -> Result<(), PlanError> {
        if permanent.contains(name) {
            return Ok(());
        }
        if !temporary.insert(name.clone()) {
            return invalid("cycle", format!("service dependency cycle includes {name}"));
        }
        if let Some(service) = map.get(name) {
            for dep in &service.depends_on {
                visit(dep, map, temporary, permanent)?;
            }
        }
        temporary.remove(name);
        permanent.insert(name.clone());
        Ok(())
    }
    let map: BTreeMap<_, _> = services.iter().map(|s| (s.name.clone(), s)).collect();
    let (mut temporary, mut permanent) = (BTreeSet::new(), BTreeSet::new());
    for name in map.keys() {
        visit(name, &map, &mut temporary, &mut permanent)?;
    }
    Ok(())
}
fn invalid<T>(category: &'static str, detail: impl Into<String>) -> Result<T, PlanError> {
    Err(PlanError::Validation {
        category,
        detail: detail.into(),
    })
}

/// Validate the bounded schema-v3 `network_path` request.
///
/// Bounds enforced here stay aligned with §6.4 / §13.3 / §22 of the M002
/// implementation plan: bounded route text, no control characters, ≤128
/// faults per direction, unique fault identities, and `slice.variation <
/// slice.average_size`.
///
/// # Errors
/// Returns a stable [`PlanError::Validation`] category for unsafe routes or
/// malformed fault plans.
pub fn validate_network_path_contract(path: &NetworkPathRequest) -> Result<(), PlanError> {
    let chain_len_bound = 1024usize;
    match &path.route.mode {
        RouteMode::Direct => {}
        RouteMode::ProxyChain { chain } => {
            if chain.is_empty() || chain.len() > chain_len_bound {
                return invalid(
                    "invalid_route",
                    format!(
                        "route chain must be 1..={chain_len_bound} bytes (got {})",
                        chain.len()
                    ),
                );
            }
            if chain.chars().any(char::is_control) {
                return invalid(
                    "invalid_route",
                    "route chain must not contain control characters",
                );
            }
            if chain.contains(['@', '%', '?', '#']) {
                return invalid(
                    "route_credentials_not_supported",
                    "route chain must not contain userinfo, encoded credentials, query, or fragment data",
                );
            }
            validate_proxy_chain_shape(chain)?;
        }
    }
    if let Some(faults) = &path.stream_faults {
        validate_stream_fault_plan(&faults.upstream, "upstream").map_err(|error| {
            PlanError::Validation {
                category: "invalid_fault_plan",
                detail: error.to_string(),
            }
        })?;
        validate_stream_fault_plan(&faults.downstream, "downstream").map_err(|error| {
            PlanError::Validation {
                category: "invalid_fault_plan",
                detail: error.to_string(),
            }
        })?;
    }
    Ok(())
}

pub(crate) fn validate_proxy_chain_shape(chain: &str) -> Result<(), PlanError> {
    for hop in chain.split("__") {
        let Some((scheme, endpoint)) = hop.split_once("://") else {
            return invalid(
                "invalid_route",
                "each route hop must use an explicit protocol://host:port form",
            );
        };
        if !matches!(scheme, "http" | "socks4" | "socks4a" | "socks5") {
            return invalid(
                "unsupported_route",
                "M002 route hops support only HTTP, SOCKS4, or SOCKS5",
            );
        }
        if endpoint.is_empty()
            || !endpoint
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || ".-:[]".contains(character))
        {
            return invalid(
                "invalid_route",
                "route endpoints must be credential-free ASCII host:port values",
            );
        }
        validate_route_endpoint(endpoint)?;
    }
    Ok(())
}

pub(crate) fn canonical_proxy_chain_text(chain: &str) -> String {
    chain
        .split("__")
        .map(|hop| {
            let (scheme, endpoint) = hop.split_once("://").expect("validated proxy hop");
            let canonical_scheme = if scheme == "socks4a" {
                "socks4"
            } else {
                scheme
            };
            let (host, port) = if let Some(bracketed) = endpoint.strip_prefix('[') {
                let close = bracketed
                    .find(']')
                    .expect("validated bracketed proxy endpoint");
                (&bracketed[..close], &bracketed[close + 2..])
            } else {
                endpoint.rsplit_once(':').expect("validated proxy endpoint")
            };
            let port = port.parse::<u16>().expect("validated proxy port");
            let host = if host.contains(':') {
                format!("[{host}]")
            } else {
                host.to_owned()
            };
            format!("{canonical_scheme}://{host}:{port}")
        })
        .collect::<Vec<_>>()
        .join("__")
}

fn validate_route_endpoint(endpoint: &str) -> Result<(), PlanError> {
    let (host, port) = if let Some(bracketed) = endpoint.strip_prefix('[') {
        let Some(close) = bracketed.find(']') else {
            return invalid("invalid_route", "bracketed route IPv6 host is incomplete");
        };
        let host = &bracketed[..close];
        let suffix = &bracketed[close + 1..];
        let Some(port) = suffix.strip_prefix(':') else {
            return invalid("invalid_route", "bracketed route host requires a port");
        };
        if host.parse::<std::net::Ipv6Addr>().is_err() {
            return invalid("invalid_route", "bracketed route host must be IPv6");
        }
        (host, port)
    } else {
        let Some((host, port)) = endpoint.rsplit_once(':') else {
            return invalid("invalid_route", "route host requires an explicit port");
        };
        if host.is_empty() || host.contains(':') {
            return invalid(
                "invalid_route",
                "route host must be a non-empty DNS name or IPv4 address",
            );
        }
        (host, port)
    };
    let port = port.parse::<u16>().ok().filter(|port| *port > 0);
    if port.is_none() {
        return invalid("invalid_route", "route port must be between 1 and 65535");
    }
    if host.contains(':') {
        return Ok(());
    }
    if !host
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || ".-".contains(character))
    {
        return invalid(
            "invalid_route",
            "route host contains unsupported characters",
        );
    }
    Ok(())
}

fn validate_path_service_configs(services: &[Service]) -> Result<(), PlanError> {
    for service in services {
        for (key, value) in &service.config {
            if !matches!(key.as_str(), "path" | "body_bytes" | "status") {
                return invalid(
                    "route_credentials_not_supported",
                    format!(
                        "network_path service {} config key {key} is not an approved non-secret key",
                        service.name
                    ),
                );
            }
            let normalized_key = key
                .as_str()
                .to_ascii_lowercase()
                .chars()
                .filter(char::is_ascii_alphanumeric)
                .collect::<String>();
            let normalized_value = value.to_ascii_lowercase();
            let sensitive_key = [
                "password",
                "passwd",
                "secret",
                "token",
                "credential",
                "privatekey",
                "apikey",
                "authorization",
            ]
            .iter()
            .any(|marker| normalized_key.contains(marker));
            let sensitive_value = (normalized_value.contains("://")
                && normalized_value.contains('@'))
                || normalized_value.contains("bearer ")
                || normalized_value.contains("basic ")
                || (normalized_value.contains('?')
                    && ["password", "secret", "token", "credential", "apikey"]
                        .iter()
                        .any(|marker| normalized_value.contains(marker)));
            if sensitive_key || sensitive_value {
                return invalid(
                    "route_credentials_not_supported",
                    format!(
                        "network_path service {} config key {key} must not contain credentials",
                        service.name
                    ),
                );
            }
        }
    }
    Ok(())
}

fn validate_stream_fault_plan(
    faults: &[crate::network_path::StreamFaultRequest],
    direction: &'static str,
) -> Result<(), PlanError> {
    let bound = 128usize;
    if faults.len() > bound {
        return invalid(
            "invalid_bound",
            format!(
                "{direction} faults exceed bound {bound} (got {})",
                faults.len()
            ),
        );
    }
    let mut seen = BTreeSet::new();
    for fault in faults {
        if !seen.insert(fault.id.clone()) {
            return invalid(
                "duplicate_identity",
                format!("{direction} fault id {} is duplicated", fault.id),
            );
        }
        if let crate::network_path::StreamFaultKind::Slice {
            average_size,
            variation,
            ..
        } = &fault.kind
            && *variation >= u64::from(average_size.get())
        {
            return invalid(
                "invalid_bound",
                format!(
                    "slice variation {variation} must be strictly less than average_size {}",
                    average_size.get()
                ),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const VALID: &str = include_str!("../tests/fixtures/minimal.json");
    fn plan_with_service() -> ExperimentPlan {
        let mut plan: ExperimentPlan = serde_json::from_str(VALID).unwrap();
        plan.services.push(Service {
            name: Name::new("api").unwrap(),
            kind: ServiceKind::Named {
                service_type: Name::new("http").unwrap(),
            },
            lifecycle: Lifecycle::External,
            depends_on: Vec::new(),
            config: BTreeMap::new(),
            readiness: None,
            shutdown: None,
            working_directory: None,
            log_limit_bytes: 4096,
        });
        plan
    }
    fn error_category(result: Result<(), PlanError>) -> String {
        match result.unwrap_err() {
            PlanError::Validation { category, .. } => category.to_owned(),
            other => panic!("expected validation error, got {other}"),
        }
    }
    #[test]
    fn proxy_chain_canonicalization_matches_native_alias_rules() {
        assert_eq!(
            canonical_proxy_chain_text("socks4a://proxy.example:1080"),
            "socks4://proxy.example:1080"
        );
    }

    #[test]
    fn json_round_trip_and_version() {
        let plan = ExperimentPlan::from_json(VALID).unwrap();
        let round = ExperimentPlan::from_json(&plan.to_json().unwrap()).unwrap();
        assert_eq!(plan, round);
        let mut version: serde_json::Value = serde_json::from_str(VALID).unwrap();
        version["schema_version"] = 99.into();
        assert!(matches!(
            ExperimentPlan::from_json(&version.to_string()),
            Err(PlanError::UnsupportedVersion(99))
        ));
    }
    #[test]
    fn toml_round_trip_preserves_semantics() {
        let plan = ExperimentPlan::from_json(VALID).unwrap();
        let toml = plan.to_toml().unwrap();
        assert_eq!(ExperimentPlan::from_toml(&toml).unwrap(), plan);
    }
    #[test]
    fn representative_open_loop_fixture_is_valid() {
        let fixture = include_str!("../tests/fixtures/multi-service-open-loop.json");
        let plan = ExperimentPlan::from_json(fixture).unwrap();
        assert!(matches!(plan.workload, Workload::OpenLoop { .. }));
        assert_eq!(plan.services.len(), 2);
    }
    #[test]
    fn invalid_fixtures_fail_with_stable_categories() {
        for (fixture, expected) in [
            (
                include_str!("../tests/fixtures/invalid-cycle.json"),
                "cycle",
            ),
            (
                include_str!("../tests/fixtures/invalid-unknown-service.json"),
                "missing_reference",
            ),
            (
                include_str!("../tests/fixtures/invalid-contradictory-workload.json"),
                "contradictory_configuration",
            ),
        ] {
            let plan: ExperimentPlan = serde_json::from_str(fixture).unwrap();
            assert_eq!(error_category(plan.validate()), expected);
        }
        assert!(
            ExperimentPlan::from_json(include_str!("../tests/fixtures/invalid-zero-trials.json"))
                .is_err()
        );
    }
    #[test]
    fn topology_and_workload_fail_closed() {
        let mut p = plan_with_service();
        p.services[0].depends_on.push(Name::new("absent").unwrap());
        assert_eq!(error_category(p.validate()), "missing_reference");
        let mut p = plan_with_service();
        let own_name = p.services[0].name.clone();
        p.services[0].depends_on.push(own_name);
        assert_eq!(error_category(p.validate()), "contradictory_configuration");
        let mut p = plan_with_service();
        p.workload = Workload::OpenLoop {
            target: Name::new("api").unwrap(),
            rate_milli_rps: RateMilliRps::new(1000).unwrap(),
            requests: None,
            duration_ms: None,
        };
        assert_eq!(error_category(p.validate()), "contradictory_configuration");
    }
    #[test]
    fn duplicate_identity_cycle_and_gate_errors() {
        let mut p = plan_with_service();
        p.services.push(p.services[0].clone());
        assert_eq!(error_category(p.validate()), "duplicate_identity");
        let mut p = plan_with_service();
        p.services.push(Service {
            name: Name::new("cache").unwrap(),
            kind: ServiceKind::Named {
                service_type: Name::new("cache").unwrap(),
            },
            lifecycle: Lifecycle::External,
            depends_on: vec![Name::new("api").unwrap()],
            config: BTreeMap::new(),
            readiness: None,
            shutdown: None,
            working_directory: None,
            log_limit_bytes: 4096,
        });
        p.services[0].depends_on.push(Name::new("cache").unwrap());
        assert_eq!(error_category(p.validate()), "cycle");
        let mut p = plan_with_service();
        p.metrics[0].intent = MetricIntent::Diagnostic;
        assert_eq!(error_category(p.validate()), "contradictory_configuration");
    }
    #[test]
    fn bounded_fields_reject_zero() {
        assert!(DurationMs::new(0).is_err());
        assert!(RateMilliRps::new(0).is_err());
        assert!(PositiveCount::new(0).is_err());
    }

    fn paired_value() -> serde_json::Value {
        let mut value: serde_json::Value = serde_json::from_str(VALID).unwrap();
        value["schema_version"] = 2.into();
        value["subject"] = serde_json::json!({"kind": "label", "label": "a-vs-b"});
        let service = |name: &str| {
            serde_json::json!({
                "name": name,
                "kind": {"kind": "named", "service_type": "http"},
                "lifecycle": "external",
                "depends_on": [],
                "config": {},
                "readiness": null,
                "shutdown": null,
                "working_directory": null,
                "log_limit_bytes": 4096,
            })
        };
        value["services"] = serde_json::json!([service("origin-a"), service("origin-b")]);
        value["workload"] = serde_json::json!({
            "kind": "finite_count", "target": "origin-a",
            "requests": 100, "concurrency": 5,
        });
        value["trials"]["measured"] = 4.into();
        value["paired"] = serde_json::json!({
            "baseline": {
                "service": "origin-a",
                "subject": {"kind": "label", "label": "variant-a"},
            },
            "candidate": {
                "service": "origin-b",
                "subject": {"kind": "external", "target": "origin-b",
                            "revision": "rev-b", "digest": null},
            },
        });
        value
    }

    fn paired_plan() -> ExperimentPlan {
        ExperimentPlan::from_json(&paired_value().to_string()).unwrap()
    }

    #[test]
    fn paired_v2_round_trip_preserves_design() {
        let plan = paired_plan();
        assert_eq!(plan.schema_version.0, 2);
        let design = plan.paired.as_ref().expect("paired design");
        assert_eq!(design.baseline.service.as_str(), "origin-a");
        assert_eq!(design.candidate.service.as_str(), "origin-b");
        let round = ExperimentPlan::from_json(&plan.to_json().unwrap()).unwrap();
        assert_eq!(plan, round);
        assert_eq!(round.schema_version.0, 2);
        let toml = ExperimentPlan::from_toml(&plan.to_toml().unwrap()).unwrap();
        assert_eq!(plan, toml);
    }

    #[test]
    fn v1_plan_with_paired_design_fails_closed_at_validation() {
        // `paired` is a known field, so a schema-v1 plan carrying it parses
        // and then fails closed at the version guard (never silently
        // accepted as unpaired).
        let mut value: serde_json::Value = serde_json::from_str(VALID).unwrap();
        value["paired"] = paired_value()["paired"].clone();
        assert!(matches!(
            ExperimentPlan::from_json(&value.to_string()),
            Err(PlanError::UnsupportedVersion(1))
        ));
    }

    #[test]
    fn paired_design_requires_schema_v2() {
        let mut plan = paired_plan();
        plan.schema_version = crate::EXPERIMENT_PLAN_SCHEMA_VERSION;
        assert!(matches!(
            plan.validate(),
            Err(PlanError::UnsupportedVersion(1))
        ));
    }

    #[test]
    fn paired_design_validation_matrix_fails_closed() {
        // Non-label top-level subject.
        let mut value = paired_value();
        value["subject"] = serde_json::json!({"kind": "external", "target": "api", "revision": null, "digest": null});
        assert_eq!(
            error_category(ExperimentPlan::from_json(&value.to_string()).map(|_| ())),
            "contradictory_configuration"
        );
        // Odd measured trial count.
        let mut value = paired_value();
        value["trials"]["measured"] = 3.into();
        assert_eq!(
            error_category(
                ExperimentPlan::from_json(&value.to_string()).and_then(|plan| {
                    plan.validate()?;
                    Ok(())
                })
            ),
            "invalid_bound"
        );
        // Unknown arm service.
        let mut value = paired_value();
        value["paired"]["candidate"]["service"] = "absent".into();
        assert_eq!(
            error_category(
                ExperimentPlan::from_json(&value.to_string()).and_then(|plan| {
                    plan.validate()?;
                    Ok(())
                })
            ),
            "missing_reference"
        );
        // Identical arm services.
        let mut value = paired_value();
        value["paired"]["candidate"]["service"] = "origin-a".into();
        assert_eq!(
            error_category(
                ExperimentPlan::from_json(&value.to_string()).and_then(|plan| {
                    plan.validate()?;
                    Ok(())
                })
            ),
            "contradictory_configuration"
        );
        // Managed-command arm subject.
        let mut value = paired_value();
        value["paired"]["baseline"]["subject"] = serde_json::json!({
            "kind": "managed_command", "argv": ["/bin/true"],
            "environment": {}, "revision": null, "digest": null,
        });
        assert_eq!(
            error_category(
                ExperimentPlan::from_json(&value.to_string()).and_then(|plan| {
                    plan.validate()?;
                    Ok(())
                })
            ),
            "unsupported_option"
        );
        // Workload target must equal the baseline arm service.
        let mut value = paired_value();
        value["workload"]["target"] = "origin-b".into();
        assert_eq!(
            error_category(
                ExperimentPlan::from_json(&value.to_string()).and_then(|plan| {
                    plan.validate()?;
                    Ok(())
                })
            ),
            "contradictory_configuration"
        );
    }

    #[test]
    fn with_target_replaces_destination_only() {
        let plan = paired_plan();
        let retargeted = plan.workload.with_target(Name::new("origin-b").unwrap());
        assert_eq!(retargeted.target().as_str(), "origin-b");
        assert_eq!(plan.workload.target().as_str(), "origin-a");
        match (&plan.workload, &retargeted) {
            (
                Workload::FiniteCount {
                    requests: left_requests,
                    concurrency: left_concurrency,
                    ..
                },
                Workload::FiniteCount {
                    requests,
                    concurrency,
                    ..
                },
            ) => {
                assert_eq!(requests, left_requests);
                assert_eq!(concurrency, left_concurrency);
            }
            _ => panic!("expected finite-count workloads"),
        }
    }

    fn semantic_value() -> serde_json::Value {
        let mut value: serde_json::Value = serde_json::from_str(VALID).unwrap();
        value["schema_version"] = 4.into();
        value["services"] = serde_json::json!([{
            "name": "origin",
            "kind": {"kind": "named", "service_type": "eggserve-origin"},
            "lifecycle": "external",
            "depends_on": [],
            "config": {},
            "readiness": null,
            "shutdown": null,
            "working_directory": null,
            "log_limit_bytes": 4096,
        }]);
        value["workload"] = serde_json::json!({
            "kind": "semantic_replay", "target": "origin", "fixture": "fixtures/replay",
        });
        value["metrics"] = serde_json::json!([{
            "name": "semantic_findings", "unit": "count",
            "direction": {"kind": "lower_is_better"}, "intent": "primary",
            "gate": {"kind": "absolute", "value": 0.0},
        }]);
        value
    }

    #[test]
    fn v4_semantic_replay_round_trip_preserves_intent() {
        let plan = ExperimentPlan::from_json(&semantic_value().to_string()).unwrap();
        assert_eq!(plan.schema_version.0, 4);
        assert!(matches!(plan.workload, Workload::SemanticReplay { .. }));
        let round = ExperimentPlan::from_json(&plan.to_json().unwrap()).unwrap();
        assert_eq!(plan, round);
        let toml = ExperimentPlan::from_toml(&plan.to_toml().unwrap()).unwrap();
        assert_eq!(plan, toml);
        // Target rewriting preserves the fixture for paired designs.
        let retargeted = plan.workload.with_target(Name::new("origin-b").unwrap());
        match retargeted {
            Workload::SemanticReplay { target, fixture } => {
                assert_eq!(target.as_str(), "origin-b");
                assert_eq!(fixture, "fixtures/replay");
            }
            _ => panic!("expected semantic replay"),
        }
    }

    #[test]
    fn v1_v3_reject_semantic_replay_and_v4_stays_compatible() {
        // v1-v3 plans remain readable.
        assert!(ExperimentPlan::from_json(VALID).is_ok());
        // A v3 plan carrying SemanticReplay fails closed.
        let mut value: serde_json::Value = serde_json::from_str(VALID).unwrap();
        value["workload"] = serde_json::json!({
            "kind": "semantic_replay", "target": "api", "fixture": "fixtures/replay",
        });
        assert_eq!(
            error_category(ExperimentPlan::from_json(&value.to_string()).map(|_| ())),
            "unsupported_option"
        );
    }

    #[test]
    fn semantic_fixture_paths_fail_closed() {
        for bad in [
            "",
            "/abs/path",
            "../escape",
            "a/../b",
            "bad\\slash",
            "a/./b",
            "a//b",
        ] {
            let mut value = semantic_value();
            value["workload"]["fixture"] = bad.into();
            assert_eq!(
                error_category(ExperimentPlan::from_json(&value.to_string()).map(|_| ())),
                "invalid_fixture",
                "fixture={bad:?}"
            );
        }
    }

    #[test]
    fn semantic_replay_rejects_network_path() {
        let mut value = semantic_value();
        value["schema_version"] = 4.into();
        value["network_path"] = serde_json::json!({
            "route": {"driver": "eggress-route", "mode": {"kind": "direct"}},
        });
        // v4 forbids network_path entirely; the semantic-specific
        // workload_path_incompatible gate fires first for replay plans.
        let result = ExperimentPlan::from_json(&value.to_string()).map(|_| ());
        assert_eq!(error_category(result), "workload_path_incompatible");
    }

    #[test]
    fn semantic_findings_rejects_relative_and_statistical_gates() {
        for gate in [
            serde_json::json!({"kind": "relative_regression", "allowance": 100}),
            serde_json::json!({
                "kind": "statistical_relative",
                "allowance": 100,
                "min_trials": 5,
            }),
        ] {
            let mut value = semantic_value();
            value["metrics"][0]["gate"] = gate;
            assert_eq!(
                error_category(ExperimentPlan::from_json(&value.to_string()).map(|_| ())),
                "unsupported_gate"
            );
        }
        // Absolute zero gate is the primary correctness use case.
        assert!(ExperimentPlan::from_json(&semantic_value().to_string()).is_ok());
    }
}
