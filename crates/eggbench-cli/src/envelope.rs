//! Machine-output envelope for `eggbench` CLI commands.
//!
//! The envelope is the stable compatibility surface. Human prose and progress
//! diagnostics are emitted on stderr and are explicitly outside the
//! compatibility surface.

use serde::{Deserialize, Serialize};

/// Current CLI machine-output schema version.
pub const CLI_OUTPUT_SCHEMA_VERSION: u32 = 1;

/// Stable exit-code categories for the CLI binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCode {
    /// Requested command completed successfully.
    Success = 0,
    /// Parse, schema, or plan validation error.
    ParseValidation = 2,
    /// Capability/doctor/preflight unsupported or invalid.
    CapabilityPreflight = 3,
    /// Run completed with `Failed`/`Cancelled`/`Invalid` execution status.
    RunCompletedNonSuccess = 4,
    /// Evidence/bundle I/O or verification failure.
    EvidenceIo = 5,
    /// Internal/unclassified CLI failure.
    Internal = 1,
}

impl ExitCode {
    /// Numeric exit code as documented in the planning record.
    #[must_use]
    pub const fn code(self) -> i32 {
        self as i32
    }
}

/// Successful command payload variants.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CliOutput {
    /// `validate` summary.
    Validate {
        /// Experiment name when parse succeeded.
        experiment: Option<String>,
        /// Schema version of the parsed plan.
        schema_version: u32,
    },
    /// `doctor` summary.
    Doctor {
        /// Whether the plan resolved.
        resolved: bool,
        /// Whether the platform supports managed execution.
        platform_supported: bool,
        /// Driver inventory entries.
        drivers: Vec<DriverSummary>,
        /// Whether at least one workload driver is registered.
        has_workload_driver: bool,
        /// Detected environment fingerprint summary fields.
        environment_fields: Vec<EnvironmentSummary>,
    },
    /// `run` summary with finalized bundle path and execution status.
    Run {
        /// Bundle directory path.
        bundle: PathBufPayload,
        /// Whether a finalized bundle was produced.
        bundle_published: bool,
        /// Truthful execution status.
        execution_status: String,
        /// Number of measured trials completed.
        measured_trials: usize,
        /// Optional primary failure category.
        primary_failure: Option<String>,
        /// Comparison verdict when present.
        comparison_verdict: Option<String>,
    },
    /// `inspect` summary with manifest-relevant fields.
    Inspect {
        /// Manifest schema version.
        manifest_schema_version: u32,
        /// Run identity.
        run_id: String,
        /// Execution status if present.
        execution_status: Option<String>,
        /// Comparison verdict if present.
        comparison_verdict: Option<String>,
        /// Legacy v1 status if applicable.
        legacy_status: Option<String>,
        /// Subject identity snapshot.
        subject: SubjectSummary,
        /// Driver inventory.
        drivers: Vec<DriverSummary>,
        /// Environment fingerprint summary fields.
        environment_fields: Vec<EnvironmentSummary>,
        /// Trial identifiers and statuses.
        trials: Vec<TrialSummary>,
        /// Artifact count and bytes.
        artifact_count: usize,
        /// Sum of retained bytes across bundle artifacts.
        artifact_bytes: u64,
        /// Normalized manifest JSON when emitted.
        manifest_json: Option<String>,
    },
}

/// JSON envelope written to stdout when `--json` is selected.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CliEnvelope {
    /// Schema version of this envelope.
    pub schema_version: u32,
    /// Command identifier.
    pub command: String,
    /// Whether the command succeeded.
    pub ok: bool,
    /// Successful command payload, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<CliOutput>,
    /// Structured error, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<CliFailurePayload>,
    /// Warning diagnostics recorded during execution.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<CliWarnings>,
}

/// Human-readable warning entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CliWarnings {
    /// Stable warning category.
    pub category: String,
    /// Human detail string.
    pub detail: String,
}

/// Failure payload with stable category.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CliFailurePayload {
    /// Stable category.
    pub category: String,
    /// Human detail string.
    pub detail: String,
}

/// Convenience alias for [`Result<CliEnvelope, CliFailure>`].
pub type CliResult = Result<CliEnvelope, CliFailure>;

/// Stable CLI failure used by [`CliEnvelope::error`].
#[derive(Debug, Clone, PartialEq)]
pub struct CliFailure {
    /// Stable category.
    pub category: String,
    /// Human detail.
    pub detail: String,
    /// Suggested exit code for the binary.
    pub exit_code: ExitCode,
}

impl CliFailure {
    /// Construct a new CLI failure with the given category, detail, and exit
    /// code.
    #[must_use]
    pub fn new(
        category: impl Into<String>,
        detail: impl Into<String>,
        exit_code: ExitCode,
    ) -> Self {
        Self {
            category: category.into(),
            detail: detail.into(),
            exit_code,
        }
    }

    /// Convert to the JSON payload shape.
    #[must_use]
    pub fn to_payload(&self) -> CliFailurePayload {
        CliFailurePayload {
            category: self.category.clone(),
            detail: self.detail.clone(),
        }
    }
}

impl CliEnvelope {
    /// Build a successful envelope.
    #[must_use]
    pub fn ok(command: impl Into<String>, result: CliOutput) -> Self {
        Self {
            schema_version: CLI_OUTPUT_SCHEMA_VERSION,
            command: command.into(),
            ok: true,
            result: Some(result),
            error: None,
            warnings: Vec::new(),
        }
    }

    /// Build a failure envelope.
    #[must_use]
    pub fn fail(command: impl Into<String>, failure: &CliFailure) -> Self {
        Self {
            schema_version: CLI_OUTPUT_SCHEMA_VERSION,
            command: command.into(),
            ok: false,
            result: None,
            error: Some(failure.to_payload()),
            warnings: Vec::new(),
        }
    }

    /// Append a warning to the envelope.
    #[must_use]
    pub fn with_warning(mut self, category: impl Into<String>, detail: impl Into<String>) -> Self {
        self.warnings.push(CliWarnings {
            category: category.into(),
            detail: detail.into(),
        });
        self
    }

    /// Serialize to pretty JSON.
    ///
    /// # Errors
    /// Returns a `serde_json` error when serialization fails.
    pub fn to_pretty_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

/// Path payload that serializes only the string form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PathBufPayload(pub String);

impl PathBufPayload {
    /// Construct from a UTF-8 path.
    #[must_use]
    pub fn from_path(path: &std::path::Path) -> Option<Self> {
        path.to_str().map(|value| Self(value.to_owned()))
    }

    /// Construct from a UTF-8 string.
    #[must_use]
    pub fn from_string(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl From<&str> for PathBufPayload {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

/// Driver summary used by `doctor` and `inspect`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriverSummary {
    /// Canonical driver name.
    pub name: String,
    /// Driver category label.
    pub category: String,
    /// Whether the driver is the registered default for its category.
    pub default: bool,
    /// Whether the driver is backed by an external process.
    pub external_process: bool,
}

/// Environment fingerprint summary used by `doctor` and `inspect`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentSummary {
    /// Field name.
    pub name: String,
    /// Field value.
    pub value: String,
    /// Comparability class label.
    pub class: String,
}

/// Subject summary used by `inspect`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectSummary {
    /// Logical subject label.
    pub label: String,
    /// Declared revision hint if any.
    pub revision: Option<String>,
    /// Declared digest hint if any.
    pub declared_digest: Option<String>,
}

/// Trial summary used by `inspect`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrialSummary {
    /// Stable trial identity.
    pub id: u32,
    /// Terminal execution status label.
    pub terminal_status: String,
    /// Measured elapsed nanoseconds when the trial completed.
    pub measurement_elapsed_ns: Option<u64>,
}
