//! Normalized per-trial metric vocabulary and evidence contract (M001).
//!
//! This module owns the versioned metric vocabulary, the normalized
//! [`TrialMetrics`] artifact schema, and the pure normalization algorithm that
//! turns driver-supplied raw observations into deterministic trial evidence.
//!
//! Design notes:
//!
//! - One measured trial is one statistical observation unit. Request counts
//!   inside a trial never increase the comparison sample count.
//! - A measured zero is distinct from [`MissingReason`], which is distinct
//!   from [`InvalidReason`]. Missing data is never encoded as `0`, `NaN`, or
//!   record absence.
//! - Units, directionality, and gate intent come from the predeclared resolved
//!   plan. Driver output cannot change them and no implicit unit conversion
//!   occurs in v1.
//! - Normalization runs after the measured workload interval ends. Warmup
//!   observations must never enter this path.
//! - This module performs no comparison, emits no verdict, and parses no
//!   protocol-specific output. Drivers own parsing; this module owns
//!   validation and normalization.

use crate::{
    ArtifactPath, BundleError, MetricDirection, MetricIntent, MetricRequest, Name, SchemaVersion,
    TrialExecutionStatus, TrialId,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Version of the built-in metric vocabulary documented below.
pub const METRIC_VOCABULARY_VERSION: u32 = 1;

/// Schema version of the normalized [`TrialMetrics`] artifact.
pub const TRIAL_METRICS_SCHEMA_VERSION: SchemaVersion = SchemaVersion(1);

/// Method/version label recorded in every normalized provenance record.
pub const NORMALIZATION_METHOD_V1: &str = "eggbench-normalize-v1";

/// Maximum normalized observations staged for one trial.
pub const MAX_NORMALIZED_METRICS_PER_TRIAL: usize = 256;
/// Maximum raw driver observations accepted for one invocation.
pub const MAX_RAW_OBSERVATIONS_PER_INVOCATION: usize = 256;
/// Maximum histogram references staged for one trial.
pub const MAX_HISTOGRAM_REFERENCES_PER_TRIAL: usize = 16;
/// Maximum raw-artifact references carried by one normalized observation.
pub const MAX_RAW_ARTIFACT_REFS_PER_METRIC: usize = 8;
/// Maximum error categories in one trial error distribution.
pub const MAX_ERROR_CATEGORIES_PER_TRIAL: usize = 32;
/// Maximum warning entries in one trial metric artifact.
pub const MAX_METRIC_WARNINGS_PER_TRIAL: usize = 16;
/// Maximum provenance string length (producer version, source field, method).
pub const MAX_PROVENANCE_FIELD_LEN: usize = 128;
/// Maximum human detail length on invalid observations and warnings.
pub const MAX_METRIC_DETAIL_LEN: usize = 512;
/// Maximum histogram format-label length.
pub const MAX_HISTOGRAM_FORMAT_LEN: usize = 64;

/// What a normalized scalar means inside one trial.
///
/// This states the meaning of the scalar; M001 never recomputes trial
/// scalars from millions of raw requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Aggregation {
    /// Value as produced, without a statistical claim.
    Direct,
    /// Minimum over the trial interval.
    Minimum,
    /// Maximum over the trial interval.
    Maximum,
    /// Arithmetic mean over the trial interval.
    Mean,
    /// Sum over the trial interval.
    Sum,
    /// Rate over the trial interval (driver-computed).
    Rate,
    /// Ratio over the trial interval (driver-computed).
    Ratio,
    /// Percentile over the trial interval, in basis points of percentile
    /// (p50 = 5000, p99 = 9900, p99.9 = 9990). Range is `1..=10000`.
    Percentile {
        /// Percentile in basis points.
        basis_points: u16,
    },
}

/// Why a requested metric has no usable value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum MissingReason {
    /// No raw observation was supplied for the requested metric.
    SourceNotProvided,
    /// The driver does not support the requested metric.
    UnsupportedByDriver,
    /// The trial did not complete, so no metric evidence is claimed.
    TrialNotCompleted,
}

/// Why a supplied value cannot become a valid observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum InvalidReason {
    /// Value was `NaN` or infinite.
    NonFinite,
    /// Raw unit did not exactly equal the requested unit.
    UnitMismatch,
    /// More than one raw observation claimed the same requested metric.
    DuplicateObservation,
    /// Raw aggregation contradicts the vocabulary contract.
    AggregationMismatch,
    /// A referenced raw artifact is unknown or outside this trial.
    MalformedSourceReference,
    /// Value violates the metric domain (for example a negative ratio).
    DomainError,
}

/// Normalized state of one requested metric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum ObservationState {
    /// A finite observed scalar value. Zero is valid where the domain permits.
    Observed {
        /// Observed scalar value.
        value: f64,
    },
    /// No usable value was produced.
    Missing {
        /// Stable missing category.
        reason: MissingReason,
    },
    /// A value was produced but violated the normalization contract.
    Invalid {
        /// Stable invalid category.
        reason: InvalidReason,
        /// Optional bounded human detail.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
}

/// Typed provenance answering "where did this number come from?".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricProvenance {
    /// Driver/producer label (for example `fake-load`).
    pub producer: Name,
    /// Producer/upstream version when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer_version: Option<String>,
    /// Source field/key label inside the producer output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_field: Option<String>,
    /// Normalization method/version label.
    pub normalization: String,
    /// Resolved same-trial artifact paths backing this observation.
    #[serde(default)]
    pub raw_artifacts: Vec<ArtifactPath>,
}

/// One normalized requested-metric record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NormalizedObservation {
    /// Stable metric name (matches the resolved plan request).
    pub name: Name,
    /// Unit copied from the resolved plan.
    pub unit: Name,
    /// Direction copied from the resolved plan.
    pub direction: MetricDirection,
    /// Gate intent copied from the resolved plan.
    pub intent: MetricIntent,
    /// What the scalar means inside this trial.
    pub aggregation: Aggregation,
    /// Observed, missing, or invalid state.
    pub state: ObservationState,
    /// Source provenance.
    pub provenance: MetricProvenance,
}

/// Reference to retained raw distribution evidence (no parsing in M001).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistogramReference {
    /// Logical metric/distribution name.
    pub metric: Name,
    /// Manifest-listed same-trial artifact path.
    pub path: ArtifactPath,
    /// Format/encoding identifier supplied by the driver.
    pub format: String,
    /// Value unit of the distribution.
    pub unit: Name,
    /// Optional correction/method label supplied by the driver.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// Optional link from a percentile scalar to its source histogram.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_metric: Option<Name>,
}

/// One error-category count entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorCategoryCount {
    /// Stable category label.
    pub category: Name,
    /// Nonnegative occurrence count.
    pub count: u64,
}

/// Bounded warning entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricWarning {
    /// Stable warning category.
    pub category: String,
    /// Bounded human detail.
    pub detail: String,
}

/// Normalized per-trial metric evidence artifact.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrialMetrics {
    /// Normalized metric schema version.
    pub schema_version: SchemaVersion,
    /// Vocabulary version used for built-in validation.
    pub vocabulary_version: u32,
    /// Stable measured-trial identity.
    pub trial_id: TrialId,
    /// One record per requested metric, sorted by metric name.
    pub observations: Vec<NormalizedObservation>,
    /// Raw distribution references, sorted by `(metric, path)`.
    #[serde(default)]
    pub histograms: Vec<HistogramReference>,
    /// Optional bounded error-category distribution, sorted by category.
    #[serde(default)]
    pub error_distribution: Vec<ErrorCategoryCount>,
    /// Bounded diagnostics (for example unrequested raw metrics ignored).
    #[serde(default)]
    pub warnings: Vec<MetricWarning>,
}

/// Driver-supplied raw metric observation (validated, never trusted as evidence).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawMetricObservation {
    /// Claimed metric name; must exactly match a resolved request to qualify.
    pub name: String,
    /// Claimed unit; must exactly equal the requested unit.
    pub unit: String,
    /// Claimed scalar value; must be finite.
    pub value: f64,
    /// Claimed aggregation semantics.
    pub aggregation: Aggregation,
    /// Optional source field/key label.
    #[serde(default)]
    pub source_field: Option<String>,
    /// Optional producer label override for this observation. When `None`
    /// the call-level producer applies. Lets telemetry observations carry
    /// their own producer (for example `gregg`) through a combined
    /// normalization call without changing the `TrialMetrics` v1 output.
    #[serde(default)]
    pub producer: Option<String>,
    /// Optional producer version override, honored only with `producer`.
    #[serde(default)]
    pub producer_version: Option<String>,
    /// Raw artifact names returned by the same invocation.
    #[serde(default)]
    pub raw_artifacts: Vec<String>,
}

/// Driver-supplied raw histogram input (validated, never parsed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawHistogramInput {
    /// Logical metric/distribution name.
    pub metric: String,
    /// Artifact name returned by the same invocation.
    pub artifact_name: String,
    /// Format/encoding identifier.
    pub format: String,
    /// Value unit of the distribution.
    pub unit: String,
    /// Optional correction/method label.
    #[serde(default)]
    pub method: Option<String>,
}

/// Inputs to trial normalization.
#[derive(Debug, Clone)]
pub struct NormalizationInput<'a> {
    /// Stable measured-trial identity.
    pub trial_id: TrialId,
    /// Resolved plan metric requests (source of name/unit/direction/intent).
    pub metrics: &'a [MetricRequest],
    /// Terminal execution state of the trial.
    pub terminal_status: TrialExecutionStatus,
    /// Raw driver observations from the same invocation.
    pub observations: &'a [RawMetricObservation],
    /// Raw histogram inputs from the same invocation.
    pub histograms: &'a [RawHistogramInput],
    /// Raw error-category counts from the same invocation.
    pub error_counts: &'a [(String, u64)],
    /// Map from safe artifact name to staged trial artifact path.
    pub artifact_map: &'a BTreeMap<String, ArtifactPath>,
    /// Driver/producer label.
    pub producer: &'a str,
    /// Driver/producer version when known.
    pub producer_version: Option<&'a str>,
}

/// Built-in vocabulary entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BuiltinMetric {
    /// Canonical unit label.
    unit: &'static str,
    /// Expected aggregation, when the vocabulary constrains it.
    aggregation: Option<Aggregation>,
    /// Whether the plan direction must match a fixed direction.
    direction: Option<MetricDirectionKind>,
}

/// Fixed direction kinds used by the vocabulary table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MetricDirectionKind {
    /// Higher is better.
    HigherIsBetter,
    /// Lower is better.
    LowerIsBetter,
}

/// Canonical artifact path for a trial's normalized metrics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrialMetricsPath;

/// Artifact path for normalized trial metrics: `trials/NNN/metrics.json`.
///
/// # Errors
/// Returns [`BundleError`] when the trial identity cannot form a valid path.
pub fn trial_metrics_path(trial_id: TrialId) -> Result<ArtifactPath, BundleError> {
    ArtifactPath::new(format!("trials/{:03}/metrics.json", trial_id.get()))
}

/// Look up the built-in vocabulary entry for a metric name.
fn builtin_metric(name: &str) -> Option<BuiltinMetric> {
    match name {
        "throughput" => Some(BuiltinMetric {
            unit: "rps",
            aggregation: Some(Aggregation::Rate),
            direction: Some(MetricDirectionKind::HigherIsBetter),
        }),
        "latency_min" => Some(BuiltinMetric {
            unit: "ms",
            aggregation: Some(Aggregation::Minimum),
            direction: Some(MetricDirectionKind::LowerIsBetter),
        }),
        "latency_mean" => Some(BuiltinMetric {
            unit: "ms",
            aggregation: Some(Aggregation::Mean),
            direction: Some(MetricDirectionKind::LowerIsBetter),
        }),
        "latency_p50" => Some(BuiltinMetric {
            unit: "ms",
            aggregation: Some(Aggregation::Percentile { basis_points: 5000 }),
            direction: Some(MetricDirectionKind::LowerIsBetter),
        }),
        "latency_p90" => Some(BuiltinMetric {
            unit: "ms",
            aggregation: Some(Aggregation::Percentile { basis_points: 9000 }),
            direction: Some(MetricDirectionKind::LowerIsBetter),
        }),
        "latency_p95" => Some(BuiltinMetric {
            unit: "ms",
            aggregation: Some(Aggregation::Percentile { basis_points: 9500 }),
            direction: Some(MetricDirectionKind::LowerIsBetter),
        }),
        "latency_p99" => Some(BuiltinMetric {
            unit: "ms",
            aggregation: Some(Aggregation::Percentile { basis_points: 9900 }),
            direction: Some(MetricDirectionKind::LowerIsBetter),
        }),
        "latency_p999" => Some(BuiltinMetric {
            unit: "ms",
            aggregation: Some(Aggregation::Percentile { basis_points: 9990 }),
            direction: Some(MetricDirectionKind::LowerIsBetter),
        }),
        "error_rate" | "timeout_rate" => Some(BuiltinMetric {
            unit: "ratio",
            aggregation: Some(Aggregation::Ratio),
            direction: Some(MetricDirectionKind::LowerIsBetter),
        }),
        "bytes_sent" | "bytes_received" => Some(BuiltinMetric {
            unit: "bytes",
            aggregation: Some(Aggregation::Sum),
            direction: None,
        }),
        "cpu_percent" => Some(BuiltinMetric {
            unit: "percent",
            aggregation: Some(Aggregation::Mean),
            direction: None,
        }),
        "rss_bytes" => Some(BuiltinMetric {
            unit: "bytes",
            aggregation: Some(Aggregation::Maximum),
            direction: None,
        }),
        _ => None,
    }
}

/// Check that a plan-declared direction matches the vocabulary expectation.
fn direction_matches(expected: MetricDirectionKind, actual: &MetricDirection) -> bool {
    matches!(
        (expected, actual),
        (
            MetricDirectionKind::HigherIsBetter,
            MetricDirection::HigherIsBetter
        ) | (
            MetricDirectionKind::LowerIsBetter,
            MetricDirection::LowerIsBetter
        )
    )
}

/// Check that a raw aggregation satisfies the vocabulary expectation.
///
/// Percentile expectations accept any `Percentile` basis-points value; the
/// percentile identity is driver-supplied precision, not a unit conversion.
fn aggregation_matches(expected: Aggregation, actual: Aggregation) -> bool {
    match (expected, actual) {
        (Aggregation::Percentile { .. }, Aggregation::Percentile { .. }) => true,
        (left, right) => left == right,
    }
}

impl Aggregation {
    /// Validate structural bounds (percentile basis points).
    ///
    /// # Errors
    /// Returns [`BundleError`] when percentile basis points are outside `1..=10000`.
    pub fn validate(self) -> Result<(), BundleError> {
        if let Self::Percentile { basis_points } = self
            && !(1..=10_000).contains(&basis_points)
        {
            return Err(BundleError::ManifestParse(format!(
                "percentile basis points out of range: {basis_points}"
            )));
        }
        Ok(())
    }
}

impl TrialMetrics {
    /// Validate the normalized artifact: versions, ordering, uniqueness, bounds.
    ///
    /// # Errors
    /// Returns [`BundleError`] for version mismatch, duplicate metric names,
    /// nonfinite observed values, invalid percentile bounds, oversized
    /// strings, bound overflows, or unsorted records.
    pub fn validate(&self) -> Result<(), BundleError> {
        if self.schema_version != TRIAL_METRICS_SCHEMA_VERSION {
            return Err(BundleError::UnsupportedManifestVersion(
                self.schema_version.0,
            ));
        }
        if self.vocabulary_version != METRIC_VOCABULARY_VERSION {
            return Err(BundleError::ManifestParse(format!(
                "unsupported metric vocabulary version {}",
                self.vocabulary_version
            )));
        }
        if self.observations.len() > MAX_NORMALIZED_METRICS_PER_TRIAL {
            return Err(BundleError::BoundExceeded("normalized metric count"));
        }
        if self.histograms.len() > MAX_HISTOGRAM_REFERENCES_PER_TRIAL {
            return Err(BundleError::BoundExceeded("histogram reference count"));
        }
        if self.error_distribution.len() > MAX_ERROR_CATEGORIES_PER_TRIAL {
            return Err(BundleError::BoundExceeded("error category count"));
        }
        if self.warnings.len() > MAX_METRIC_WARNINGS_PER_TRIAL {
            return Err(BundleError::BoundExceeded("metric warning count"));
        }
        let mut names = BTreeSet::new();
        let mut previous: Option<&str> = None;
        for observation in &self.observations {
            let name = observation.name.as_str();
            if !names.insert(name) {
                return Err(BundleError::ManifestParse(format!(
                    "duplicate normalized metric name: {name}"
                )));
            }
            if let Some(prev) = previous
                && prev >= name
            {
                return Err(BundleError::InvalidManifest(
                    "normalized observations must be sorted by metric name",
                ));
            }
            previous = Some(name);
            observation.validate()?;
        }
        let mut previous_histogram: Option<(&str, &str)> = None;
        for histogram in &self.histograms {
            histogram.validate()?;
            let key = (histogram.metric.as_str(), histogram.path.as_str());
            if let Some(prev) = previous_histogram
                && prev >= key
            {
                return Err(BundleError::InvalidManifest(
                    "histogram references must be sorted by (metric, path)",
                ));
            }
            previous_histogram = Some(key);
        }
        let mut previous_category: Option<&str> = None;
        for entry in &self.error_distribution {
            if let Some(prev) = previous_category
                && prev >= entry.category.as_str()
            {
                return Err(BundleError::InvalidManifest(
                    "error distribution must be sorted by category",
                ));
            }
            previous_category = Some(entry.category.as_str());
        }
        for warning in &self.warnings {
            warning.validate()?;
        }
        Ok(())
    }

    /// Serialize deterministically to pretty JSON bytes.
    ///
    /// # Errors
    /// Returns [`BundleError`] when serialization fails.
    pub fn to_json_bytes(&self) -> Result<Vec<u8>, BundleError> {
        self.validate()?;
        serde_json::to_vec_pretty(self)
            .map_err(|error| BundleError::ManifestParse(error.to_string()))
    }
}

impl NormalizedObservation {
    /// Validate one normalized observation record.
    ///
    /// # Errors
    /// Returns [`BundleError`] for nonfinite observed values, invalid
    /// percentile bounds, oversized provenance/detail strings, or too many
    /// raw-artifact references.
    pub fn validate(&self) -> Result<(), BundleError> {
        self.aggregation.validate()?;
        if let ObservationState::Observed { value } = self.state {
            if !value.is_finite() {
                return Err(BundleError::ManifestParse(format!(
                    "nonfinite observed value for metric {}",
                    self.name.as_str()
                )));
            }
            if is_ratio_metric(self.name.as_str()) && !(0.0..=1.0).contains(&value) {
                return Err(BundleError::ManifestParse(format!(
                    "ratio metric {} outside [0,1]",
                    self.name.as_str()
                )));
            }
        }
        if let ObservationState::Invalid {
            detail: Some(detail),
            ..
        } = &self.state
            && detail.len() > MAX_METRIC_DETAIL_LEN
        {
            return Err(BundleError::BoundExceeded("invalid observation detail"));
        }
        self.provenance.validate()?;
        Ok(())
    }
}

/// Whether a metric name denotes a `[0,1]` ratio domain.
fn is_ratio_metric(name: &str) -> bool {
    name == "error_rate" || name == "timeout_rate"
}

impl MetricProvenance {
    /// Validate provenance bounds.
    ///
    /// # Errors
    /// Returns [`BundleError`] for oversized fields or too many raw-artifact refs.
    pub fn validate(&self) -> Result<(), BundleError> {
        if self.normalization.len() > MAX_PROVENANCE_FIELD_LEN || self.normalization.is_empty() {
            return Err(BundleError::InvalidManifest(
                "provenance normalization label out of bounds",
            ));
        }
        if let Some(version) = &self.producer_version
            && version.len() > MAX_PROVENANCE_FIELD_LEN
        {
            return Err(BundleError::BoundExceeded("producer version"));
        }
        if let Some(field) = &self.source_field
            && (field.is_empty() || field.len() > MAX_PROVENANCE_FIELD_LEN)
        {
            return Err(BundleError::InvalidManifest(
                "provenance source field out of bounds",
            ));
        }
        if self.raw_artifacts.len() > MAX_RAW_ARTIFACT_REFS_PER_METRIC {
            return Err(BundleError::BoundExceeded("raw artifact reference count"));
        }
        Ok(())
    }
}

impl HistogramReference {
    /// Validate one histogram reference record.
    ///
    /// # Errors
    /// Returns [`BundleError`] for oversized format/method labels.
    pub fn validate(&self) -> Result<(), BundleError> {
        if self.format.is_empty() || self.format.len() > MAX_HISTOGRAM_FORMAT_LEN {
            return Err(BundleError::InvalidManifest(
                "histogram format label out of bounds",
            ));
        }
        if let Some(method) = &self.method
            && (method.is_empty() || method.len() > MAX_PROVENANCE_FIELD_LEN)
        {
            return Err(BundleError::InvalidManifest(
                "histogram method label out of bounds",
            ));
        }
        Ok(())
    }
}

impl MetricWarning {
    /// Validate one warning entry.
    ///
    /// # Errors
    /// Returns [`BundleError`] for oversized category/detail strings.
    pub fn validate(&self) -> Result<(), BundleError> {
        if self.category.is_empty() || self.category.len() > MAX_PROVENANCE_FIELD_LEN {
            return Err(BundleError::InvalidManifest(
                "metric warning category out of bounds",
            ));
        }
        if self.detail.len() > MAX_METRIC_DETAIL_LEN {
            return Err(BundleError::BoundExceeded("metric warning detail"));
        }
        Ok(())
    }
}

/// Normalize one measured trial's raw driver output into [`TrialMetrics`].
///
/// Semantic data problems (missing source, unit mismatch, duplicates,
/// nonfinite input) become `missing`/`invalid` states inside a valid
/// artifact. Structural problems (bound overflows, oversized inputs) return
/// [`BundleError`] so the runner can use the mandatory evidence-error path.
///
/// # Errors
/// Returns [`BundleError`] for structural bound violations only.
#[allow(clippy::too_many_lines)]
pub fn normalize_trial_metrics(
    input: &NormalizationInput<'_>,
) -> Result<TrialMetrics, BundleError> {
    if input.metrics.len() > MAX_NORMALIZED_METRICS_PER_TRIAL {
        return Err(BundleError::BoundExceeded("requested metric count"));
    }
    if input.observations.len() > MAX_RAW_OBSERVATIONS_PER_INVOCATION {
        return Err(BundleError::BoundExceeded("raw metric observation count"));
    }
    if input.histograms.len() > MAX_HISTOGRAM_REFERENCES_PER_TRIAL {
        return Err(BundleError::BoundExceeded("raw histogram count"));
    }
    if input.error_counts.len() > MAX_ERROR_CATEGORIES_PER_TRIAL {
        return Err(BundleError::BoundExceeded("raw error category count"));
    }
    let producer = Name::new(input.producer)
        .map_err(|error| BundleError::ManifestParse(format!("invalid producer label: {error}")))?;
    if let Some(version) = input.producer_version
        && version.len() > MAX_PROVENANCE_FIELD_LEN
    {
        return Err(BundleError::BoundExceeded("producer version"));
    }

    // Group raw observations by exact metric name.
    let mut by_name: BTreeMap<&str, Vec<&RawMetricObservation>> = BTreeMap::new();
    for observation in input.observations {
        if observation.name.len() > Name::MAX_LEN
            || observation.unit.len() > Name::MAX_LEN
            || observation.raw_artifacts.len() > MAX_RAW_ARTIFACT_REFS_PER_METRIC
        {
            return Err(BundleError::BoundExceeded("raw metric observation"));
        }
        if let Some(field) = &observation.source_field
            && field.len() > MAX_PROVENANCE_FIELD_LEN
        {
            return Err(BundleError::BoundExceeded("raw source field"));
        }
        by_name
            .entry(observation.name.as_str())
            .or_default()
            .push(observation);
    }

    let completed = input.terminal_status == TrialExecutionStatus::Completed;
    let mut observations = Vec::with_capacity(input.metrics.len());
    let mut seen_requests = BTreeSet::new();

    for request in input.metrics {
        let name = request.name.as_str();
        if !seen_requests.insert(name) {
            return Err(BundleError::ManifestParse(format!(
                "duplicate requested metric name: {name}"
            )));
        }
        let (state, aggregation, provenance) = if completed {
            let raw = by_name.get(name);
            normalize_one_request(request, raw, input)?
        } else {
            (
                ObservationState::Missing {
                    reason: MissingReason::TrialNotCompleted,
                },
                request_aggregation_fallback(request),
                MetricProvenance {
                    producer: producer.clone(),
                    producer_version: input.producer_version.map(str::to_owned),
                    source_field: None,
                    normalization: NORMALIZATION_METHOD_V1.to_owned(),
                    raw_artifacts: Vec::new(),
                },
            )
        };
        observations.push(NormalizedObservation {
            name: request.name.clone(),
            unit: request.unit.clone(),
            direction: request.direction.clone(),
            intent: request.intent,
            aggregation,
            state,
            provenance,
        });
    }

    // Deterministic ordering by metric name.
    observations.sort_by(|left, right| left.name.as_str().cmp(right.name.as_str()));

    let histograms = normalize_histograms(input)?;
    let error_distribution = normalize_error_counts(input.error_counts)?;

    let mut warnings = Vec::new();
    for (name, group) in &by_name {
        if !seen_requests.contains(*name) {
            push_warning(
                &mut warnings,
                "unrequested_raw_metric",
                &format!("raw observation for unrequested metric {name} ignored"),
            )?;
        } else if group.len() > 1 {
            // Duplicate already surfaced as invalid; keep a diagnostic pointer.
            push_warning(
                &mut warnings,
                "duplicate_raw_observation",
                &format!("duplicate raw observations for metric {name} marked invalid"),
            )?;
        }
    }

    let metrics = TrialMetrics {
        schema_version: TRIAL_METRICS_SCHEMA_VERSION,
        vocabulary_version: METRIC_VOCABULARY_VERSION,
        trial_id: input.trial_id,
        observations,
        histograms,
        error_distribution,
        warnings,
    };
    metrics.validate()?;
    Ok(metrics)
}

/// Aggregation to record when the trial did not complete.
///
/// The value is structural only; the state is always `missing` so no claim
/// about the scalar is made.
fn request_aggregation_fallback(request: &MetricRequest) -> Aggregation {
    if let Some(builtin) = builtin_metric(request.name.as_str())
        && let Some(expected) = builtin.aggregation
    {
        return expected;
    }
    Aggregation::Direct
}

/// Normalize one requested metric against its raw group.
#[allow(clippy::too_many_lines)]
fn normalize_one_request(
    request: &MetricRequest,
    raw: Option<&Vec<&RawMetricObservation>>,
    input: &NormalizationInput<'_>,
) -> Result<(ObservationState, Aggregation, MetricProvenance), BundleError> {
    let producer = Name::new(input.producer)
        .map_err(|error| BundleError::ManifestParse(format!("invalid producer label: {error}")))?;
    let base_provenance =
        |source_field: Option<String>, raw_artifacts: Vec<ArtifactPath>| MetricProvenance {
            producer: producer.clone(),
            producer_version: input.producer_version.map(str::to_owned),
            source_field,
            normalization: NORMALIZATION_METHOD_V1.to_owned(),
            raw_artifacts,
        };
    // Provenance honoring a per-observation producer override (telemetry
    // observations through a combined call). An override without a valid
    // label is a structural error; a version override travels only with its
    // label and is length-checked by provenance validation.
    let observation_provenance = |raw: &RawMetricObservation,
                                  raw_artifacts: Vec<ArtifactPath>|
     -> Result<MetricProvenance, BundleError> {
        let (producer, producer_version) = match &raw.producer {
            None => (producer.clone(), input.producer_version.map(str::to_owned)),
            Some(label) => (
                Name::new(label).map_err(|error| {
                    BundleError::ManifestParse(format!(
                        "invalid observation producer label: {error}"
                    ))
                })?,
                raw.producer_version.clone(),
            ),
        };
        Ok(MetricProvenance {
            producer,
            producer_version,
            source_field: raw.source_field.clone(),
            normalization: NORMALIZATION_METHOD_V1.to_owned(),
            raw_artifacts,
        })
    };

    let Some(group) = raw else {
        return Ok((
            ObservationState::Missing {
                reason: MissingReason::SourceNotProvided,
            },
            request_aggregation_fallback(request),
            base_provenance(None, Vec::new()),
        ));
    };

    // Vocabulary consistency: built-in direction mismatch surfaces as invalid
    // rather than silently rewriting the plan.
    if let Some(builtin) = builtin_metric(request.name.as_str()) {
        if request.unit.as_str() != builtin.unit {
            // A plan-declared unit that contradicts the vocabulary is a plan
            // inconsistency; still, per-request normalization must not abort
            // the run, so surface it as invalid when a source exists.
            let provenance = base_provenance(
                group.first().and_then(|first| first.source_field.clone()),
                Vec::new(),
            );
            return Ok((
                ObservationState::Invalid {
                    reason: InvalidReason::UnitMismatch,
                    detail: Some(format!(
                        "plan unit {} contradicts vocabulary unit {}",
                        request.unit.as_str(),
                        builtin.unit
                    )),
                },
                request_aggregation_fallback(request),
                provenance,
            ));
        }
        if let Some(expected) = builtin.direction
            && !direction_matches(expected, &request.direction)
        {
            let provenance = base_provenance(
                group.first().and_then(|first| first.source_field.clone()),
                Vec::new(),
            );
            return Ok((
                ObservationState::Invalid {
                    reason: InvalidReason::DomainError,
                    detail: Some(format!(
                        "plan direction contradicts vocabulary for {}",
                        request.name.as_str()
                    )),
                },
                request_aggregation_fallback(request),
                provenance,
            ));
        }
    }

    if group.len() != 1 {
        // Cross-producer duplicates attribute to the overriding producer
        // when one is present, independent of input order; the invalid
        // marking itself is what preserves the collision policy.
        let overridden = group.iter().find(|raw| raw.producer.is_some());
        let provenance = match overridden {
            Some(first) => observation_provenance(first, Vec::new())?,
            None => base_provenance(
                group.first().and_then(|first| first.source_field.clone()),
                Vec::new(),
            ),
        };
        return Ok((
            ObservationState::Invalid {
                reason: InvalidReason::DuplicateObservation,
                detail: Some(format!(
                    "{} raw observations for {}",
                    group.len(),
                    request.name.as_str()
                )),
            },
            request_aggregation_fallback(request),
            provenance,
        ));
    }

    let raw = group[0];
    raw.aggregation.validate()?;
    if raw.unit.as_str() != request.unit.as_str() {
        let provenance = observation_provenance(raw, Vec::new())?;
        return Ok((
            ObservationState::Invalid {
                reason: InvalidReason::UnitMismatch,
                detail: Some(format!(
                    "raw unit {} does not equal requested unit {}",
                    raw.unit.as_str(),
                    request.unit.as_str()
                )),
            },
            raw.aggregation,
            provenance,
        ));
    }
    if !raw.value.is_finite() {
        let provenance = observation_provenance(raw, Vec::new())?;
        return Ok((
            ObservationState::Invalid {
                reason: InvalidReason::NonFinite,
                detail: None,
            },
            raw.aggregation,
            provenance,
        ));
    }
    if is_ratio_metric(request.name.as_str()) && !(0.0..=1.0).contains(&raw.value) {
        let provenance = observation_provenance(raw, Vec::new())?;
        return Ok((
            ObservationState::Invalid {
                reason: InvalidReason::DomainError,
                detail: Some(format!(
                    "ratio metric {} outside [0,1]",
                    request.name.as_str()
                )),
            },
            raw.aggregation,
            provenance,
        ));
    }
    if let Some(builtin) = builtin_metric(request.name.as_str())
        && let Some(expected) = builtin.aggregation
        && !aggregation_matches(expected, raw.aggregation)
    {
        let provenance = observation_provenance(raw, Vec::new())?;
        return Ok((
            ObservationState::Invalid {
                reason: InvalidReason::AggregationMismatch,
                detail: Some(format!(
                    "raw aggregation {:?} contradicts vocabulary {expected:?}",
                    raw.aggregation
                )),
            },
            raw.aggregation,
            provenance,
        ));
    }

    // Resolve raw artifact names against the same-trial staged map.
    let mut resolved = Vec::new();
    for name in &raw.raw_artifacts {
        if let Some(path) = input.artifact_map.get(name) {
            resolved.push(path.clone());
        } else {
            let provenance = observation_provenance(raw, Vec::new())?;
            return Ok((
                ObservationState::Invalid {
                    reason: InvalidReason::MalformedSourceReference,
                    detail: Some(format!("unknown raw artifact {name}")),
                },
                raw.aggregation,
                provenance,
            ));
        }
    }
    let provenance = observation_provenance(raw, resolved)?;
    Ok((
        ObservationState::Observed { value: raw.value },
        raw.aggregation,
        provenance,
    ))
}

/// Normalize histogram inputs against the same-trial artifact map.
fn normalize_histograms(
    input: &NormalizationInput<'_>,
) -> Result<Vec<HistogramReference>, BundleError> {
    let mut histograms = Vec::new();
    for raw in input.histograms {
        if raw.metric.len() > Name::MAX_LEN
            || raw.artifact_name.len() > Name::MAX_LEN
            || raw.unit.len() > Name::MAX_LEN
        {
            return Err(BundleError::BoundExceeded("raw histogram"));
        }
        if raw.format.is_empty() || raw.format.len() > MAX_HISTOGRAM_FORMAT_LEN {
            return Err(BundleError::BoundExceeded("raw histogram format"));
        }
        if let Some(method) = &raw.method
            && method.len() > MAX_PROVENANCE_FIELD_LEN
        {
            return Err(BundleError::BoundExceeded("raw histogram method"));
        }
        // Unknown references are dropped with no claim; the scalar path marks
        // unknown refs invalid explicitly. Histograms are references only.
        let Some(path) = input.artifact_map.get(&raw.artifact_name) else {
            continue;
        };
        histograms.push(HistogramReference {
            metric: Name::new(raw.metric.clone()).map_err(|error| {
                BundleError::ManifestParse(format!("invalid histogram metric: {error}"))
            })?,
            path: path.clone(),
            format: raw.format.clone(),
            unit: Name::new(raw.unit.clone()).map_err(|error| {
                BundleError::ManifestParse(format!("invalid histogram unit: {error}"))
            })?,
            method: raw.method.clone(),
            source_metric: None,
        });
    }
    histograms.sort_by(|left, right| {
        (left.metric.as_str(), left.path.as_str())
            .cmp(&(right.metric.as_str(), right.path.as_str()))
    });
    Ok(histograms)
}

/// Normalize error-category counts into deterministic ordering.
fn normalize_error_counts(
    counts: &[(String, u64)],
) -> Result<Vec<ErrorCategoryCount>, BundleError> {
    let mut map = BTreeMap::new();
    for (category, count) in counts {
        let name = Name::new(category.clone()).map_err(|error| {
            BundleError::ManifestParse(format!("invalid error category: {error}"))
        })?;
        // Last write wins for duplicate categories; counts are descriptive.
        map.insert(name, *count);
    }
    Ok(map
        .into_iter()
        .map(|(category, count)| ErrorCategoryCount { category, count })
        .collect())
}

/// Push a bounded warning entry.
fn push_warning(
    warnings: &mut Vec<MetricWarning>,
    category: &str,
    detail: &str,
) -> Result<(), BundleError> {
    if warnings.len() >= MAX_METRIC_WARNINGS_PER_TRIAL {
        return Err(BundleError::BoundExceeded("metric warning count"));
    }
    let warning = MetricWarning {
        category: category.to_owned(),
        detail: detail.chars().take(MAX_METRIC_DETAIL_LEN).collect(),
    };
    warning.validate()?;
    warnings.push(warning);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MetricDirection, MetricIntent, PositiveCount};

    fn request(name: &str, unit: &str) -> MetricRequest {
        MetricRequest {
            name: Name::new(name).unwrap(),
            unit: Name::new(unit).unwrap(),
            direction: MetricDirection::LowerIsBetter,
            intent: MetricIntent::Primary,
            gate: None,
        }
    }

    fn input<'a>(
        trial_id: TrialId,
        metrics: &'a [MetricRequest],
        observations: &'a [RawMetricObservation],
        artifact_map: &'a BTreeMap<String, ArtifactPath>,
    ) -> NormalizationInput<'a> {
        NormalizationInput {
            trial_id,
            metrics,
            terminal_status: TrialExecutionStatus::Completed,
            observations,
            histograms: &[],
            error_counts: &[],
            artifact_map,
            producer: "fake-load",
            producer_version: Some("0.1.0"),
        }
    }

    #[test]
    fn vocabulary_versions_are_explicit() {
        assert_eq!(METRIC_VOCABULARY_VERSION, 1);
        assert_eq!(TRIAL_METRICS_SCHEMA_VERSION.0, 1);
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn valid_throughput_normalizes_to_observed() {
        let metrics = [MetricRequest {
            name: Name::new("throughput").unwrap(),
            unit: Name::new("rps").unwrap(),
            direction: MetricDirection::HigherIsBetter,
            intent: MetricIntent::Primary,
            gate: None,
        }];
        let observations = [RawMetricObservation {
            name: "throughput".to_owned(),
            unit: "rps".to_owned(),
            value: 12_345.0,
            aggregation: Aggregation::Rate,
            source_field: Some("requests_per_second".to_owned()),
            producer: None,
            producer_version: None,
            raw_artifacts: Vec::new(),
        }];
        let map = BTreeMap::new();
        let normalized = normalize_trial_metrics(&input(
            TrialId::new(1).unwrap(),
            &metrics,
            &observations,
            &map,
        ))
        .unwrap();
        assert_eq!(normalized.observations.len(), 1);
        assert!(matches!(
            normalized.observations[0].state,
            ObservationState::Observed { value } if value == 12_345.0
        ));
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn observed_zero_is_distinct_from_missing() {
        let metrics = [request("latency_p99", "ms")];
        let observations = [RawMetricObservation {
            name: "latency_p99".to_owned(),
            unit: "ms".to_owned(),
            value: 0.0,
            aggregation: Aggregation::Percentile { basis_points: 9900 },
            source_field: None,
            producer: None,
            producer_version: None,
            raw_artifacts: Vec::new(),
        }];
        let map = BTreeMap::new();
        let normalized = normalize_trial_metrics(&input(
            TrialId::new(1).unwrap(),
            &metrics,
            &observations,
            &map,
        ))
        .unwrap();
        assert!(matches!(
            normalized.observations[0].state,
            ObservationState::Observed { value } if value == 0.0
        ));

        let empty: Vec<RawMetricObservation> = Vec::new();
        let missing =
            normalize_trial_metrics(&input(TrialId::new(1).unwrap(), &metrics, &empty, &map))
                .unwrap();
        assert!(matches!(
            missing.observations[0].state,
            ObservationState::Missing {
                reason: MissingReason::SourceNotProvided
            }
        ));
    }

    #[test]
    fn nonfinite_and_unit_mismatch_are_invalid() {
        let metrics = [request("latency_p99", "ms")];
        let map = BTreeMap::new();
        let bad_value = [RawMetricObservation {
            name: "latency_p99".to_owned(),
            unit: "ms".to_owned(),
            value: f64::NAN,
            aggregation: Aggregation::Percentile { basis_points: 9900 },
            source_field: None,
            producer: None,
            producer_version: None,
            raw_artifacts: Vec::new(),
        }];
        let normalized =
            normalize_trial_metrics(&input(TrialId::new(1).unwrap(), &metrics, &bad_value, &map))
                .unwrap();
        assert!(matches!(
            normalized.observations[0].state,
            ObservationState::Invalid {
                reason: InvalidReason::NonFinite,
                ..
            }
        ));

        let bad_unit = [RawMetricObservation {
            name: "latency_p99".to_owned(),
            unit: "us".to_owned(),
            value: 1.0,
            aggregation: Aggregation::Percentile { basis_points: 9900 },
            source_field: None,
            producer: None,
            producer_version: None,
            raw_artifacts: Vec::new(),
        }];
        let normalized =
            normalize_trial_metrics(&input(TrialId::new(1).unwrap(), &metrics, &bad_unit, &map))
                .unwrap();
        assert!(matches!(
            normalized.observations[0].state,
            ObservationState::Invalid {
                reason: InvalidReason::UnitMismatch,
                ..
            }
        ));
    }

    #[test]
    fn duplicate_raw_observations_are_invalid() {
        let metrics = [request("latency_p99", "ms")];
        let observations = [
            RawMetricObservation {
                name: "latency_p99".to_owned(),
                unit: "ms".to_owned(),
                value: 1.0,
                aggregation: Aggregation::Percentile { basis_points: 9900 },
                source_field: None,
                producer: None,
                producer_version: None,
                raw_artifacts: Vec::new(),
            },
            RawMetricObservation {
                name: "latency_p99".to_owned(),
                unit: "ms".to_owned(),
                value: 2.0,
                aggregation: Aggregation::Percentile { basis_points: 9900 },
                source_field: None,
                producer: None,
                producer_version: None,
                raw_artifacts: Vec::new(),
            },
        ];
        let map = BTreeMap::new();
        let normalized = normalize_trial_metrics(&input(
            TrialId::new(1).unwrap(),
            &metrics,
            &observations,
            &map,
        ))
        .unwrap();
        assert!(matches!(
            normalized.observations[0].state,
            ObservationState::Invalid {
                reason: InvalidReason::DuplicateObservation,
                ..
            }
        ));
    }

    #[test]
    fn per_observation_producer_override_attributes_telemetry() {
        let metrics = [request("host_cpu_percent", "percent")];
        let observations = [RawMetricObservation {
            name: "host_cpu_percent".to_owned(),
            unit: "percent".to_owned(),
            value: 12.5,
            aggregation: Aggregation::Mean,
            source_field: Some("v2.cpu.usage_pct".to_owned()),
            producer: Some("gregg".to_owned()),
            producer_version: Some("1.0.14".to_owned()),
            raw_artifacts: Vec::new(),
        }];
        let map = BTreeMap::new();
        let normalized = normalize_trial_metrics(&input(
            TrialId::new(1).unwrap(),
            &metrics,
            &observations,
            &map,
        ))
        .unwrap();
        let observation = &normalized.observations[0];
        assert!(matches!(
            observation.state,
            ObservationState::Observed { .. }
        ));
        assert_eq!(observation.provenance.producer.as_str(), "gregg");
        assert_eq!(
            observation.provenance.producer_version.as_deref(),
            Some("1.0.14")
        );
        assert_eq!(
            observation.provenance.source_field.as_deref(),
            Some("v2.cpu.usage_pct")
        );
    }

    #[test]
    fn cross_producer_duplicates_stay_invalid() {
        // A workload observation and a telemetry observation for one
        // requested metric must not silently select one producer.
        let metrics = [request("host_cpu_percent", "percent")];
        let observations = [
            RawMetricObservation {
                name: "host_cpu_percent".to_owned(),
                unit: "percent".to_owned(),
                value: 1.0,
                aggregation: Aggregation::Mean,
                source_field: None,
                producer: None,
                producer_version: None,
                raw_artifacts: Vec::new(),
            },
            RawMetricObservation {
                name: "host_cpu_percent".to_owned(),
                unit: "percent".to_owned(),
                value: 2.0,
                aggregation: Aggregation::Mean,
                source_field: None,
                producer: Some("gregg".to_owned()),
                producer_version: None,
                raw_artifacts: Vec::new(),
            },
        ];
        let map = BTreeMap::new();
        let normalized = normalize_trial_metrics(&input(
            TrialId::new(1).unwrap(),
            &metrics,
            &observations,
            &map,
        ))
        .unwrap();
        assert!(matches!(
            normalized.observations[0].state,
            ObservationState::Invalid {
                reason: InvalidReason::DuplicateObservation,
                ..
            }
        ));
        // The collision diagnostic attributes to the overriding producer.
        assert_eq!(
            normalized.observations[0].provenance.producer.as_str(),
            "gregg"
        );
    }

    #[test]
    fn invalid_producer_override_is_structural() {
        let metrics = [request("host_cpu_percent", "percent")];
        let observations = [RawMetricObservation {
            name: "host_cpu_percent".to_owned(),
            unit: "percent".to_owned(),
            value: 1.0,
            aggregation: Aggregation::Mean,
            source_field: None,
            producer: Some(String::new()),
            producer_version: None,
            raw_artifacts: Vec::new(),
        }];
        let map = BTreeMap::new();
        assert!(
            normalize_trial_metrics(&input(
                TrialId::new(1).unwrap(),
                &metrics,
                &observations,
                &map,
            ))
            .is_err()
        );
    }

    #[test]
    fn observations_are_sorted_by_name() {
        let metrics = [
            request("throughput-x", "rps"),
            request("aaa-custom", "widgets"),
        ];
        let observations = [
            RawMetricObservation {
                name: "throughput-x".to_owned(),
                unit: "rps".to_owned(),
                value: 1.0,
                aggregation: Aggregation::Direct,
                source_field: None,
                producer: None,
                producer_version: None,
                raw_artifacts: Vec::new(),
            },
            RawMetricObservation {
                name: "aaa-custom".to_owned(),
                unit: "widgets".to_owned(),
                value: 2.0,
                aggregation: Aggregation::Direct,
                source_field: None,
                producer: None,
                producer_version: None,
                raw_artifacts: Vec::new(),
            },
        ];
        let map = BTreeMap::new();
        let normalized = normalize_trial_metrics(&input(
            TrialId::new(1).unwrap(),
            &metrics,
            &observations,
            &map,
        ))
        .unwrap();
        assert_eq!(normalized.observations[0].name.as_str(), "aaa-custom");
        assert_eq!(normalized.observations[1].name.as_str(), "throughput-x");
    }

    #[test]
    fn percentile_basis_points_encode_p999() {
        let aggregation = Aggregation::Percentile { basis_points: 9990 };
        aggregation.validate().unwrap();
        assert!(builtin_metric("latency_p999").is_some());
        let bad = Aggregation::Percentile {
            basis_points: 10_001,
        };
        assert!(bad.validate().is_err());
        let zero = Aggregation::Percentile { basis_points: 0 };
        assert!(zero.validate().is_err());
    }

    #[test]
    fn failed_trial_marks_metrics_missing() {
        let metrics = [request("latency_p99", "ms")];
        let observations = [RawMetricObservation {
            name: "latency_p99".to_owned(),
            unit: "ms".to_owned(),
            value: 5.0,
            aggregation: Aggregation::Percentile { basis_points: 9900 },
            source_field: None,
            producer: None,
            producer_version: None,
            raw_artifacts: Vec::new(),
        }];
        let map = BTreeMap::new();
        let result = normalize_trial_metrics(&NormalizationInput {
            trial_id: TrialId::new(1).unwrap(),
            metrics: &metrics,
            terminal_status: TrialExecutionStatus::Failed,
            observations: &observations,
            histograms: &[],
            error_counts: &[],
            artifact_map: &map,
            producer: "fake-load",
            producer_version: None,
        })
        .unwrap();
        assert!(matches!(
            result.observations[0].state,
            ObservationState::Missing {
                reason: MissingReason::TrialNotCompleted
            }
        ));
    }

    #[test]
    fn unknown_artifact_reference_is_invalid() {
        let metrics = [request("latency_p99", "ms")];
        let observations = [RawMetricObservation {
            name: "latency_p99".to_owned(),
            unit: "ms".to_owned(),
            value: 5.0,
            aggregation: Aggregation::Percentile { basis_points: 9900 },
            source_field: None,
            producer: None,
            producer_version: None,
            raw_artifacts: vec!["ghost.bin".to_owned()],
        }];
        let map = BTreeMap::new();
        let normalized = normalize_trial_metrics(&input(
            TrialId::new(1).unwrap(),
            &metrics,
            &observations,
            &map,
        ))
        .unwrap();
        assert!(matches!(
            normalized.observations[0].state,
            ObservationState::Invalid {
                reason: InvalidReason::MalformedSourceReference,
                ..
            }
        ));
    }

    #[test]
    fn duplicate_normalized_names_are_rejected() {
        let observation = NormalizedObservation {
            name: Name::new("latency_p99").unwrap(),
            unit: Name::new("ms").unwrap(),
            direction: MetricDirection::LowerIsBetter,
            intent: MetricIntent::Primary,
            aggregation: Aggregation::Percentile { basis_points: 9900 },
            state: ObservationState::Observed { value: 1.0 },
            provenance: MetricProvenance {
                producer: Name::new("fake-load").unwrap(),
                producer_version: None,
                source_field: None,
                normalization: NORMALIZATION_METHOD_V1.to_owned(),
                raw_artifacts: Vec::new(),
            },
        };
        let metrics = TrialMetrics {
            schema_version: TRIAL_METRICS_SCHEMA_VERSION,
            vocabulary_version: METRIC_VOCABULARY_VERSION,
            trial_id: TrialId::new(1).unwrap(),
            observations: vec![observation.clone(), observation],
            histograms: Vec::new(),
            error_distribution: Vec::new(),
            warnings: Vec::new(),
        };
        assert!(metrics.validate().is_err());
    }

    #[test]
    fn error_distribution_is_sorted() {
        let map = BTreeMap::new();
        let counts = vec![("timeout".to_owned(), 2_u64), ("refused".to_owned(), 1_u64)];
        let result = normalize_trial_metrics(&NormalizationInput {
            trial_id: TrialId::new(1).unwrap(),
            metrics: &[],
            terminal_status: TrialExecutionStatus::Completed,
            observations: &[],
            histograms: &[],
            error_counts: &counts,
            artifact_map: &map,
            producer: "fake-load",
            producer_version: None,
        })
        .unwrap();
        assert_eq!(result.error_distribution.len(), 2);
        assert_eq!(result.error_distribution[0].category.as_str(), "refused");
        assert_eq!(result.error_distribution[1].category.as_str(), "timeout");
    }

    #[test]
    fn positive_count_import_is_used() {
        let count = PositiveCount::new(3).unwrap();
        assert_eq!(count.get(), 3);
    }
}
