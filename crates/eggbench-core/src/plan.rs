use crate::{
    BasisPoints, DurationMs, EXPERIMENT_PLAN_SCHEMA_VERSION, Name, PositiveCount, RateMilliRps,
    SchemaVersion, SecretRef,
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
    /// Upper bounds for later evidence creation.
    pub bounds: ArtifactBounds,
}

/// Subject declaration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
}
/// Explicit load model discriminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

impl ExperimentPlan {
    /// Parse JSON, enforce schema version, and validate semantics.
    ///
    /// # Errors
    /// Returns a parse, unsupported-version, or semantic validation error.
    pub fn from_json(input: &str) -> Result<Self, PlanError> {
        let plan: Self = serde_json::from_str(input).map_err(|e| PlanError::Parse {
            format: "JSON",
            message: e.to_string(),
        })?;
        plan.validate()?;
        Ok(plan)
    }
    /// Parse TOML, enforce schema version, and validate semantics.
    ///
    /// # Errors
    /// Returns a parse, unsupported-version, or semantic validation error.
    pub fn from_toml(input: &str) -> Result<Self, PlanError> {
        let plan: Self = toml::from_str(input).map_err(|e| PlanError::Parse {
            format: "TOML",
            message: e.to_string(),
        })?;
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
        if self.schema_version != EXPERIMENT_PLAN_SCHEMA_VERSION {
            return Err(PlanError::UnsupportedVersion(self.schema_version.0));
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

fn workload_target(workload: &Workload) -> &Name {
    match workload {
        Workload::ClosedLoop { target, .. }
        | Workload::OpenLoop { target, .. }
        | Workload::FiniteCount { target, .. }
        | Workload::TimeBounded { target, .. } => target,
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
    }
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
}
