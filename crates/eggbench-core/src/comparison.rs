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
    ArtifactRecord, ArtifactRole, BundleError, BundleReader, DiagnosticPhase, DiagnosticProbe,
    DriverCategory, EnvironmentFieldClass, EnvironmentFingerprint, EnvironmentPolicy, Gate,
    MetricDirection, MetricIntent, MetricRequest, Name, ObservationState, ResolvedPlan, RunId,
    SchemaVersion, Subject, TrialArm, TrialExecutionResult, TrialExecutionStatus, TrialId,
    TrialMetrics, Workload, validate_resolved_plan_bytes,
};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Schema version of the standalone [`ComparisonReceipt`] (v2 adds `paired`).
pub const COMPARISON_RECEIPT_SCHEMA_VERSION: SchemaVersion = SchemaVersion(2);
/// Previous receipt schema version, still accepted on read.
pub const COMPARISON_RECEIPT_SCHEMA_VERSION_1: SchemaVersion = SchemaVersion(1);

/// Immutable comparison-policy identifier for trial-level bootstrap v1.
pub const COMPARISON_POLICY_V1: &str = "eggbench.trial-bootstrap.v1";

/// Immutable comparison-policy identifier for network-path-aware bootstrap v1.
pub const COMPARISON_POLICY_NETWORK_PATH_V1: &str = "eggbench.trial-bootstrap-network-path.v1";

/// Immutable comparison-policy identifier for paired trial-level bootstrap
/// v1: pairs are the resampling unit; per-pair oriented log-differences are
/// resampled with replacement under the same percentile-interval rules as v1.
pub const COMPARISON_POLICY_V2: &str = "eggbench.trial-bootstrap-paired.v1";

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

/// Statistical method label recorded in paired receipts.
pub const STATISTICAL_METHOD_V2: &str = "paired-trial-bootstrap-percentile-95";

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

/// Standalone immutable-by-content comparison receipt, schema v2.
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
    /// Paired-design evidence, present only for paired comparisons.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paired: Option<PairedComparisonSection>,
    /// Bounded diagnostics.
    #[serde(default)]
    pub warnings: Vec<ComparisonWarning>,
}

/// Paired-design evidence for one paired comparison (receipt schema v2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PairedComparisonSection {
    /// Schedule identifier from the paired manifest record.
    pub schedule: String,
    /// Pairs declared by the run (half the measured trial count).
    pub pairs_declared: u32,
    /// Baseline arm service name.
    pub baseline_service: Name,
    /// Candidate arm service name.
    pub candidate_service: Name,
    /// Declared baseline arm subject identity.
    pub baseline_subject: Subject,
    /// Declared candidate arm subject identity.
    pub candidate_subject: Subject,
    /// Per-metric paired evidence in metric-name order.
    pub metrics: Vec<PairedMetricRecord>,
    /// Statistical method label for paired gating.
    pub statistical_method: String,
    /// Final bootstrap resample count for paired gating.
    pub resamples: usize,
    /// Base seed for paired gating.
    pub base_seed: u64,
}

/// Per-metric paired evidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PairedMetricRecord {
    /// Metric name.
    pub name: Name,
    /// Complete pair identities, in execution order.
    pub pairs_complete: Vec<u32>,
    /// Excluded pairs with stable reasons; pairs are never split.
    pub pairs_excluded: Vec<ExcludedPair>,
    /// Per-pair oriented effects (`ratio - 1`) in execution order.
    pub pair_effects: Vec<PairedEffect>,
    /// Descriptive drift diagnostics; never gates.
    pub drift: DriftDiagnostics,
    /// Effective per-metric seed, when paired statistical gating ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_seed: Option<u64>,
}

/// One excluded pair with a stable reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExcludedPair {
    /// Pair identity.
    pub pair_id: u32,
    /// Stable `snake_case` reason.
    pub reason: String,
}

/// One pair's oriented effect in degradation space.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PairedEffect {
    /// Pair identity.
    pub pair_id: u32,
    /// Oriented effect as a fraction (0.03 = 3% worse for the candidate).
    pub effect: f64,
}

/// Descriptive drift diagnostics over pair effects in execution order.
///
/// Drift evidence never changes a gate verdict; it only helps interpret one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriftDiagnostics {
    /// Mean pair effect over the first half of pairs, when computable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_half_mean: Option<f64>,
    /// Mean pair effect over the second half of pairs, when computable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second_half_mean: Option<f64>,
    /// Sign of `second_half_mean - first_half_mean`, or `insufficient`.
    pub trend: DriftTrend,
}

/// Drift trend vocabulary (descriptive only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriftTrend {
    /// Second-half mean exceeds the first-half mean.
    Up,
    /// Second-half mean is below the first-half mean.
    Down,
    /// Half means are exactly equal.
    Flat,
    /// Fewer than two complete pairs; no trend is claimed.
    Insufficient,
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
    /// Paired-run summary, present only for paired bundles.
    pub paired: Option<PairedRunSummary>,
    /// Verified network-path evidence identity, present only for path runs.
    pub network_path_evidence: Option<NetworkPathEvidenceIdentity>,
    /// Verified semantic-replay evidence identity, present only for
    /// semantic-replay runs (Eggstack M003a).
    pub semantic_replay_evidence: Option<SemanticReplayEvidenceIdentity>,
    /// Verified diagnostic evidence identity, present only for runs that
    /// request pre/post workload diagnostics (Eggstack M003b).
    pub diagnostics_evidence: Option<DiagnosticsEvidenceIdentity>,
    /// Verified security-correctness evidence identity, present only for
    /// runs that request Eggsec strict-scope WAF checks (Eggstack M004a).
    /// Result values (pass/fail/counts) never participate; only requested
    /// configuration plus producer/scope provenance does.
    pub security_evidence: Option<SecurityEvidenceIdentity>,
}

/// Comparison-critical identity loaded from `semantic-replay.json`.
///
/// The workstation-local fixture path is never part of this identity;
/// the aggregate digest plus schema/policy/provenance is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticReplayEvidenceIdentity {
    /// Deterministic aggregate digest over the confined fixture tree.
    pub fixture_digest: String,
    /// `EggReplay` fixture session schema accepted at preflight.
    pub fixture_session_schema: u32,
    /// `EggReplay` CLI envelope schema version (M003a pins 1).
    pub envelope_schema: u32,
    /// `EggReplay` `RegressionReport` schema version (M003a pins 2).
    pub report_schema: u32,
    /// Observed `eggreplay` tool version.
    pub executable_version: String,
    /// SHA-256 of the selected `eggreplay` executable.
    pub executable_sha256: String,
}

/// Comparison-critical identity loaded from `diagnostics.json`.
///
/// Observed probe statuses/timings are result evidence, never configuration
/// identity: only the requested configuration plus producer provenance
/// participates in comparability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsEvidenceIdentity {
    /// Ordered per-request summaries (`id phase required target
    /// probes=... timeout_ms=...`).
    pub requests: Vec<String>,
    /// Observed `eggprobe` tool version.
    pub executable_version: String,
    /// SHA-256 of the selected `eggprobe` executable (empty only when every
    /// execution was skipped before any diagnostic ran).
    pub executable_sha256: String,
    /// Accepted machine schema (M003b pins `0.3`).
    pub machine_schema: String,
}

/// Comparison-critical identity loaded from `security-checks.json`
/// (Eggstack M004a).
///
/// Result values (pass/fail/counts) are result evidence, never
/// configuration identity: only the requested check configuration plus
/// producer/scope provenance participates in comparability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecurityEvidenceIdentity {
    /// Ordered per-check summaries (`id source target test_type
    /// max_successful_bypasses=... concurrency=... timeout_ms=...`).
    pub checks: Vec<String>,
    /// Observed `eggsec` tool version.
    pub executable_version: String,
    /// SHA-256 of the selected `eggsec` executable.
    pub executable_sha256: String,
    /// SHA-256 of the generated strict scope manifest.
    pub scope_sha256: String,
    /// Correctness adapter semantic version.
    pub adapter_semantic_version: String,
}

/// Comparison-critical identity loaded from `network-path.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkPathEvidenceIdentity {
    /// Canonical credential-free route-chain digest.
    pub chain_config_digest: Option<String>,
    /// Exact Eggress URI parser version.
    pub eggress_uri_version: String,
}

/// Paired-run summary carried from the bundle manifest.
#[derive(Debug, Clone)]
pub struct PairedRunSummary {
    /// Schedule identifier.
    pub schedule: String,
    /// Pairs declared by the run.
    pub pairs: u32,
}

/// One trial's comparison evidence.
#[derive(Debug, Clone)]
pub struct InputTrial {
    /// Stable trial identity.
    pub id: TrialId,
    /// Terminal execution state.
    pub terminal: TrialExecutionStatus,
    /// Paired arm, when the trial carries pair identity.
    pub arm: Option<TrialArm>,
    /// Pair identity, when the trial carries pair identity.
    pub pair_id: Option<u32>,
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredNetworkPathEvidence {
    schema_version: SchemaVersion,
    adapter_version: String,
    route_driver: StoredNetworkPathDriver,
    eggress_outbound_version: String,
    eggress_uri_version: String,
    fault_driver: Option<StoredNetworkPathDriver>,
    semantics: StoredNetworkPathSemantics,
    route: crate::RouteRequest,
    redacted_chain: Option<String>,
    chain_config_digest: Option<String>,
    configured_hop_count: u16,
    stream_faults: Option<StoredNetworkPathFaults>,
    diagnostics: StoredNetworkPathDiagnostics,
    policy_mode: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredNetworkPathDriver {
    name: String,
    adapter_version: String,
    upstream_name: String,
    upstream_version: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredNetworkPathSemantics {
    ordering_version: String,
    ordering: String,
    fault_layer: String,
    upstream: String,
    downstream: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredNetworkPathFaults {
    request: crate::StreamFaultPlanRequest,
    seed_namespace: Option<u64>,
    rng_version: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredNetworkPathDiagnostics {
    physical_dial_attempts: u64,
    successful_dials: u64,
    fault_wrapped_connections: u64,
    fault_wrapper_construction_failures: u64,
    route_failure_buckets_dropped: u64,
    route_failures: BTreeMap<String, u64>,
    hop_count_distribution: BTreeMap<u64, u64>,
    max_observed_hop_count: u64,
    connection_ordinal_min: u64,
    connection_ordinal_max: u64,
    connection_ordinal_count: u64,
}

fn network_path_evidence_record(
    reader: &BundleReader,
    resolved: &ResolvedPlan,
) -> Result<Option<ArtifactRecord>, ComparisonError> {
    let mut record = None;
    for artifact in &reader.manifest().artifacts {
        let has_path = artifact.path.as_str() == "network-path.json";
        let has_role = matches!(
            &artifact.role,
            ArtifactRole::Other { label } if label.as_str() == "network-path"
        );
        if has_path || has_role {
            if record.is_some() || has_path != has_role {
                return Err(BundleError::InvalidManifest(
                    "network-path evidence path and role must occur exactly once",
                )
                .into());
            }
            record = Some(artifact);
        }
    }
    let Some(record) = record else {
        if resolved.network_path.is_some() {
            return Err(BundleError::InvalidManifest(
                "comparison-ready path bundle lacks network-path evidence",
            )
            .into());
        }
        return Ok(None);
    };
    if record.media_type != "application/json"
        || record.sensitivity != crate::Sensitivity::Redacted
        || record.byte_size > 128 * 1024
    {
        return Err(
            BundleError::InvalidManifest("network-path evidence metadata is invalid").into(),
        );
    }
    Ok(Some(record.clone()))
}

fn read_stored_network_path_evidence(
    reader: &BundleReader,
    record: &ArtifactRecord,
) -> Result<StoredNetworkPathEvidence, ComparisonError> {
    let mut file = reader.open_artifact(&record.path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| BundleError::Io {
            path: PathBuf::from(record.path.as_str()),
            source: error,
        })?;
    serde_json::from_slice(&bytes)
        .map_err(|error| BundleError::ManifestParse(error.to_string()).into())
}

fn stored_network_path_request_is_valid(evidence: &StoredNetworkPathEvidence) -> bool {
    let request = crate::NetworkPathRequest {
        route: evidence.route.clone(),
        stream_faults: evidence
            .stream_faults
            .as_ref()
            .map(|faults| faults.request.clone()),
    };
    crate::validate_network_path_contract(&request).is_ok()
}

fn stored_route_matches(
    evidence: &StoredNetworkPathEvidence,
    path: &crate::ResolvedNetworkPath,
) -> bool {
    let driver = &path.route_driver.descriptor;
    evidence.schema_version == SchemaVersion(1)
        && evidence.adapter_version == driver.adapter_version
        && evidence.route == path.route
        && evidence.route_driver.name == driver.name.to_string()
        && evidence.route_driver.adapter_version == driver.adapter_version
        && evidence.route_driver.upstream_name == driver.upstream_name
        && evidence.route_driver.upstream_version == driver.upstream_version
        && evidence.eggress_outbound_version == driver.upstream_version.clone().unwrap_or_default()
        && !evidence.eggress_uri_version.is_empty()
        && evidence.semantics.ordering_version == path.semantics_version
        && evidence.semantics.ordering == "route_first_fault_second"
        && evidence.semantics.fault_layer == "user_space_stream"
        && evidence.semantics.upstream == "client_to_target"
        && evidence.semantics.downstream == "target_to_client"
        && evidence.policy_mode == "static"
}

fn stored_canonical_route_matches(evidence: &StoredNetworkPathEvidence) -> bool {
    let (expected_chain, expected_digest, expected_hops) = match &evidence.route.mode {
        crate::RouteMode::Direct => (None, None, 0),
        crate::RouteMode::ProxyChain { chain } => {
            let canonical = crate::plan::canonical_proxy_chain_text(chain);
            let digest = format!("{:x}", sha2::Sha256::digest(canonical.as_bytes()));
            let hops = u16::try_from(canonical.split("__").count()).unwrap_or(u16::MAX);
            (Some(canonical), Some(digest), hops)
        }
    };
    evidence.redacted_chain == expected_chain
        && evidence.chain_config_digest == expected_digest
        && evidence.configured_hop_count == expected_hops
}

fn stored_faults_match(
    evidence: &StoredNetworkPathEvidence,
    path: &crate::ResolvedNetworkPath,
    seed: Option<u64>,
) -> bool {
    let active = evidence.stream_faults.as_ref().is_some_and(|faults| {
        !faults.request.upstream.is_empty() || !faults.request.downstream.is_empty()
    });
    match (
        &path.stream_faults,
        &evidence.stream_faults,
        &evidence.fault_driver,
    ) {
        (None, None, None) => true,
        (Some(resolved), Some(stored), Some(driver)) => {
            let descriptor = &resolved.fault_driver.descriptor;
            stored.request == resolved.request
                && stored.rng_version == resolved.rng_version
                && stored.seed_namespace == if active { seed } else { None }
                && driver.name == descriptor.name.to_string()
                && driver.adapter_version == descriptor.adapter_version
                && driver.upstream_name == descriptor.upstream_name
                && driver.upstream_version == descriptor.upstream_version
        }
        _ => false,
    }
}

fn safe_route_failure_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 256
        && label.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, ':' | '=' | '_' | '-' | '.')
        })
        && !label.to_ascii_lowercase().contains("secret")
        && !label.to_ascii_lowercase().contains("password")
        && !label.to_ascii_lowercase().contains("token")
}

fn stored_diagnostics_match(evidence: &StoredNetworkPathEvidence) -> bool {
    let diagnostics = &evidence.diagnostics;
    let total = diagnostics
        .hop_count_distribution
        .values()
        .try_fold(0_u64, |total, count| total.checked_add(*count));
    let max_hop = diagnostics
        .hop_count_distribution
        .keys()
        .copied()
        .max()
        .unwrap_or(0);
    let faults_active = evidence.stream_faults.as_ref().is_some_and(|faults| {
        !faults.request.upstream.is_empty() || !faults.request.downstream.is_empty()
    });
    diagnostics.route_failure_buckets_dropped <= diagnostics.physical_dial_attempts
        && diagnostics.route_failures.len() <= 16
        && diagnostics
            .route_failures
            .iter()
            .all(|(key, count)| safe_route_failure_label(key) && *count > 0)
        && diagnostics
            .hop_count_distribution
            .iter()
            .all(|(hops, count)| *count > 0 && *hops <= u64::from(evidence.configured_hop_count))
        && total == Some(diagnostics.successful_dials)
        && diagnostics.max_observed_hop_count == max_hop
        && diagnostics.successful_dials <= diagnostics.physical_dial_attempts
        && diagnostics.fault_wrapped_connections <= diagnostics.successful_dials
        && diagnostics.fault_wrapper_construction_failures <= diagnostics.successful_dials
        && diagnostics
            .fault_wrapped_connections
            .checked_add(diagnostics.fault_wrapper_construction_failures)
            == if faults_active {
                Some(diagnostics.successful_dials)
            } else {
                Some(0)
            }
        && diagnostics.connection_ordinal_count == diagnostics.successful_dials
        && (diagnostics.successful_dials == 0
            || diagnostics.connection_ordinal_min <= diagnostics.connection_ordinal_max)
        && (diagnostics.successful_dials > 0
            || (diagnostics.connection_ordinal_min == 0 && diagnostics.connection_ordinal_max == 0))
}

fn load_network_path_evidence_identity(
    reader: &BundleReader,
    resolved: &ResolvedPlan,
) -> Result<Option<NetworkPathEvidenceIdentity>, ComparisonError> {
    let Some(record) = network_path_evidence_record(reader, resolved)? else {
        return Ok(None);
    };
    let Some(path) = resolved.network_path.as_ref() else {
        return Err(BundleError::InvalidManifest(
            "path-free bundle contains network-path evidence",
        )
        .into());
    };
    let evidence = read_stored_network_path_evidence(reader, &record)?;
    if !stored_network_path_request_is_valid(&evidence) {
        return Err(
            BundleError::InvalidManifest("network-path request contract is invalid").into(),
        );
    }
    if !stored_route_matches(&evidence, path) {
        return Err(BundleError::InvalidManifest(
            "network-path route provenance contradicts the resolved plan",
        )
        .into());
    }
    if !stored_canonical_route_matches(&evidence) {
        return Err(BundleError::InvalidManifest(
            "network-path canonical route contradicts the resolved plan",
        )
        .into());
    }
    if !stored_faults_match(&evidence, path, resolved.seed) {
        return Err(BundleError::InvalidManifest(
            "network-path fault provenance contradicts the resolved plan",
        )
        .into());
    }
    if !stored_diagnostics_match(&evidence) {
        return Err(BundleError::InvalidManifest(
            "network-path diagnostics contradict the resolved plan",
        )
        .into());
    }
    Ok(Some(NetworkPathEvidenceIdentity {
        chain_config_digest: evidence.chain_config_digest,
        eggress_uri_version: evidence.eggress_uri_version,
    }))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredSemanticReplayEvidence {
    schema_version: SchemaVersion,
    driver: String,
    adapter_version: String,
    fixture_digest: String,
    fixture_relative: String,
    fixture_session_schema: u32,
    envelope_schema: u32,
    report_schema: u32,
    executable_version: String,
    executable_sha256: String,
    // Accepted for schema completeness (the writer emits it); comparison
    // identity keys on `fixture_digest`, not the flow count.
    #[allow(dead_code)]
    flow_count: u64,
}

fn semantic_replay_evidence_record(
    reader: &BundleReader,
    resolved: &ResolvedPlan,
) -> Result<Option<ArtifactRecord>, ComparisonError> {
    let mut record = None;
    for artifact in &reader.manifest().artifacts {
        let has_path = artifact.path.as_str() == "semantic-replay.json";
        let has_role = matches!(
            &artifact.role,
            ArtifactRole::Other { label } if label.as_str() == "semantic-replay"
        );
        if has_path || has_role {
            if record.is_some() || has_path != has_role {
                return Err(BundleError::InvalidManifest(
                    "semantic-replay evidence path and role must occur exactly once",
                )
                .into());
            }
            record = Some(artifact);
        }
    }
    let Some(record) = record else {
        if matches!(resolved.workload, Workload::SemanticReplay { .. }) {
            return Err(BundleError::InvalidManifest(
                "semantic-replay workload bundle lacks semantic-replay evidence",
            )
            .into());
        }
        return Ok(None);
    };
    if record.media_type != "application/json" {
        return Err(
            BundleError::InvalidManifest("semantic-replay evidence metadata is invalid").into(),
        );
    }
    Ok(Some(record.clone()))
}

fn load_semantic_replay_evidence_identity(
    reader: &BundleReader,
    resolved: &ResolvedPlan,
) -> Result<Option<SemanticReplayEvidenceIdentity>, ComparisonError> {
    let Some(record) = semantic_replay_evidence_record(reader, resolved)? else {
        return Ok(None);
    };
    if !matches!(resolved.workload, Workload::SemanticReplay { .. }) {
        return Err(BundleError::InvalidManifest(
            "path-free bundle contains semantic-replay evidence",
        )
        .into());
    }
    let mut file = reader.open_artifact(&record.path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| BundleError::Io {
            path: PathBuf::from(record.path.as_str()),
            source: error,
        })?;
    if bytes.len() > 128 * 1024 {
        return Err(BundleError::InvalidManifest("semantic-replay evidence exceeds bound").into());
    }
    let evidence: StoredSemanticReplayEvidence = serde_json::from_slice(&bytes)
        .map_err(|error| BundleError::ManifestParse(error.to_string()))?;
    if evidence.schema_version != SchemaVersion(1)
        || evidence.driver != "eggreplay-semantic"
        || evidence.adapter_version.is_empty()
        || evidence.adapter_version.len() > 128
        || evidence.fixture_digest.len() != 64
        || !evidence
            .fixture_digest
            .chars()
            .all(|c| c.is_ascii_hexdigit())
        || evidence.fixture_relative.is_empty()
        || evidence.fixture_relative.len() > 512
        || evidence.envelope_schema != 1
        || evidence.report_schema != 2
        || evidence.executable_version.is_empty()
        || evidence.executable_version.len() > 128
        || evidence.executable_sha256.len() != 64
    {
        return Err(
            BundleError::InvalidManifest("semantic-replay evidence contract is invalid").into(),
        );
    }
    Ok(Some(SemanticReplayEvidenceIdentity {
        fixture_digest: evidence.fixture_digest.to_ascii_lowercase(),
        fixture_session_schema: evidence.fixture_session_schema,
        envelope_schema: evidence.envelope_schema,
        report_schema: evidence.report_schema,
        executable_version: evidence.executable_version,
        executable_sha256: evidence.executable_sha256.to_ascii_lowercase(),
    }))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredDiagnosticsIndex {
    schema_version: SchemaVersion,
    driver: String,
    adapter_version: String,
    executable_version: String,
    executable_sha256: String,
    machine_schema: String,
    #[serde(default)]
    executions: Vec<StoredDiagnosticExecution>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredDiagnosticExecution {
    id: String,
    phase: String,
    required: bool,
    target: String,
    probes: Vec<String>,
    timeout_ms: u64,
    disposition: String,
    report_status: String,
    artifact: String,
    artifact_sha256: String,
    producer_version: String,
    executable_sha256: String,
    machine_schema: String,
    #[serde(default)]
    warnings: Vec<String>,
    #[serde(default)]
    skipped_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredSecurityChecksIndex {
    schema_version: SchemaVersion,
    driver: String,
    adapter_version: String,
    executable_version: String,
    executable_sha256: String,
    operation: String,
    scope_sha256: String,
    lifecycle_placement: String,
    #[serde(default)]
    checks: Vec<StoredSecurityCheckRecord>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredSecurityCheckRecord {
    id: String,
    source: String,
    target: String,
    test_type: String,
    disposition: String,
    evaluated_cases: u32,
    successful_bypasses: u32,
    allowed_successful_bypasses: u32,
    artifact: String,
    artifact_sha256: String,
}

/// Expected per-execution identity expanded from resolved requests
/// (`Both` requests execute twice: pre then post).
fn expected_diagnostic_executions(resolved: &ResolvedPlan) -> Vec<DiagnosticExecutionExpectation> {
    let mut out = Vec::new();
    for request in &resolved.diagnostics {
        let slots: &[&str] = match request.phase {
            DiagnosticPhase::PreWorkload => &["pre_workload"],
            DiagnosticPhase::PostWorkload => &["post_workload"],
            DiagnosticPhase::Both => &["pre_workload", "post_workload"],
        };
        for slot in slots {
            out.push(DiagnosticExecutionExpectation {
                id: request.id.as_str().to_owned(),
                phase: (*slot).to_owned(),
                required: request.required,
                target: request.target.as_str().to_owned(),
                probes: {
                    let mut probes: Vec<String> = request
                        .probes
                        .iter()
                        .copied()
                        .map(diagnostic_probe_label)
                        .collect();
                    probes.sort();
                    probes
                },
                timeout_ms: request.timeout_ms.get(),
            });
        }
    }
    out
}

struct DiagnosticExecutionExpectation {
    id: String,
    phase: String,
    required: bool,
    target: String,
    probes: Vec<String>,
    timeout_ms: u64,
}

/// True when every staged execution matches the resolved request expansion
/// in order. Result evidence (statuses, artifact digests, warnings) never
/// participates in identity, but its shape is re-validated so a tampered
/// index cannot smuggle unbounded fields past comparison.
fn diagnostics_executions_match_resolved(
    executions: &[StoredDiagnosticExecution],
    expected: &[DiagnosticExecutionExpectation],
) -> bool {
    executions.len() == expected.len()
        && executions
            .iter()
            .zip(expected.iter())
            .all(|(execution, expectation)| {
                let mut probes = execution.probes.clone();
                probes.sort();
                let result_shape_ok = !execution.report_status.is_empty()
                    && execution.report_status.len() <= 64
                    && !execution.artifact.is_empty()
                    && execution.artifact.len() <= 256
                    && execution.warnings.len() <= 32
                    && execution
                        .warnings
                        .iter()
                        .all(|warning| warning.len() <= 512)
                    && execution
                        .skipped_reason
                        .as_ref()
                        .is_none_or(|reason| !reason.is_empty() && reason.len() <= 128);
                execution.id == expectation.id
                    && execution.phase == expectation.phase
                    && execution.required == expectation.required
                    && execution.target == expectation.target
                    && probes == expectation.probes
                    && execution.timeout_ms == expectation.timeout_ms
                    && execution.artifact_sha256.len() == 64
                    && execution.producer_version.len() <= 128
                    && execution.executable_sha256.len() <= 64
                    && execution.machine_schema.len() <= 16
                    && result_shape_ok
            })
}

fn diagnostic_probe_label(probe: DiagnosticProbe) -> String {
    match probe {
        DiagnosticProbe::Dns => "dns".to_owned(),
        DiagnosticProbe::Tcp => "tcp".to_owned(),
        DiagnosticProbe::Tls => "tls".to_owned(),
        DiagnosticProbe::Http => "http".to_owned(),
    }
}

fn diagnostics_evidence_record(
    reader: &BundleReader,
    resolved: &ResolvedPlan,
) -> Result<Option<ArtifactRecord>, ComparisonError> {
    let mut record = None;
    for artifact in &reader.manifest().artifacts {
        // The run-level index is identified by path: per-diagnostic raw
        // reports share the `diagnostics` role label but live under
        // `diagnostics/pre|post/`, so role alone cannot select the index.
        if artifact.path.as_str() == "diagnostics.json" {
            if record.is_some() {
                return Err(BundleError::InvalidManifest(
                    "diagnostics evidence path and role must occur exactly once",
                )
                .into());
            }
            record = Some(artifact);
        }
    }
    let Some(record) = record else {
        if !resolved.diagnostics.is_empty() {
            return Err(BundleError::InvalidManifest(
                "diagnostic run bundle lacks diagnostics evidence",
            )
            .into());
        }
        return Ok(None);
    };
    if !matches!(
        &record.role,
        ArtifactRole::Other { label } if label.as_str() == "diagnostics"
    ) {
        return Err(BundleError::InvalidManifest(
            "diagnostics evidence path and role must occur exactly once",
        )
        .into());
    }
    if record.media_type != "application/json" {
        return Err(
            BundleError::InvalidManifest("diagnostics evidence metadata is invalid").into(),
        );
    }
    Ok(Some(record.clone()))
}

/// Validate `diagnostics.json` index-level provenance. An all-skipped index
/// (cancellation before any diagnostic ran) carries no producer provenance;
/// otherwise the tool version and executable digest are required.
fn validate_diagnostics_index_provenance(
    evidence: &StoredDiagnosticsIndex,
) -> Result<(), ComparisonError> {
    let invalid = || {
        ComparisonError::from(BundleError::InvalidManifest(
            "diagnostics evidence contract is invalid",
        ))
    };
    let all_skipped = !evidence.executions.is_empty()
        && evidence
            .executions
            .iter()
            .all(|execution| execution.disposition == "skipped");
    if all_skipped {
        if !evidence.executable_sha256.is_empty() && evidence.executable_sha256.len() != 64 {
            return Err(invalid());
        }
    } else {
        if evidence.executable_version.is_empty() || evidence.executable_sha256.len() != 64 {
            return Err(invalid());
        }
        if !evidence
            .executable_sha256
            .chars()
            .all(|c| c.is_ascii_hexdigit())
        {
            return Err(invalid());
        }
    }
    Ok(())
}

fn load_diagnostics_evidence_identity(
    reader: &BundleReader,
    resolved: &ResolvedPlan,
) -> Result<Option<DiagnosticsEvidenceIdentity>, ComparisonError> {
    let Some(record) = diagnostics_evidence_record(reader, resolved)? else {
        return Ok(None);
    };
    if resolved.diagnostics.is_empty() {
        return Err(BundleError::InvalidManifest(
            "diagnostic-free bundle contains diagnostics evidence",
        )
        .into());
    }
    let mut file = reader.open_artifact(&record.path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| BundleError::Io {
            path: PathBuf::from(record.path.as_str()),
            source: error,
        })?;
    if bytes.len() > 128 * 1024 {
        return Err(BundleError::InvalidManifest("diagnostics evidence exceeds bound").into());
    }
    let evidence: StoredDiagnosticsIndex = serde_json::from_slice(&bytes)
        .map_err(|error| BundleError::ManifestParse(error.to_string()))?;
    if evidence.schema_version != SchemaVersion(1)
        || evidence.driver != "eggprobe"
        || evidence.adapter_version.is_empty()
        || evidence.adapter_version.len() > 128
        || evidence.executable_version.len() > 128
        || evidence.machine_schema != "0.3"
    {
        return Err(
            BundleError::InvalidManifest("diagnostics evidence contract is invalid").into(),
        );
    }
    validate_diagnostics_index_provenance(&evidence)?;
    // Every execution must match the resolved request expansion in order;
    // observed statuses/timings never participate in identity.
    let expected = expected_diagnostic_executions(resolved);
    if evidence.executions.len() != expected.len() {
        return Err(BundleError::InvalidManifest(
            "diagnostics evidence execution count contradicts the resolved plan",
        )
        .into());
    }
    if !diagnostics_executions_match_resolved(&evidence.executions, &expected) {
        return Err(BundleError::InvalidManifest(
            "diagnostics evidence execution contradicts the resolved plan",
        )
        .into());
    }
    let mut requests = Vec::with_capacity(resolved.diagnostics.len());
    for request in &resolved.diagnostics {
        let mut probes: Vec<String> = request
            .probes
            .iter()
            .copied()
            .map(diagnostic_probe_label)
            .collect();
        probes.sort();
        let phase = match request.phase {
            DiagnosticPhase::PreWorkload => "pre_workload",
            DiagnosticPhase::PostWorkload => "post_workload",
            DiagnosticPhase::Both => "both",
        };
        requests.push(format!(
            "{} {phase} required={} target={} probes=[{}] timeout_ms={}",
            request.id.as_str(),
            request.required,
            request.target.as_str(),
            probes.join(","),
            request.timeout_ms.get(),
        ));
    }
    Ok(Some(DiagnosticsEvidenceIdentity {
        requests,
        executable_version: evidence.executable_version,
        executable_sha256: evidence.executable_sha256.to_ascii_lowercase(),
        machine_schema: evidence.machine_schema,
    }))
}

fn security_evidence_record(
    reader: &BundleReader,
    resolved: &ResolvedPlan,
) -> Result<Option<ArtifactRecord>, ComparisonError> {
    let mut record = None;
    for artifact in &reader.manifest().artifacts {
        // The run-level index is identified by path: per-check sanitized
        // results share the `security` role label but live under
        // `security/`, so role alone cannot select the index.
        if artifact.path.as_str() == "security-checks.json" {
            if record.is_some() {
                return Err(BundleError::InvalidManifest(
                    "security evidence path and role must occur exactly once",
                )
                .into());
            }
            record = Some(artifact);
        }
    }
    let Some(record) = record else {
        if !resolved.security_checks.is_empty() {
            return Err(BundleError::InvalidManifest(
                "security run bundle lacks security evidence",
            )
            .into());
        }
        return Ok(None);
    };
    if !matches!(
        &record.role,
        ArtifactRole::Other { label } if label.as_str() == "security"
    ) {
        return Err(BundleError::InvalidManifest(
            "security evidence path and role must occur exactly once",
        )
        .into());
    }
    if record.media_type != "application/json" {
        return Err(BundleError::InvalidManifest("security evidence metadata is invalid").into());
    }
    Ok(Some(record.clone()))
}

fn load_security_evidence_identity(
    reader: &BundleReader,
    resolved: &ResolvedPlan,
) -> Result<Option<SecurityEvidenceIdentity>, ComparisonError> {
    let Some(record) = security_evidence_record(reader, resolved)? else {
        return Ok(None);
    };
    if resolved.security_checks.is_empty() {
        return Err(BundleError::InvalidManifest(
            "security-free bundle contains security evidence",
        )
        .into());
    }
    let mut file = reader.open_artifact(&record.path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| BundleError::Io {
            path: PathBuf::from(record.path.as_str()),
            source: error,
        })?;
    if bytes.len() > 128 * 1024 {
        return Err(BundleError::InvalidManifest("security evidence exceeds bound").into());
    }
    let evidence: StoredSecurityChecksIndex = serde_json::from_slice(&bytes)
        .map_err(|error| BundleError::ManifestParse(error.to_string()))?;
    if evidence.schema_version != SchemaVersion(crate::SECURITY_CHECKS_INDEX_SCHEMA)
        || evidence.driver != crate::EGGSEC_WAF_DRIVER_NAME
        || evidence.adapter_version.is_empty()
        || evidence.adapter_version.len() > 128
        || evidence.executable_version.is_empty()
        || evidence.executable_version.len() > 128
        || evidence.executable_sha256.len() != 64
        || !evidence
            .executable_sha256
            .chars()
            .all(|c| c.is_ascii_hexdigit())
        || evidence.scope_sha256.len() != 64
        || !evidence.scope_sha256.chars().all(|c| c.is_ascii_hexdigit())
        || evidence.operation.is_empty()
        || evidence.operation.len() > 256
        || evidence.lifecycle_placement.is_empty()
        || evidence.lifecycle_placement.len() > 128
    {
        return Err(BundleError::InvalidManifest("security evidence contract is invalid").into());
    }
    // Every staged check must match the resolved request list in order;
    // observed dispositions/counts never participate in identity, but the
    // per-check shape is re-validated so a tampered index cannot smuggle
    // unbounded fields past comparison.
    if evidence.checks.len() != resolved.security_checks.len() {
        return Err(BundleError::InvalidManifest(
            "security evidence check count contradicts the resolved plan",
        )
        .into());
    }
    for (staged, request) in evidence.checks.iter().zip(resolved.security_checks.iter()) {
        let shape_ok = !staged.id.is_empty()
            && staged.id.len() <= 64
            && !staged.source.is_empty()
            && staged.source.len() <= 64
            && !staged.target.is_empty()
            && staged.target.len() <= 128
            && !staged.test_type.is_empty()
            && staged.test_type.len() <= 32
            && matches!(staged.disposition.as_str(), "pass" | "fail" | "invalid")
            && !staged.artifact.is_empty()
            && staged.artifact.len() <= 256
            && staged.artifact_sha256.len() == 64
            && staged.evaluated_cases <= crate::MAX_SECURITY_CASES
            && staged.successful_bypasses <= staged.evaluated_cases
            && staged.allowed_successful_bypasses <= crate::MAX_SECURITY_CASES;
        if !shape_ok
            || staged.id != request.id.as_str()
            || staged.source != request.source.as_str()
            || staged.target != request.target.as_str()
        {
            return Err(BundleError::InvalidManifest(
                "security evidence check contradicts the resolved plan",
            )
            .into());
        }
    }
    let mut checks = Vec::with_capacity(resolved.security_checks.len());
    for request in &resolved.security_checks {
        checks.push(format!(
            "{} {} {} {} max_successful_bypasses={} concurrency={} timeout_ms={}",
            request.id.as_str(),
            request.source.as_str(),
            request.target.as_str(),
            request.test_type.as_cli_str(),
            request.max_successful_bypasses,
            request.concurrency.get(),
            request.timeout_ms.get(),
        ));
    }
    Ok(Some(SecurityEvidenceIdentity {
        checks,
        executable_version: evidence.executable_version,
        executable_sha256: evidence.executable_sha256.to_ascii_lowercase(),
        scope_sha256: evidence.scope_sha256.to_ascii_lowercase(),
        adapter_semantic_version: crate::CORRECTNESS_ADAPTER_SEMANTIC_VERSION.to_owned(),
    }))
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
            arm: result.arm,
            pair_id: result.pair_id,
            metrics,
        });
    }
    let paired = manifest.paired.as_ref().map(|record| PairedRunSummary {
        schedule: record.schedule.clone(),
        pairs: record.pairs,
    });
    let network_path_evidence = load_network_path_evidence_identity(reader, &resolved)?;
    let semantic_replay_evidence = load_semantic_replay_evidence_identity(reader, &resolved)?;
    let diagnostics_evidence = load_diagnostics_evidence_identity(reader, &resolved)?;
    let security_evidence = load_security_evidence_identity(reader, &resolved)?;
    Ok(ComparisonInput {
        identity,
        resolved,
        environment,
        trials,
        paired,
        network_path_evidence,
        semantic_replay_evidence,
        diagnostics_evidence,
        security_evidence,
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

fn unpaired_policy_id(
    candidate: &ComparisonInput,
    baseline: Option<&ComparisonInput>,
) -> &'static str {
    if candidate.network_path_evidence.is_some()
        || candidate.resolved.network_path.is_some()
        || baseline.is_some_and(|input| {
            input.network_path_evidence.is_some() || input.resolved.network_path.is_some()
        })
    {
        COMPARISON_POLICY_NETWORK_PATH_V1
    } else {
        COMPARISON_POLICY_V1
    }
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
    let policy_id = unpaired_policy_id(candidate, request.baseline.as_ref().map(|side| side.input));
    let base_seed = match options.seed {
        Some(seed) => seed,
        None => derive_base_seed(
            candidate,
            request.baseline.as_ref().map(|side| side.input),
            policy_id,
        ),
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
    // Unpaired inference over paired evidence would silently mix arms or
    // invent pairing, so every metric fails closed with a stable reason.
    // Only new paired bundles are affected; all v1 bundles compare as before.
    if candidate.paired.is_some() {
        warnings.push(ComparisonWarning {
            category: "paired_bundle_requires_paired_comparison".to_owned(),
            detail: "candidate bundle contains paired trials; use paired comparison".to_owned(),
        });
    }
    let mut metrics = Vec::with_capacity(metric_names.len().min(MAX_RECEIPT_METRICS));
    for name in metric_names {
        let request_metric = candidate
            .resolved
            .metrics
            .iter()
            .find(|metric| &metric.name == name)
            .expect("metric name from candidate plan");
        if candidate.paired.is_some() {
            metrics.push(paired_rejection_record(request_metric));
            if metrics.len() >= MAX_RECEIPT_METRICS {
                break;
            }
            continue;
        }
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
        schema_version: COMPARISON_RECEIPT_SCHEMA_VERSION_1,
        policy_id: policy_id.to_owned(),
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
        paired: None,
        warnings,
    }
}

/// Fail-closed record for unpaired comparison of a paired bundle.
fn paired_rejection_record(request: &MetricRequest) -> MetricComparison {
    MetricComparison {
        name: request.name.clone(),
        unit: request.unit.clone(),
        direction: request.direction.clone(),
        intent: request.intent,
        gate: request.gate.clone(),
        candidate_included: Vec::new(),
        candidate_excluded: Vec::new(),
        baseline_included: Vec::new(),
        baseline_excluded: Vec::new(),
        candidate_estimate: None,
        baseline_estimate: None,
        degradation: None,
        confidence_low: None,
        confidence_high: None,
        threshold: request.gate.as_ref().and_then(allowance_fraction),
        statistical_method: None,
        resamples: None,
        effective_seed: None,
        disposition: Some(GateDisposition::Invalid),
        reason: Some("paired_evidence_requires_paired_comparison".to_owned()),
    }
}

// ---- Paired comparison (policy v2) ----

/// One complete pair: both arms measured and observed for one metric.
struct CompletePair {
    pair_id: u32,
    candidate_trial: u32,
    baseline_trial: u32,
    candidate_value: f64,
    baseline_value: f64,
}

/// Pair join outcome for one metric.
struct PairSelection {
    complete: Vec<CompletePair>,
    excluded_pairs: Vec<ExcludedPair>,
    candidate_included: Vec<u32>,
    candidate_excluded: Vec<ExcludedTrial>,
    baseline_included: Vec<u32>,
    baseline_excluded: Vec<ExcludedTrial>,
}

/// Compare the two arms of one paired bundle under policy v2.
///
/// The single bundle supplies both sides: candidate-arm trials against
/// baseline-arm trials, joined by runner-assigned pair identities. Never
/// modifies the bundle. Deterministic for fixed input, policy, and seed.
///
/// Inputs without a paired design yield all-Invalid metrics with a stable
/// reason; the caller maps the aggregate to the locked exit codes exactly
/// as for v1.
///
/// # Panics
/// Panics only on the internal invariant that every metric name collected
/// from the plan resolves back to its request; this indicates a programming
/// defect, never input data.
#[allow(clippy::too_many_lines)] // One auditable paired policy pass, as for v1.
#[must_use]
pub fn compare_paired(
    bundle_path: &Path,
    input: &ComparisonInput,
    options: &ComparisonOptions,
) -> ComparisonReceipt {
    let base_seed = match options.seed {
        Some(seed) => seed,
        None => derive_paired_base_seed(input),
    };
    // Same bundle on both sides: testbed, workload, driver, and topology
    // match by construction. Arm subject identities are expected to differ
    // and are recorded as provenance in the paired section, never as
    // comparability mismatches.
    let comparability = evaluate_comparability(input, input);
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
    let design = input
        .paired
        .as_ref()
        .and_then(|_| input.resolved.paired.clone());
    if input.paired.is_none() {
        warnings.push(ComparisonWarning {
            category: "paired_design_absent".to_owned(),
            detail: "bundle carries no paired-run record".to_owned(),
        });
    } else if design.is_none() {
        warnings.push(ComparisonWarning {
            category: "paired_design_absent_from_resolved_plan".to_owned(),
            detail: "paired manifest record has no resolved paired design".to_owned(),
        });
    }
    let mut metric_names: BTreeSet<&Name> = BTreeSet::new();
    for metric in &input.resolved.metrics {
        metric_names.insert(&metric.name);
    }
    let mut metrics = Vec::with_capacity(metric_names.len().min(MAX_RECEIPT_METRICS));
    let mut paired_metrics = Vec::with_capacity(metric_names.len().min(MAX_RECEIPT_METRICS));
    for name in metric_names {
        let request_metric = input
            .resolved
            .metrics
            .iter()
            .find(|metric| &metric.name == name)
            .expect("metric name from candidate plan");
        let effective_seed = derive_metric_seed(base_seed, name.as_str());
        let (record, paired_record) = match &design {
            Some(_) => evaluate_paired_metric(request_metric, input, effective_seed, &mut warnings),
            None => (
                invalid_paired_record(
                    request_metric,
                    if input.paired.is_none() {
                        "paired_design_absent"
                    } else {
                        "paired_design_absent_from_resolved_plan"
                    },
                ),
                None,
            ),
        };
        metrics.push(record);
        if let Some(paired_record) = paired_record {
            paired_metrics.push(paired_record);
        }
        if metrics.len() >= MAX_RECEIPT_METRICS {
            break;
        }
    }
    let aggregate_verdict = aggregate(&metrics);
    let paired = input.paired.as_ref().and_then(|summary| {
        let design = input.resolved.paired.as_ref()?;
        Some(PairedComparisonSection {
            schedule: summary.schedule.clone(),
            pairs_declared: summary.pairs,
            baseline_service: design.baseline.service.clone(),
            candidate_service: design.candidate.service.clone(),
            baseline_subject: design.baseline.subject.clone(),
            candidate_subject: design.candidate.subject.clone(),
            metrics: paired_metrics,
            statistical_method: STATISTICAL_METHOD_V2.to_owned(),
            resamples: BOOTSTRAP_RESAMPLES,
            base_seed,
        })
    });
    ComparisonReceipt {
        schema_version: COMPARISON_RECEIPT_SCHEMA_VERSION,
        policy_id: COMPARISON_POLICY_V2.to_owned(),
        created_by_version: env!("CARGO_PKG_VERSION").to_owned(),
        candidate_identity: input.identity.clone(),
        baseline_reference: Some(BaselineReference::Bundle {
            identity: input.identity.clone(),
            path: bundle_path.display().to_string(),
        }),
        baseline_identity: Some(input.identity.clone()),
        environment_policy: input.resolved.environment_policy,
        comparability,
        base_seed,
        metrics,
        aggregate_verdict,
        paired,
        warnings,
    }
}

/// Fail-closed paired record when the design itself is absent.
fn invalid_paired_record(request: &MetricRequest, reason: &str) -> MetricComparison {
    let mut record = paired_rejection_record(request);
    record.threshold = request.gate.as_ref().and_then(allowance_fraction);
    record.reason = Some(reason.to_owned());
    record
}

/// Evaluate one metric over complete pairs.
#[allow(clippy::too_many_lines)] // Sequential paired-gate preconditions stay auditable.
fn evaluate_paired_metric(
    request: &MetricRequest,
    input: &ComparisonInput,
    effective_seed: u64,
    warnings: &mut Vec<ComparisonWarning>,
) -> (MetricComparison, Option<PairedMetricRecord>) {
    let selection = select_pairs(input, &request.name);
    let threshold = request.gate.as_ref().and_then(allowance_fraction);
    let mut record = MetricComparison {
        name: request.name.clone(),
        unit: request.unit.clone(),
        direction: request.direction.clone(),
        intent: request.intent,
        gate: request.gate.clone(),
        candidate_included: selection.candidate_included.clone(),
        candidate_excluded: selection.candidate_excluded.clone(),
        baseline_included: selection.baseline_included.clone(),
        baseline_excluded: selection.baseline_excluded.clone(),
        candidate_estimate: arithmetic_mean(
            &selection
                .complete
                .iter()
                .map(|pair| pair.candidate_value)
                .collect::<Vec<_>>(),
        ),
        baseline_estimate: geometric_mean(
            &selection
                .complete
                .iter()
                .map(|pair| pair.baseline_value)
                .collect::<Vec<_>>(),
        ),
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
    // Per-pair oriented effects are descriptive evidence for any directed
    // metric with domain-valid complete pairs; gates consume them below.
    let directed = matches!(
        request.direction,
        MetricDirection::HigherIsBetter | MetricDirection::LowerIsBetter
    );
    let domain_valid = directed && complete_pairs_domain_valid(&selection.complete);
    let mut pair_effects = Vec::new();
    if domain_valid {
        for pair in &selection.complete {
            let difference = oriented_log_difference(
                pair.candidate_value,
                pair.baseline_value,
                &request.direction,
            );
            let effect = difference.exp() - 1.0;
            if effect.is_finite() {
                pair_effects.push(PairedEffect {
                    pair_id: pair.pair_id,
                    effect,
                });
            }
        }
    }
    let drift = drift_diagnostics(&pair_effects);
    let paired_record = PairedMetricRecord {
        name: request.name.clone(),
        pairs_complete: selection.complete.iter().map(|pair| pair.pair_id).collect(),
        pairs_excluded: selection.excluded_pairs.clone(),
        pair_effects,
        drift,
        effective_seed: None,
    };
    let finish =
        |record: MetricComparison, mut paired_record: PairedMetricRecord, seed: Option<u64>| {
            paired_record.effective_seed = seed;
            (record, Some(paired_record))
        };
    let Some(gate) = &request.gate else {
        // Ungated metrics are descriptive by construction.
        return finish(record, paired_record, None);
    };
    if request.intent != MetricIntent::Primary {
        // Plan validation forbids gated diagnostics; stay fail-closed anyway.
        record.disposition = Some(GateDisposition::Invalid);
        record.reason = Some("diagnostic_metric_cannot_gate".to_owned());
        return finish(record, paired_record, None);
    }
    // Absolute gates over paired evidence would silently mix variants.
    if matches!(gate, Gate::Absolute { .. }) {
        record.disposition = Some(GateDisposition::Invalid);
        record.reason = Some("absolute_gate_unsupported_for_paired_evidence".to_owned());
        return finish(record, paired_record, None);
    }
    if let Err(reason) = check_metric_semantics(request, input) {
        record.disposition = Some(GateDisposition::Invalid);
        record.reason = Some(reason);
        return finish(record, paired_record, None);
    }
    if !directed {
        record.disposition = Some(GateDisposition::Invalid);
        record.reason = Some("unsupported_direction_for_relative_gate".to_owned());
        return finish(record, paired_record, None);
    }
    if !domain_valid {
        record.disposition = Some(GateDisposition::Invalid);
        record.reason = Some("nonpositive_relative_value".to_owned());
        return finish(record, paired_record, None);
    }
    let differences: Vec<f64> = selection
        .complete
        .iter()
        .map(|pair| {
            oriented_log_difference(
                pair.candidate_value,
                pair.baseline_value,
                &request.direction,
            )
        })
        .collect();
    let mean_difference = arithmetic_mean(&differences).expect("complete pairs are non-empty");
    record.degradation = Some(mean_difference.exp() - 1.0);
    match gate {
        Gate::RelativeRegression { .. } => {
            let threshold = record.threshold.expect("relative gate has threshold");
            let degradation = record
                .degradation
                .expect("domain-valid pairs yield degradation");
            if !degradation.is_finite() {
                record.disposition = Some(GateDisposition::Invalid);
                record.reason = Some("nonfinite_paired_effect".to_owned());
                return finish(record, paired_record, None);
            }
            record.disposition = Some(if degradation <= threshold {
                GateDisposition::Pass
            } else {
                GateDisposition::Fail
            });
            finish(record, paired_record, None)
        }
        Gate::StatisticalRelative { min_trials, .. } => {
            let required = min_observations(min_trials.get());
            if selection.complete.len() < required {
                record.disposition = Some(GateDisposition::Invalid);
                record.reason = Some("insufficient_pairs".to_owned());
                return finish(record, paired_record, None);
            }
            if selection.complete.len() < RECOMMENDED_OBSERVATIONS_PER_SIDE {
                warnings.push(ComparisonWarning {
                    category: "below_recommended_pair_count".to_owned(),
                    detail: format!(
                        "metric {} has {} complete pairs; 7+ recommended",
                        request.name,
                        selection.complete.len(),
                    ),
                });
            }
            let threshold = record.threshold.expect("statistical gate has threshold");
            let (low, high) =
                paired_bootstrap_interval(&differences, BOOTSTRAP_RESAMPLES, effective_seed);
            if !low.is_finite() || !high.is_finite() {
                record.disposition = Some(GateDisposition::Invalid);
                record.reason = Some("nonfinite_paired_effect".to_owned());
                return finish(record, paired_record, None);
            }
            record.confidence_low = Some(low);
            record.confidence_high = Some(high);
            record.statistical_method = Some(STATISTICAL_METHOD_V2.to_owned());
            record.resamples = Some(BOOTSTRAP_RESAMPLES);
            record.effective_seed = Some(effective_seed);
            record.disposition = Some(if low > threshold {
                GateDisposition::Fail
            } else if high <= threshold {
                GateDisposition::Pass
            } else {
                GateDisposition::Inconclusive
            });
            finish(record, paired_record, Some(effective_seed))
        }
        Gate::Absolute { .. } => {
            // Handled above; unreachable here.
            record.disposition = Some(GateDisposition::Invalid);
            record.reason = Some("absolute_gate_unsupported_for_paired_evidence".to_owned());
            finish(record, paired_record, None)
        }
    }
}

/// Join arm-classified trials into complete pairs.
///
/// One trial contributes at most one scalar per arm. Trials without usable
/// pair identity are excluded on both sides; pairs with only one arm present
/// are excluded whole — pairs are never split and nothing is imputed.
#[allow(clippy::too_many_lines)] // Per-trial classification mirrors select_trials.
fn select_pairs(input: &ComparisonInput, metric: &Name) -> PairSelection {
    let mut candidate_values: BTreeMap<u32, (u32, f64)> = BTreeMap::new();
    let mut baseline_values: BTreeMap<u32, (u32, f64)> = BTreeMap::new();
    let mut candidate_excluded = Vec::new();
    let mut baseline_excluded = Vec::new();
    let mut seen: BTreeMap<(TrialArm, u32), u32> = BTreeMap::new();
    for trial in &input.trials {
        let trial_number = trial.id.get();
        let (Some(arm), Some(pair_id)) = (trial.arm, trial.pair_id) else {
            let exclusion = ExcludedTrial {
                trial_id: trial_number,
                reason: "trial_missing_pair_identity".to_owned(),
            };
            candidate_excluded.push(exclusion.clone());
            baseline_excluded.push(exclusion);
            continue;
        };
        if seen.insert((arm, pair_id), trial_number).is_some() {
            let exclusion = ExcludedTrial {
                trial_id: trial_number,
                reason: "duplicate_pair_identity".to_owned(),
            };
            match arm {
                TrialArm::Baseline => baseline_excluded.push(exclusion),
                TrialArm::Candidate => candidate_excluded.push(exclusion),
            }
            continue;
        }
        let exclusion_for = |reason: String| ExcludedTrial {
            trial_id: trial_number,
            reason,
        };
        let Some(metrics) = &trial.metrics else {
            let exclusion = exclusion_for("no_normalized_metrics".to_owned());
            match arm {
                TrialArm::Baseline => baseline_excluded.push(exclusion),
                TrialArm::Candidate => candidate_excluded.push(exclusion),
            }
            continue;
        };
        let Some(observation) = metrics
            .observations
            .iter()
            .find(|observation| &observation.name == metric)
        else {
            let exclusion = exclusion_for("metric_not_requested".to_owned());
            match arm {
                TrialArm::Baseline => baseline_excluded.push(exclusion),
                TrialArm::Candidate => candidate_excluded.push(exclusion),
            }
            continue;
        };
        if trial.terminal != TrialExecutionStatus::Completed {
            let exclusion = exclusion_for("trial_not_completed".to_owned());
            match arm {
                TrialArm::Baseline => baseline_excluded.push(exclusion),
                TrialArm::Candidate => candidate_excluded.push(exclusion),
            }
            continue;
        }
        let value = match &observation.state {
            ObservationState::Observed { value } => *value,
            ObservationState::Missing { reason } => {
                let exclusion = exclusion_for(missing_reason_str(*reason));
                match arm {
                    TrialArm::Baseline => baseline_excluded.push(exclusion),
                    TrialArm::Candidate => candidate_excluded.push(exclusion),
                }
                continue;
            }
            ObservationState::Invalid { reason, .. } => {
                let exclusion = exclusion_for(invalid_reason_str(*reason));
                match arm {
                    TrialArm::Baseline => baseline_excluded.push(exclusion),
                    TrialArm::Candidate => candidate_excluded.push(exclusion),
                }
                continue;
            }
        };
        match arm {
            TrialArm::Baseline => {
                baseline_values.insert(pair_id, (trial_number, value));
            }
            TrialArm::Candidate => {
                candidate_values.insert(pair_id, (trial_number, value));
            }
        }
    }
    let mut pair_ids: BTreeSet<u32> = BTreeSet::new();
    pair_ids.extend(candidate_values.keys().copied());
    pair_ids.extend(baseline_values.keys().copied());
    let mut complete = Vec::new();
    let mut excluded_pairs = Vec::new();
    for pair_id in pair_ids {
        match (
            candidate_values.get(&pair_id),
            baseline_values.get(&pair_id),
        ) {
            (Some((candidate_trial, candidate_value)), Some((baseline_trial, baseline_value))) => {
                complete.push(CompletePair {
                    pair_id,
                    candidate_trial: *candidate_trial,
                    baseline_trial: *baseline_trial,
                    candidate_value: *candidate_value,
                    baseline_value: *baseline_value,
                });
            }
            _ => excluded_pairs.push(ExcludedPair {
                pair_id,
                reason: "pair_incomplete".to_owned(),
            }),
        }
    }
    // Included trial lists follow pair execution order, never manifest order.
    let candidate_included = complete.iter().map(|pair| pair.candidate_trial).collect();
    let baseline_included = complete.iter().map(|pair| pair.baseline_trial).collect();
    PairSelection {
        complete,
        excluded_pairs,
        candidate_included,
        candidate_excluded,
        baseline_included,
        baseline_excluded,
    }
}

/// True when every complete pair carries finite strictly positive values on
/// both arms (the domain precondition for relative gates).
fn complete_pairs_domain_valid(pairs: &[CompletePair]) -> bool {
    !pairs.is_empty()
        && pairs.iter().all(|pair| {
            pair.candidate_value.is_finite()
                && pair.baseline_value.is_finite()
                && pair.candidate_value > 0.0
                && pair.baseline_value > 0.0
        })
}

/// Oriented log-difference for one pair: positive means the candidate is
/// worse than the baseline under the declared direction.
fn oriented_log_difference(candidate: f64, baseline: f64, direction: &MetricDirection) -> f64 {
    match direction {
        MetricDirection::LowerIsBetter => candidate.ln() - baseline.ln(),
        MetricDirection::HigherIsBetter => baseline.ln() - candidate.ln(),
        MetricDirection::TargetRange { .. } | MetricDirection::Informational => 0.0,
    }
}

/// Descriptive drift diagnostics over pair effects in execution order.
///
/// Halves split at `len / 2`; for odd counts the second half holds one more
/// pair. The trend is the exact sign of the half-mean difference.
fn drift_diagnostics(effects: &[PairedEffect]) -> DriftDiagnostics {
    if effects.len() < 2 {
        return DriftDiagnostics {
            first_half_mean: None,
            second_half_mean: None,
            trend: DriftTrend::Insufficient,
        };
    }
    let half = effects.len() / 2;
    let first: Vec<f64> = effects[..half].iter().map(|entry| entry.effect).collect();
    let second: Vec<f64> = effects[half..].iter().map(|entry| entry.effect).collect();
    let first_half_mean = arithmetic_mean(&first);
    let second_half_mean = arithmetic_mean(&second);
    let trend = match (first_half_mean, second_half_mean) {
        (Some(first), Some(second)) => {
            if second > first {
                DriftTrend::Up
            } else if second < first {
                DriftTrend::Down
            } else {
                DriftTrend::Flat
            }
        }
        _ => DriftTrend::Insufficient,
    };
    DriftDiagnostics {
        first_half_mean,
        second_half_mean,
        trend,
    }
}

/// Derive a deterministic paired base seed from the single bundle identity
/// and the paired policy identifier.
fn derive_paired_base_seed(input: &ComparisonInput) -> u64 {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(input.identity.manifest_sha256.as_bytes());
    bytes.extend_from_slice(b"|");
    bytes.extend_from_slice(COMPARISON_POLICY_V2.as_bytes());
    fnv1a64(&bytes)
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
    let (workload_match, workload_detail) = compare_workload(candidate, baseline);
    let (driver_match, driver_detail) = compare_driver(candidate, baseline);
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
        // The workstation-local fixture path is never comparison-critical;
        // digest/schema/provenance identity lives in `semantic-replay.json`
        // evidence and is checked by `compare_semantic_replay`. The workload
        // summary keeps only kind + target so semantic vs non-semantic
        // bundles are incomparable without leaking local paths.
        Workload::SemanticReplay { target, .. } => format!("semantic_replay target={target}"),
    }
}

fn termination_summary(requests: Option<u32>, duration_ms: Option<u64>) -> String {
    match (requests, duration_ms) {
        (Some(count), None) => format!("requests={count}"),
        (None, Some(ms)) => format!("duration_ms={ms}"),
        other => format!("invalid-termination={other:?}"),
    }
}

fn compare_workload(candidate: &ComparisonInput, baseline: &ComparisonInput) -> (bool, String) {
    let left = workload_summary(&candidate.resolved.workload);
    let right = workload_summary(&baseline.resolved.workload);
    if left != right {
        return (
            false,
            format!("workload semantics differ: candidate {left} vs baseline {right}"),
        );
    }
    let (replay_match, replay_detail) = compare_semantic_replay(candidate, baseline);
    if !replay_match {
        return (
            false,
            format!("workload semantics match ({left}); {replay_detail}"),
        );
    }
    if left.contains("semantic_replay") {
        (
            true,
            format!("workload semantics match ({left}); {replay_detail}"),
        )
    } else {
        (true, format!("workload semantics match ({left})"))
    }
}

fn semantic_replay_summary(input: &ComparisonInput) -> String {
    if !matches!(input.resolved.workload, Workload::SemanticReplay { .. }) {
        return "absent".to_owned();
    }
    match &input.semantic_replay_evidence {
        None => "missing-evidence".to_owned(),
        Some(evidence) => format!(
            "digest={} session={} envelope={} report={} tool={}:{}",
            evidence.fixture_digest,
            evidence.fixture_session_schema,
            evidence.envelope_schema,
            evidence.report_schema,
            evidence.executable_version,
            evidence.executable_sha256,
        ),
    }
}

fn compare_semantic_replay(
    candidate: &ComparisonInput,
    baseline: &ComparisonInput,
) -> (bool, String) {
    let left_is_replay = matches!(candidate.resolved.workload, Workload::SemanticReplay { .. });
    let right_is_replay = matches!(baseline.resolved.workload, Workload::SemanticReplay { .. });
    if !left_is_replay && !right_is_replay {
        return (true, "semantic replay absent on both sides".to_owned());
    }
    let left = semantic_replay_summary(candidate);
    let right = semantic_replay_summary(baseline);
    if left == right {
        (true, format!("semantic replay semantics match ({left})"))
    } else {
        (
            false,
            format!("semantic replay differs: candidate {left} vs baseline {right}"),
        )
    }
}

fn compare_driver(candidate: &ComparisonInput, baseline: &ComparisonInput) -> (bool, String) {
    let workload = compare_workload_driver(&candidate.resolved, &baseline.resolved);
    // Diagnostic configuration is comparison-critical for runs that claim
    // comparable diagnostic context; observed probe statuses/timings never
    // participate (see diagnostics_summary). Network-path and diagnostics
    // never coexist in one bundle (plan validation rejects the combination),
    // but both dimensions are still compared across bundles.
    let (diagnostics_match, diagnostics_detail) = compare_diagnostics(candidate, baseline);
    // Security-check configuration is comparison-critical for runs that
    // claim comparable correctness context; observed pass/fail/counts never
    // participate (see security_summary). M004a rejects security+path
    // composition in one bundle, but the dimension is still compared
    // across bundles.
    let (security_match, security_detail) = compare_security(candidate, baseline);
    if candidate.resolved.network_path.is_none() && baseline.resolved.network_path.is_none() {
        return (
            workload.0 && diagnostics_match && security_match,
            format!(
                "{}; {}; {}",
                workload.1, diagnostics_detail, security_detail
            ),
        );
    }
    let path = compare_network_path(candidate, baseline);
    (
        workload.0 && path.0 && diagnostics_match && security_match,
        format!(
            "{}; {}; {}; {}",
            workload.1, path.1, diagnostics_detail, security_detail
        ),
    )
}

fn diagnostics_summary(input: &ComparisonInput) -> String {
    if input.resolved.diagnostics.is_empty() {
        return "absent".to_owned();
    }
    match &input.diagnostics_evidence {
        None => "missing-evidence".to_owned(),
        Some(evidence) => format!(
            "requests=[{}] tool={}:{} schema={}",
            evidence.requests.join(" | "),
            evidence.executable_version,
            evidence.executable_sha256,
            evidence.machine_schema,
        ),
    }
}

fn compare_diagnostics(candidate: &ComparisonInput, baseline: &ComparisonInput) -> (bool, String) {
    let left = diagnostics_summary(candidate);
    let right = diagnostics_summary(baseline);
    if left == right {
        (true, format!("diagnostics match ({left})"))
    } else {
        (
            false,
            format!("diagnostics differ: candidate {left} vs baseline {right}"),
        )
    }
}

fn security_summary(input: &ComparisonInput) -> String {
    if input.resolved.security_checks.is_empty() {
        return "absent".to_owned();
    }
    match &input.security_evidence {
        None => "missing-evidence".to_owned(),
        Some(evidence) => format!(
            "checks=[{}] tool={}:{} scope={} adapter={}",
            evidence.checks.join(" | "),
            evidence.executable_version,
            evidence.executable_sha256,
            evidence.scope_sha256,
            evidence.adapter_semantic_version,
        ),
    }
}

fn compare_security(candidate: &ComparisonInput, baseline: &ComparisonInput) -> (bool, String) {
    let left = security_summary(candidate);
    let right = security_summary(baseline);
    if left == right {
        (true, format!("security checks match ({left})"))
    } else {
        (
            false,
            format!("security checks differ: candidate {left} vs baseline {right}"),
        )
    }
}

fn compare_workload_driver(candidate: &ResolvedPlan, baseline: &ResolvedPlan) -> (bool, String) {
    let left = candidate.drivers.get(&DriverCategory::Workload);
    let right = baseline.drivers.get(&DriverCategory::Workload);
    match (left, right) {
        (None, None) => (true, "no workload driver on either side".to_owned()),
        (Some(_), None) | (None, Some(_)) => {
            (false, "workload driver present on one side only".to_owned())
        }
        (Some(left), Some(right)) => {
            let left_summary = descriptor_summary(&left.descriptor);
            let right_summary = descriptor_summary(&right.descriptor);
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

fn compare_network_path(candidate: &ComparisonInput, baseline: &ComparisonInput) -> (bool, String) {
    let left = network_path_summary(candidate);
    let right = network_path_summary(baseline);
    if left == right {
        (true, format!("network path semantics match ({left})"))
    } else {
        (
            false,
            format!("network path differs: candidate {left} vs baseline {right}"),
        )
    }
}

fn descriptor_summary(descriptor: &crate::DriverDescriptor) -> String {
    let mut capabilities: Vec<String> = descriptor
        .capabilities
        .iter()
        .map(|capability| format!("{capability:?}"))
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
}

fn stable_digest(value: &impl Serialize) -> String {
    let bytes = serde_json::to_vec(value).expect("resolved path identity serializes");
    format!("{:x}", sha2::Sha256::digest(bytes))
}

fn network_path_summary(input: &ComparisonInput) -> String {
    let resolved = &input.resolved;
    let Some(path) = &resolved.network_path else {
        return "absent".to_owned();
    };
    let route = input
        .network_path_evidence
        .as_ref()
        .and_then(|evidence| evidence.chain_config_digest.clone())
        .unwrap_or_else(|| stable_digest(&path.route));
    let route_driver = descriptor_summary(&path.route_driver.descriptor);
    let eggress_uri_version = input.network_path_evidence.as_ref().map_or_else(
        || "unverified".to_owned(),
        |evidence| evidence.eggress_uri_version.clone(),
    );
    let faults = path.stream_faults.as_ref().map_or_else(
        || "absent".to_owned(),
        |faults| {
            let active =
                !faults.request.upstream.is_empty() || !faults.request.downstream.is_empty();
            let seed = if active {
                resolved.seed.unwrap_or_default()
            } else {
                0
            };
            format!(
                "request={} driver={} rng={} seed={seed}",
                stable_digest(&faults.request),
                descriptor_summary(&faults.fault_driver.descriptor),
                faults.rng_version
            )
        },
    );
    format!(
        "route={route} route_driver={route_driver} eggress_uri={eggress_uri_version} semantics={} faults={faults}",
        path.semantics_version
    )
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
/// For `n = 10_000` these are indices 250 and 9749. Shared by both policies.
fn percentile_interval(mut effects: Vec<f64>) -> (f64, f64) {
    effects.sort_by(f64::total_cmp);
    let count = effects.len();
    let low_index = (25_usize.saturating_mul(count) / 1000).min(count - 1);
    let high_index = (975_usize.saturating_mul(count).saturating_add(999) / 1000)
        .saturating_sub(1)
        .min(count - 1);
    (effects[low_index], effects[high_index])
}
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
    percentile_interval(effects)
}

/// Deterministic 95% percentile bootstrap interval over paired oriented
/// degradation.
///
/// Paired only: each resample draws `differences.len()` oriented
/// log-differences with replacement, takes their mean, and transforms to
/// degradation space. Quantile extraction is identical to policy v1.
fn paired_bootstrap_interval(differences: &[f64], resamples: usize, seed: u64) -> (f64, f64) {
    debug_assert!(!differences.is_empty());
    let mut rng = SplitMix64::new(seed);
    let mut effects = Vec::with_capacity(resamples);
    for _ in 0..resamples {
        let mean = resampled_mean_log(differences, &mut rng);
        effects.push(mean.exp() - 1.0);
    }
    percentile_interval(effects)
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
fn derive_base_seed(
    candidate: &ComparisonInput,
    baseline: Option<&ComparisonInput>,
    policy_id: &str,
) -> u64 {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(candidate.identity.manifest_sha256.as_bytes());
    bytes.extend_from_slice(b"|");
    match baseline {
        Some(input) => bytes.extend_from_slice(input.identity.manifest_sha256.as_bytes()),
        None => bytes.extend_from_slice(b"absolute-only"),
    }
    bytes.extend_from_slice(b"|");
    bytes.extend_from_slice(policy_id.as_bytes());
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
    let result: TrialExecutionResult = serde_json::from_slice(&bytes)
        .map_err(|error| BundleError::ManifestParse(error.to_string()))?;
    if (result.schema_version != SchemaVersion(1) && result.schema_version != SchemaVersion(2))
        || result.trial_id != descriptor.id
    {
        return Err(BundleError::InvalidManifest(
            "trial result schema or identity does not match its manifest descriptor",
        )
        .into());
    }
    Ok(result)
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
        trial_metrics_for_directed(id, pairs, &MetricDirection::LowerIsBetter)
    }

    fn trial_metrics_for_directed(
        id: u32,
        pairs: &[(&str, &str, f64)],
        direction: &MetricDirection,
    ) -> TrialMetrics {
        let observations = pairs
            .iter()
            .map(|(name, unit, value)| NormalizedObservation {
                name: metric_name(name),
                unit: metric_name(unit),
                direction: direction.clone(),
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
            paired: None,
            network_path: None,
            diagnostics: Vec::new(),
            security_checks: Vec::new(),
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
                    arm: None,
                    pair_id: None,
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
            paired: None,
            network_path_evidence: None,
            semantic_replay_evidence: None,
            diagnostics_evidence: None,
            security_evidence: None,
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

    fn attach_network_path(resolved: &mut ResolvedPlan, proxy: bool, faults: bool) {
        let route_mode = if proxy {
            serde_json::json!({
                "kind": "proxy_chain",
                "chain": "http://proxy.example:8080"
            })
        } else {
            serde_json::json!({ "kind": "direct" })
        };
        let stream_faults = faults.then(|| {
            serde_json::json!({
                "driver": "eggchaos-stream",
                "upstream": [{
                    "id": "latency",
                    "kind": {
                        "kind": "latency",
                        "delay_ms": 5,
                        "jitter_ms": 1,
                        "max_buffer_bytes": 1024
                    }
                }],
                "downstream": []
            })
        });
        let mut value = serde_json::json!({
            "route": {
                "driver": "eggress-route",
                "mode": route_mode
            },
            "route_driver": {
                "descriptor": {
                    "name": "eggress-route",
                    "adapter_version": "0.1.0",
                    "upstream_name": "eggress-outbound",
                    "upstream_version": "1.0.10",
                    "category": "route",
                    "capabilities": [{ "kind": "proxy_routing" }],
                    "supported_platforms": [],
                    "machine_output_schema": null,
                    "external_process": false,
                    "default": true,
                    "compatible_service_types": []
                },
                "executable_path": null
            },
            "semantics_version": "route-first-fault-second-v1"
        });
        if let Some(faults) = stream_faults {
            value["stream_faults"] = serde_json::json!({
                "request": faults,
                "fault_driver": {
                    "descriptor": {
                        "name": "eggchaos-stream",
                        "adapter_version": "0.1.0",
                        "upstream_name": "eggchaos-core",
                        "upstream_version": "0.1.0",
                        "category": "fault",
                        "capabilities": [{ "kind": "stream_fault_plan" }],
                        "supported_platforms": [],
                        "machine_output_schema": null,
                        "external_process": false,
                        "default": true,
                        "compatible_service_types": []
                    },
                    "executable_path": null
                },
                "rng_version": "splitmix64-v1"
            });
        }
        resolved.network_path = Some(serde_json::from_value(value).expect("resolved path"));
        resolved.seed = Some(19);
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
    fn network_path_identity_is_comparison_critical() {
        let (mut candidate, mut baseline, _) = statistical_inputs(&[101.0; 7], &[100.0; 7]);
        attach_network_path(&mut candidate.resolved, true, true);
        attach_network_path(&mut baseline.resolved, true, true);
        let receipt = compare_pair(&candidate, &baseline);
        assert_eq!(receipt.policy_id, COMPARISON_POLICY_NETWORK_PATH_V1);
        assert_eq!(disposition_of(&receipt), Some(GateDisposition::Pass));

        baseline
            .resolved
            .network_path
            .as_mut()
            .expect("path")
            .route
            .mode = crate::RouteMode::Direct;
        let receipt = compare_pair(&candidate, &baseline);
        assert!(!receipt.comparability.driver_match);
        assert!(receipt.comparability.critical_mismatch);
        assert_eq!(disposition_of(&receipt), Some(GateDisposition::Invalid));

        let (mut candidate, mut baseline, _) = statistical_inputs(&[101.0; 7], &[100.0; 7]);
        attach_network_path(&mut candidate.resolved, false, true);
        attach_network_path(&mut baseline.resolved, false, true);
        baseline.resolved.seed = Some(23);
        assert_eq!(
            disposition_of(&compare_pair(&candidate, &baseline)),
            Some(GateDisposition::Invalid)
        );

        let (mut candidate, mut baseline, _) = statistical_inputs(&[101.0; 7], &[100.0; 7]);
        attach_network_path(&mut candidate.resolved, false, true);
        attach_network_path(&mut baseline.resolved, false, false);
        assert_eq!(
            disposition_of(&compare_pair(&candidate, &baseline)),
            Some(GateDisposition::Invalid)
        );
    }

    #[test]
    fn every_network_path_identity_dimension_is_comparison_critical() {
        type IdentityMutation = fn(&mut ComparisonInput, &mut ComparisonInput);
        let cases: Vec<(&str, IdentityMutation)> = vec![
            ("eggress-version", |candidate, baseline| {
                baseline
                    .resolved
                    .network_path
                    .as_mut()
                    .expect("baseline path")
                    .route_driver
                    .descriptor
                    .upstream_version = Some("1.0.9".to_owned());
                let _ = candidate;
            }),
            ("eggress-uri-version", |candidate, baseline| {
                candidate.network_path_evidence = Some(NetworkPathEvidenceIdentity {
                    chain_config_digest: None,
                    eggress_uri_version: "1.0.10".to_owned(),
                });
                baseline.network_path_evidence = Some(NetworkPathEvidenceIdentity {
                    chain_config_digest: None,
                    eggress_uri_version: "1.0.9".to_owned(),
                });
            }),
            ("eggchaos-version", |candidate, baseline| {
                candidate
                    .resolved
                    .network_path
                    .as_mut()
                    .expect("candidate path")
                    .stream_faults
                    .as_mut()
                    .expect("candidate faults")
                    .fault_driver
                    .descriptor
                    .upstream_version = Some("0.1.1".to_owned());
                let _ = baseline;
            }),
            ("chain", |candidate, baseline| {
                candidate
                    .resolved
                    .network_path
                    .as_mut()
                    .expect("candidate path")
                    .route
                    .mode = crate::RouteMode::ProxyChain {
                    chain: "http://other-proxy.example:8080".to_owned(),
                };
                let _ = baseline;
            }),
            ("upstream-plan", |candidate, baseline| {
                candidate
                    .resolved
                    .network_path
                    .as_mut()
                    .expect("candidate path")
                    .stream_faults
                    .as_mut()
                    .expect("candidate faults")
                    .request
                    .upstream[0]
                    .id = Name::new("other-upstream").expect("fault id");
                let _ = baseline;
            }),
            ("downstream-plan", |candidate, baseline| {
                candidate
                    .resolved
                    .network_path
                    .as_mut()
                    .expect("candidate path")
                    .stream_faults
                    .as_mut()
                    .expect("candidate faults")
                    .request
                    .downstream = vec![
                    serde_json::from_value(serde_json::json!({
                        "id": "downstream",
                        "kind": { "kind": "blackhole" }
                    }))
                    .expect("fault request"),
                ];
                let _ = baseline;
            }),
        ];
        for (name, mutate) in cases {
            let (mut candidate, mut baseline, _) = statistical_inputs(&[101.0; 7], &[100.0; 7]);
            attach_network_path(&mut candidate.resolved, true, true);
            attach_network_path(&mut baseline.resolved, true, true);
            mutate(&mut candidate, &mut baseline);
            let receipt = compare_pair(&candidate, &baseline);
            assert!(receipt.comparability.critical_mismatch, "{name}");
            assert_eq!(
                disposition_of(&receipt),
                Some(GateDisposition::Invalid),
                "{name}"
            );
        }
    }

    #[test]
    fn canonical_evidence_identity_controls_equivalent_route_spellings() {
        let (mut candidate, mut baseline, _) = statistical_inputs(&[101.0; 7], &[100.0; 7]);
        attach_network_path(&mut candidate.resolved, true, false);
        attach_network_path(&mut baseline.resolved, true, false);
        candidate
            .resolved
            .network_path
            .as_mut()
            .expect("candidate path")
            .route
            .mode = crate::RouteMode::ProxyChain {
            chain: "socks4a://proxy.example:1080".to_owned(),
        };
        baseline
            .resolved
            .network_path
            .as_mut()
            .expect("baseline path")
            .route
            .mode = crate::RouteMode::ProxyChain {
            chain: "socks4://proxy.example:1080".to_owned(),
        };
        let identity = NetworkPathEvidenceIdentity {
            chain_config_digest: Some("canonical-socks4-digest".to_owned()),
            eggress_uri_version: "1.0.10".to_owned(),
        };
        candidate.network_path_evidence = Some(identity.clone());
        baseline.network_path_evidence = Some(identity);
        assert_eq!(
            disposition_of(&compare_pair(&candidate, &baseline)),
            Some(GateDisposition::Pass)
        );
    }

    #[test]
    fn stored_route_evidence_must_be_bound_to_the_resolved_chain() {
        let evidence: StoredNetworkPathEvidence = serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "adapter_version": "adapter-v1",
            "route_driver": {
                "name": "eggress-route",
                "adapter_version": "adapter-v1",
                "upstream_name": "eggress-outbound",
                "upstream_version": "1.0.10"
            },
            "eggress_outbound_version": "1.0.10",
            "eggress_uri_version": "1.0.10",
            "fault_driver": null,
            "semantics": {
                "ordering_version": "route-first-fault-second-v1",
                "ordering": "route_first_fault_second",
                "fault_layer": "user_space_stream",
                "upstream": "client_to_target",
                "downstream": "target_to_client"
            },
            "route": {
                "driver": "eggress-route",
                "mode": { "kind": "proxy_chain", "chain": "http://declared.example:8080" }
            },
            "redacted_chain": "http://forged.example:8080",
            "chain_config_digest": format!("{:x}", sha2::Sha256::digest(b"http://forged.example:8080")),
            "configured_hop_count": 1,
            "stream_faults": null,
            "diagnostics": {
                "physical_dial_attempts": 0,
                "successful_dials": 0,
                "fault_wrapped_connections": 0,
                "fault_wrapper_construction_failures": 0,
                "route_failure_buckets_dropped": 0,
                "route_failures": {},
                "hop_count_distribution": {},
                "max_observed_hop_count": 0,
                "connection_ordinal_min": 0,
                "connection_ordinal_max": 0,
                "connection_ordinal_count": 0
            },
            "policy_mode": "static"
        }))
        .expect("stored evidence");
        assert!(stored_network_path_request_is_valid(&evidence));
        assert!(!stored_canonical_route_matches(&evidence));
    }

    #[test]
    fn seed_matters_only_when_faults_are_active() {
        let (mut candidate, mut baseline, _) = statistical_inputs(&[101.0; 7], &[100.0; 7]);
        attach_network_path(&mut candidate.resolved, true, false);
        attach_network_path(&mut baseline.resolved, true, false);
        baseline.resolved.seed = Some(101);
        assert_eq!(
            disposition_of(&compare_pair(&candidate, &baseline)),
            Some(GateDisposition::Pass)
        );

        let (mut candidate, mut baseline, _) = statistical_inputs(&[101.0; 7], &[100.0; 7]);
        attach_network_path(&mut candidate.resolved, true, true);
        attach_network_path(&mut baseline.resolved, true, true);
        baseline.resolved.seed = Some(101);
        assert_eq!(
            disposition_of(&compare_pair(&candidate, &baseline)),
            Some(GateDisposition::Invalid)
        );
    }

    #[test]
    fn warn_policy_preserves_path_effects_but_suppresses_relative_verdict() {
        let metric = latency_request(relative_gate(500));
        let mut candidate = input_with_values(
            &"aa".repeat(32),
            metric.clone(),
            EnvironmentPolicy::WarnOnMismatch,
            &[130.0; 7],
            standard_environment(),
        );
        let mut baseline = input_with_values(
            &"bb".repeat(32),
            metric,
            EnvironmentPolicy::WarnOnMismatch,
            &[100.0; 7],
            standard_environment(),
        );
        attach_network_path(&mut candidate.resolved, true, true);
        attach_network_path(&mut baseline.resolved, false, true);
        let receipt = compare_pair(&candidate, &baseline);
        assert!(receipt.comparability.critical_mismatch);
        assert!(receipt.metrics[0].degradation.is_some());
        assert_eq!(disposition_of(&receipt), Some(GateDisposition::Descriptive));
        assert_eq!(receipt.aggregate_verdict, None);
    }

    #[test]
    fn cross_testbed_path_mismatch_remains_descriptive() {
        let metric = latency_request(statistical_gate(500));
        let mut candidate = input_with_values(
            &"aa".repeat(32),
            metric.clone(),
            EnvironmentPolicy::CrossTestbedDescriptive,
            &[130.0; 7],
            standard_environment(),
        );
        let mut baseline = input_with_values(
            &"bb".repeat(32),
            metric,
            EnvironmentPolicy::CrossTestbedDescriptive,
            &[100.0; 7],
            standard_environment(),
        );
        attach_network_path(&mut candidate.resolved, true, true);
        attach_network_path(&mut baseline.resolved, false, true);
        let receipt = compare_pair(&candidate, &baseline);
        assert!(receipt.comparability.critical_mismatch);
        assert_eq!(disposition_of(&receipt), Some(GateDisposition::Descriptive));
        assert_eq!(receipt.aggregate_verdict, None);
    }

    #[test]
    fn absolute_gate_remains_eligible_when_path_identity_differs() {
        let metric = latency_request(Gate::Absolute { value: 500.0 });
        let mut candidate = input_with_values(
            &"aa".repeat(32),
            metric.clone(),
            EnvironmentPolicy::WarnOnMismatch,
            &[100.0; 3],
            standard_environment(),
        );
        let mut baseline = input_with_values(
            &"bb".repeat(32),
            metric,
            EnvironmentPolicy::WarnOnMismatch,
            &[100.0; 3],
            standard_environment(),
        );
        attach_network_path(&mut candidate.resolved, true, true);
        attach_network_path(&mut baseline.resolved, false, true);
        let receipt = compare_pair(&candidate, &baseline);
        assert!(receipt.comparability.critical_mismatch);
        assert_eq!(disposition_of(&receipt), Some(GateDisposition::Pass));
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

    fn semantic_input(digest: &str) -> ComparisonInput {
        let (mut candidate, _, _) = statistical_inputs(&[0.0; 7], &[0.0; 7]);
        candidate.resolved.workload = Workload::SemanticReplay {
            target: metric_name("origin"),
            fixture: "fixtures/replay".to_owned(),
        };
        candidate.semantic_replay_evidence = Some(SemanticReplayEvidenceIdentity {
            fixture_digest: digest.to_owned(),
            fixture_session_schema: 2,
            envelope_schema: 1,
            report_schema: 2,
            executable_version: "0.1.0".to_owned(),
            executable_sha256: "ab".repeat(32),
        });
        candidate
    }

    #[test]
    fn semantic_replay_digest_mismatch_invalidates() {
        let candidate = semantic_input(&"aa".repeat(32));
        let mut baseline = semantic_input(&"aa".repeat(32));
        let receipt = compare(
            &ComparisonRequest {
                candidate: &candidate,
                baseline: Some(BaselineSide {
                    reference: BaselineReference::Bundle {
                        identity: baseline.identity.clone(),
                        path: "baseline.eggb".to_owned(),
                    },
                    input: &baseline,
                }),
            },
            &ComparisonOptions::default(),
        );
        assert!(receipt.comparability.workload_match);
        baseline.semantic_replay_evidence = Some(SemanticReplayEvidenceIdentity {
            fixture_digest: "bb".repeat(32),
            fixture_session_schema: 2,
            envelope_schema: 1,
            report_schema: 2,
            executable_version: "0.1.0".to_owned(),
            executable_sha256: "ab".repeat(32),
        });
        let receipt = compare(
            &ComparisonRequest {
                candidate: &candidate,
                baseline: Some(BaselineSide {
                    reference: BaselineReference::Bundle {
                        identity: baseline.identity.clone(),
                        path: "baseline.eggb".to_owned(),
                    },
                    input: &baseline,
                }),
            },
            &ComparisonOptions::default(),
        );
        assert!(!receipt.comparability.workload_match);
        assert!(receipt.comparability.critical_mismatch);
    }

    #[test]
    fn semantic_vs_non_semantic_workloads_are_incomparable() {
        let candidate = semantic_input(&"aa".repeat(32));
        let (baseline, _, _) = statistical_inputs(&[0.0; 7], &[0.0; 7]);
        let receipt = compare(
            &ComparisonRequest {
                candidate: &candidate,
                baseline: Some(BaselineSide {
                    reference: BaselineReference::Bundle {
                        identity: baseline.identity.clone(),
                        path: "baseline.eggb".to_owned(),
                    },
                    input: &baseline,
                }),
            },
            &ComparisonOptions::default(),
        );
        assert!(!receipt.comparability.workload_match);
    }

    fn diagnostics_input() -> ComparisonInput {
        let (mut candidate, _, _) = statistical_inputs(&[0.0; 7], &[0.0; 7]);
        candidate.resolved.diagnostics = vec![crate::DiagnosticRequest {
            id: Name::new("pre-check").unwrap(),
            source: Name::new("eggprobe").unwrap(),
            phase: crate::DiagnosticPhase::PreWorkload,
            target: metric_name("origin"),
            probes: vec![crate::DiagnosticProbe::Tcp, crate::DiagnosticProbe::Http],
            required: true,
            timeout_ms: crate::DurationMs::new(5000).unwrap(),
        }];
        candidate.diagnostics_evidence = Some(DiagnosticsEvidenceIdentity {
            requests: vec![
                "pre-check pre_workload required=true target=origin probes=[http,tcp] timeout_ms=5000"
                    .to_owned(),
            ],
            executable_version: "0.1.1".to_owned(),
            executable_sha256: "ab".repeat(32),
            machine_schema: "0.3".to_owned(),
        });
        candidate
    }

    #[test]
    fn identical_diagnostics_compare_equal() {
        let candidate = diagnostics_input();
        let baseline = diagnostics_input();
        let receipt = compare_pair(&candidate, &baseline);
        assert!(receipt.comparability.driver_match);
        assert!(!receipt.comparability.critical_mismatch);
    }

    #[test]
    fn diagnostic_config_or_provenance_mismatch_invalidates() {
        let candidate = diagnostics_input();
        for name in [
            "timeout",
            "probes",
            "required",
            "tool-version",
            "tool-digest",
            "machine-schema",
        ] {
            let mut baseline = diagnostics_input();
            match name {
                "timeout" => {
                    baseline.resolved.diagnostics[0].timeout_ms =
                        crate::DurationMs::new(6000).unwrap();
                    baseline.diagnostics_evidence.as_mut().unwrap().requests = vec![
                        "pre-check pre_workload required=true target=origin probes=[http,tcp] timeout_ms=6000"
                            .to_owned(),
                    ];
                }
                "probes" => {
                    baseline.resolved.diagnostics[0].probes = vec![crate::DiagnosticProbe::Tcp];
                    baseline.diagnostics_evidence.as_mut().unwrap().requests = vec![
                        "pre-check pre_workload required=true target=origin probes=[tcp] timeout_ms=5000"
                            .to_owned(),
                    ];
                }
                "required" => {
                    baseline.resolved.diagnostics[0].required = false;
                    baseline.diagnostics_evidence.as_mut().unwrap().requests = vec![
                        "pre-check pre_workload required=false target=origin probes=[http,tcp] timeout_ms=5000"
                            .to_owned(),
                    ];
                }
                "tool-version" => {
                    baseline
                        .diagnostics_evidence
                        .as_mut()
                        .unwrap()
                        .executable_version = "0.1.2".to_owned();
                }
                "tool-digest" => {
                    baseline
                        .diagnostics_evidence
                        .as_mut()
                        .unwrap()
                        .executable_sha256 = "bb".repeat(32);
                }
                _ => {
                    baseline
                        .diagnostics_evidence
                        .as_mut()
                        .unwrap()
                        .machine_schema = "0.4".to_owned();
                }
            }
            let receipt = compare_pair(&candidate, &baseline);
            assert!(!receipt.comparability.driver_match, "{name}");
            assert!(receipt.comparability.critical_mismatch, "{name}");
        }
    }

    #[test]
    fn diagnostics_vs_absent_runs_are_incomparable() {
        let candidate = diagnostics_input();
        let (baseline, _, _) = statistical_inputs(&[0.0; 7], &[0.0; 7]);
        let receipt = compare_pair(&candidate, &baseline);
        assert!(!receipt.comparability.driver_match);
        assert!(receipt.comparability.critical_mismatch);
    }

    fn security_input() -> ComparisonInput {
        let (mut candidate, _, _) = statistical_inputs(&[0.0; 7], &[0.0; 7]);
        candidate.resolved.security_checks = vec![crate::SecurityCheckRequest {
            id: Name::new("waf-sqli").unwrap(),
            source: Name::new(crate::SECURITY_SOURCE_EGGSEC_WAF).unwrap(),
            target: metric_name("origin"),
            test_type: crate::EggsecWafTestType::Sqli,
            max_successful_bypasses: 0,
            concurrency: crate::PositiveCount::new(2).unwrap(),
            timeout_ms: crate::DurationMs::new(5000).unwrap(),
        }];
        candidate.security_evidence = Some(SecurityEvidenceIdentity {
            checks: vec![
                "waf-sqli eggsec-waf origin sqli max_successful_bypasses=0 concurrency=2 timeout_ms=5000"
                    .to_owned(),
            ],
            executable_version: "0.1.0".to_owned(),
            executable_sha256: "ab".repeat(32),
            scope_sha256: "cd".repeat(32),
            adapter_semantic_version: crate::CORRECTNESS_ADAPTER_SEMANTIC_VERSION.to_owned(),
        });
        candidate
    }

    #[test]
    fn identical_security_checks_compare_equal() {
        let candidate = security_input();
        let baseline = security_input();
        let receipt = compare_pair(&candidate, &baseline);
        assert!(receipt.comparability.driver_match);
        assert!(!receipt.comparability.critical_mismatch);
    }

    #[test]
    fn security_config_or_provenance_mismatch_invalidates() {
        let candidate = security_input();
        for name in [
            "allowance",
            "test-type",
            "concurrency",
            "tool-version",
            "tool-digest",
            "scope-digest",
        ] {
            let mut baseline = security_input();
            match name {
                "allowance" => {
                    baseline.resolved.security_checks[0].max_successful_bypasses = 1;
                    baseline.security_evidence.as_mut().unwrap().checks = vec![
                        "waf-sqli eggsec-waf origin sqli max_successful_bypasses=1 concurrency=2 timeout_ms=5000"
                            .to_owned(),
                    ];
                }
                "test-type" => {
                    baseline.resolved.security_checks[0].test_type = crate::EggsecWafTestType::Xss;
                    baseline.security_evidence.as_mut().unwrap().checks = vec![
                        "waf-sqli eggsec-waf origin xss max_successful_bypasses=0 concurrency=2 timeout_ms=5000"
                            .to_owned(),
                    ];
                }
                "concurrency" => {
                    baseline.resolved.security_checks[0].concurrency =
                        crate::PositiveCount::new(4).unwrap();
                    baseline.security_evidence.as_mut().unwrap().checks = vec![
                        "waf-sqli eggsec-waf origin sqli max_successful_bypasses=0 concurrency=4 timeout_ms=5000"
                            .to_owned(),
                    ];
                }
                "tool-version" => {
                    baseline
                        .security_evidence
                        .as_mut()
                        .unwrap()
                        .executable_version = "0.2.0".to_owned();
                }
                "tool-digest" => {
                    baseline
                        .security_evidence
                        .as_mut()
                        .unwrap()
                        .executable_sha256 = "bb".repeat(32);
                }
                _ => {
                    baseline.security_evidence.as_mut().unwrap().scope_sha256 = "ee".repeat(32);
                }
            }
            let receipt = compare_pair(&candidate, &baseline);
            assert!(!receipt.comparability.driver_match, "{name}");
            assert!(receipt.comparability.critical_mismatch, "{name}");
        }
    }

    #[test]
    fn security_vs_absent_runs_are_incomparable() {
        let candidate = security_input();
        let (baseline, _, _) = statistical_inputs(&[0.0; 7], &[0.0; 7]);
        let receipt = compare_pair(&candidate, &baseline);
        assert!(!receipt.comparability.driver_match);
        assert!(receipt.comparability.critical_mismatch);
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

    // ---- Paired comparison (policy v2) tests ----

    fn throughput_request(gate: Gate) -> MetricRequest {
        MetricRequest {
            name: metric_name("throughput"),
            unit: metric_name("rps"),
            direction: MetricDirection::HigherIsBetter,
            intent: MetricIntent::Primary,
            gate: Some(gate),
        }
    }

    /// Build a paired input: per pair, trial `2i-1` measures baseline and
    /// trial `2i` measures candidate under the v1 alternating schedule.
    fn paired_input(
        sha: &str,
        metric: MetricRequest,
        metric_unit: &str,
        direction: &MetricDirection,
        pairs: &[(f64, f64)],
    ) -> ComparisonInput {
        let mut trials = Vec::new();
        for (index, (baseline_value, candidate_value)) in pairs.iter().enumerate() {
            let pair_id = u32::try_from(index + 1).unwrap();
            let baseline_trial = 2 * pair_id - 1;
            let candidate_trial = 2 * pair_id;
            trials.push(InputTrial {
                id: TrialId::new(baseline_trial).unwrap(),
                terminal: TrialExecutionStatus::Completed,
                arm: Some(TrialArm::Baseline),
                pair_id: Some(pair_id),
                metrics: Some(trial_metrics_for_directed(
                    baseline_trial,
                    &[(metric.name.as_str(), metric_unit, *baseline_value)],
                    direction,
                )),
            });
            trials.push(InputTrial {
                id: TrialId::new(candidate_trial).unwrap(),
                terminal: TrialExecutionStatus::Completed,
                arm: Some(TrialArm::Candidate),
                pair_id: Some(pair_id),
                metrics: Some(trial_metrics_for_directed(
                    candidate_trial,
                    &[(metric.name.as_str(), metric_unit, *candidate_value)],
                    direction,
                )),
            });
        }
        let pair_count = u32::try_from(pairs.len()).unwrap();
        let mut resolved = resolved_with(vec![metric], EnvironmentPolicy::StrictSameTestbed);
        resolved.paired = Some(crate::ResolvedPairedDesign {
            baseline: crate::ResolvedPairedArm {
                service: metric_name("origin-a"),
                subject: Subject::Label {
                    label: metric_name("variant-a"),
                },
            },
            candidate: crate::ResolvedPairedArm {
                service: metric_name("origin-b"),
                subject: Subject::Label {
                    label: metric_name("variant-b"),
                },
            },
            schedule: crate::PAIRED_SCHEDULE_V1.to_owned(),
            pairs: pair_count,
        });
        ComparisonInput {
            identity: BundleIdentity {
                manifest_schema_version: SchemaVersion(2),
                run_id: RunId::parse("123e4567-e89b-12d3-a456-426614174000").unwrap(),
                manifest_sha256: sha.to_owned(),
                subject_revision: None,
                subject_digest: None,
            },
            resolved,
            environment: standard_environment(),
            trials,
            paired: Some(PairedRunSummary {
                schedule: crate::PAIRED_SCHEDULE_V1.to_owned(),
                pairs: pair_count,
            }),
            network_path_evidence: None,
            semantic_replay_evidence: None,
            diagnostics_evidence: None,
            security_evidence: None,
        }
    }

    fn paired_latency_input(pairs: &[(f64, f64)]) -> (ComparisonInput, MetricRequest) {
        let metric = latency_request(statistical_gate(500));
        let input = paired_input(
            &"cc".repeat(32),
            metric.clone(),
            "ms",
            &MetricDirection::LowerIsBetter,
            pairs,
        );
        (input, metric)
    }

    fn compare_paired_test(input: &ComparisonInput) -> ComparisonReceipt {
        compare_paired(
            std::path::Path::new("paired.eggb"),
            input,
            &ComparisonOptions::default(),
        )
    }

    #[test]
    fn paired_clear_regression_fails() {
        let (input, _) = paired_latency_input(&[(100.0, 130.0); 6]);
        let receipt = compare_paired_test(&input);
        assert_eq!(receipt.policy_id, COMPARISON_POLICY_V2);
        assert_eq!(receipt.schema_version, COMPARISON_RECEIPT_SCHEMA_VERSION);
        assert_eq!(
            receipt.candidate_identity,
            receipt.baseline_identity.unwrap()
        );
        let record = receipt
            .metrics
            .iter()
            .find(|m| m.name.as_str() == "latency_p99")
            .unwrap();
        assert_eq!(record.disposition, Some(GateDisposition::Fail));
        assert_eq!(record.candidate_included, vec![2, 4, 6, 8, 10, 12]);
        assert_eq!(record.baseline_included, vec![1, 3, 5, 7, 9, 11]);
        assert!(record.candidate_excluded.is_empty());
        assert!(record.baseline_excluded.is_empty());
        assert_eq!(
            record.statistical_method.as_deref(),
            Some(STATISTICAL_METHOD_V2)
        );
        assert_eq!(record.resamples, Some(BOOTSTRAP_RESAMPLES));
        assert!(record.effective_seed.is_some());
        let degradation = record.degradation.expect("degradation");
        assert!(
            (degradation - 0.30).abs() < 1e-9,
            "degradation {degradation}"
        );
        assert_eq!(receipt.aggregate_verdict, Some(AggregateVerdict::Fail));
        let paired = receipt.paired.expect("paired section");
        assert_eq!(paired.schedule, crate::PAIRED_SCHEDULE_V1);
        assert_eq!(paired.pairs_declared, 6);
        assert_eq!(paired.baseline_service.as_str(), "origin-a");
        assert_eq!(paired.candidate_service.as_str(), "origin-b");
        assert_eq!(paired.metrics.len(), 1);
        assert_eq!(paired.metrics[0].pairs_complete, vec![1, 2, 3, 4, 5, 6]);
        assert!(paired.metrics[0].pairs_excluded.is_empty());
        assert_eq!(paired.metrics[0].pair_effects.len(), 6);
        // Constant effects: identical half means, flat trend.
        assert_eq!(paired.metrics[0].drift.trend, DriftTrend::Flat);
        assert!(
            receipt
                .warnings
                .iter()
                .any(|w| w.category == "below_recommended_pair_count")
        );
    }

    #[test]
    fn paired_non_regression_passes() {
        let (input, _) = paired_latency_input(&[(100.0, 100.0); 6]);
        let receipt = compare_paired_test(&input);
        let record = receipt
            .metrics
            .iter()
            .find(|m| m.name.as_str() == "latency_p99")
            .unwrap();
        assert_eq!(record.disposition, Some(GateDisposition::Pass));
        assert_eq!(receipt.aggregate_verdict, Some(AggregateVerdict::Pass));
        let paired = receipt.paired.expect("paired section");
        assert_eq!(paired.metrics[0].drift.trend, DriftTrend::Flat);
    }

    #[test]
    fn paired_higher_is_better_regression_fails() {
        let metric = throughput_request(statistical_gate(500));
        let input = paired_input(
            &"dd".repeat(32),
            metric,
            "rps",
            &MetricDirection::HigherIsBetter,
            &[(200.0, 150.0); 6],
        );
        let receipt = compare_paired_test(&input);
        let record = receipt
            .metrics
            .iter()
            .find(|m| m.name.as_str() == "throughput")
            .unwrap();
        assert_eq!(record.disposition, Some(GateDisposition::Fail));
        let degradation = record.degradation.expect("degradation");
        assert!(
            degradation > 0.30 && degradation < 0.34,
            "degradation {degradation}"
        );
        assert_eq!(receipt.aggregate_verdict, Some(AggregateVerdict::Fail));
    }

    #[test]
    fn paired_threshold_crossing_is_inconclusive() {
        let (input, _) = paired_latency_input(&[
            (100.0, 110.0),
            (100.0, 110.0),
            (100.0, 110.0),
            (100.0, 90.0),
            (100.0, 90.0),
            (100.0, 90.0),
        ]);
        let receipt = compare_paired_test(&input);
        let record = receipt
            .metrics
            .iter()
            .find(|m| m.name.as_str() == "latency_p99")
            .unwrap();
        assert_eq!(record.disposition, Some(GateDisposition::Inconclusive));
        assert_eq!(
            receipt.aggregate_verdict,
            Some(AggregateVerdict::Inconclusive)
        );
    }

    #[test]
    fn paired_insufficient_pairs_are_invalid() {
        let (input, _) = paired_latency_input(&[(100.0, 130.0); 2]);
        let receipt = compare_paired_test(&input);
        let record = receipt
            .metrics
            .iter()
            .find(|m| m.name.as_str() == "latency_p99")
            .unwrap();
        assert_eq!(record.disposition, Some(GateDisposition::Invalid));
        assert_eq!(record.reason.as_deref(), Some("insufficient_pairs"));
        assert_eq!(receipt.aggregate_verdict, Some(AggregateVerdict::Invalid));
    }

    #[test]
    fn paired_split_pairs_are_excluded_never_imputed() {
        let (mut input, _) = paired_latency_input(&[(100.0, 130.0); 6]);
        // Drop the candidate observation of pair 3: the pair must vanish
        // whole while the other five still gate.
        input.trials[5].metrics = None;
        let receipt = compare_paired_test(&input);
        let record = receipt
            .metrics
            .iter()
            .find(|m| m.name.as_str() == "latency_p99")
            .unwrap();
        assert_eq!(record.candidate_included, vec![2, 4, 8, 10, 12]);
        assert_eq!(record.baseline_included, vec![1, 3, 7, 9, 11]);
        assert!(
            record.candidate_excluded.iter().any(
                |excluded| excluded.trial_id == 6 && excluded.reason == "no_normalized_metrics"
            )
        );
        let paired = receipt.paired.expect("paired section");
        assert_eq!(paired.metrics[0].pairs_complete, vec![1, 2, 4, 5, 6]);
        assert_eq!(paired.metrics[0].pairs_excluded.len(), 1);
        assert_eq!(paired.metrics[0].pairs_excluded[0].pair_id, 3);
        assert_eq!(
            paired.metrics[0].pairs_excluded[0].reason,
            "pair_incomplete"
        );
        // Five complete pairs still satisfy the minimum: the gate evaluates.
        assert_eq!(record.disposition, Some(GateDisposition::Fail));
    }

    #[test]
    fn paired_untagged_trials_are_excluded_on_both_sides() {
        let (mut input, _) = paired_latency_input(&[(100.0, 130.0); 6]);
        input.trials.push(InputTrial {
            id: TrialId::new(13).unwrap(),
            terminal: TrialExecutionStatus::Completed,
            arm: None,
            pair_id: None,
            metrics: Some(trial_metrics_for(13, &[("latency_p99", "ms", 100.0)])),
        });
        let receipt = compare_paired_test(&input);
        let record = receipt
            .metrics
            .iter()
            .find(|m| m.name.as_str() == "latency_p99")
            .unwrap();
        for excluded in [&record.candidate_excluded, &record.baseline_excluded] {
            assert!(
                excluded
                    .iter()
                    .any(|entry| entry.trial_id == 13
                        && entry.reason == "trial_missing_pair_identity"),
                "untagged trial must be excluded"
            );
        }
        assert_eq!(record.disposition, Some(GateDisposition::Fail));
    }

    #[test]
    fn paired_absolute_gate_is_invalid() {
        let metric = latency_request(Gate::Absolute { value: 500.0 });
        let input = paired_input(
            &"ee".repeat(32),
            metric,
            "ms",
            &MetricDirection::LowerIsBetter,
            &[(100.0, 100.0); 6],
        );
        let receipt = compare_paired_test(&input);
        let record = receipt
            .metrics
            .iter()
            .find(|m| m.name.as_str() == "latency_p99")
            .unwrap();
        assert_eq!(record.disposition, Some(GateDisposition::Invalid));
        assert_eq!(
            record.reason.as_deref(),
            Some("absolute_gate_unsupported_for_paired_evidence")
        );
    }

    #[test]
    fn paired_nonpositive_values_are_invalid() {
        let (mut input, _) = paired_latency_input(&[(100.0, 130.0); 6]);
        input.trials[0].metrics = Some(trial_metrics_for(1, &[("latency_p99", "ms", 0.0)]));
        let receipt = compare_paired_test(&input);
        let record = receipt
            .metrics
            .iter()
            .find(|m| m.name.as_str() == "latency_p99")
            .unwrap();
        assert_eq!(record.disposition, Some(GateDisposition::Invalid));
        assert_eq!(record.reason.as_deref(), Some("nonpositive_relative_value"));
    }

    #[test]
    fn unpaired_compare_of_paired_bundle_is_invalid() {
        let (input, _) = paired_latency_input(&[(100.0, 130.0); 6]);
        let reference = BaselineReference::Bundle {
            identity: input.identity.clone(),
            path: "paired.eggb".to_owned(),
        };
        let request = ComparisonRequest {
            candidate: &input,
            baseline: Some(BaselineSide {
                reference,
                input: &input,
            }),
        };
        let receipt = compare(&request, &ComparisonOptions::default());
        assert_eq!(receipt.policy_id, COMPARISON_POLICY_V1);
        for record in &receipt.metrics {
            assert_eq!(record.disposition, Some(GateDisposition::Invalid));
            assert_eq!(
                record.reason.as_deref(),
                Some("paired_evidence_requires_paired_comparison")
            );
        }
        assert_eq!(receipt.aggregate_verdict, Some(AggregateVerdict::Invalid));
        assert!(
            receipt
                .warnings
                .iter()
                .any(|w| w.category == "paired_bundle_requires_paired_comparison")
        );
    }

    #[test]
    fn paired_comparison_without_design_is_invalid() {
        let metric = latency_request(statistical_gate(500));
        let input = input_with_values(
            &"ff".repeat(32),
            metric,
            EnvironmentPolicy::StrictSameTestbed,
            &[100.0; 6],
            standard_environment(),
        );
        let receipt = compare_paired_test(&input);
        let record = receipt
            .metrics
            .iter()
            .find(|m| m.name.as_str() == "latency_p99")
            .unwrap();
        assert_eq!(record.disposition, Some(GateDisposition::Invalid));
        assert_eq!(record.reason.as_deref(), Some("paired_design_absent"));
        assert_eq!(receipt.aggregate_verdict, Some(AggregateVerdict::Invalid));
        assert!(receipt.paired.is_none());
    }

    #[test]
    fn paired_drift_trend_sign_is_correct_and_never_gates() {
        // Monotone candidate worsening under a wide allowance: the gate
        // passes while drift points up, proving drift never gates.
        let (input, _) = paired_latency_input(&[
            (100.0, 100.0),
            (100.0, 101.0),
            (100.0, 102.0),
            (100.0, 103.0),
            (100.0, 104.0),
            (100.0, 105.0),
        ]);
        let mut wide = input;
        wide.resolved.metrics[0].gate = Some(statistical_gate(5000));
        let receipt = compare_paired_test(&wide);
        let record = receipt
            .metrics
            .iter()
            .find(|m| m.name.as_str() == "latency_p99")
            .unwrap();
        assert_eq!(record.disposition, Some(GateDisposition::Pass));
        let paired = receipt.paired.expect("paired section");
        let drift = &paired.metrics[0].drift;
        assert_eq!(drift.trend, DriftTrend::Up);
        let first = drift.first_half_mean.expect("first half");
        let second = drift.second_half_mean.expect("second half");
        assert!(second > first, "second {second} first {first}");
        assert_eq!(paired.metrics[0].pair_effects.len(), 6);
        let effects: Vec<f64> = paired.metrics[0]
            .pair_effects
            .iter()
            .map(|entry| entry.effect)
            .collect();
        let mut ordered = effects.clone();
        ordered.sort_by(f64::total_cmp);
        assert_eq!(effects, ordered, "effects follow execution order");
    }

    #[test]
    fn paired_comparison_is_deterministic_with_stable_seeds() {
        let (input, _) = paired_latency_input(&[
            (100.0, 112.0),
            (100.0, 108.0),
            (100.0, 115.0),
            (100.0, 109.0),
            (100.0, 111.0),
            (100.0, 113.0),
        ]);
        let first = compare_paired_test(&input);
        let second = compare_paired_test(&input);
        assert_eq!(first, second);
        let record = first
            .metrics
            .iter()
            .find(|m| m.name.as_str() == "latency_p99")
            .unwrap();
        let first_seed = record.effective_seed.expect("seed");
        let explicit = compare_paired(
            std::path::Path::new("paired.eggb"),
            &input,
            &ComparisonOptions { seed: Some(7) },
        );
        assert_eq!(explicit.base_seed, 7);
        assert_ne!(first.base_seed, explicit.base_seed);
        let explicit_record = explicit
            .metrics
            .iter()
            .find(|m| m.name.as_str() == "latency_p99")
            .unwrap();
        assert_ne!(explicit_record.effective_seed, Some(first_seed));
    }

    #[test]
    fn paired_relative_regression_gate_uses_complete_pairs_only() {
        let metric = latency_request(relative_gate(1000));
        let mut input = paired_input(
            &"ab".repeat(32),
            metric,
            "ms",
            &MetricDirection::LowerIsBetter,
            &[(100.0, 105.0); 4],
        );
        // One incomplete pair must not move the point estimate: the
        // remaining three pairs hold 5% degradation under a 10% allowance.
        input.trials[7].metrics = None;
        let receipt = compare_paired_test(&input);
        let record = receipt
            .metrics
            .iter()
            .find(|m| m.name.as_str() == "latency_p99")
            .unwrap();
        assert_eq!(record.disposition, Some(GateDisposition::Pass));
        let degradation = record.degradation.expect("degradation");
        assert!(
            (degradation - 0.05).abs() < 1e-9,
            "degradation {degradation}"
        );
    }

    fn paired_golden_receipt(kind: &str) -> ComparisonReceipt {
        let (input, _) = match kind {
            "paired-pass" => paired_latency_input(&[(100.0, 101.0); 6]),
            "paired-fail" => paired_latency_input(&[(100.0, 130.0); 6]),
            "paired-inconclusive" => paired_latency_input(&[
                (100.0, 110.0),
                (100.0, 110.0),
                (100.0, 110.0),
                (100.0, 90.0),
                (100.0, 90.0),
                (100.0, 90.0),
            ]),
            _ => panic!("unknown paired golden kind {kind}"),
        };
        compare_paired_test(&input)
    }

    #[test]
    fn paired_golden_receipts_are_stable() {
        for kind in ["paired-pass", "paired-fail", "paired-inconclusive"] {
            let receipt = paired_golden_receipt(kind);
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
