//! Immutable-bundle comparison: baselines, comparability, statistical gates.
//!
//! M002 makes two finalized `.eggb` bundles comparable under one
//! transparent, versioned policy without mutating either bundle.
//!
//! Design notes:
//!
//! - One measured trial is one statistical observation unit. Request counts,
//!   histogram buckets, and per-request samples never increase the
//!   comparison sample count: only normalized trial-level scalars are
//!   consumed.
//! - Candidate gates come from the candidate's predeclared resolved plan.
//!   Baseline gate declarations never override candidate policy.
//! - Practical threshold and uncertainty remain separate fields in every
//!   receipt.
//! - Missing/invalid trial metrics are never imputed; excluded trials are
//!   listed with stable reasons.
//! - Policy v1 is immutable once evidence is emitted. Any semantic change to
//!   resampling, interval extraction, degradation orientation, comparability
//!   behavior, or aggregate verdict ordering requires a new policy
//!   identifier.
//! - No p-value is computed or reported in policy v1. Paired inference is
//!   not attempted: M002 resampling is unpaired only.
//! - Comparison is deterministic: the same bundles, policy, and seed yield
//!   byte-equivalent receipts. No OS randomness participates; no wall-clock
//!   timestamp enters the canonical receipt.

use crate::{
    ArtifactRole, BundleError, BundleReader, DriverCategory, EnvironmentFieldClass,
    EnvironmentFingerprint, EnvironmentPolicy, Gate, MetricDirection, MetricIntent, MetricRequest,
    Name, ObservationState, ResolvedPlan, RunId, SchemaVersion, Subject, TrialExecutionResult,
    TrialExecutionStatus, TrialId, TrialMetrics, Workload, validate_resolved_plan_bytes,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Schema version of the standalone [`ComparisonReceipt`].
pub const COMPARISON_RECEIPT_SCHEMA_VERSION: SchemaVersion = SchemaVersion(1);

/// Immutable comparison-policy identifier for trial-level bootstrap v1.
pub const COMPARISON_POLICY_V1: &str = "eggbench.trial-bootstrap.v1";

/// Schema version of the human-managed baseline alias file.
pub const BASELINE_ALIAS_SCHEMA_VERSION: u32 = 1;

/// Final bootstrap resample count for policy v1. Production comparison never
/// reduces this; unit tests may exercise the internal evaluator with smaller
/// counts.
pub const BOOTSTRAP_RESAMPLES: usize = 10_000;

/// Minimum valid observations required per side for statistical gating.
/// The effective requirement is `max(plan.min_trials, 5)`.
pub const MIN_OBSERVATIONS_PER_SIDE: usize = 5;

/// Recommended observations per side for qualification. Diagnostic only: it
/// produces a warning, never a failure.
pub const RECOMMENDED_OBSERVATIONS_PER_SIDE: usize = 7;

/// Statistical method label recorded in receipts.
pub const STATISTICAL_METHOD_V1: &str = "unpaired-trial-bootstrap-percentile-95";

/// Normalization method label expected from trial evidence (informational).
const MAX_RECEIPT_METRICS: usize = 256;

/// Fail-closed comparison errors.
#[derive(Debug, Error)]
pub enum ComparisonError {
    /// Bundle I/O, verification, or schema failure.
    #[error(transparent)]
    Bundle(#[from] BundleError),
    /// Baseline alias file failure.
    #[error("{category}: {detail}")]
    Alias {
        /// Stable machine-readable category.
        category: &'static str,
        /// Human-readable context.
        detail: String,
    },
    /// Comparison input is structurally valid but cannot be gated.
    #[error("{0}")]
    Unsupported(&'static str),
}

impl ComparisonError {
    /// Stable machine-readable category for CLI presentation.
    #[must_use]
    pub fn category(&self) -> &'static str {
        match self {
            Self::Bundle(_) => "bundle",
            Self::Alias { category, .. } => category,
            Self::Unsupported(_) => "unsupported_comparison",
        }
    }
}

/// Stable immutable identity of one verified bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BundleIdentity {
    /// Manifest schema version.
    pub manifest_schema_version: SchemaVersion,
    /// Run identity.
    pub run_id: RunId,
    /// SHA-256 of the exact finalized `manifest.json` bytes.
    pub manifest_sha256: String,
    /// Declared subject revision, presentation only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_revision: Option<String>,
    /// Declared subject digest, presentation only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_digest: Option<String>,
}

/// Human-managed baseline alias file (`*.eggbaseline.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineAliasFile {
    /// Alias schema version (must be 1).
    pub schema_version: u32,
    /// Human alias name.
    pub alias: String,
    /// Bundle path, relative to the alias file when relative.
    pub bundle_path: String,
    /// Required SHA-256 of the referenced bundle's `manifest.json`.
    pub manifest_sha256: String,
    /// Optional human note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Typed baseline reference recorded in the receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BaselineReference {
    /// Direct bundle reference.
    Bundle {
        /// Resolved immutable identity.
        identity: BundleIdentity,
        /// Bundle path as supplied.
        path: String,
    },
    /// Human-managed alias reference with its resolution pinned.
    Alias {
        /// Human alias name.
        alias: String,
        /// Alias file path as supplied.
        alias_file: String,
        /// Immutable identity the alias resolved to.
        resolved_identity: BundleIdentity,
        /// Bundle path the alias resolved to.
        resolved_path: String,
    },
}

/// Per-metric gate disposition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateDisposition {
    /// Gate evaluated and satisfied.
    Pass,
    /// Gate evaluated and violated.
    Fail,
    /// Statistical interval crosses the threshold.
    Inconclusive,
    /// Gate could not be evaluated honestly.
    Invalid,
    /// Descriptive baseline effect only; never a same-testbed gate verdict.
    Descriptive,
}

/// Aggregate comparison verdict. Descriptive effects never appear here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AggregateVerdict {
    /// A gated primary metric is invalid.
    Invalid,
    /// A gated primary metric failed.
    Fail,
    /// A gated primary metric is inconclusive.
    Inconclusive,
    /// At least one gated primary metric passed and none failed.
    Pass,
}

/// One comparability dimension outcome for a testbed field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldComparison {
    /// Environment field name.
    pub field: String,
    /// Candidate value, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate: Option<String>,
    /// Baseline value, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline: Option<String>,
    /// Dimension outcome.
    pub outcome: FieldOutcome,
}

/// Dimension-level comparability outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldOutcome {
    /// Values are equal.
    Equal,
    /// Field is absent from the candidate fingerprint.
    MissingCandidate,
    /// Field is absent from the baseline fingerprint.
    MissingBaseline,
    /// Comparison-critical values differ.
    Unequal,
    /// Warning-only values differ; does not invalidate same-testbed gating.
    WarningMismatch,
}

/// Typed comparability report v1: four explicit dimensions rather than one
/// raw equality test.
#[allow(clippy::struct_excessive_bools)] // Four named dimensions are the contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComparabilityReport {
    /// Per-field testbed comparison, sorted by field name.
    pub testbed: Vec<FieldComparison>,
    /// Workload semantics agree for baseline-dependent gating.
    pub workload_match: bool,
    /// Workload comparison detail.
    pub workload_detail: String,
    /// Selected workload-driver semantics agree.
    pub driver_match: bool,
    /// Driver comparison detail.
    pub driver_detail: String,
    /// Resolved service topology agrees (subject identity excluded).
    pub topology_match: bool,
    /// Topology comparison detail.
    pub topology_detail: String,
    /// True when any comparison-critical dimension mismatches.
    pub critical_mismatch: bool,
}

/// One excluded trial with a stable reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExcludedTrial {
    /// Stable trial identity.
    pub trial_id: u32,
    /// Stable `snake_case` reason.
    pub reason: String,
}

/// Per-metric comparison record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricComparison {
    /// Metric name.
    pub name: Name,
    /// Unit from the candidate plan.
    pub unit: Name,
    /// Direction from the candidate plan.
    pub direction: MetricDirection,
    /// Intent from the candidate plan.
    pub intent: MetricIntent,
    /// Gate declaration from the candidate plan, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<Gate>,
    /// Included candidate trial identities.
    pub candidate_included: Vec<u32>,
    /// Excluded candidate trials with reasons.
    pub candidate_excluded: Vec<ExcludedTrial>,
    /// Included baseline trial identities.
    pub baseline_included: Vec<u32>,
    /// Excluded baseline trials with reasons.
    pub baseline_excluded: Vec<ExcludedTrial>,
    /// Arithmetic mean of valid candidate observations, when any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_estimate: Option<f64>,
    /// Geometric mean of valid baseline observations, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_estimate: Option<f64>,
    /// Oriented degradation as a fraction (0.03 = 3% worse), when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub degradation: Option<f64>,
    /// Lower bootstrap interval bound in degradation space, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence_low: Option<f64>,
    /// Upper bootstrap interval bound in degradation space, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence_high: Option<f64>,
    /// Practical allowance as a fraction, when the gate declares one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold: Option<f64>,
    /// Statistical method label, when statistical gating ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statistical_method: Option<String>,
    /// Final bootstrap resample count, when statistical gating ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resamples: Option<usize>,
    /// Effective per-metric seed, when statistical gating ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_seed: Option<u64>,
    /// Gate disposition; `None` for ungated descriptive metrics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disposition: Option<GateDisposition>,
    /// Stable `snake_case` reason for invalid/descriptive outcomes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Bounded comparison warning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComparisonWarning {
    /// Stable warning category.
    pub category: String,
    /// Bounded human detail.
    pub detail: String,
}

/// Standalone immutable-by-content comparison receipt, schema v1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComparisonReceipt {
    /// Receipt schema version.
    pub schema_version: SchemaVersion,
    /// Comparison policy identifier.
    pub policy_id: String,
    /// Eggbench build version that produced the receipt.
    pub created_by_version: String,
    /// Candidate bundle identity.
    pub candidate_identity: BundleIdentity,
    /// How the baseline was referenced, when a baseline participates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_reference: Option<BaselineReference>,
    /// Baseline bundle identity, when a baseline participates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_identity: Option<BundleIdentity>,
    /// Environment policy from the candidate plan.
    pub environment_policy: EnvironmentPolicy,
    /// Typed comparability report (empty testbed list for absolute-only).
    pub comparability: ComparabilityReport,
    /// Base seed: explicit CLI seed or deterministic derivation.
    pub base_seed: u64,
    /// Per-metric records in metric-name order.
    pub metrics: Vec<MetricComparison>,
    /// Conservative aggregate over gated primary metrics, when any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aggregate_verdict: Option<AggregateVerdict>,
    /// Bounded diagnostics.
    #[serde(default)]
    pub warnings: Vec<ComparisonWarning>,
}

/// Comparison options.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ComparisonOptions {
    /// Explicit seed; when absent a deterministic base seed is derived from
    /// candidate/baseline manifest digests and the policy identifier.
    pub seed: Option<u64>,
}

/// Verified comparison-ready input for one bundle.
#[derive(Debug, Clone)]
pub struct ComparisonInput {
    /// Stable bundle identity.
    pub identity: BundleIdentity,
    /// Resolved plan from bundle evidence.
    pub resolved: ResolvedPlan,
    /// Environment fingerprint from bundle evidence.
    pub environment: EnvironmentFingerprint,
    /// Measured-trial evidence in manifest order.
    pub trials: Vec<InputTrial>,
}

/// One trial's comparison evidence.
#[derive(Debug, Clone)]
pub struct InputTrial {
    /// Stable trial identity.
    pub id: TrialId,
    /// Terminal execution state.
    pub terminal: TrialExecutionStatus,
    /// Normalized metrics, when the trial staged them.
    pub metrics: Option<TrialMetrics>,
}

/// Baseline side of a comparison request.
#[derive(Debug, Clone)]
pub struct BaselineSide<'a> {
    /// Typed reference recorded in the receipt.
    pub reference: BaselineReference,
    /// Verified baseline input.
    pub input: &'a ComparisonInput,
}

/// Comparison request: candidate plus an optional baseline side.
#[derive(Debug, Clone)]
pub struct ComparisonRequest<'a> {
    /// Candidate bundle input (authoritative for gates and policy).
    pub candidate: &'a ComparisonInput,
    /// Baseline side, when baseline-dependent comparison is requested.
    pub baseline: Option<BaselineSide<'a>>,
}

/// Load a verified comparison-ready input from an opened bundle.
///
/// Verifies the bundle before deriving identity or reading evidence.
///
/// # Errors
/// Returns [`ComparisonError`] on verification, identity, or evidence-read
/// failures.
pub fn load_comparison_input(reader: &BundleReader) -> Result<ComparisonInput, ComparisonError> {
    reader.verify()?;
    let manifest = reader.manifest();
    let manifest_bytes = read_manifest_bytes(reader)?;
    let identity = BundleIdentity {
        manifest_schema_version: manifest.schema_version,
        run_id: manifest.run_id,
        manifest_sha256: sha256_hex(&manifest_bytes),
        subject_revision: subject_revision(&manifest.subject),
        subject_digest: subject_digest(&manifest.subject),
    };
    let resolved = load_role_json::<ResolvedPlan>(reader, &ArtifactRole::ResolvedPlan)?;
    validate_resolved_plan_bytes(
        &serde_json::to_vec(&resolved)
            .map_err(|error| BundleError::ManifestParse(error.to_string()))?,
    )?;
    let environment =
        load_role_json::<EnvironmentFingerprint>(reader, &ArtifactRole::EnvironmentFingerprint)?;
    environment.validate()?;
    let mut trials = Vec::with_capacity(manifest.trials.len());
    for descriptor in &manifest.trials {
        let result = load_trial_result(reader, descriptor)?;
        let metrics = reader.trial_metrics(descriptor.id)?;
        trials.push(InputTrial {
            id: descriptor.id,
            terminal: result.terminal_status,
            metrics,
        });
    }
    Ok(ComparisonInput {
        identity,
        resolved,
        environment,
        trials,
    })
}

/// Load a candidate comparison input from a bundle path.
///
/// # Errors
/// Returns [`ComparisonError`] on open, verification, or evidence-read
/// failures.
pub fn load_candidate_bundle(path: &Path) -> Result<ComparisonInput, ComparisonError> {
    let reader = BundleReader::open(path)?;
    load_comparison_input(&reader)
}

/// Load a baseline input plus its typed reference from a bundle path.
///
/// # Errors
/// Returns [`ComparisonError`] on open, verification, or evidence-read
/// failures.
pub fn load_baseline_bundle(
    path: &Path,
) -> Result<(BaselineReference, ComparisonInput), ComparisonError> {
    let reader = BundleReader::open(path)?;
    let input = load_comparison_input(&reader)?;
    let reference = BaselineReference::Bundle {
        identity: input.identity.clone(),
        path: path.display().to_string(),
    };
    Ok((reference, input))
}

/// Resolve a human-managed baseline alias file and load its bundle.
///
/// Relative bundle paths resolve relative to the alias file. The referenced
/// bundle is verified and its manifest digest must equal the alias digest;
/// mismatch fails closed.
///
/// # Errors
/// Returns [`ComparisonError::Alias`] on alias read/parse/digest failures and
/// [`ComparisonError::Bundle`] on bundle failures.
pub fn load_baseline_alias(
    alias_file: &Path,
) -> Result<(BaselineReference, ComparisonInput), ComparisonError> {
    let raw = std::fs::read(alias_file).map_err(|error| ComparisonError::Alias {
        category: "baseline_alias_unreadable",
        detail: format!(
            "could not read alias file {}: {error}",
            alias_file.display()
        ),
    })?;
    if raw.len() > 64 * 1024 {
        return Err(ComparisonError::Alias {
            category: "baseline_alias_invalid",
            detail: "alias file exceeds 64 KiB".to_owned(),
        });
    }
    let alias: BaselineAliasFile =
        serde_json::from_slice(&raw).map_err(|error| ComparisonError::Alias {
            category: "baseline_alias_invalid",
            detail: format!("alias file is not a valid baseline alias: {error}"),
        })?;
    if alias.schema_version != BASELINE_ALIAS_SCHEMA_VERSION {
        return Err(ComparisonError::Alias {
            category: "baseline_alias_unsupported",
            detail: format!("unsupported alias schema version {}", alias.schema_version),
        });
    }
    if alias.alias.trim().is_empty() || alias.bundle_path.trim().is_empty() {
        return Err(ComparisonError::Alias {
            category: "baseline_alias_invalid",
            detail: "alias name and bundle path must be non-empty".to_owned(),
        });
    }
    let bundle_path = PathBuf::from(&alias.bundle_path);
    let resolved_path = if bundle_path.is_absolute() {
        bundle_path
    } else {
        alias_file
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(bundle_path)
    };
    let reader = BundleReader::open(&resolved_path)?;
    let input = load_comparison_input(&reader)?;
    if input.identity.manifest_sha256 != alias.manifest_sha256.to_ascii_lowercase() {
        return Err(ComparisonError::Alias {
            category: "baseline_alias_digest_mismatch",
            detail: format!(
                "alias {} digest does not match bundle {}",
                alias.alias,
                resolved_path.display()
            ),
        });
    }
    let reference = BaselineReference::Alias {
        alias: alias.alias.clone(),
        alias_file: alias_file.display().to_string(),
        resolved_identity: input.identity.clone(),
        resolved_path: resolved_path.display().to_string(),
    };
    Ok((reference, input))
}

/// Compare one candidate against an optional baseline under policy v1.
///
/// Never modifies either bundle. Deterministic for fixed inputs, policy, and
/// seed: metric iteration is name-ordered and resampling uses a documented
/// deterministic RNG.
///
/// # Panics
/// Panics only on internal policy invariants (a declared relative gate
/// without a threshold, or a metric name lost between collection and
/// evaluation); these indicate a programming defect, never input data.
#[allow(clippy::too_many_lines)] // One auditable policy pass over all metrics.
#[must_use]
pub fn compare(request: &ComparisonRequest<'_>, options: &ComparisonOptions) -> ComparisonReceipt {
    let candidate = request.candidate;
    let base_seed = match options.seed {
        Some(seed) => seed,
        None => derive_base_seed(candidate, request.baseline.as_ref().map(|side| side.input)),
    };
    let comparability = match request.baseline.as_ref().map(|side| side.input) {
        Some(baseline) => evaluate_comparability(candidate, baseline),
        None => ComparabilityReport {
            testbed: Vec::new(),
            workload_match: true,
            workload_detail: "absolute-only comparison uses no baseline".to_owned(),
            driver_match: true,
            driver_detail: "absolute-only comparison uses no baseline".to_owned(),
            topology_match: true,
            topology_detail: "absolute-only comparison uses no baseline".to_owned(),
            critical_mismatch: false,
        },
    };
    let mut warnings = Vec::new();
    for field in &comparability.testbed {
        if field.outcome == FieldOutcome::WarningMismatch {
            warnings.push(ComparisonWarning {
                category: "environment_warning_mismatch".to_owned(),
                detail: format!(
                    "warning-only environment field {} differs (candidate {:?} vs baseline {:?})",
                    field.field, field.candidate, field.baseline
                ),
            });
        }
    }
    if comparability.critical_mismatch {
        warnings.push(ComparisonWarning {
            category: "environment_critical_mismatch".to_owned(),
            detail: "comparison-critical testbed/workload/driver/topology mismatch".to_owned(),
        });
    }
    let mut metric_names: BTreeSet<&Name> = BTreeSet::new();
    for metric in &candidate.resolved.metrics {
        metric_names.insert(&metric.name);
    }
    let mut metrics = Vec::with_capacity(metric_names.len().min(MAX_RECEIPT_METRICS));
    for name in metric_names {
        let request_metric = candidate
            .resolved
            .metrics
            .iter()
            .find(|metric| &metric.name == name)
            .expect("metric name from candidate plan");
        let effective_seed = derive_metric_seed(base_seed, name.as_str());
        metrics.push(evaluate_metric(
            request_metric,
            candidate,
            request.baseline.as_ref().map(|side| side.input),
            candidate.resolved.environment_policy,
            &comparability,
            effective_seed,
            &mut warnings,
        ));
        if metrics.len() >= MAX_RECEIPT_METRICS {
            break;
        }
    }
    let aggregate_verdict = aggregate(&metrics);
    ComparisonReceipt {
        schema_version: COMPARISON_RECEIPT_SCHEMA_VERSION,
        policy_id: COMPARISON_POLICY_V1.to_owned(),
        created_by_version: env!("CARGO_PKG_VERSION").to_owned(),
        candidate_identity: candidate.identity.clone(),
        baseline_reference: request.baseline.as_ref().map(|side| side.reference.clone()),
        baseline_identity: request
            .baseline
            .as_ref()
            .map(|side| side.input.identity.clone()),
        environment_policy: candidate.resolved.environment_policy,
        comparability,
        base_seed,
        metrics,
        aggregate_verdict,
        warnings,
    }
}

// ---- Comparability ----

/// Evaluate typed comparability between candidate and baseline inputs.
fn evaluate_comparability(
    candidate: &ComparisonInput,
    baseline: &ComparisonInput,
) -> ComparabilityReport {
    let mut testbed = Vec::new();
    let mut critical_mismatch = false;
    let mut fields: BTreeSet<&Name> = BTreeSet::new();
    for name in candidate.environment.fields.keys() {
        fields.insert(name);
    }
    for name in baseline.environment.fields.keys() {
        fields.insert(name);
    }
    for field in fields {
        let candidate_field = candidate.environment.fields.get(field);
        let baseline_field = baseline.environment.fields.get(field);
        let class = candidate_field
            .map(|entry| entry.class)
            .or_else(|| baseline_field.map(|entry| entry.class));
        let outcome = match (candidate_field, baseline_field) {
            (Some(left), Some(right)) if left.value == right.value => FieldOutcome::Equal,
            (Some(_), Some(_)) => match class {
                Some(EnvironmentFieldClass::WarningOnly) => FieldOutcome::WarningMismatch,
                Some(EnvironmentFieldClass::Informational) => continue,
                _ => {
                    critical_mismatch = true;
                    FieldOutcome::Unequal
                }
            },
            (None, Some(_)) => match class {
                Some(EnvironmentFieldClass::WarningOnly) => FieldOutcome::WarningMismatch,
                Some(EnvironmentFieldClass::Informational) => continue,
                _ => {
                    critical_mismatch = true;
                    FieldOutcome::MissingCandidate
                }
            },
            (Some(_), None) => match class {
                Some(EnvironmentFieldClass::WarningOnly) => FieldOutcome::WarningMismatch,
                Some(EnvironmentFieldClass::Informational) => continue,
                _ => {
                    critical_mismatch = true;
                    FieldOutcome::MissingBaseline
                }
            },
            (None, None) => continue,
        };
        testbed.push(FieldComparison {
            field: field.as_str().to_owned(),
            candidate: candidate_field.map(|entry| entry.value.clone()),
            baseline: baseline_field.map(|entry| entry.value.clone()),
            outcome,
        });
    }
    let (workload_match, workload_detail) =
        compare_workload(&candidate.resolved, &baseline.resolved);
    let (driver_match, driver_detail) = compare_driver(&candidate.resolved, &baseline.resolved);
    let (topology_match, topology_detail) =
        compare_topology(&candidate.resolved, &baseline.resolved);
    if !workload_match || !driver_match || !topology_match {
        critical_mismatch = true;
    }
    ComparabilityReport {
        testbed,
        workload_match,
        workload_detail,
        driver_match,
        driver_detail,
        topology_match,
        topology_detail,
        critical_mismatch,
    }
}
/// Canonical workload semantics for comparison. Trial counts are excluded.
fn workload_summary(workload: &Workload) -> String {
    match workload {
        Workload::ClosedLoop {
            target,
            concurrency,
            requests,
            duration_ms,
        } => format!(
            "closed_loop target={target} concurrency={} termination={}",
            concurrency.get(),
            termination_summary(
                requests.map(crate::PositiveCount::get),
                duration_ms.map(crate::DurationMs::get)
            ),
        ),
        Workload::OpenLoop {
            target,
            rate_milli_rps,
            requests,
            duration_ms,
        } => format!(
            "open_loop target={target} rate_milli_rps={} termination={}",
            rate_milli_rps.get(),
            termination_summary(
                requests.map(crate::PositiveCount::get),
                duration_ms.map(crate::DurationMs::get)
            ),
        ),
        Workload::FiniteCount {
            target,
            requests,
            concurrency,
        } => format!(
            "finite_count target={target} requests={} concurrency={}",
            requests.get(),
            concurrency.get()
        ),
        Workload::TimeBounded {
            target,
            duration_ms,
            mode,
            concurrency,
            rate_milli_rps,
        } => format!(
            "time_bounded target={target} duration_ms={} mode={mode:?} concurrency={:?} rate={:?}",
            duration_ms.get(),
            concurrency.map(crate::PositiveCount::get),
            rate_milli_rps.map(crate::RateMilliRps::get),
        ),
    }
}

fn termination_summary(requests: Option<u32>, duration_ms: Option<u64>) -> String {
    match (requests, duration_ms) {
        (Some(count), None) => format!("requests={count}"),
        (None, Some(ms)) => format!("duration_ms={ms}"),
        other => format!("invalid-termination={other:?}"),
    }
}

fn compare_workload(candidate: &ResolvedPlan, baseline: &ResolvedPlan) -> (bool, String) {
    let left = workload_summary(&candidate.workload);
    let right = workload_summary(&baseline.workload);
    if left == right {
        (true, format!("workload semantics match ({left})"))
    } else {
        (
            false,
            format!("workload semantics differ: candidate {left} vs baseline {right}"),
        )
    }
}

fn compare_driver(candidate: &ResolvedPlan, baseline: &ResolvedPlan) -> (bool, String) {
    let left = candidate.drivers.get(&DriverCategory::Workload);
    let right = baseline.drivers.get(&DriverCategory::Workload);
    match (left, right) {
        (None, None) => (true, "no workload driver on either side".to_owned()),
        (Some(_), None) | (None, Some(_)) => {
            (false, "workload driver present on one side only".to_owned())
        }
        (Some(left), Some(right)) => {
            let summary = |descriptor: &crate::DriverDescriptor| {
                let mut capabilities: Vec<String> = descriptor
                    .capabilities
                    .iter()
                    .map(|c| format!("{c:?}"))
                    .collect();
                capabilities.sort();
                format!(
                    "name={} adapter={} upstream={}:{:?} capabilities=[{}]",
                    descriptor.name,
                    descriptor.adapter_version,
                    descriptor.upstream_name,
                    descriptor.upstream_version,
                    capabilities.join(","),
                )
            };
            let left_summary = summary(&left.descriptor);
            let right_summary = summary(&right.descriptor);
            if left_summary == right_summary {
                (
                    true,
                    format!("workload driver semantics match ({left_summary})"),
                )
            } else {
                (
                    false,
                    format!(
                        "workload driver differs: candidate {left_summary} vs baseline {right_summary}"
                    ),
                )
            }
        }
    }
}

fn compare_topology(candidate: &ResolvedPlan, baseline: &ResolvedPlan) -> (bool, String) {
    if candidate.topology != baseline.topology {
        return (
            false,
            format!(
                "service topology differs ({} vs {} services)",
                candidate.topology.len(),
                baseline.topology.len()
            ),
        );
    }
    let left_subject = subject_summary(&candidate.subject);
    let right_subject = subject_summary(&baseline.subject);
    if left_subject == right_subject {
        (true, "service topology and subject kind match".to_owned())
    } else {
        (
            false,
            format!("subject kind differs: candidate {left_subject} vs baseline {right_subject}"),
        )
    }
}

/// Subject kind summary excluding revision/digest identity, which is expected
/// to differ between candidate and baseline.
fn subject_summary(subject: &Subject) -> String {
    match subject {
        Subject::ManagedCommand { .. } => "managed_command".to_owned(),
        Subject::External { target, .. } => format!("external target={target}"),
        Subject::Label { label } => format!("label {label}"),
    }
}

fn subject_revision(subject: &Subject) -> Option<String> {
    match subject {
        Subject::External { revision, .. } => revision.clone(),
        Subject::ManagedCommand { .. } | Subject::Label { .. } => None,
    }
}

fn subject_digest(subject: &Subject) -> Option<String> {
    match subject {
        Subject::External { digest, .. } => digest.clone(),
        Subject::ManagedCommand { .. } | Subject::Label { .. } => None,
    }
}
/// Requirement for baseline-dependent gating: which baseline-use mode applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BaselineUse {
    /// Same-testbed gate verdicts are permitted.
    Gated,
    /// Estimates only; disposition becomes descriptive or invalid.
    DescriptiveOnly,
}

fn baseline_use(policy: EnvironmentPolicy, comparability: &ComparabilityReport) -> BaselineUse {
    if policy == EnvironmentPolicy::CrossTestbedDescriptive {
        return BaselineUse::DescriptiveOnly;
    }
    if comparability.critical_mismatch {
        return BaselineUse::DescriptiveOnly;
    }
    BaselineUse::Gated
}

/// Evaluate one candidate metric request.
#[allow(clippy::too_many_arguments)] // One policy bundle per metric.
#[allow(clippy::too_many_lines)] // Gate-type dispatch stays in one auditable place.
fn evaluate_metric(
    request: &MetricRequest,
    candidate: &ComparisonInput,
    baseline: Option<&ComparisonInput>,
    policy: EnvironmentPolicy,
    comparability: &ComparabilityReport,
    effective_seed: u64,
    warnings: &mut Vec<ComparisonWarning>,
) -> MetricComparison {
    let (candidate_values, candidate_included, candidate_excluded) =
        select_trials(candidate, &request.name);
    let candidate_estimate = arithmetic_mean(&candidate_values);
    let threshold = request.gate.as_ref().and_then(allowance_fraction);
    let mut record = MetricComparison {
        name: request.name.clone(),
        unit: request.unit.clone(),
        direction: request.direction.clone(),
        intent: request.intent,
        gate: request.gate.clone(),
        candidate_included,
        candidate_excluded,
        baseline_included: Vec::new(),
        baseline_excluded: Vec::new(),
        candidate_estimate,
        baseline_estimate: None,
        degradation: None,
        confidence_low: None,
        confidence_high: None,
        threshold,
        statistical_method: None,
        resamples: None,
        effective_seed: None,
        disposition: None,
        reason: None,
    };
    let Some(gate) = &request.gate else {
        // Ungated metrics are descriptive by construction.
        return record;
    };
    if request.intent != MetricIntent::Primary {
        // Plan validation forbids gated diagnostics; stay fail-closed anyway.
        record.disposition = Some(GateDisposition::Invalid);
        record.reason = Some("diagnostic_metric_cannot_gate".to_owned());
        return record;
    }
    match gate {
        Gate::Absolute { value } => {
            evaluate_absolute(&mut record, &candidate_values, &request.direction, *value);
        }
        Gate::RelativeRegression { .. } | Gate::StatisticalRelative { .. } => {
            evaluate_relative(
                &mut record,
                request,
                &candidate_values,
                baseline,
                policy,
                comparability,
                effective_seed,
                warnings,
            );
        }
    }
    record
}
#[allow(clippy::too_many_arguments)] // One policy bundle per relative gate.
#[allow(clippy::too_many_lines)] // Sequential gate preconditions stay auditable.
fn evaluate_relative(
    record: &mut MetricComparison,
    request: &MetricRequest,
    candidate_values: &[f64],
    baseline: Option<&ComparisonInput>,
    policy: EnvironmentPolicy,
    comparability: &ComparabilityReport,
    effective_seed: u64,
    warnings: &mut Vec<ComparisonWarning>,
) {
    let use_mode = baseline_use(policy, comparability);
    let Some(baseline_input) = baseline else {
        record.disposition = Some(GateDisposition::Invalid);
        record.reason = Some("baseline_required".to_owned());
        return;
    };
    let (baseline_values, included, excluded) = select_trials(baseline_input, &request.name);
    record.baseline_included = included;
    record.baseline_excluded = excluded;
    if let Err(reason) = check_metric_semantics(request, baseline_input) {
        record.disposition = Some(GateDisposition::Invalid);
        record.reason = Some(reason);
        return;
    }
    if !all_strictly_positive(candidate_values) || !all_strictly_positive(&baseline_values) {
        record.disposition = Some(GateDisposition::Invalid);
        record.reason = Some("nonpositive_relative_value".to_owned());
        return;
    }
    if !matches!(
        request.direction,
        MetricDirection::HigherIsBetter | MetricDirection::LowerIsBetter
    ) {
        record.disposition = Some(GateDisposition::Invalid);
        record.reason = Some("unsupported_direction_for_relative_gate".to_owned());
        return;
    }
    let candidate_geomean = geometric_mean(candidate_values);
    let baseline_geomean = geometric_mean(&baseline_values);
    record.baseline_estimate = baseline_geomean;
    let degradation = oriented_degradation(candidate_geomean, baseline_geomean, &request.direction);
    record.degradation = degradation;
    if use_mode == BaselineUse::DescriptiveOnly {
        // Descriptive cross-testbed or mismatched evidence never masquerades
        // as a pass. Strict-mode mismatch additionally marks the primary
        // gate invalid per policy.
        if policy == EnvironmentPolicy::StrictSameTestbed {
            record.disposition = Some(GateDisposition::Invalid);
            record.reason = Some("comparability_mismatch".to_owned());
        } else {
            record.disposition = Some(GateDisposition::Descriptive);
            record.reason = Some(descriptive_reason(policy));
        }
        return;
    }
    match &request.gate {
        Some(Gate::RelativeRegression { .. }) => {
            let threshold = record.threshold.expect("relative gate has threshold");
            let degradation = degradation.expect("positive inputs yield degradation");
            record.disposition = Some(if degradation <= threshold {
                GateDisposition::Pass
            } else {
                GateDisposition::Fail
            });
        }
        Some(Gate::StatisticalRelative { min_trials, .. }) => {
            let required = min_observations(min_trials.get());
            if candidate_values.len() < required || baseline_values.len() < required {
                record.disposition = Some(GateDisposition::Invalid);
                record.reason = Some("insufficient_trials".to_owned());
                return;
            }
            if candidate_values.len() < RECOMMENDED_OBSERVATIONS_PER_SIDE
                || baseline_values.len() < RECOMMENDED_OBSERVATIONS_PER_SIDE
            {
                warnings.push(ComparisonWarning {
                    category: "below_recommended_trial_count".to_owned(),
                    detail: format!(
                        "metric {} has {} candidate and {} baseline trials; 7+ per side recommended",
                        request.name,
                        candidate_values.len(),
                        baseline_values.len(),
                    ),
                });
            }
            let threshold = record.threshold.expect("statistical gate has threshold");
            let (low, high) = bootstrap_interval(
                candidate_values,
                &baseline_values,
                &request.direction,
                BOOTSTRAP_RESAMPLES,
                effective_seed,
            );
            record.confidence_low = Some(low);
            record.confidence_high = Some(high);
            record.statistical_method = Some(STATISTICAL_METHOD_V1.to_owned());
            record.resamples = Some(BOOTSTRAP_RESAMPLES);
            record.effective_seed = Some(effective_seed);
            record.disposition = Some(if low > threshold {
                GateDisposition::Fail
            } else if high <= threshold {
                GateDisposition::Pass
            } else {
                GateDisposition::Inconclusive
            });
        }
        _ => {}
    }
}

fn descriptive_reason(policy: EnvironmentPolicy) -> String {
    match policy {
        EnvironmentPolicy::CrossTestbedDescriptive => "cross_testbed_descriptive".to_owned(),
        EnvironmentPolicy::WarnOnMismatch => "comparability_mismatch_descriptive".to_owned(),
        EnvironmentPolicy::StrictSameTestbed => "comparability_mismatch".to_owned(),
    }
}

fn evaluate_absolute(
    record: &mut MetricComparison,
    candidate_values: &[f64],
    direction: &MetricDirection,
    value: f64,
) {
    let Some(estimate) = record.candidate_estimate else {
        record.disposition = Some(GateDisposition::Invalid);
        record.reason = Some("no_valid_candidate_observations".to_owned());
        return;
    };
    if candidate_values.iter().any(|v| !v.is_finite()) {
        record.disposition = Some(GateDisposition::Invalid);
        record.reason = Some("nonfinite_candidate_value".to_owned());
        return;
    }
    let pass = match direction {
        MetricDirection::LowerIsBetter => estimate <= value,
        MetricDirection::HigherIsBetter => estimate >= value,
        MetricDirection::TargetRange { .. } => {
            // Plan schema v1 supplies one scalar threshold while the
            // direction owns two bounds; do not reinterpret silently.
            record.disposition = Some(GateDisposition::Invalid);
            record.reason = Some("target_range_absolute_unsupported".to_owned());
            return;
        }
        MetricDirection::Informational => {
            record.disposition = Some(GateDisposition::Invalid);
            record.reason = Some("informational_metric_cannot_gate".to_owned());
            return;
        }
    };
    record.disposition = Some(if pass {
        GateDisposition::Pass
    } else {
        GateDisposition::Fail
    });
}

/// Check baseline metric semantics agree with the candidate request.
fn check_metric_semantics(
    request: &MetricRequest,
    baseline: &ComparisonInput,
) -> Result<(), String> {
    for trial in &baseline.trials {
        let Some(metrics) = &trial.metrics else {
            continue;
        };
        let Some(observation) = metrics
            .observations
            .iter()
            .find(|observation| observation.name == request.name)
        else {
            continue;
        };
        if observation.unit != request.unit || observation.direction != request.direction {
            return Err("metric_semantics_mismatch".to_owned());
        }
        return Ok(());
    }
    // No baseline trial carries this metric at all.
    Err("baseline_metric_absent".to_owned())
}
/// Select valid trial-level scalars for one metric.
///
/// One trial contributes at most one scalar. Missing and invalid
/// observations are counted with reasons, never imputed.
fn select_trials(
    input: &ComparisonInput,
    metric: &Name,
) -> (Vec<f64>, Vec<u32>, Vec<ExcludedTrial>) {
    let mut values = Vec::new();
    let mut included = Vec::new();
    let mut excluded = Vec::new();
    for trial in &input.trials {
        let trial_number = trial.id.get();
        let Some(metrics) = &trial.metrics else {
            excluded.push(ExcludedTrial {
                trial_id: trial_number,
                reason: "no_normalized_metrics".to_owned(),
            });
            continue;
        };
        let Some(observation) = metrics
            .observations
            .iter()
            .find(|observation| &observation.name == metric)
        else {
            excluded.push(ExcludedTrial {
                trial_id: trial_number,
                reason: "metric_not_requested".to_owned(),
            });
            continue;
        };
        if trial.terminal != TrialExecutionStatus::Completed {
            excluded.push(ExcludedTrial {
                trial_id: trial_number,
                reason: "trial_not_completed".to_owned(),
            });
            continue;
        }
        match &observation.state {
            ObservationState::Observed { value } => {
                values.push(*value);
                included.push(trial_number);
            }
            ObservationState::Missing { reason } => excluded.push(ExcludedTrial {
                trial_id: trial_number,
                reason: missing_reason_str(*reason),
            }),
            ObservationState::Invalid { reason, .. } => excluded.push(ExcludedTrial {
                trial_id: trial_number,
                reason: invalid_reason_str(*reason),
            }),
        }
    }
    (values, included, excluded)
}

fn missing_reason_str(reason: crate::MissingReason) -> String {
    match reason {
        crate::MissingReason::SourceNotProvided => "source_not_provided".to_owned(),
        crate::MissingReason::UnsupportedByDriver => "unsupported_by_driver".to_owned(),
        crate::MissingReason::TrialNotCompleted => "trial_not_completed".to_owned(),
    }
}

fn invalid_reason_str(reason: crate::InvalidReason) -> String {
    match reason {
        crate::InvalidReason::NonFinite => "non_finite".to_owned(),
        crate::InvalidReason::UnitMismatch => "unit_mismatch".to_owned(),
        crate::InvalidReason::DuplicateObservation => "duplicate_observation".to_owned(),
        crate::InvalidReason::AggregationMismatch => "aggregation_mismatch".to_owned(),
        crate::InvalidReason::MalformedSourceReference => "malformed_source_reference".to_owned(),
        crate::InvalidReason::DomainError => "domain_error".to_owned(),
    }
}

/// Arithmetic mean of valid trial observations, or `None` when empty.
///
/// Trial counts are bounded by `PositiveCount` (≤ 1M), far below the f64
/// integer-exact range, so the length conversion cannot lose precision.
#[allow(clippy::cast_precision_loss)]
fn arithmetic_mean(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let sum: f64 = values.iter().sum();
    let count = values.len() as f64;
    let mean = sum / count;
    mean.is_finite().then_some(mean)
}

/// Geometric mean via mean of logs. Returns `None` for empty or
/// non-positive/domain-invalid inputs.
///
/// Trial counts are bounded by `PositiveCount` (≤ 1M); see
/// [`arithmetic_mean`] for the precision argument.
#[allow(clippy::cast_precision_loss)]
fn geometric_mean(values: &[f64]) -> Option<f64> {
    if values.is_empty() || !all_strictly_positive(values) {
        return None;
    }
    let log_sum: f64 = values.iter().map(|v| v.ln()).sum();
    let count = values.len() as f64;
    let geomean = (log_sum / count).exp();
    geomean.is_finite().then_some(geomean)
}

fn all_strictly_positive(values: &[f64]) -> bool {
    !values.is_empty() && values.iter().all(|v| v.is_finite() && *v > 0.0)
}

/// Oriented degradation as a fraction: positive means the candidate is worse
/// than the baseline under the declared direction.
fn oriented_degradation(
    candidate_geomean: Option<f64>,
    baseline_geomean: Option<f64>,
    direction: &MetricDirection,
) -> Option<f64> {
    let (Some(candidate), Some(baseline)) = (candidate_geomean, baseline_geomean) else {
        return None;
    };
    if !candidate.is_finite() || !baseline.is_finite() {
        return None;
    }
    let degradation = match direction {
        MetricDirection::LowerIsBetter => candidate / baseline - 1.0,
        MetricDirection::HigherIsBetter => baseline / candidate - 1.0,
        MetricDirection::TargetRange { .. } | MetricDirection::Informational => return None,
    };
    degradation.is_finite().then_some(degradation)
}

/// Allowance in basis points as a threshold fraction. Absolute gates carry
/// no allowance; the threshold field stays `None` for them.
fn allowance_fraction(gate: &Gate) -> Option<f64> {
    match gate {
        Gate::Absolute { .. } => None,
        Gate::RelativeRegression { allowance } | Gate::StatisticalRelative { allowance, .. } => {
            Some(f64::from(allowance.get()) / 10_000.0)
        }
    }
}

fn min_observations(plan_min_trials: u32) -> usize {
    MIN_OBSERVATIONS_PER_SIDE.max(plan_min_trials as usize)
}
/// Deterministic 95% percentile bootstrap interval over oriented degradation.
///
/// Unpaired only: each resample draws `candidate.len()` candidate values and
/// `baseline.len()` baseline values with replacement, computes each side's
/// mean log value, derives oriented degradation in log space, and transforms
/// to degradation space.
///
/// Quantile indexing is exact and documented using integer arithmetic: for
/// `n` sorted resamples, the lower bound is index `(25 * n) / 1000` and the
/// upper bound is index `((975 * n + 999) / 1000) - 1`, clamped into range.
/// For `n = 10_000` these are indices 250 and 9749.
fn bootstrap_interval(
    candidate: &[f64],
    baseline: &[f64],
    direction: &MetricDirection,
    resamples: usize,
    seed: u64,
) -> (f64, f64) {
    debug_assert!(!candidate.is_empty() && !baseline.is_empty());
    debug_assert!(all_strictly_positive(candidate) && all_strictly_positive(baseline));
    let candidate_logs: Vec<f64> = candidate.iter().map(|v| v.ln()).collect();
    let baseline_logs: Vec<f64> = baseline.iter().map(|v| v.ln()).collect();
    let mut rng = SplitMix64::new(seed);
    let mut effects = Vec::with_capacity(resamples);
    for _ in 0..resamples {
        let candidate_mean = resampled_mean_log(&candidate_logs, &mut rng);
        let baseline_mean = resampled_mean_log(&baseline_logs, &mut rng);
        let log_effect = match direction {
            MetricDirection::LowerIsBetter => candidate_mean - baseline_mean,
            MetricDirection::HigherIsBetter => baseline_mean - candidate_mean,
            MetricDirection::TargetRange { .. } | MetricDirection::Informational => 0.0,
        };
        effects.push(log_effect.exp() - 1.0);
    }
    effects.sort_by(f64::total_cmp);
    let count = effects.len();
    let low_index = (25_usize.saturating_mul(count) / 1000).min(count - 1);
    let high_index = (975_usize.saturating_mul(count).saturating_add(999) / 1000)
        .saturating_sub(1)
        .min(count - 1);
    (effects[low_index], effects[high_index])
}

/// Mean of resampled log values. Trial counts are bounded (see
/// [`arithmetic_mean`]), so the length conversion cannot lose precision.
#[allow(clippy::cast_precision_loss)]
fn resampled_mean_log(logs: &[f64], rng: &mut SplitMix64) -> f64 {
    let len = logs.len();
    let len_u64 = u64::try_from(len).unwrap_or(u64::MAX);
    let mut sum = 0.0;
    for _ in 0..len {
        let index = usize::try_from(rng.next() % len_u64).unwrap_or(usize::MAX);
        sum += logs[index.min(len - 1)];
    }
    sum / len as f64
}

/// Tiny documented deterministic RNG (`SplitMix64`) owned by policy v1, used
/// solely for bootstrap index generation.
struct SplitMix64(u64);

impl SplitMix64 {
    const fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        value ^ (value >> 31)
    }
}

/// FNV-1a 64-bit hash for deterministic seed derivation.
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01B3);
    }
    hash
}

/// Derive a deterministic base seed from bundle identities and policy.
fn derive_base_seed(candidate: &ComparisonInput, baseline: Option<&ComparisonInput>) -> u64 {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(candidate.identity.manifest_sha256.as_bytes());
    bytes.extend_from_slice(b"|");
    match baseline {
        Some(input) => bytes.extend_from_slice(input.identity.manifest_sha256.as_bytes()),
        None => bytes.extend_from_slice(b"absolute-only"),
    }
    bytes.extend_from_slice(b"|");
    bytes.extend_from_slice(COMPARISON_POLICY_V1.as_bytes());
    fnv1a64(&bytes)
}

/// Derive a stable per-metric seed from the base seed and metric name.
fn derive_metric_seed(base_seed: u64, metric: &str) -> u64 {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&base_seed.to_le_bytes());
    bytes.extend_from_slice(b"|");
    bytes.extend_from_slice(metric.as_bytes());
    fnv1a64(&bytes)
}

/// Conservative aggregate over gated primary metrics: any `Invalid` beats any
/// `Fail` beats any `Inconclusive`; all-pass yields `Pass`; no gate-eligible
/// primary verdict yields no aggregate. Descriptive and ungated metrics never
/// affect the aggregate.
fn aggregate(metrics: &[MetricComparison]) -> Option<AggregateVerdict> {
    let mut saw_pass = false;
    let mut saw_inconclusive = false;
    let mut saw_fail = false;
    let mut saw_invalid = false;
    for metric in metrics {
        if metric.intent != MetricIntent::Primary {
            continue;
        }
        match metric.disposition {
            Some(GateDisposition::Invalid) => saw_invalid = true,
            Some(GateDisposition::Fail) => saw_fail = true,
            Some(GateDisposition::Inconclusive) => saw_inconclusive = true,
            Some(GateDisposition::Pass) => saw_pass = true,
            Some(GateDisposition::Descriptive) | None => {}
        }
    }
    if saw_invalid {
        Some(AggregateVerdict::Invalid)
    } else if saw_fail {
        Some(AggregateVerdict::Fail)
    } else if saw_inconclusive {
        Some(AggregateVerdict::Inconclusive)
    } else if saw_pass {
        Some(AggregateVerdict::Pass)
    } else {
        None
    }
}
/// Upper bound for manifest bytes read during comparison input loading.
const MAX_COMPARISON_MANIFEST_BYTES: u64 = 8 * 1024 * 1024;

fn read_manifest_bytes(reader: &BundleReader) -> Result<Vec<u8>, ComparisonError> {
    let path = reader.root().join("manifest.json");
    let file = std::fs::File::open(&path).map_err(|error| BundleError::Io {
        path: path.clone(),
        source: error,
    })?;
    let mut bytes = Vec::new();
    file.take(MAX_COMPARISON_MANIFEST_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| BundleError::Io {
            path: path.clone(),
            source: error,
        })?;
    if bytes.len() as u64 > MAX_COMPARISON_MANIFEST_BYTES {
        return Err(BundleError::BoundExceeded("manifest bytes").into());
    }
    Ok(bytes)
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    let mut hasher = sha2::Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push(char::from_digit(u32::from(byte >> 4), 16).expect("hex digit"));
        out.push(char::from_digit(u32::from(byte & 0x0F), 16).expect("hex digit"));
    }
    out
}

fn load_role_json<T>(reader: &BundleReader, role: &ArtifactRole) -> Result<T, ComparisonError>
where
    T: serde::de::DeserializeOwned,
{
    let path = reader
        .manifest()
        .artifacts
        .iter()
        .find(|artifact| &artifact.role == role)
        .map(|artifact| artifact.path.clone())
        .ok_or(BundleError::InvalidManifest(
            "required bundle artifact is absent",
        ))?;
    let mut file = reader.open_artifact(&path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| BundleError::Io {
            path: path.to_path_buf(),
            source: error,
        })?;
    serde_json::from_slice(&bytes)
        .map_err(|error| BundleError::ManifestParse(error.to_string()).into())
}

fn load_trial_result(
    reader: &BundleReader,
    descriptor: &crate::TrialDescriptor,
) -> Result<TrialExecutionResult, ComparisonError> {
    let mut file = reader.open_artifact(&descriptor.result)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| BundleError::Io {
            path: descriptor.result.to_path_buf(),
            source: error,
        })?;
    serde_json::from_slice(&bytes)
        .map_err(|error| BundleError::ManifestParse(error.to_string()).into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Aggregation, ArtifactBounds, BasisPoints, EnvironmentField, NormalizedObservation,
        PositiveCount, ResetPolicy, TrialPolicy,
    };
    use std::collections::{BTreeMap, BTreeSet};

    const VOCAB_VERSION: u32 = crate::METRIC_VOCABULARY_VERSION;

    fn metric_name(value: &str) -> Name {
        Name::new(value).unwrap()
    }

    fn latency_request(gate: Gate) -> MetricRequest {
        MetricRequest {
            name: metric_name("latency_p99"),
            unit: metric_name("ms"),
            direction: MetricDirection::LowerIsBetter,
            intent: MetricIntent::Primary,
            gate: Some(gate),
        }
    }

    fn statistical_gate(allowance: u16) -> Gate {
        Gate::StatisticalRelative {
            allowance: BasisPoints::new(allowance).unwrap(),
            min_trials: PositiveCount::new(1).unwrap(),
        }
    }

    fn relative_gate(allowance: u16) -> Gate {
        Gate::RelativeRegression {
            allowance: BasisPoints::new(allowance).unwrap(),
        }
    }

    fn trial_metrics_for(id: u32, pairs: &[(&str, &str, f64)]) -> TrialMetrics {
        let observations = pairs
            .iter()
            .map(|(name, unit, value)| NormalizedObservation {
                name: metric_name(name),
                unit: metric_name(unit),
                direction: MetricDirection::LowerIsBetter,
                intent: MetricIntent::Primary,
                aggregation: Aggregation::Direct,
                state: ObservationState::Observed { value: *value },
                provenance: crate::MetricProvenance {
                    producer: metric_name("fake-load"),
                    producer_version: None,
                    source_field: Some("fake.value".to_owned()),
                    normalization: crate::NORMALIZATION_METHOD_V1.to_owned(),
                    raw_artifacts: Vec::new(),
                },
            })
            .collect();
        TrialMetrics {
            schema_version: crate::TRIAL_METRICS_SCHEMA_VERSION,
            vocabulary_version: VOCAB_VERSION,
            trial_id: TrialId::new(id).unwrap(),
            observations,
            histograms: Vec::new(),
            error_distribution: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn resolved_with(metrics: Vec<MetricRequest>, policy: EnvironmentPolicy) -> ResolvedPlan {
        let mut drivers = BTreeMap::new();
        drivers.insert(
            DriverCategory::Workload,
            crate::ResolvedDriver {
                descriptor: crate::DriverDescriptor {
                    name: metric_name("fake-load"),
                    adapter_version: "0.1.0".to_owned(),
                    upstream_name: "fake-load".to_owned(),
                    upstream_version: None,
                    category: DriverCategory::Workload,
                    capabilities: [crate::Capability::LoadMode {
                        mode: crate::LoadMode::ClosedLoop,
                    }]
                    .into_iter()
                    .collect(),
                    supported_platforms: BTreeSet::new(),
                    machine_output_schema: None,
                    external_process: false,
                    default: true,
                    compatible_service_types: BTreeSet::new(),
                },
                executable_path: None,
            },
        );
        ResolvedPlan {
            schema_version: crate::RESOLVED_PLAN_SCHEMA_VERSION,
            source_plan_schema_version: crate::EXPERIMENT_PLAN_SCHEMA_VERSION,
            experiment: metric_name("smoke"),
            drivers,
            subject: Subject::Label {
                label: metric_name("smoke"),
            },
            topology: Vec::new(),
            workload: Workload::FiniteCount {
                target: metric_name("api"),
                requests: PositiveCount::new(100).unwrap(),
                concurrency: PositiveCount::new(5).unwrap(),
            },
            trials: TrialPolicy {
                measured: PositiveCount::new(7).unwrap(),
                warmup: 0,
                cooldown_ms: None,
                reset: ResetPolicy::None,
                timeouts: BTreeMap::new(),
            },
            telemetry: Vec::new(),
            defaults: crate::ResolvedDefaults {
                platform: metric_name("linux-x86_64"),
                warmup_trials: 0,
                measured_trials: 7,
            },
            environment_policy: policy,
            metrics,
            artifact_bounds: ArtifactBounds {
                artifact_count: PositiveCount::new(256).unwrap(),
                artifact_bytes: 1024,
                total_bytes: 4096,
            },
            seed: None,
            warnings: Vec::new(),
        }
    }

    fn environment_with(pairs: &[(&str, &str, EnvironmentFieldClass)]) -> EnvironmentFingerprint {
        let fields = pairs
            .iter()
            .map(|(name, value, class)| {
                (
                    metric_name(name),
                    EnvironmentField {
                        value: (*value).to_owned(),
                        class: *class,
                    },
                )
            })
            .collect();
        EnvironmentFingerprint::new(fields)
    }

    fn standard_environment() -> EnvironmentFingerprint {
        environment_with(&[
            (
                "os_family",
                "linux",
                EnvironmentFieldClass::ComparisonCritical,
            ),
            (
                "cpu_class",
                "x86_64",
                EnvironmentFieldClass::ComparisonCritical,
            ),
            ("kernel_release", "6.8", EnvironmentFieldClass::WarningOnly),
            ("hostname", "lab", EnvironmentFieldClass::Informational),
        ])
    }

    fn input_with_values(
        sha: &str,
        metric: MetricRequest,
        policy: EnvironmentPolicy,
        values: &[f64],
        environment: EnvironmentFingerprint,
    ) -> ComparisonInput {
        let trials = values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let id = u32::try_from(index + 1).unwrap();
                InputTrial {
                    id: TrialId::new(id).unwrap(),
                    terminal: TrialExecutionStatus::Completed,
                    metrics: Some(trial_metrics_for(id, &[("latency_p99", "ms", *value)])),
                }
            })
            .collect();
        ComparisonInput {
            identity: BundleIdentity {
                manifest_schema_version: SchemaVersion(2),
                run_id: RunId::parse("123e4567-e89b-12d3-a456-426614174000").unwrap(),
                manifest_sha256: sha.to_owned(),
                subject_revision: None,
                subject_digest: None,
            },
            resolved: resolved_with(vec![metric], policy),
            environment,
            trials,
        }
    }

    fn statistical_inputs(
        candidate_values: &[f64],
        baseline_values: &[f64],
    ) -> (ComparisonInput, ComparisonInput, MetricRequest) {
        let metric = latency_request(statistical_gate(500));
        let candidate = input_with_values(
            &"aa".repeat(32),
            metric.clone(),
            EnvironmentPolicy::StrictSameTestbed,
            candidate_values,
            standard_environment(),
        );
        let baseline = input_with_values(
            &"bb".repeat(32),
            metric.clone(),
            EnvironmentPolicy::StrictSameTestbed,
            baseline_values,
            standard_environment(),
        );
        (candidate, baseline, metric)
    }

    fn compare_pair(candidate: &ComparisonInput, baseline: &ComparisonInput) -> ComparisonReceipt {
        let request = ComparisonRequest {
            candidate,
            baseline: Some(BaselineSide {
                reference: BaselineReference::Bundle {
                    identity: baseline.identity.clone(),
                    path: "baseline.eggb".to_owned(),
                },
                input: baseline,
            }),
        };
        compare(&request, &ComparisonOptions::default())
    }

    fn disposition_of(receipt: &ComparisonReceipt) -> Option<GateDisposition> {
        receipt
            .metrics
            .first()
            .and_then(|metric| metric.disposition)
    }

    #[test]
    fn same_inputs_yield_byte_equivalent_receipts() {
        let (candidate, baseline, _) = statistical_inputs(&[101.0; 7], &[100.0; 7]);
        let first = compare_pair(&candidate, &baseline);
        let second = compare_pair(&candidate, &baseline);
        let first_json = serde_json::to_string_pretty(&first).unwrap();
        let second_json = serde_json::to_string_pretty(&second).unwrap();
        assert_eq!(first_json, second_json);
        assert_eq!(first.base_seed, second.base_seed);
        assert!(!first_json.contains("created_unix") && !first_json.contains("timestamp"));
    }

    #[test]
    fn derived_seed_is_stable_and_metric_scoped() {
        let (candidate, baseline, _) = statistical_inputs(&[101.0; 7], &[100.0; 7]);
        let first = compare_pair(&candidate, &baseline);
        let second = compare_pair(&candidate, &baseline);
        assert_eq!(first.base_seed, second.base_seed);
        let metric = &first.metrics[0];
        assert_eq!(metric.resamples, Some(BOOTSTRAP_RESAMPLES));
        assert!(metric.effective_seed.is_some());
    }

    #[test]
    fn clear_regression_fails_and_non_regression_passes() {
        let (candidate, baseline, _) = statistical_inputs(&[101.0; 7], &[100.0; 7]);
        let pass = compare_pair(&candidate, &baseline);
        assert_eq!(disposition_of(&pass), Some(GateDisposition::Pass));
        assert_eq!(pass.aggregate_verdict, Some(AggregateVerdict::Pass));

        let (candidate, baseline, _) = statistical_inputs(&[130.0; 7], &[100.0; 7]);
        let fail = compare_pair(&candidate, &baseline);
        assert_eq!(disposition_of(&fail), Some(GateDisposition::Fail));
        assert_eq!(fail.aggregate_verdict, Some(AggregateVerdict::Fail));
        let degradation = fail.metrics[0].degradation.unwrap();
        assert!(
            (degradation - 0.30).abs() < 1e-9,
            "degradation={degradation}"
        );
    }

    #[test]
    fn threshold_crossing_is_inconclusive() {
        let (candidate, baseline, _) =
            statistical_inputs(&[82.0, 118.0, 85.0, 125.0, 90.0, 122.0, 105.0], &[100.0; 7]);
        let receipt = compare_pair(&candidate, &baseline);
        let metric = &receipt.metrics[0];
        assert_eq!(metric.disposition, Some(GateDisposition::Inconclusive));
        let (low, high) = (
            metric.confidence_low.unwrap(),
            metric.confidence_high.unwrap(),
        );
        assert!(low <= 0.05 && high > 0.05, "interval=[{low},{high}]");
    }

    #[test]
    fn insufficient_trials_are_invalid() {
        let (candidate, baseline, _) = statistical_inputs(&[101.0; 3], &[100.0; 3]);
        let receipt = compare_pair(&candidate, &baseline);
        assert_eq!(disposition_of(&receipt), Some(GateDisposition::Invalid));
        assert_eq!(
            receipt.metrics[0].reason.as_deref(),
            Some("insufficient_trials")
        );
        assert_eq!(receipt.aggregate_verdict, Some(AggregateVerdict::Invalid));
    }

    #[test]
    fn zero_relative_values_are_invalid() {
        let (candidate, baseline, _) = statistical_inputs(
            &[0.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0],
            &[100.0; 7],
        );
        let receipt = compare_pair(&candidate, &baseline);
        assert_eq!(disposition_of(&receipt), Some(GateDisposition::Invalid));
        assert_eq!(
            receipt.metrics[0].reason.as_deref(),
            Some("nonpositive_relative_value")
        );
    }

    #[test]
    fn absolute_gate_needs_no_baseline() {
        let metric = latency_request(Gate::Absolute { value: 500.0 });
        let candidate = input_with_values(
            &"aa".repeat(32),
            metric,
            EnvironmentPolicy::StrictSameTestbed,
            &[100.0; 3],
            standard_environment(),
        );
        let request = ComparisonRequest {
            candidate: &candidate,
            baseline: None,
        };
        let receipt = compare(&request, &ComparisonOptions::default());
        assert_eq!(disposition_of(&receipt), Some(GateDisposition::Pass));
        assert!(receipt.baseline_identity.is_none());
        assert_eq!(receipt.aggregate_verdict, Some(AggregateVerdict::Pass));
    }

    #[test]
    fn relative_gate_without_baseline_is_invalid() {
        let metric = latency_request(relative_gate(500));
        let candidate = input_with_values(
            &"aa".repeat(32),
            metric,
            EnvironmentPolicy::StrictSameTestbed,
            &[100.0; 7],
            standard_environment(),
        );
        let request = ComparisonRequest {
            candidate: &candidate,
            baseline: None,
        };
        let receipt = compare(&request, &ComparisonOptions::default());
        assert_eq!(disposition_of(&receipt), Some(GateDisposition::Invalid));
        assert_eq!(
            receipt.metrics[0].reason.as_deref(),
            Some("baseline_required")
        );
    }

    #[test]
    fn target_range_absolute_is_unsupported() {
        let mut metric = latency_request(Gate::Absolute { value: 1.0 });
        metric.direction = MetricDirection::TargetRange { min: 0.0, max: 2.0 };
        let candidate = input_with_values(
            &"aa".repeat(32),
            metric,
            EnvironmentPolicy::StrictSameTestbed,
            &[1.0; 3],
            standard_environment(),
        );
        let request = ComparisonRequest {
            candidate: &candidate,
            baseline: None,
        };
        let receipt = compare(&request, &ComparisonOptions::default());
        assert_eq!(disposition_of(&receipt), Some(GateDisposition::Invalid));
        assert_eq!(
            receipt.metrics[0].reason.as_deref(),
            Some("target_range_absolute_unsupported")
        );
    }

    #[test]
    fn worsening_candidate_cannot_improve_degradation() {
        let (_, baseline, _) = statistical_inputs(&[100.0; 7], &[100.0; 7]);
        let mut previous = f64::NEG_INFINITY;
        for value in [90.0, 100.0, 105.0, 120.0, 200.0] {
            let (candidate, _, _) = statistical_inputs(&[value; 7], &[100.0; 7]);
            let receipt = compare_pair(&candidate, &baseline);
            let degradation = receipt.metrics[0].degradation.unwrap();
            assert!(
                degradation >= previous,
                "degradation went down: {degradation} < {previous}"
            );
            previous = degradation;
        }
    }

    #[test]
    fn increasing_allowance_cannot_turn_pass_into_fail() {
        let (_, baseline, _) = statistical_inputs(&[100.0; 7], &[100.0; 7]);
        let mut seen_pass = false;
        for allowance in [0, 100, 500, 2000, 10_000] {
            let metric = latency_request(relative_gate(allowance));
            let candidate = input_with_values(
                &"aa".repeat(32),
                metric,
                EnvironmentPolicy::StrictSameTestbed,
                &[104.0; 7],
                standard_environment(),
            );
            let receipt = compare_pair(&candidate, &baseline);
            let disposition = disposition_of(&receipt).unwrap();
            if disposition == GateDisposition::Pass {
                seen_pass = true;
            } else if seen_pass {
                panic!("allowance {allowance} turned a pass into {disposition:?}");
            }
        }
        assert!(seen_pass);
    }

    #[test]
    fn warning_only_mismatch_warns_but_gates() {
        let (candidate, mut baseline, _) = statistical_inputs(&[101.0; 7], &[100.0; 7]);
        baseline.environment = environment_with(&[
            (
                "os_family",
                "linux",
                EnvironmentFieldClass::ComparisonCritical,
            ),
            (
                "cpu_class",
                "x86_64",
                EnvironmentFieldClass::ComparisonCritical,
            ),
            ("kernel_release", "6.9", EnvironmentFieldClass::WarningOnly),
            ("hostname", "lab", EnvironmentFieldClass::Informational),
        ]);
        let receipt = compare_pair(&candidate, &baseline);
        assert_eq!(disposition_of(&receipt), Some(GateDisposition::Pass));
        assert!(
            receipt
                .warnings
                .iter()
                .any(|warning| warning.category == "environment_warning_mismatch")
        );
    }

    #[test]
    fn critical_mismatch_invalidates_strict_gates() {
        let (candidate, mut baseline, _) = statistical_inputs(&[101.0; 7], &[100.0; 7]);
        baseline.environment = environment_with(&[
            (
                "os_family",
                "linux",
                EnvironmentFieldClass::ComparisonCritical,
            ),
            (
                "cpu_class",
                "aarch64",
                EnvironmentFieldClass::ComparisonCritical,
            ),
        ]);
        let receipt = compare_pair(&candidate, &baseline);
        assert!(receipt.comparability.critical_mismatch);
        assert_eq!(disposition_of(&receipt), Some(GateDisposition::Invalid));
        assert_eq!(
            receipt.metrics[0].reason.as_deref(),
            Some("comparability_mismatch")
        );
        assert!(receipt.metrics[0].degradation.is_some());
        assert_eq!(receipt.aggregate_verdict, Some(AggregateVerdict::Invalid));
    }

    #[test]
    fn warn_policy_suppresses_verdicts_to_descriptive() {
        let metric = latency_request(relative_gate(500));
        let candidate = input_with_values(
            &"aa".repeat(32),
            metric.clone(),
            EnvironmentPolicy::WarnOnMismatch,
            &[101.0; 7],
            standard_environment(),
        );
        let baseline = input_with_values(
            &"bb".repeat(32),
            metric,
            EnvironmentPolicy::WarnOnMismatch,
            &[100.0; 7],
            environment_with(&[
                (
                    "os_family",
                    "freebsd",
                    EnvironmentFieldClass::ComparisonCritical,
                ),
                (
                    "cpu_class",
                    "x86_64",
                    EnvironmentFieldClass::ComparisonCritical,
                ),
            ]),
        );
        let receipt = compare_pair(&candidate, &baseline);
        assert_eq!(disposition_of(&receipt), Some(GateDisposition::Descriptive));
        assert_eq!(receipt.aggregate_verdict, None);

        let absolute = latency_request(Gate::Absolute { value: 500.0 });
        let candidate = input_with_values(
            &"aa".repeat(32),
            absolute,
            EnvironmentPolicy::WarnOnMismatch,
            &[100.0; 3],
            standard_environment(),
        );
        let relative_only = latency_request(Gate::Absolute { value: 500.0 });
        let baseline_only = input_with_values(
            &"bb".repeat(32),
            relative_only,
            EnvironmentPolicy::WarnOnMismatch,
            &[100.0; 3],
            environment_with(&[(
                "os_family",
                "freebsd",
                EnvironmentFieldClass::ComparisonCritical,
            )]),
        );
        let receipt = compare_pair(&candidate, &baseline_only);
        assert_eq!(disposition_of(&receipt), Some(GateDisposition::Pass));
    }

    #[test]
    fn cross_testbed_is_always_descriptive() {
        let metric = latency_request(statistical_gate(500));
        let candidate = input_with_values(
            &"aa".repeat(32),
            metric.clone(),
            EnvironmentPolicy::CrossTestbedDescriptive,
            &[130.0; 7],
            standard_environment(),
        );
        let baseline = input_with_values(
            &"bb".repeat(32),
            metric,
            EnvironmentPolicy::CrossTestbedDescriptive,
            &[100.0; 7],
            standard_environment(),
        );
        let receipt = compare_pair(&candidate, &baseline);
        assert_eq!(disposition_of(&receipt), Some(GateDisposition::Descriptive));
        assert_eq!(
            receipt.metrics[0].reason.as_deref(),
            Some("cross_testbed_descriptive")
        );
        assert_eq!(receipt.aggregate_verdict, None);
    }

    #[test]
    fn driver_version_mismatch_invalidates() {
        let (candidate, mut baseline, _) = statistical_inputs(&[101.0; 7], &[100.0; 7]);
        if let Some(driver) = baseline.resolved.drivers.get_mut(&DriverCategory::Workload) {
            driver.descriptor.adapter_version = "9.9.9".to_owned();
        }
        let receipt = compare_pair(&candidate, &baseline);
        assert!(!receipt.comparability.driver_match);
        assert_eq!(disposition_of(&receipt), Some(GateDisposition::Invalid));
    }

    #[test]
    fn workload_shape_mismatch_invalidates() {
        let (candidate, mut baseline, _) = statistical_inputs(&[101.0; 7], &[100.0; 7]);
        baseline.resolved.workload = Workload::FiniteCount {
            target: metric_name("api"),
            requests: PositiveCount::new(50).unwrap(),
            concurrency: PositiveCount::new(5).unwrap(),
        };
        let receipt = compare_pair(&candidate, &baseline);
        assert!(!receipt.comparability.workload_match);
        assert_eq!(disposition_of(&receipt), Some(GateDisposition::Invalid));
    }

    #[test]
    fn subject_digest_difference_alone_does_not_invalidate() {
        let (candidate, mut baseline, _) = statistical_inputs(&[101.0; 7], &[100.0; 7]);
        baseline.identity.subject_digest = Some("deadbeef".repeat(8));
        baseline.identity.subject_revision = Some("v2".to_owned());
        let receipt = compare_pair(&candidate, &baseline);
        assert!(receipt.comparability.topology_match);
        assert_eq!(disposition_of(&receipt), Some(GateDisposition::Pass));
    }

    #[test]
    fn missing_and_invalid_trials_are_counted_not_imputed() {
        let (mut candidate, baseline, _) = statistical_inputs(&[101.0; 7], &[100.0; 7]);
        if let Some(metrics) = &mut candidate.trials[0].metrics {
            metrics.observations[0].state = ObservationState::Missing {
                reason: crate::MissingReason::SourceNotProvided,
            };
        }
        if let Some(metrics) = &mut candidate.trials[1].metrics {
            metrics.observations[0].state = ObservationState::Invalid {
                reason: crate::InvalidReason::NonFinite,
                detail: None,
            };
        }
        let receipt = compare_pair(&candidate, &baseline);
        let metric = &receipt.metrics[0];
        assert_eq!(metric.candidate_included.len(), 5);
        assert_eq!(metric.candidate_excluded.len(), 2);
        assert_eq!(metric.disposition, Some(GateDisposition::Pass));
    }

    #[test]
    fn request_and_histogram_volume_never_change_sample_count() {
        let (mut candidate, baseline, _) = statistical_inputs(&[101.0; 7], &[100.0; 7]);
        for trial in &mut candidate.trials {
            if let Some(metrics) = &mut trial.metrics {
                metrics.histograms = (0..16)
                    .map(|index| crate::HistogramReference {
                        metric: metric_name("latency_p99"),
                        path: crate::ArtifactPath::new(format!(
                            "trials/001/artifacts/{index:03}-latency.hdr"
                        ))
                        .unwrap(),
                        format: "hdr".to_owned(),
                        unit: metric_name("ms"),
                        method: None,
                        source_metric: None,
                    })
                    .collect();
                metrics.error_distribution = vec![crate::ErrorCategoryCount {
                    category: metric_name("transport"),
                    count: 100_000,
                }];
            }
        }
        let receipt = compare_pair(&candidate, &baseline);
        let metric = &receipt.metrics[0];
        assert_eq!(metric.candidate_included.len(), 7);
        assert_eq!(metric.candidate_excluded.len(), 0);
    }

    #[test]
    fn higher_is_better_orientation_is_correct() {
        let mut metric = latency_request(relative_gate(500));
        metric.name = metric_name("throughput");
        metric.unit = metric_name("rps");
        metric.direction = MetricDirection::HigherIsBetter;
        let mut candidate = input_with_values(
            &"cc".repeat(32),
            metric.clone(),
            EnvironmentPolicy::StrictSameTestbed,
            &[70.0; 7],
            standard_environment(),
        );
        for trial in &mut candidate.trials {
            if let Some(metrics) = &mut trial.metrics {
                for observation in &mut metrics.observations {
                    observation.direction = MetricDirection::HigherIsBetter;
                    observation.name = metric_name("throughput");
                    observation.unit = metric_name("rps");
                }
            }
        }
        let mut baseline = input_with_values(
            &"dd".repeat(32),
            metric,
            EnvironmentPolicy::StrictSameTestbed,
            &[100.0; 7],
            standard_environment(),
        );
        for trial in &mut baseline.trials {
            if let Some(metrics) = &mut trial.metrics {
                for observation in &mut metrics.observations {
                    observation.direction = MetricDirection::HigherIsBetter;
                    observation.name = metric_name("throughput");
                    observation.unit = metric_name("rps");
                }
            }
        }
        // Candidate throughput collapsed 70 vs 100: oriented degradation is
        // positive and the gate must fail.
        let receipt = compare_pair(&candidate, &baseline);
        assert_eq!(disposition_of(&receipt), Some(GateDisposition::Fail));
        let degradation = receipt.metrics[0].degradation.unwrap();
        assert!(degradation > 0.30, "degradation={degradation}");
    }

    // ---- Golden receipt fixtures ----

    fn golden_receipt(kind: &str) -> ComparisonReceipt {
        match kind {
            "pass" => {
                let (candidate, baseline, _) = statistical_inputs(&[101.0; 7], &[100.0; 7]);
                compare_pair(&candidate, &baseline)
            }
            "fail" => {
                let (candidate, baseline, _) = statistical_inputs(&[130.0; 7], &[100.0; 7]);
                compare_pair(&candidate, &baseline)
            }
            "inconclusive" => {
                let (candidate, baseline, _) = statistical_inputs(
                    &[82.0, 118.0, 85.0, 125.0, 90.0, 122.0, 105.0],
                    &[100.0; 7],
                );
                compare_pair(&candidate, &baseline)
            }
            "invalid-comparability" => {
                let (candidate, mut baseline, _) = statistical_inputs(&[101.0; 7], &[100.0; 7]);
                baseline.environment = environment_with(&[
                    (
                        "os_family",
                        "linux",
                        EnvironmentFieldClass::ComparisonCritical,
                    ),
                    (
                        "cpu_class",
                        "aarch64",
                        EnvironmentFieldClass::ComparisonCritical,
                    ),
                ]);
                compare_pair(&candidate, &baseline)
            }
            "descriptive-cross-testbed" => {
                let metric = latency_request(statistical_gate(500));
                let candidate = input_with_values(
                    &"aa".repeat(32),
                    metric.clone(),
                    EnvironmentPolicy::CrossTestbedDescriptive,
                    &[130.0; 7],
                    standard_environment(),
                );
                let baseline = input_with_values(
                    &"bb".repeat(32),
                    metric,
                    EnvironmentPolicy::CrossTestbedDescriptive,
                    &[100.0; 7],
                    standard_environment(),
                );
                compare_pair(&candidate, &baseline)
            }
            "absolute-only" => {
                let metric = latency_request(Gate::Absolute { value: 500.0 });
                let candidate = input_with_values(
                    &"aa".repeat(32),
                    metric,
                    EnvironmentPolicy::StrictSameTestbed,
                    &[100.0; 3],
                    standard_environment(),
                );
                let request = ComparisonRequest {
                    candidate: &candidate,
                    baseline: None,
                };
                compare(&request, &ComparisonOptions::default())
            }
            _ => panic!("unknown golden kind {kind}"),
        }
    }

    #[test]
    fn golden_receipts_are_stable() {
        for kind in [
            "pass",
            "fail",
            "inconclusive",
            "invalid-comparability",
            "descriptive-cross-testbed",
            "absolute-only",
        ] {
            let receipt = golden_receipt(kind);
            let actual = serde_json::to_string_pretty(&receipt).unwrap() + "\n";
            let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("golden")
                .join(format!("comparison-{kind}.json"));
            if std::env::var("EGGBENCH_UPDATE_GOLDEN").is_ok() {
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, &actual).unwrap();
                continue;
            }
            let expected = std::fs::read_to_string(&path)
                .unwrap_or_else(|_| panic!("missing golden {}", path.display()));
            assert_eq!(actual, expected, "golden mismatch for {kind}");
        }
    }
}
