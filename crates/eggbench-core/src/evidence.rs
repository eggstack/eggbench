//! Immutable, portable `.eggb` evidence bundles.
use crate::{
    ArtifactBounds, DriverDescriptor, Name, ResolvedPlan, SchemaVersion, Subject, TrialMetrics,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    num::NonZeroU32,
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use thiserror::Error;
use uuid::Uuid;

/// Current evidence manifest schema version.
pub const EVIDENCE_MANIFEST_SCHEMA_VERSION: SchemaVersion = SchemaVersion(2);
/// Current placeholder schema version for testbed metadata.
pub const ENVIRONMENT_FINGERPRINT_SCHEMA_VERSION: SchemaVersion = SchemaVersion(1);
/// Maximum encoded manifest size.
pub const MAX_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;
/// Absolute artifact-count safety cap, in addition to plan-specific bounds.
pub const MAX_ARTIFACT_COUNT: usize = 10_000;
/// Maximum retained bytes for any single artifact.
pub const MAX_ARTIFACT_BYTES: u64 = 256 * 1024 * 1024;
/// Maximum total retained artifact bytes per bundle.
pub const MAX_TOTAL_ARTIFACT_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_PATH_BYTES: usize = 1024;
const MAX_PATH_DEPTH: usize = 16;
const HASH_BUFFER_BYTES: usize = 64 * 1024;

/// Globally unique run identity; independent of host process identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RunId(Uuid);

impl RunId {
    /// Generate a random stable run identity.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Parse a UUID-form run identity.
    ///
    /// # Errors
    /// Returns an error when the input is not a UUID.
    pub fn parse(value: &str) -> Result<Self, uuid::Error> {
        Uuid::parse_str(value).map(Self)
    }
}

impl Default for RunId {
    fn default() -> Self {
        Self::new()
    }
}
impl fmt::Display for RunId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Stable positive trial identity inside a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct TrialId(NonZeroU32);

impl TrialId {
    /// Create a positive trial identity.
    ///
    /// # Errors
    /// Returns an error for zero.
    pub fn new(value: u32) -> Result<Self, BundleError> {
        NonZeroU32::new(value)
            .map(Self)
            .ok_or(BundleError::InvalidManifest("trial id must be positive"))
    }

    /// Numeric identity, independent of directory ordering.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0.get()
    }
}
impl TryFrom<u32> for TrialId {
    type Error = BundleError;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}
impl From<TrialId> for u32 {
    fn from(value: TrialId) -> Self {
        value.get()
    }
}

/// Validated, portable, bundle-relative artifact path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ArtifactPath(String);

impl ArtifactPath {
    /// Validate an artifact path using portable forward-slash separators.
    ///
    /// # Errors
    /// Returns an error for empty, absolute, traversing, reserved, overlong, or over-deep paths.
    pub fn new(value: impl Into<String>) -> Result<Self, BundleError> {
        let value = value.into();
        let segments: Vec<_> = value.split('/').collect();
        if value.is_empty()
            || value.len() > MAX_PATH_BYTES
            || value.starts_with('/')
            || value.contains('\\')
            || value.chars().any(char::is_control)
            || segments.len() > MAX_PATH_DEPTH
            || segments.iter().any(|part| {
                part.is_empty()
                    || *part == "."
                    || *part == ".."
                    || part.contains(':')
                    || part.ends_with(' ')
                    || part.ends_with('.')
                    || is_windows_device_name(part)
            })
            || segments.first().is_some_and(|part| part.ends_with(':'))
            || value.eq_ignore_ascii_case("manifest.json")
        {
            return Err(BundleError::UnsafeArtifactPath(value));
        }
        Ok(Self(value))
    }

    /// Borrow the portable slash-separated path.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn to_path_buf(&self) -> PathBuf {
        self.0.split('/').collect()
    }
}
impl TryFrom<String> for ArtifactPath {
    type Error = BundleError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}
impl From<ArtifactPath> for String {
    fn from(value: ArtifactPath) -> Self {
        value.0
    }
}
impl fmt::Display for ArtifactPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Legacy terminal state used by manifest v1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyRunStatus {
    /// Legacy status: execution completed.
    Succeeded,
    /// Legacy status: run failed.
    Failed,
    /// Legacy status: run was cancelled.
    Cancelled,
    /// Legacy status: run was invalid.
    Invalid,
    /// Legacy status: ambiguous between no comparison and an inconclusive comparison.
    Inconclusive,
}

/// Lifecycle outcome of executing a run. It carries no comparison meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    /// The requested lifecycle completed and evidence was finalized.
    Completed,
    /// An operational/runtime phase failed.
    Failed,
    /// Execution was cancelled.
    Cancelled,
    /// Execution evidence violates a required validity contract.
    Invalid,
}

/// Verdict produced by a comparison policy, independent of run execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComparisonVerdict {
    /// Candidate satisfies the comparison policy.
    Pass,
    /// Candidate fails the comparison policy.
    Fail,
    /// Evidence does not support a pass or fail conclusion.
    Inconclusive,
    /// Evidence or comparison inputs are invalid.
    Invalid,
}

/// Logical artifact role recorded in the manifest.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ArtifactRole {
    /// Source `ExperimentPlan`.
    ExperimentPlan,
    /// `ResolvedPlan` snapshot.
    ResolvedPlan,
    /// Testbed fingerprint.
    EnvironmentFingerprint,
    /// Topology snapshot.
    Topology,
    /// Subject identity snapshot.
    Subject,
    /// One trial's machine-readable result.
    TrialResult,
    /// Trial telemetry stream.
    Telemetry,
    /// Captured standard output.
    Stdout,
    /// Captured standard error.
    Stderr,
    /// Additional trial artifact.
    TrialArtifact,
    /// Comparison summary.
    Comparison,
    /// Human or machine report.
    Report,
    /// Other bounded, named role.
    Other {
        /// Stable role label.
        label: Name,
    },
}

/// Artifact sensitivity classification. The manifest stores this label, never secret contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sensitivity {
    /// Safe for ordinary inspection.
    Public,
    /// Sanitized or redacted content.
    Redacted,
    /// Sensitive raw content governed by a future integration policy.
    Sensitive,
}

/// Verified metadata for one retained file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactRecord {
    /// Confined relative path.
    pub path: ArtifactPath,
    /// Logical purpose.
    pub role: ArtifactRole,
    /// Media type or format hint.
    pub media_type: String,
    /// Sensitivity label.
    pub sensitivity: Sensitivity,
    /// Exact retained byte count.
    pub byte_size: u64,
    /// Lowercase SHA-256 hex digest.
    pub sha256: String,
}

/// Stable per-trial references. The identity is explicit, not inferred from path sorting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrialDescriptor {
    /// Stable identity within the run.
    pub id: TrialId,
    /// Primary machine-readable result artifact.
    pub result: ArtifactPath,
    /// Other artifacts associated with this trial.
    #[serde(default)]
    pub artifacts: Vec<ArtifactPath>,
}

/// Versioned execution facts for one measured trial; contains no metric interpretation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrialExecutionResult {
    /// Trial execution schema version.
    pub schema_version: SchemaVersion,
    /// Stable measured-trial identity.
    pub trial_id: TrialId,
    /// Start offset from the run monotonic origin, in nanoseconds.
    pub measurement_start_offset_ns: u64,
    /// Measured workload invocation duration, in nanoseconds.
    pub measurement_elapsed_ns: u64,
    /// Terminal execution state for this invocation.
    pub terminal_status: TrialExecutionStatus,
    /// Redaction-safe failure classification, when the invocation did not complete.
    pub failure_category: Option<TrialExecutionFailure>,
    /// Paired arm measured by this trial; absent for unpaired trials and
    /// schema-v1 evidence.
    #[serde(default)]
    pub arm: Option<TrialArm>,
    /// Pair identity within a paired run; absent for unpaired trials and
    /// schema-v1 evidence.
    #[serde(default)]
    pub pair_id: Option<u32>,
}

/// Paired arm identity for one measured trial.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrialArm {
    /// Control arm (odd trials under the v1 alternating schedule).
    Baseline,
    /// Candidate arm under qualification (even trials).
    Candidate,
}

/// Terminal execution state for a measured trial.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrialExecutionStatus {
    /// Workload invocation completed successfully.
    Completed,
    /// Workload invocation failed.
    Failed,
    /// Workload invocation exceeded its safety timeout.
    TimedOut,
    /// Workload invocation was cancelled.
    Cancelled,
}

/// Redaction-safe failure category for one measured workload invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrialExecutionFailure {
    /// The workload adapter returned an operational error.
    WorkloadFailed,
    /// A trial telemetry collector failed outside measured workload timing.
    TelemetryFailed,
    /// The runner safety timeout expired.
    TimedOut,
    /// The invocation was cancelled.
    Cancelled,
}

/// Paired schedule identifier for M003 v1: strictly alternating arms
/// starting with baseline (trial 1 = baseline of pair 1).
pub const PAIRED_SCHEDULE_V1: &str = "alternating-baseline-first";

/// Paired-run record stored in the manifest of a paired bundle.
///
/// Present only when the run executed a predeclared paired design; absent
/// for all unpaired and legacy bundles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PairedRunRecord {
    /// Schedule identifier.
    pub schedule: String,
    /// Number of pairs executed.
    pub pairs: u32,
    /// Baseline arm service name.
    pub baseline_service: Name,
    /// Candidate arm service name.
    pub candidate_service: Name,
    /// Declared baseline arm subject identity.
    pub baseline_subject: Subject,
    /// Declared candidate arm subject identity.
    pub candidate_subject: Subject,
}

/// Authoritative immutable manifest for a completed bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleManifest {
    /// Manifest schema version.
    pub schema_version: SchemaVersion,
    /// Unique run identity.
    pub run_id: RunId,
    /// Lifecycle outcome. Missing only in the normalized view of ambiguous v1 evidence.
    pub execution_status: Option<ExecutionStatus>,
    /// Comparison outcome; absent when comparison was not performed.
    pub comparison_verdict: Option<ComparisonVerdict>,
    /// Subject identity snapshot; environment secrets remain references in this type.
    pub subject: Subject,
    /// Creation time as Unix milliseconds; optional for imported synthetic evidence.
    pub created_unix_ms: Option<u64>,
    /// Finalization time as Unix milliseconds; optional for imported synthetic evidence.
    pub finalized_unix_ms: Option<u64>,
    /// Required source plan artifact.
    pub plan: ArtifactPath,
    /// Required resolved plan artifact.
    pub resolved_plan: ArtifactPath,
    /// Required environment fingerprint artifact.
    pub environment: ArtifactPath,
    /// Stable trial descriptors; may be empty for preflight/failed runs.
    #[serde(default)]
    pub trials: Vec<TrialDescriptor>,
    /// Optional comparison summary.
    pub comparison: Option<ArtifactPath>,
    /// Optional paired-run record; present only for paired bundles.
    #[serde(default)]
    pub paired: Option<PairedRunRecord>,
    /// Optional report.
    pub report: Option<ArtifactPath>,
    /// Driver inventory snapshot.
    #[serde(default)]
    pub drivers: Vec<DriverDescriptor>,
    /// Artifact count and byte limits applied during staging.
    pub limits: ArtifactBounds,
    /// All non-manifest files retained by this bundle.
    pub artifacts: Vec<ArtifactRecord>,
    /// Must be true for a published bundle.
    pub finalized: bool,
}

/// Explicit manifest-v1 representation. Its `inconclusive` state is inherently ambiguous.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct LegacyManifestV1 {
    /// Original schema version (always 1).
    pub schema_version: SchemaVersion,
    /// Unique run identity.
    pub run_id: RunId,
    /// Original overloaded terminal status.
    pub status: LegacyRunStatus,
    /// Subject identity snapshot.
    pub subject: Subject,
    /// Creation timestamp.
    pub created_unix_ms: Option<u64>,
    /// Finalization timestamp.
    pub finalized_unix_ms: Option<u64>,
    /// Source plan artifact.
    pub plan: ArtifactPath,
    /// Resolved plan artifact.
    pub resolved_plan: ArtifactPath,
    /// Environment artifact.
    pub environment: ArtifactPath,
    /// Trial descriptors.
    #[serde(default)]
    pub trials: Vec<TrialDescriptor>,
    /// Optional comparison artifact.
    pub comparison: Option<ArtifactPath>,
    /// Optional report artifact.
    pub report: Option<ArtifactPath>,
    /// Driver inventory.
    #[serde(default)]
    pub drivers: Vec<DriverDescriptor>,
    /// Artifact bounds.
    pub limits: ArtifactBounds,
    /// Retained artifact metadata.
    pub artifacts: Vec<ArtifactRecord>,
    /// Finalization marker.
    pub finalized: bool,
}

impl LegacyManifestV1 {
    fn into_current_view(self) -> BundleManifest {
        let execution_status = match self.status {
            LegacyRunStatus::Succeeded | LegacyRunStatus::Inconclusive => {
                // The reader exposes the exact legacy value separately. For `Inconclusive`,
                // this normalized display value must not be interpreted without checking
                // `BundleReader::legacy_status`.
                Some(ExecutionStatus::Completed)
            }
            LegacyRunStatus::Failed => Some(ExecutionStatus::Failed),
            LegacyRunStatus::Cancelled => Some(ExecutionStatus::Cancelled),
            LegacyRunStatus::Invalid => Some(ExecutionStatus::Invalid),
        };
        BundleManifest {
            schema_version: EVIDENCE_MANIFEST_SCHEMA_VERSION,
            run_id: self.run_id,
            execution_status,
            comparison_verdict: None,
            subject: self.subject,
            created_unix_ms: self.created_unix_ms,
            finalized_unix_ms: self.finalized_unix_ms,
            plan: self.plan,
            resolved_plan: self.resolved_plan,
            environment: self.environment,
            trials: self.trials,
            comparison: self.comparison,
            paired: None,
            report: self.report,
            drivers: self.drivers,
            limits: self.limits,
            artifacts: self.artifacts,
            finalized: self.finalized,
        }
    }
}

impl BundleManifest {
    /// Validate schema, authoritative references, unique IDs/paths, and finalization state.
    ///
    /// # Errors
    /// Returns a categorized [`BundleError`] if the manifest is not a complete finalized contract.
    #[allow(clippy::too_many_lines)] // The authoritative manifest invariants are reviewed together.
    pub fn validate(&self) -> Result<(), BundleError> {
        self.validate_inner(false)
    }

    fn validate_legacy_view(&self) -> Result<(), BundleError> {
        self.validate_inner(true)
    }

    #[allow(clippy::too_many_lines)] // Keep current and bounded legacy invariants together.
    fn validate_inner(&self, legacy: bool) -> Result<(), BundleError> {
        if self.schema_version != EVIDENCE_MANIFEST_SCHEMA_VERSION {
            return Err(BundleError::UnsupportedManifestVersion(
                self.schema_version.0,
            ));
        }
        if self.execution_status.is_none() {
            return Err(BundleError::InvalidManifest("execution status is required"));
        }
        if !legacy && self.comparison.is_none() && self.comparison_verdict.is_some() {
            return Err(BundleError::InvalidManifest(
                "comparison verdict requires comparison artifact",
            ));
        }
        if !legacy && self.comparison.is_some() && self.comparison_verdict.is_none() {
            return Err(BundleError::InvalidManifest(
                "comparison artifact requires comparison verdict",
            ));
        }
        if !legacy
            && matches!(
                self.execution_status,
                Some(ExecutionStatus::Failed | ExecutionStatus::Cancelled)
            )
            && self.comparison_verdict.is_some()
        {
            return Err(BundleError::InvalidManifest(
                "failed or cancelled execution cannot have a comparison verdict",
            ));
        }
        if !self.finalized {
            return Err(BundleError::IncompleteBundle);
        }
        if self.limits.artifact_bytes == 0
            || self.limits.total_bytes < self.limits.artifact_bytes
            || self.limits.artifact_count.get() as usize > MAX_ARTIFACT_COUNT
            || self.limits.artifact_bytes > MAX_ARTIFACT_BYTES
            || self.limits.total_bytes > MAX_TOTAL_ARTIFACT_BYTES
            || self.artifacts.len() > self.limits.artifact_count.get() as usize
        {
            return Err(BundleError::InvalidManifest("invalid artifact bounds"));
        }
        let mut paths = BTreeMap::new();
        let mut roles = BTreeMap::<ArtifactRole, usize>::new();
        let mut total_bytes = 0_u64;
        for artifact in &self.artifacts {
            if paths.insert(&artifact.path, artifact).is_some() {
                return Err(BundleError::InvalidManifest("duplicate artifact path"));
            }
            *roles.entry(artifact.role.clone()).or_default() += 1;
            if artifact.sha256.len() != 64
                || !artifact
                    .sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            {
                return Err(BundleError::InvalidManifest("invalid SHA-256 digest"));
            }
            if artifact.media_type.is_empty()
                || artifact.media_type.len() > 255
                || artifact.media_type.chars().any(char::is_control)
            {
                return Err(BundleError::InvalidManifest("invalid media type"));
            }
            if artifact.byte_size > self.limits.artifact_bytes {
                return Err(BundleError::InvalidManifest(
                    "artifact exceeds declared byte bound",
                ));
            }
            total_bytes = total_bytes
                .checked_add(artifact.byte_size)
                .ok_or(BundleError::InvalidManifest("artifact byte total overflow"))?;
        }
        if total_bytes > self.limits.total_bytes {
            return Err(BundleError::InvalidManifest(
                "artifacts exceed declared total byte bound",
            ));
        }
        for required_role in [
            ArtifactRole::ExperimentPlan,
            ArtifactRole::ResolvedPlan,
            ArtifactRole::EnvironmentFingerprint,
        ] {
            if roles.get(&required_role) != Some(&1) {
                return Err(BundleError::InvalidManifest(
                    "required primary artifact role must occur exactly once",
                ));
            }
        }
        for optional_role in [ArtifactRole::Comparison, ArtifactRole::Report] {
            if roles.get(&optional_role).is_some_and(|count| *count > 1) {
                return Err(BundleError::InvalidManifest(
                    "comparison/report artifact role occurs more than once",
                ));
            }
        }
        for required in [&self.plan, &self.resolved_plan, &self.environment] {
            if !paths.contains_key(required) {
                return Err(BundleError::MissingArtifact(required.clone()));
            }
        }
        if paths[&self.plan].role != ArtifactRole::ExperimentPlan
            || paths[&self.resolved_plan].role != ArtifactRole::ResolvedPlan
            || paths[&self.environment].role != ArtifactRole::EnvironmentFingerprint
        {
            return Err(BundleError::InvalidManifest(
                "primary artifact reference has the wrong logical role",
            ));
        }
        let mut trial_ids = BTreeSet::new();
        for trial in &self.trials {
            if !trial_ids.insert(trial.id) {
                return Err(BundleError::InvalidManifest("duplicate trial identity"));
            }
            if !paths.contains_key(&trial.result)
                || paths[&trial.result].role != ArtifactRole::TrialResult
            {
                return Err(BundleError::MissingArtifact(trial.result.clone()));
            }
            let mut trial_paths = BTreeSet::new();
            for path in &trial.artifacts {
                if !trial_paths.insert(path) {
                    return Err(BundleError::InvalidManifest(
                        "duplicate artifact reference within a trial",
                    ));
                }
                if !paths.contains_key(path) {
                    return Err(BundleError::MissingArtifact(path.clone()));
                }
            }
        }
        if let Some(path) = &self.comparison
            && (!paths.contains_key(path) || paths[path].role != ArtifactRole::Comparison)
        {
            return Err(BundleError::MissingArtifact(path.clone()));
        }
        if let Some(path) = &self.report
            && (!paths.contains_key(path) || paths[path].role != ArtifactRole::Report)
        {
            return Err(BundleError::MissingArtifact(path.clone()));
        }
        if self.artifacts.len() > MAX_ARTIFACT_COUNT || self.trials.len() > MAX_ARTIFACT_COUNT {
            return Err(BundleError::InvalidManifest(
                "artifact count exceeds hard limit",
            ));
        }
        if self.drivers.len() > 256 {
            return Err(BundleError::InvalidManifest(
                "driver inventory exceeds bound",
            ));
        }
        let mut driver_names = BTreeSet::new();
        if self
            .drivers
            .iter()
            .any(|driver| !driver_names.insert(&driver.name))
        {
            return Err(BundleError::InvalidManifest("duplicate driver identity"));
        }
        if self
            .created_unix_ms
            .zip(self.finalized_unix_ms)
            .is_some_and(|(created, finalized)| finalized < created)
        {
            return Err(BundleError::InvalidManifest(
                "finalization time precedes creation time",
            ));
        }
        Ok(())
    }
}

/// Versioned placeholder contract for selected, non-secret testbed attributes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentFingerprint {
    /// Environment-fingerprint schema version.
    pub schema_version: SchemaVersion,
    /// Selected non-secret attributes such as OS family or CPU class.
    pub fields: BTreeMap<Name, EnvironmentField>,
}

/// Comparability policy class for one environment field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentFieldClass {
    /// A mismatch prevents strict same-testbed comparison.
    ComparisonCritical,
    /// A mismatch should be surfaced as a warning.
    WarningOnly,
    /// Recorded for context without affecting comparability.
    Informational,
}

/// One selected non-secret environment attribute and its comparability class.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentField {
    /// Selected value.
    pub value: String,
    /// How future comparison policy treats mismatches.
    pub class: EnvironmentFieldClass,
}

impl EnvironmentFingerprint {
    /// Construct schema-v1 synthetic or collected environment metadata.
    #[must_use]
    pub fn new(fields: BTreeMap<Name, EnvironmentField>) -> Self {
        Self {
            schema_version: ENVIRONMENT_FINGERPRINT_SCHEMA_VERSION,
            fields,
        }
    }

    /// Validate version and bounded attribute metadata.
    ///
    /// # Errors
    /// Returns a bundle error for an unsupported version or excessive/invalid field values.
    pub fn validate(&self) -> Result<(), BundleError> {
        if self.schema_version != ENVIRONMENT_FINGERPRINT_SCHEMA_VERSION {
            return Err(BundleError::InvalidManifest(
                "unsupported environment-fingerprint schema version",
            ));
        }
        if self.fields.len() > 128
            || self.fields.iter().any(|(name, field)| {
                name.as_str().len() > 128
                    || field.value.len() > 4096
                    || field.value.chars().any(char::is_control)
            })
        {
            return Err(BundleError::InvalidManifest(
                "environment fingerprint field exceeds bounds",
            ));
        }
        Ok(())
    }
}

/// Bundle creation, path, schema, and verification failures.
#[derive(Debug, Error)]
pub enum BundleError {
    /// Filesystem operation failed.
    #[error("I/O error at {path}: {source}")]
    Io {
        /// Affected path.
        path: PathBuf,
        /// Underlying I/O failure.
        #[source]
        source: io::Error,
    },
    /// Path is not safe for portable bundle use.
    #[error("unsafe artifact path: {0}")]
    UnsafeArtifactPath(String),
    /// Bundle path already exists.
    #[error("bundle destination already exists: {0}")]
    DestinationExists(PathBuf),
    /// Bundle is incomplete or still in staging.
    #[error("bundle is incomplete or still in staging")]
    IncompleteBundle,
    /// Manifest schema is unsupported.
    #[error("unsupported evidence manifest schema version {0}")]
    UnsupportedManifestVersion(u32),
    /// An artifact reference is missing.
    #[error("manifest references missing artifact {0}")]
    MissingArtifact(ArtifactPath),
    /// Artifact content, bytes, or digest does not match its manifest.
    #[error("artifact verification failed for {path}: {reason}")]
    ArtifactVerification {
        /// Artifact path.
        path: ArtifactPath,
        /// Failure detail.
        reason: &'static str,
    },
    /// Unmanifested file exists in the bundle.
    #[error("unmanifested file in bundle: {0}")]
    ExtraFile(PathBuf),
    /// A symlink is present; inspection never follows links.
    #[error("symbolic link is not allowed in evidence bundle: {0}")]
    Symlink(PathBuf),
    /// Caller exceeded a declared artifact bound.
    #[error("artifact bound exceeded: {0}")]
    BoundExceeded(&'static str),
    /// Manifest references or fields are inconsistent.
    #[error("invalid bundle manifest: {0}")]
    InvalidManifest(&'static str),
    /// JSON manifest could not be decoded.
    #[error("manifest parse error: {0}")]
    ManifestParse(String),
    /// Destination cannot support an atomic same-directory rename.
    #[error("atomic finalization is unsupported for destination {0}")]
    AtomicRenameUnsupported(PathBuf),
}

fn io_error(path: impl Into<PathBuf>, source: io::Error) -> BundleError {
    BundleError::Io {
        path: path.into(),
        source,
    }
}

fn validate_bounds(bounds: ArtifactBounds) -> Result<(), BundleError> {
    if bounds.artifact_bytes == 0
        || bounds.total_bytes < bounds.artifact_bytes
        || bounds.artifact_count.get() as usize > MAX_ARTIFACT_COUNT
        || bounds.artifact_bytes > MAX_ARTIFACT_BYTES
        || bounds.total_bytes > MAX_TOTAL_ARTIFACT_BYTES
    {
        return Err(BundleError::InvalidManifest(
            "declared artifact bounds exceed supported limits",
        ));
    }
    Ok(())
}

fn is_windows_device_name(segment: &str) -> bool {
    let stem = segment
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix).is_some_and(|suffix| {
                matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            })
        })
}

/// Writer restricted to one new sibling staging directory.
pub struct BundleWriter {
    final_path: PathBuf,
    staging_path: PathBuf,
    run_id: RunId,
    created_unix_ms: u64,
    bounds: ArtifactBounds,
    artifacts: BTreeMap<ArtifactPath, ArtifactRecord>,
    total_bytes: u64,
    paired_record: Option<PairedRunRecord>,
}

impl BundleWriter {
    /// Create a new sibling staging directory for a `.eggb` destination.
    ///
    /// # Errors
    /// Returns an error if the destination exists, the parent is invalid, or staging cannot be created.
    pub fn create(
        final_path: impl AsRef<Path>,
        run_id: RunId,
        bounds: ArtifactBounds,
    ) -> Result<Self, BundleError> {
        let mut final_path = final_path.as_ref().to_path_buf();
        if final_path.extension().and_then(|ext| ext.to_str()) != Some("eggb") {
            return Err(BundleError::InvalidManifest(
                "bundle destination must end in .eggb",
            ));
        }
        if final_path
            .parent()
            .is_some_and(|parent| parent.as_os_str().is_empty())
        {
            final_path = Path::new(".").join(final_path);
        }
        let parent = final_path.parent().unwrap_or_else(|| Path::new("."));
        let file_name = final_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(BundleError::InvalidManifest(
                "bundle path needs a valid file name",
            ))?;
        if fs::symlink_metadata(&final_path).is_ok() {
            return Err(BundleError::DestinationExists(final_path));
        }
        if !parent.is_dir() {
            return Err(io_error(
                parent,
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "bundle parent directory does not exist",
                ),
            ));
        }
        validate_bounds(bounds)?;
        let staging_path = parent.join(format!(".{file_name}.staging-{}", Uuid::new_v4()));
        fs::create_dir(&staging_path).map_err(|error| io_error(&staging_path, error))?;
        let created_unix_ms = unix_time_ms()?;
        Ok(Self {
            final_path,
            staging_path,
            run_id,
            created_unix_ms,
            bounds,
            artifacts: BTreeMap::new(),
            total_bytes: 0,
            paired_record: None,
        })
    }

    /// Record the paired-run manifest record for a paired run.
    ///
    /// Must be called before [`BundleWriter::finalize`] when the run
    /// executed a predeclared paired design; unpaired runs leave it absent.
    pub fn set_paired_record(&mut self, record: PairedRunRecord) {
        self.paired_record = Some(record);
    }

    /// Staging directory path, for diagnostics and incomplete-state inspection.
    #[must_use]
    pub fn staging_path(&self) -> &Path {
        &self.staging_path
    }

    /// Run identity for adapters creating run-scoped execution records.
    #[must_use]
    pub const fn run_id(&self) -> RunId {
        self.run_id
    }

    /// Intended finalized bundle path.
    #[must_use]
    pub fn final_path(&self) -> &Path {
        &self.final_path
    }

    /// Number of additional artifacts that can be registered before the count bound is reached.
    #[must_use]
    pub fn remaining_artifact_count(&self) -> usize {
        (self.bounds.artifact_count.get() as usize).saturating_sub(self.artifacts.len())
    }

    /// Remaining total bytes available for artifact contents.
    #[must_use]
    pub const fn remaining_total_bytes(&self) -> u64 {
        self.bounds.total_bytes - self.total_bytes
    }

    /// Maximum byte size permitted for one artifact.
    #[must_use]
    pub const fn max_artifact_bytes(&self) -> u64 {
        self.bounds.artifact_bytes
    }

    /// Stream an artifact into staging and register its size and SHA-256 digest.
    ///
    /// The file is copied in fixed 64 KiB chunks and is never buffered in full.
    ///
    /// # Errors
    /// Returns an error for unsafe/duplicate paths, I/O failures, or any artifact/count/total bound.
    pub fn add_artifact<R: Read>(
        &mut self,
        path: ArtifactPath,
        role: ArtifactRole,
        media_type: impl Into<String>,
        sensitivity: Sensitivity,
        mut source: R,
    ) -> Result<ArtifactRecord, BundleError> {
        let media_type = media_type.into();
        if media_type.is_empty()
            || media_type.len() > 255
            || media_type.chars().any(char::is_control)
        {
            return Err(BundleError::InvalidManifest("invalid media type"));
        }
        if self.artifacts.contains_key(&path) {
            return Err(BundleError::InvalidManifest("duplicate artifact path"));
        }
        if self.artifacts.len() >= self.bounds.artifact_count.get() as usize
            || self.artifacts.len() >= MAX_ARTIFACT_COUNT
        {
            return Err(BundleError::BoundExceeded("artifact count"));
        }
        let relative = path.to_path_buf();
        let destination = self.staging_path.join(&relative);
        create_safe_parents(&self.staging_path, &relative)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
            .map_err(|error| io_error(&destination, error))?;
        let write_result = (|| {
            let mut hasher = Sha256::new();
            let mut buffer = vec![0_u8; HASH_BUFFER_BYTES].into_boxed_slice();
            let mut size = 0_u64;
            let mut total = self.total_bytes;
            loop {
                let read = source
                    .read(&mut buffer)
                    .map_err(|error| io_error(&destination, error))?;
                if read == 0 {
                    break;
                }
                let next_size = size
                    .checked_add(read as u64)
                    .ok_or(BundleError::BoundExceeded("artifact byte size"))?;
                let next_total = total
                    .checked_add(read as u64)
                    .ok_or(BundleError::BoundExceeded("total bytes"))?;
                if next_size > self.bounds.artifact_bytes {
                    return Err(BundleError::BoundExceeded("individual artifact bytes"));
                }
                if next_total > self.bounds.total_bytes {
                    return Err(BundleError::BoundExceeded("total artifact bytes"));
                }
                file.write_all(&buffer[..read])
                    .map_err(|error| io_error(&destination, error))?;
                hasher.update(&buffer[..read]);
                size = next_size;
                total = next_total;
            }
            file.sync_all()
                .map_err(|error| io_error(&destination, error))?;
            Ok((size, total, to_hex(&hasher.finalize())))
        })();
        drop(file);
        let (size, new_total, sha256) = match write_result {
            Ok(result) => result,
            Err(error) => {
                let _ = fs::remove_file(&destination);
                return Err(error);
            }
        };
        self.total_bytes = new_total;
        let record = ArtifactRecord {
            path: path.clone(),
            role,
            media_type,
            sensitivity,
            byte_size: size,
            sha256,
        };
        self.artifacts.insert(path, record.clone());
        Ok(record)
    }

    /// Finalize and atomically publish an immutable bundle.
    ///
    /// The manifest is created only after all artifacts and references validate, then staging is
    /// renamed to the final sibling path. No cross-filesystem copy fallback is attempted.
    ///
    /// # Errors
    /// Returns an error for missing required artifacts, invalid references, bounds, or failed atomic publication.
    #[allow(clippy::too_many_arguments)] // Keep finalization's independent evidence fields explicit.
    pub fn finalize(
        self,
        execution_status: ExecutionStatus,
        comparison_verdict: Option<ComparisonVerdict>,
        subject: Subject,
        drivers: Vec<DriverDescriptor>,
        mut trials: Vec<TrialDescriptor>,
        comparison: Option<ArtifactPath>,
        report: Option<ArtifactPath>,
    ) -> Result<BundleReader, BundleError> {
        let find_role = |role: ArtifactRole| {
            self.artifacts
                .values()
                .find(|artifact| artifact.role == role)
                .map(|artifact| artifact.path.clone())
        };
        let plan = find_role(ArtifactRole::ExperimentPlan).ok_or(BundleError::InvalidManifest(
            "required plan artifact is absent",
        ))?;
        let resolved_plan = find_role(ArtifactRole::ResolvedPlan).ok_or(
            BundleError::InvalidManifest("required resolved-plan artifact is absent"),
        )?;
        let environment = find_role(ArtifactRole::EnvironmentFingerprint).ok_or(
            BundleError::InvalidManifest("required environment artifact is absent"),
        )?;
        for trial in &mut trials {
            trial.artifacts.sort();
        }
        trials.sort_by_key(|trial| trial.id);
        let mut drivers = drivers;
        drivers.sort_by(|left, right| left.name.cmp(&right.name));
        let manifest = BundleManifest {
            schema_version: EVIDENCE_MANIFEST_SCHEMA_VERSION,
            run_id: self.run_id,
            execution_status: Some(execution_status),
            comparison_verdict,
            subject,
            created_unix_ms: Some(self.created_unix_ms),
            finalized_unix_ms: Some(unix_time_ms()?),
            plan,
            resolved_plan,
            environment,
            trials,
            comparison,
            paired: self.paired_record.clone(),
            report,
            drivers,
            limits: self.bounds,
            artifacts: self.artifacts.into_values().collect(),
            finalized: true,
        };
        manifest.validate()?;
        verify_artifact_tree(&self.staging_path, &manifest.artifacts, false)?;
        let bytes = serde_json::to_vec_pretty(&manifest)
            .map_err(|error| BundleError::ManifestParse(error.to_string()))?;
        if bytes.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(BundleError::BoundExceeded("manifest bytes"));
        }
        let manifest_path = self.staging_path.join("manifest.json");
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&manifest_path)
            .map_err(|error| io_error(&manifest_path, error))?;
        file.write_all(&bytes)
            .map_err(|error| io_error(&manifest_path, error))?;
        file.sync_all()
            .map_err(|error| io_error(&manifest_path, error))?;
        // Windows cannot rename the staging directory while a manifest handle is open without
        // delete-sharing. Close it before syncing/publishing the directory.
        drop(file);
        sync_directory(&self.staging_path)?;

        publish_staging(&self.staging_path, &self.final_path)?;
        let parent = self.final_path.parent().unwrap_or_else(|| Path::new("."));
        sync_directory(parent)?;
        let bundle = BundleReader::open(&self.final_path)?;
        bundle.verify()?;
        Ok(bundle)
    }
}

/// Read-only finalized bundle inspector.
pub struct BundleReader {
    root: PathBuf,
    manifest: BundleManifest,
    legacy_status: Option<LegacyRunStatus>,
}

impl BundleReader {
    /// Open a finalized `.eggb` directory without following symlinks.
    ///
    /// # Errors
    /// Returns an error for staging, malformed/unsupported manifests, missing references, or symlinks.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, BundleError> {
        let root = path.as_ref().to_path_buf();
        let name = root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if name.contains(".staging-")
            || root.extension().and_then(|ext| ext.to_str()) != Some("eggb")
        {
            return Err(BundleError::IncompleteBundle);
        }
        let metadata = fs::symlink_metadata(&root).map_err(|error| io_error(&root, error))?;
        if metadata.file_type().is_symlink() {
            return Err(BundleError::Symlink(root));
        }
        if !metadata.is_dir() {
            return Err(BundleError::IncompleteBundle);
        }
        let manifest_path = root.join("manifest.json");
        let manifest_meta = fs::symlink_metadata(&manifest_path)
            .map_err(|error| io_error(&manifest_path, error))?;
        if manifest_meta.file_type().is_symlink() {
            return Err(BundleError::Symlink(manifest_path));
        }
        if manifest_meta.len() > MAX_MANIFEST_BYTES {
            return Err(BundleError::BoundExceeded("manifest bytes"));
        }
        let manifest_file = secure_open(&root, Path::new("manifest.json"))?;
        let manifest_capacity = usize::try_from(manifest_meta.len())
            .map_err(|_| BundleError::BoundExceeded("manifest bytes"))?;
        let mut bytes = Vec::with_capacity(manifest_capacity);
        manifest_file
            .take(MAX_MANIFEST_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| io_error(&manifest_path, error))?;
        if bytes.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(BundleError::BoundExceeded("manifest bytes"));
        }
        let manifest_version = serde_json::from_slice::<serde_json::Value>(&bytes)
            .map_err(|error| BundleError::ManifestParse(error.to_string()))?
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| BundleError::ManifestParse("missing schema_version".to_owned()))?;
        let (manifest, legacy_status) = match manifest_version {
            1 => {
                let legacy: LegacyManifestV1 = serde_json::from_slice(&bytes)
                    .map_err(|error| BundleError::ManifestParse(error.to_string()))?;
                if legacy.schema_version != SchemaVersion(1) {
                    return Err(BundleError::UnsupportedManifestVersion(
                        legacy.schema_version.0,
                    ));
                }
                let status = legacy.status;
                (legacy.into_current_view(), Some(status))
            }
            2 => (
                serde_json::from_slice::<BundleManifest>(&bytes)
                    .map_err(|error| BundleError::ManifestParse(error.to_string()))?,
                None,
            ),
            _ => {
                return Err(BundleError::UnsupportedManifestVersion(
                    u32::try_from(manifest_version).unwrap_or(u32::MAX),
                ));
            }
        };
        if legacy_status.is_some() {
            manifest.validate_legacy_view()?;
        } else {
            manifest.validate()?;
        }
        let reader = Self {
            root,
            manifest,
            legacy_status,
        };
        reader.validate_paths()?;
        Ok(reader)
    }

    /// Read-only manifest metadata.
    #[must_use]
    pub const fn manifest(&self) -> &BundleManifest {
        &self.manifest
    }

    /// Original overloaded v1 status, if this reader opened legacy evidence.
    ///
    /// `Inconclusive` cannot be resolved as either “no comparison” or an inconclusive
    /// comparison and must be treated as explicitly ambiguous.
    #[must_use]
    pub const fn legacy_status(&self) -> Option<LegacyRunStatus> {
        self.legacy_status
    }

    /// Bundle directory root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Verify exact artifact set, paths, sizes, and SHA-256 digests.
    ///
    /// # Errors
    /// Returns an error for missing/corrupt/extra files, symlinks, or path escapes.
    pub fn verify(&self) -> Result<(), BundleError> {
        self.validate_paths()?;
        verify_artifact_tree(&self.root, &self.manifest.artifacts, true)
    }

    /// Load normalized per-trial metrics when the trial staged `metrics.json`.
    ///
    /// Returns `None` for legacy/pre-M001 bundles that carry execution facts
    /// only. The metrics artifact must be manifest-listed and belong to the
    /// requested trial; no caller-supplied path is ever opened blindly.
    ///
    /// # Errors
    /// Returns an error when the metrics artifact is listed but unreadable or
    /// fails normalized-schema validation.
    pub fn trial_metrics(&self, trial_id: TrialId) -> Result<Option<TrialMetrics>, BundleError> {
        let descriptor = self
            .manifest
            .trials
            .iter()
            .find(|trial| trial.id == trial_id);
        let Some(descriptor) = descriptor else {
            return Ok(None);
        };
        let expected = crate::trial_metrics_path(trial_id)?;
        if !descriptor.artifacts.contains(&expected) {
            return Ok(None);
        }
        let mut file = self.open_artifact(&expected)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|error| io_error(expected.to_path_buf(), error))?;
        if bytes.len() as u64 > MAX_ARTIFACT_BYTES {
            return Err(BundleError::BoundExceeded("trial metrics bytes"));
        }
        let metrics: TrialMetrics = serde_json::from_slice(&bytes)
            .map_err(|error| BundleError::ManifestParse(error.to_string()))?;
        if metrics.trial_id != trial_id {
            return Err(BundleError::ManifestParse(format!(
                "trial metrics identity mismatch: expected trial {}",
                trial_id.get()
            )));
        }
        metrics.validate()?;
        Ok(Some(metrics))
    }

    /// Open a manifest-listed artifact read-only after checking path components for symlinks.
    ///
    /// # Errors
    /// Returns an error if the path is not listed, is unsafe, or cannot be opened.
    pub fn open_artifact(&self, path: &ArtifactPath) -> Result<File, BundleError> {
        if !self
            .manifest
            .artifacts
            .iter()
            .any(|artifact| &artifact.path == path)
        {
            return Err(BundleError::MissingArtifact(path.clone()));
        }
        let relative = path.to_path_buf();
        secure_open(&self.root, &relative)
    }

    fn validate_paths(&self) -> Result<(), BundleError> {
        for record in &self.manifest.artifacts {
            check_no_symlink_components(&self.root, &record.path.to_path_buf())?;
        }
        Ok(())
    }
}

fn unix_time_ms() -> Result<u64, BundleError> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| io_error("system clock", io::Error::other(error)))?;
    u64::try_from(duration.as_millis()).map_err(|_| BundleError::BoundExceeded("timestamp"))
}

fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[(byte >> 4) as usize]));
        output.push(char::from(HEX[(byte & 0x0f) as usize]));
    }
    output
}

fn hash_reader(file: &mut File, path: &Path) -> Result<String, BundleError> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; HASH_BUFFER_BYTES].into_boxed_slice();
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| io_error(path, error))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(to_hex(&hasher.finalize()))
}

#[cfg(unix)]
fn secure_open(root: &Path, relative: &Path) -> Result<File, BundleError> {
    use rustix::fs::{Mode, OFlags, open, openat};
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW;
    let mut directory = open(root, flags, Mode::empty())
        .map_err(|error| io_error(root, io::Error::other(error.to_string())))?;
    let components: Vec<_> = relative.components().collect();
    if components.is_empty() {
        return Err(BundleError::UnsafeArtifactPath(
            relative.display().to_string(),
        ));
    }
    for component in components.iter().take(components.len() - 1) {
        let Component::Normal(segment) = component else {
            return Err(BundleError::UnsafeArtifactPath(
                relative.display().to_string(),
            ));
        };
        directory = openat(directory, *segment, flags, Mode::empty())
            .map_err(|error| io_error(root.join(relative), io::Error::other(error.to_string())))?;
    }
    let Component::Normal(file_name) = components[components.len() - 1] else {
        return Err(BundleError::UnsafeArtifactPath(
            relative.display().to_string(),
        ));
    };
    let file = openat(
        directory,
        file_name,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map_err(|error| io_error(root.join(relative), io::Error::other(error.to_string())))?;
    Ok(File::from(file))
}

#[cfg(target_os = "linux")]
fn publish_staging(staging: &Path, final_path: &Path) -> Result<(), BundleError> {
    use rustix::{
        fs::{Mode, OFlags, RenameFlags, open, renameat_with},
        io::Errno,
    };
    let parent = final_path.parent().unwrap_or_else(|| Path::new("."));
    let directory = open(
        parent,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|error| io_error(parent, io::Error::other(error.to_string())))?;
    let staging_name = staging.file_name().ok_or(BundleError::InvalidManifest(
        "staging path needs a file name",
    ))?;
    let final_name = final_path
        .file_name()
        .ok_or(BundleError::InvalidManifest("final path needs a file name"))?;
    match renameat_with(
        &directory,
        staging_name,
        &directory,
        final_name,
        RenameFlags::NOREPLACE,
    ) {
        Ok(()) => Ok(()),
        Err(Errno::EXIST) => Err(BundleError::DestinationExists(final_path.to_path_buf())),
        Err(Errno::XDEV | Errno::INVAL | Errno::NOSYS | Errno::OPNOTSUPP) => Err(
            BundleError::AtomicRenameUnsupported(final_path.to_path_buf()),
        ),
        Err(error) => Err(io_error(final_path, io::Error::other(error.to_string()))),
    }
}

#[cfg(not(target_os = "linux"))]
fn publish_staging(staging: &Path, final_path: &Path) -> Result<(), BundleError> {
    let parent = final_path.parent().unwrap_or_else(|| Path::new("."));
    let final_name = final_path
        .file_name()
        .ok_or(BundleError::InvalidManifest("final path needs a file name"))?
        .to_string_lossy();
    let lock_path = parent.join(format!(".{final_name}.finalize-lock"));
    let lock = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_path)
        .map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                BundleError::DestinationExists(final_path.to_path_buf())
            } else {
                io_error(&lock_path, error)
            }
        })?;
    drop(lock);
    if fs::symlink_metadata(final_path).is_ok() {
        let _ = fs::remove_file(&lock_path);
        return Err(BundleError::DestinationExists(final_path.to_path_buf()));
    }
    let result = fs::rename(staging, final_path).map_err(|error| {
        if error.kind() == io::ErrorKind::CrossesDevices {
            BundleError::AtomicRenameUnsupported(final_path.to_path_buf())
        } else {
            io_error(final_path, error)
        }
    });
    let _ = fs::remove_file(lock_path);
    result
}

#[cfg(not(unix))]
fn secure_open(root: &Path, relative: &Path) -> Result<File, BundleError> {
    check_no_symlink_components(root, relative)?;
    let path = root.join(relative);
    File::open(&path).map_err(|error| io_error(path, error))
}

fn create_safe_parents(root: &Path, relative: &Path) -> Result<(), BundleError> {
    let mut cursor = root.to_path_buf();
    let components: Vec<_> = relative.components().collect();
    for component in components.iter().take(components.len().saturating_sub(1)) {
        let Component::Normal(segment) = component else {
            return Err(BundleError::UnsafeArtifactPath(
                relative.display().to_string(),
            ));
        };
        cursor.push(segment);
        match fs::create_dir(&cursor) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(io_error(&cursor, error)),
        }
        let meta = fs::symlink_metadata(&cursor).map_err(|error| io_error(&cursor, error))?;
        if meta.file_type().is_symlink() {
            return Err(BundleError::Symlink(cursor));
        }
        if !meta.is_dir() {
            return Err(BundleError::UnsafeArtifactPath(
                relative.display().to_string(),
            ));
        }
    }
    Ok(())
}

fn check_no_symlink_components(root: &Path, relative: &Path) -> Result<(), BundleError> {
    let root_meta = fs::symlink_metadata(root).map_err(|error| io_error(root, error))?;
    if root_meta.file_type().is_symlink() {
        return Err(BundleError::Symlink(root.to_path_buf()));
    }
    let mut cursor = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(segment) = component else {
            return Err(BundleError::UnsafeArtifactPath(
                relative.display().to_string(),
            ));
        };
        cursor.push(segment);
        match fs::symlink_metadata(&cursor) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(BundleError::Symlink(cursor));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(io_error(&cursor, error)),
        }
    }
    Ok(())
}

fn verify_artifact_tree(
    root: &Path,
    artifacts: &[ArtifactRecord],
    manifest_present: bool,
) -> Result<(), BundleError> {
    let expected: BTreeSet<PathBuf> = artifacts
        .iter()
        .map(|artifact| artifact.path.to_path_buf())
        .collect();
    let mut actual = BTreeSet::new();
    collect_files(root, root, &mut actual)?;
    if manifest_present {
        actual.remove(Path::new("manifest.json"));
    }
    if let Some(extra) = actual.difference(&expected).next() {
        return Err(BundleError::ExtraFile(extra.clone()));
    }
    if let Some(missing) = expected.difference(&actual).next() {
        return Err(BundleError::MissingArtifact(ArtifactPath::new(
            missing.to_string_lossy().to_string(),
        )?));
    }
    for artifact in artifacts {
        let path = root.join(artifact.path.to_path_buf());
        let metadata = fs::symlink_metadata(&path).map_err(|error| io_error(&path, error))?;
        if metadata.file_type().is_symlink() {
            return Err(BundleError::Symlink(path));
        }
        if !metadata.is_file() {
            return Err(BundleError::ArtifactVerification {
                path: artifact.path.clone(),
                reason: "not a regular file",
            });
        }
        if metadata.len() != artifact.byte_size {
            return Err(BundleError::ArtifactVerification {
                path: artifact.path.clone(),
                reason: "size mismatch",
            });
        }
        let mut file = secure_open(root, &artifact.path.to_path_buf())?;
        let digest = hash_reader(&mut file, &path)?;
        if digest != artifact.sha256 {
            return Err(BundleError::ArtifactVerification {
                path: artifact.path.clone(),
                reason: "SHA-256 mismatch",
            });
        }
    }
    Ok(())
}

fn collect_files(
    root: &Path,
    directory: &Path,
    files: &mut BTreeSet<PathBuf>,
) -> Result<(), BundleError> {
    for entry in fs::read_dir(directory).map_err(|error| io_error(directory, error))? {
        let entry = entry.map_err(|error| io_error(directory, error))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| io_error(&path, error))?;
        if metadata.file_type().is_symlink() {
            return Err(BundleError::Symlink(path));
        }
        if metadata.is_dir() {
            collect_files(root, &path, files)?;
        } else if metadata.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| BundleError::UnsafeArtifactPath(path.display().to_string()))?;
            files.insert(relative.to_path_buf());
        } else {
            return Err(BundleError::ExtraFile(path));
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn sync_directory(path: &Path) -> Result<(), BundleError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| io_error(path, error))
}

#[cfg(windows)]
#[allow(clippy::unnecessary_wraps)] // Keep the platform helper's fallible call contract uniform.
fn sync_directory(_path: &Path) -> Result<(), BundleError> {
    // Windows does not expose portable directory fsync through std; artifact and manifest files
    // are still flushed individually before the same-volume rename.
    Ok(())
}

fn validate_resolved_network_path(
    plan: &ResolvedPlan,
    path: &crate::ResolvedNetworkPath,
) -> Result<(), BundleError> {
    if plan.schema_version != crate::RESOLVED_PLAN_SCHEMA_VERSION
        || plan.source_plan_schema_version != crate::EXPERIMENT_PLAN_SCHEMA_VERSION_3
        || plan.paired.is_some()
        || matches!(plan.subject, crate::Subject::External { .. })
    {
        return Err(BundleError::InvalidManifest(
            "resolved network path requires an unpaired schema-v3 transport-owning plan",
        ));
    }
    let transport_workload_matches = plan
        .drivers
        .get(&crate::DriverCategory::Workload)
        .is_some_and(|workload| {
            workload.descriptor.category == crate::DriverCategory::Workload
                && workload.descriptor.name.as_str() == "eggfetch-http"
                && !workload.descriptor.adapter_version.is_empty()
                && workload.descriptor.adapter_version.len() <= 128
                && workload.descriptor.upstream_name == "eggfetch-core"
                && workload
                    .descriptor
                    .upstream_version
                    .as_deref()
                    .is_some_and(|version| !version.is_empty())
                && workload
                    .descriptor
                    .capabilities
                    .contains(&crate::Capability::NetworkPath)
                && !workload.descriptor.external_process
                && workload.executable_path.is_none()
        });
    if !transport_workload_matches {
        return Err(BundleError::InvalidManifest(
            "resolved network path requires a transport-owning NetworkPath workload",
        ));
    }
    let request = crate::NetworkPathRequest {
        route: path.route.clone(),
        stream_faults: path
            .stream_faults
            .as_ref()
            .map(|faults| faults.request.clone()),
    };
    crate::validate_network_path_contract(&request)
        .map_err(|_| BundleError::InvalidManifest("resolved network path contract is invalid"))?;
    validate_resolved_network_path_drivers(plan, path, &request)
}

fn validate_resolved_network_path_drivers(
    plan: &ResolvedPlan,
    path: &crate::ResolvedNetworkPath,
    request: &crate::NetworkPathRequest,
) -> Result<(), BundleError> {
    let route_descriptor = &path.route_driver.descriptor;
    let route_driver_matches = path.route_driver.executable_path.is_none()
        && route_descriptor.category == crate::DriverCategory::Route
        && route_descriptor.name.as_str() == "eggress-route"
        && !route_descriptor.adapter_version.is_empty()
        && route_descriptor.adapter_version.len() <= 128
        && route_descriptor.name == path.route.driver
        && route_descriptor.upstream_name == "eggress-outbound"
        && route_descriptor
            .upstream_version
            .as_deref()
            .is_some_and(|version| !version.is_empty())
        && route_descriptor
            .capabilities
            .contains(&crate::Capability::ProxyRouting)
        && !route_descriptor.external_process
        && plan
            .drivers
            .get(&crate::DriverCategory::Route)
            .is_some_and(|selected| selected.executable_path.is_none())
        && plan
            .drivers
            .get(&crate::DriverCategory::Route)
            .is_some_and(|selected| &selected.descriptor == route_descriptor);
    let fault_count = request
        .stream_faults
        .as_ref()
        .map_or(0, |faults| faults.upstream.len() + faults.downstream.len());
    let fault_driver_matches = path.stream_faults.as_ref().is_none_or(|faults| {
        let descriptor = &faults.fault_driver.descriptor;
        faults.fault_driver.executable_path.is_none()
            && descriptor.category == crate::DriverCategory::Fault
            && descriptor.name.as_str() == "eggchaos-stream"
            && !descriptor.adapter_version.is_empty()
            && descriptor.adapter_version.len() <= 128
            && descriptor.name == faults.request.driver
            && descriptor.upstream_name == "eggchaos-core"
            && descriptor
                .upstream_version
                .as_deref()
                .is_some_and(|version| !version.is_empty())
            && descriptor
                .capabilities
                .contains(&crate::Capability::StreamFaultPlan)
            && !descriptor.external_process
            && plan
                .drivers
                .get(&crate::DriverCategory::Fault)
                .is_some_and(|selected| {
                    &selected.descriptor == descriptor && selected.executable_path.is_none()
                })
    });
    let semantics_match = path.semantics_version == crate::NETWORK_PATH_SEMANTICS_VERSION
        && path
            .stream_faults
            .as_ref()
            .is_none_or(|faults| faults.rng_version == crate::NETWORK_PATH_RNG_VERSION);
    if route_driver_matches
        && fault_driver_matches
        && semantics_match
        && (fault_count == 0 || plan.seed.is_some())
    {
        Ok(())
    } else {
        Err(BundleError::InvalidManifest(
            "resolved network path failed route, driver, seed, or semantics validation",
        ))
    }
}

/// Extract the typed resolved-plan schema version from JSON bytes for bundle callers.
///
/// This helper rejects invalid `ResolvedPlan` snapshots before they are registered under the role.
///
/// # Errors
/// Returns an error when bytes do not decode as a versioned [`ResolvedPlan`].
pub fn validate_resolved_plan_bytes(bytes: &[u8]) -> Result<ResolvedPlan, BundleError> {
    let plan: ResolvedPlan = serde_json::from_slice(bytes)
        .map_err(|error| BundleError::ManifestParse(error.to_string()))?;
    if plan.schema_version != crate::RESOLVED_PLAN_SCHEMA_VERSION
        && plan.schema_version != crate::RESOLVED_PLAN_SCHEMA_VERSION_4
        && plan.schema_version != crate::RESOLVED_PLAN_SCHEMA_VERSION_3
        && plan.schema_version != crate::RESOLVED_PLAN_SCHEMA_VERSION_2
        && plan.schema_version != crate::RESOLVED_PLAN_SCHEMA_VERSION_1
    {
        return Err(BundleError::InvalidManifest(
            "unsupported resolved-plan schema version",
        ));
    }
    if let Some(path) = &plan.network_path {
        validate_resolved_network_path(&plan, path)?;
    } else if plan.schema_version != crate::RESOLVED_PLAN_SCHEMA_VERSION
        && plan.schema_version != crate::RESOLVED_PLAN_SCHEMA_VERSION_4
        && plan.source_plan_schema_version >= crate::EXPERIMENT_PLAN_SCHEMA_VERSION_3
    {
        return Err(BundleError::InvalidManifest(
            "legacy resolved plan cannot omit required schema-v3 network path",
        ));
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        DefaultDriverPolicy, DriverCategory, DriverDescriptor, LoadMode, PositiveCount,
        ResolutionOptions, SecretRef, Subject,
    };
    use std::{cell::Cell, rc::Rc};
    use tempfile::TempDir;

    fn bounds() -> ArtifactBounds {
        ArtifactBounds {
            artifact_count: PositiveCount::new(100).unwrap(),
            artifact_bytes: 8 * 1024 * 1024,
            total_bytes: 16 * 1024 * 1024,
        }
    }
    fn path(value: &str) -> ArtifactPath {
        ArtifactPath::new(value).unwrap()
    }

    #[test]
    fn trial_result_v1_parses_with_defaulted_arm_and_pair() {
        // Schema-v1 evidence carries no arm/pair tags; it must keep parsing
        // with both tags absent.
        let raw = serde_json::json!({
            "schema_version": 1,
            "trial_id": 3,
            "measurement_start_offset_ns": 100,
            "measurement_elapsed_ns": 200,
            "terminal_status": "completed",
            "failure_category": null,
        });
        let result: TrialExecutionResult = serde_json::from_value(raw).unwrap();
        assert_eq!(result.schema_version, SchemaVersion(1));
        assert_eq!(result.arm, None);
        assert_eq!(result.pair_id, None);
    }

    #[test]
    fn trial_result_v2_round_trips_arm_and_pair() {
        let result = TrialExecutionResult {
            schema_version: SchemaVersion(2),
            trial_id: TrialId::new(2).unwrap(),
            measurement_start_offset_ns: 100,
            measurement_elapsed_ns: 200,
            terminal_status: TrialExecutionStatus::Completed,
            failure_category: None,
            arm: Some(TrialArm::Candidate),
            pair_id: Some(1),
        };
        let round: TrialExecutionResult =
            serde_json::from_slice(&serde_json::to_vec(&result).unwrap()).unwrap();
        assert_eq!(round, result);
    }
    fn add_required(writer: &mut BundleWriter) {
        writer
            .add_artifact(
                path("plan.toml"),
                ArtifactRole::ExperimentPlan,
                "application/toml",
                Sensitivity::Redacted,
                &b"plan = true"[..],
            )
            .unwrap();
        writer
            .add_artifact(
                path("resolved-plan.json"),
                ArtifactRole::ResolvedPlan,
                "application/json",
                Sensitivity::Redacted,
                &b"{}"[..],
            )
            .unwrap();
        writer
            .add_artifact(
                path("environment.json"),
                ArtifactRole::EnvironmentFingerprint,
                "application/json",
                Sensitivity::Redacted,
                &b"{}"[..],
            )
            .unwrap();
    }
    fn finalized_bundle(root: &TempDir, name: &str, with_trials: bool) -> BundleReader {
        let destination = root.path().join(format!("{name}.eggb"));
        let mut writer = BundleWriter::create(&destination, RunId::new(), bounds()).unwrap();
        add_required(&mut writer);
        let trials = if with_trials {
            writer
                .add_artifact(
                    path("trials/007/result.json"),
                    ArtifactRole::TrialResult,
                    "application/json",
                    Sensitivity::Public,
                    &b"{\"ok\":true}"[..],
                )
                .unwrap();
            vec![TrialDescriptor {
                id: TrialId::new(7).unwrap(),
                result: path("trials/007/result.json"),
                artifacts: Vec::new(),
            }]
        } else {
            Vec::new()
        };
        writer
            .finalize(
                ExecutionStatus::Completed,
                None,
                Subject::External {
                    target: Name::new("api").unwrap(),
                    revision: None,
                    digest: None,
                },
                Vec::new(),
                trials,
                None,
                None,
            )
            .unwrap()
    }

    fn finalized_with_status(root: &TempDir, name: &str, status: ExecutionStatus) -> BundleReader {
        let destination = root.path().join(format!("{name}.eggb"));
        let mut writer = BundleWriter::create(&destination, RunId::new(), bounds()).unwrap();
        add_required(&mut writer);
        writer
            .finalize(
                status,
                None,
                Subject::External {
                    target: Name::new("api").unwrap(),
                    revision: None,
                    digest: None,
                },
                Vec::new(),
                Vec::new(),
                None,
                None,
            )
            .unwrap()
    }

    #[test]
    fn finalize_reopen_verify_and_manifest_is_written_last() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("complete.eggb");
        let mut writer = BundleWriter::create(&destination, RunId::new(), bounds()).unwrap();
        add_required(&mut writer);
        writer
            .add_artifact(
                path("trials/003/result.json"),
                ArtifactRole::TrialResult,
                "application/json",
                Sensitivity::Public,
                &b"result"[..],
            )
            .unwrap();
        let stage = writer.staging_path().to_path_buf();
        assert!(!stage.join("manifest.json").exists());
        let bundle = writer
            .finalize(
                ExecutionStatus::Completed,
                None,
                Subject::External {
                    target: Name::new("api").unwrap(),
                    revision: None,
                    digest: None,
                },
                Vec::new(),
                vec![TrialDescriptor {
                    id: TrialId::new(3).unwrap(),
                    result: path("trials/003/result.json"),
                    artifacts: Vec::new(),
                }],
                None,
                None,
            )
            .unwrap();
        assert!(bundle.root().join("manifest.json").is_file());
        assert_eq!(bundle.manifest().trials[0].id.get(), 3);
        bundle.verify().unwrap();
        let reopened = BundleReader::open(&destination).unwrap();
        reopened.verify().unwrap();
    }

    #[test]
    fn committed_synthetic_bundle_fixture_opens_and_verifies() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/example.eggb");
        let bundle = BundleReader::open(fixture).unwrap();
        bundle.verify().unwrap();
        assert_eq!(bundle.manifest().trials.len(), 1);
        assert_eq!(
            bundle.manifest().execution_status,
            Some(ExecutionStatus::Completed)
        );
        assert_eq!(bundle.manifest().comparison_verdict, None);
        assert_eq!(bundle.legacy_status(), Some(LegacyRunStatus::Inconclusive));
    }

    #[test]
    fn current_v2_fixture_opens_and_records_execution_without_comparison() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/current-v2.eggb");
        let bundle = BundleReader::open(fixture).unwrap();
        bundle.verify().unwrap();
        assert_eq!(bundle.manifest().schema_version, SchemaVersion(2));
        assert_eq!(
            bundle.manifest().execution_status,
            Some(ExecutionStatus::Completed)
        );
        assert_eq!(bundle.manifest().comparison_verdict, None);
        assert_eq!(bundle.legacy_status(), None);
    }

    #[test]
    fn execution_outcomes_and_comparison_verdicts_are_independent_and_coherent() {
        let temp = tempfile::tempdir().unwrap();
        for (name, status) in [
            ("failed", ExecutionStatus::Failed),
            ("cancelled", ExecutionStatus::Cancelled),
            ("invalid", ExecutionStatus::Invalid),
        ] {
            let bundle = finalized_with_status(&temp, name, status);
            assert_eq!(bundle.manifest().execution_status, Some(status));
            assert_eq!(bundle.manifest().comparison_verdict, None);
        }

        let mut compared = None;
        for (name, verdict) in [
            ("pass", ComparisonVerdict::Pass),
            ("fail", ComparisonVerdict::Fail),
            ("inconclusive", ComparisonVerdict::Inconclusive),
            ("invalid", ComparisonVerdict::Invalid),
        ] {
            let mut writer = BundleWriter::create(
                temp.path().join(format!("compared-{name}.eggb")),
                RunId::new(),
                bounds(),
            )
            .unwrap();
            add_required(&mut writer);
            let comparison_path = path("comparison.json");
            writer
                .add_artifact(
                    comparison_path.clone(),
                    ArtifactRole::Comparison,
                    "application/json",
                    Sensitivity::Public,
                    &b"{}"[..],
                )
                .unwrap();
            let bundle = writer
                .finalize(
                    ExecutionStatus::Completed,
                    Some(verdict),
                    Subject::External {
                        target: Name::new("api").unwrap(),
                        revision: None,
                        digest: None,
                    },
                    Vec::new(),
                    Vec::new(),
                    Some(comparison_path),
                    None,
                )
                .unwrap();
            assert_eq!(bundle.manifest().comparison_verdict, Some(verdict));
            if verdict == ComparisonVerdict::Pass {
                compared = Some(bundle);
            }
        }
        let compared = compared.unwrap();

        let mut contradictory = compared.manifest().clone();
        contradictory.execution_status = Some(ExecutionStatus::Failed);
        assert!(matches!(
            contradictory.validate(),
            Err(BundleError::InvalidManifest(_))
        ));
        contradictory.execution_status = Some(ExecutionStatus::Completed);
        contradictory.comparison_verdict = None;
        assert!(matches!(
            contradictory.validate(),
            Err(BundleError::InvalidManifest(_))
        ));
    }

    #[test]
    fn environment_fingerprint_is_versioned_and_bounded() {
        let mut fields = BTreeMap::new();
        fields.insert(
            Name::new("platform").unwrap(),
            EnvironmentField {
                value: "linux-x86_64".into(),
                class: EnvironmentFieldClass::ComparisonCritical,
            },
        );
        let fingerprint = EnvironmentFingerprint::new(fields);
        fingerprint.validate().unwrap();
        assert_eq!(
            fingerprint.schema_version,
            ENVIRONMENT_FINGERPRINT_SCHEMA_VERSION
        );
        let encoded = serde_json::to_string(&fingerprint).unwrap();
        let decoded: EnvironmentFingerprint = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, fingerprint);
    }

    #[test]
    fn destination_collision_and_interrupted_staging_fail_closed() {
        let temp = tempfile::tempdir().unwrap();
        finalized_bundle(&temp, "used", false);
        assert!(matches!(
            BundleWriter::create(temp.path().join("used.eggb"), RunId::new(), bounds()),
            Err(BundleError::DestinationExists(_))
        ));
        let writer =
            BundleWriter::create(temp.path().join("interrupted.eggb"), RunId::new(), bounds())
                .unwrap();
        let staging = writer.staging_path().to_path_buf();
        drop(writer);
        assert!(matches!(
            BundleReader::open(staging),
            Err(BundleError::IncompleteBundle)
        ));
        let excessive = ArtifactBounds {
            artifact_count: PositiveCount::new(100).unwrap(),
            artifact_bytes: MAX_ARTIFACT_BYTES + 1,
            total_bytes: MAX_TOTAL_ARTIFACT_BYTES,
        };
        assert!(matches!(
            BundleWriter::create(temp.path().join("oversize.eggb"), RunId::new(), excessive),
            Err(BundleError::InvalidManifest(_))
        ));
    }

    #[test]
    fn staging_tampering_is_detected_before_publication() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("tampered.eggb");
        let mut writer = BundleWriter::create(&destination, RunId::new(), bounds()).unwrap();
        add_required(&mut writer);
        fs::write(
            writer.staging_path().join("plan.toml"),
            b"changed after registration",
        )
        .unwrap();
        assert!(matches!(
            writer.finalize(
                ExecutionStatus::Completed,
                None,
                Subject::External {
                    target: Name::new("api").unwrap(),
                    revision: None,
                    digest: None,
                },
                Vec::new(),
                Vec::new(),
                None,
                None,
            ),
            Err(BundleError::ArtifactVerification { .. })
        ));
        assert!(!destination.exists());
    }

    #[test]
    fn empty_trial_failed_bundle_is_valid_and_multiple_trials_are_stable() {
        let temp = tempfile::tempdir().unwrap();
        let empty = finalized_bundle(&temp, "failed", false);
        assert!(empty.manifest().trials.is_empty());
        empty.verify().unwrap();
        let multiple = finalized_bundle(&temp, "trials", true);
        assert_eq!(multiple.manifest().trials[0].id, TrialId::new(7).unwrap());
        multiple.verify().unwrap();
    }

    #[test]
    fn missing_wrong_size_wrong_digest_and_extra_files_are_detected() {
        let temp = tempfile::tempdir().unwrap();
        let missing = finalized_bundle(&temp, "missing", false);
        fs::remove_file(missing.root().join("plan.toml")).unwrap();
        assert!(matches!(
            missing.verify(),
            Err(BundleError::MissingArtifact(_))
        ));

        let wrong_size = finalized_bundle(&temp, "wrong-size", false);
        fs::write(wrong_size.root().join("plan.toml"), b"different length").unwrap();
        assert!(matches!(
            wrong_size.verify(),
            Err(BundleError::ArtifactVerification {
                reason: "size mismatch",
                ..
            })
        ));

        let wrong_hash = finalized_bundle(&temp, "wrong-hash", false);
        fs::write(wrong_hash.root().join("plan.toml"), b"PLAN = true").unwrap();
        assert!(matches!(
            wrong_hash.verify(),
            Err(BundleError::ArtifactVerification {
                reason: "SHA-256 mismatch",
                ..
            })
        ));

        let extra = finalized_bundle(&temp, "extra", false);
        fs::write(extra.root().join("unexpected.bin"), b"extra").unwrap();
        assert!(matches!(extra.verify(), Err(BundleError::ExtraFile(_))));
    }

    #[test]
    fn path_collision_absolute_and_parent_traversal_are_rejected() {
        assert!(ArtifactPath::new("").is_err());
        assert!(ArtifactPath::new("/tmp/file").is_err());
        assert!(ArtifactPath::new("../outside").is_err());
        assert!(ArtifactPath::new("trials/../../outside").is_err());
        assert!(ArtifactPath::new("C:/outside").is_err());
        assert!(ArtifactPath::new("manifest.json").is_err());
        assert!(ArtifactPath::new("CON.txt").is_err());
        assert!(ArtifactPath::new("unsafe:name").is_err());
        assert!(ArtifactPath::new("trailing-dot.").is_err());
        let temp = tempfile::tempdir().unwrap();
        let mut writer =
            BundleWriter::create(temp.path().join("duplicate.eggb"), RunId::new(), bounds())
                .unwrap();
        writer
            .add_artifact(
                path("same.json"),
                ArtifactRole::Other {
                    label: Name::new("one").unwrap(),
                },
                "application/json",
                Sensitivity::Public,
                &b"one"[..],
            )
            .unwrap();
        assert!(matches!(
            writer.add_artifact(
                path("same.json"),
                ArtifactRole::Other {
                    label: Name::new("two").unwrap()
                },
                "application/json",
                Sensitivity::Public,
                &b"two"[..]
            ),
            Err(BundleError::InvalidManifest(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_is_rejected_without_following() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let bundle = finalized_bundle(&temp, "symlink", false);
        let outside = temp.path().join("outside.txt");
        fs::write(&outside, b"outside").unwrap();
        fs::remove_file(bundle.root().join("plan.toml")).unwrap();
        symlink(&outside, bundle.root().join("plan.toml")).unwrap();
        assert!(matches!(
            BundleReader::open(bundle.root()),
            Err(BundleError::Symlink(_))
        ));
    }

    #[test]
    fn unsupported_version_and_unknown_fields_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let unsupported = finalized_bundle(&temp, "unknown-version", false);
        let manifest_path = unsupported.root().join("manifest.json");
        let mut json: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        json["schema_version"] = 99.into();
        fs::write(&manifest_path, serde_json::to_vec(&json).unwrap()).unwrap();
        assert!(matches!(
            BundleReader::open(unsupported.root()),
            Err(BundleError::UnsupportedManifestVersion(99))
        ));

        let additive = finalized_bundle(&temp, "unknown-field", false);
        let manifest_path = additive.root().join("manifest.json");
        let mut json: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        json["future_field"] = true.into();
        fs::write(&manifest_path, serde_json::to_vec(&json).unwrap()).unwrap();
        let forward_compatible = BundleReader::open(additive.root()).unwrap();
        forward_compatible.verify().unwrap();
    }

    #[test]
    #[allow(clippy::too_many_lines)] // This is an end-to-end redaction boundary assertion.
    fn secret_references_and_manifest_labels_do_not_embed_secret_values() {
        let secret_values = [
            "Bearer bearer-token-value",
            "http://user:proxy-password@proxy.invalid",
            "session-cookie-value",
            "secret-environment-value",
        ];
        let mut plan =
            crate::ExperimentPlan::from_json(include_str!("../tests/fixtures/minimal.json"))
                .unwrap();
        plan.subject = Subject::ManagedCommand {
            argv: vec!["server".into()],
            environment: BTreeMap::from([
                (
                    "AUTHORIZATION".into(),
                    SecretRef {
                        reference: Name::new("API_TOKEN_ENV").unwrap(),
                    },
                ),
                (
                    "HTTP_PROXY_PASSWORD".into(),
                    SecretRef {
                        reference: Name::new("PROXY_PASSWORD_ENV").unwrap(),
                    },
                ),
                (
                    "COOKIE".into(),
                    SecretRef {
                        reference: Name::new("SESSION_COOKIE_ENV").unwrap(),
                    },
                ),
                (
                    "APP_SECRET".into(),
                    SecretRef {
                        reference: Name::new("APP_SECRET_ENV").unwrap(),
                    },
                ),
            ]),
            revision: None,
            digest: None,
        };
        plan.services.push(crate::Service {
            name: Name::new("api").unwrap(),
            kind: crate::ServiceKind::Named {
                service_type: Name::new("http").unwrap(),
            },
            lifecycle: crate::Lifecycle::External,
            depends_on: Vec::new(),
            config: BTreeMap::new(),
            http_url: None,
            readiness: None,
            shutdown: None,
            working_directory: None,
            log_limit_bytes: 4096,
        });
        plan.validate().unwrap();
        let plan_json = plan.to_json().unwrap();
        let service_driver = DriverDescriptor {
            name: Name::new("fake-service").unwrap(),
            adapter_version: "1".into(),
            upstream_name: "fake-service".into(),
            upstream_version: Some("1".into()),
            category: DriverCategory::Service,
            capabilities: BTreeSet::new(),
            supported_platforms: BTreeSet::new(),
            machine_output_schema: None,
            external_process: false,
            default: true,
            compatible_service_types: BTreeSet::new(),
        };
        let workload_driver = DriverDescriptor {
            name: Name::new("fake-load").unwrap(),
            adapter_version: "1".into(),
            upstream_name: "fake-load".into(),
            upstream_version: Some("1".into()),
            category: DriverCategory::Workload,
            capabilities: BTreeSet::from([crate::Capability::LoadMode {
                mode: LoadMode::ClosedLoop,
            }]),
            supported_platforms: BTreeSet::new(),
            machine_output_schema: None,
            external_process: false,
            default: true,
            compatible_service_types: BTreeSet::new(),
        };
        let resolved = crate::resolve_plan(
            &plan,
            &[service_driver, workload_driver],
            &ResolutionOptions {
                selections: BTreeMap::new(),
                default_policy: DefaultDriverPolicy::Deterministic,
                platform: Name::new("linux-x86_64").unwrap(),
                executable_paths: BTreeMap::new(),
                required_capabilities: BTreeMap::new(),
            },
        )
        .unwrap();
        let resolved_json = serde_json::to_string(&resolved).unwrap();
        let environment_json =
            serde_json::to_string(&EnvironmentFingerprint::new(BTreeMap::from([(
                Name::new("platform").unwrap(),
                EnvironmentField {
                    value: "linux-x86_64".into(),
                    class: EnvironmentFieldClass::ComparisonCritical,
                },
            )])))
            .unwrap();
        for secret in secret_values {
            assert!(!plan_json.contains(secret));
            assert!(!resolved_json.contains(secret));
            assert!(!environment_json.contains(secret));
        }

        let temp = tempfile::tempdir().unwrap();
        let mut writer =
            BundleWriter::create(temp.path().join("redacted.eggb"), RunId::new(), bounds())
                .unwrap();
        writer
            .add_artifact(
                path("plan.json"),
                ArtifactRole::ExperimentPlan,
                "application/json",
                Sensitivity::Redacted,
                plan_json.as_bytes(),
            )
            .unwrap();
        writer
            .add_artifact(
                path("resolved-plan.json"),
                ArtifactRole::ResolvedPlan,
                "application/json",
                Sensitivity::Redacted,
                resolved_json.as_bytes(),
            )
            .unwrap();
        writer
            .add_artifact(
                path("environment.json"),
                ArtifactRole::EnvironmentFingerprint,
                "application/json",
                Sensitivity::Redacted,
                environment_json.as_bytes(),
            )
            .unwrap();
        let bundle = writer
            .finalize(
                ExecutionStatus::Completed,
                None,
                plan.subject.clone(),
                Vec::new(),
                Vec::new(),
                None,
                None,
            )
            .unwrap();
        let manifest = serde_json::to_string(bundle.manifest()).unwrap();
        for secret in secret_values {
            assert!(!manifest.contains(secret));
        }
    }

    struct RepeatingReader {
        remaining: usize,
        max_request: Rc<Cell<usize>>,
    }
    impl Read for RepeatingReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.max_request
                .set(self.max_request.get().max(buffer.len()));
            let count = self.remaining.min(buffer.len());
            buffer[..count].fill(b'x');
            self.remaining -= count;
            Ok(count)
        }
    }

    #[test]
    fn large_artifact_hashing_uses_bounded_stream_buffers() {
        let temp = tempfile::tempdir().unwrap();
        let mut writer =
            BundleWriter::create(temp.path().join("large.eggb"), RunId::new(), bounds()).unwrap();
        add_required(&mut writer);
        let max_request = Rc::new(Cell::new(0));
        writer
            .add_artifact(
                path("large.data"),
                ArtifactRole::Other {
                    label: Name::new("large").unwrap(),
                },
                "application/octet-stream",
                Sensitivity::Public,
                RepeatingReader {
                    remaining: 2 * 1024 * 1024,
                    max_request: Rc::clone(&max_request),
                },
            )
            .unwrap();
        assert_eq!(max_request.get(), HASH_BUFFER_BYTES);
        writer
            .finalize(
                ExecutionStatus::Failed,
                None,
                Subject::External {
                    target: Name::new("api").unwrap(),
                    revision: None,
                    digest: None,
                },
                Vec::new(),
                Vec::new(),
                None,
                None,
            )
            .unwrap()
            .verify()
            .unwrap();
    }
}
