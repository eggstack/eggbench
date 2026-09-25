//! One-shot lifecycle diagnostic seam (Eggstack M003b).
//!
//! Diagnostics are bounded one-shot lifecycle checks that run outside every
//! measured workload interval: pre-workload after readiness/before warmups,
//! post-workload after drain/before teardown. They are distinct from repeated
//! trial telemetry and from workload execution.
//!
//! This module is sibling-neutral: no Eggprobe types cross here. Executors
//! receive a resolved [`DiagnosticContext`] and return a [`DiagnosticOutput`]
//! with bounded raw report bytes plus typed disposition/provenance.

use eggbench_core::{DiagnosticPhase, RunId};
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use crate::service::RuntimeBindings;

/// Context passed to one diagnostic execution.
#[derive(Debug, Clone)]
pub struct DiagnosticContext {
    /// Bundle/run identity.
    pub run_id: RunId,
    /// Diagnostic request identity.
    pub diagnostic_id: String,
    /// Which lifecycle slot this execution fills.
    pub phase: DiagnosticPhase,
    /// Target service name from the resolved request.
    pub target: String,
    /// Requested probe families for this execution.
    pub probes: Vec<eggbench_core::DiagnosticProbe>,
    /// Whether a negative/failed outcome invalidates the run.
    pub required: bool,
    /// Startup-established runtime bindings snapshot (read-only).
    pub bindings: RuntimeBindings,
    /// Cancellation token for this diagnostic.
    pub cancellation: CancellationToken,
    /// Per-diagnostic timeout.
    pub timeout: Duration,
}

/// Typed execution disposition for one diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticDisposition {
    /// Valid report with a positive (passing) outcome.
    Positive,
    /// Valid report with a negative outcome (NOT a process failure).
    Negative,
    /// Operational failure (spawn, timeout, invalid plan, internal error).
    Failed,
    /// Skipped (e.g. cancellation already requested before post diagnostics).
    Skipped,
}

/// Bounded diagnostic output.
#[derive(Debug, Clone)]
pub struct DiagnosticOutput {
    /// Bounded raw report bytes (Eggprobe `ProbeReport` JSON).
    pub raw_report: Vec<u8>,
    /// Typed disposition.
    pub disposition: DiagnosticDisposition,
    /// Producer name (e.g. `eggprobe`).
    pub producer: String,
    /// Producer version string.
    pub producer_version: String,
    /// SHA-256 of the selected executable.
    pub executable_sha256: String,
    /// Machine schema version (e.g. `0.3`).
    pub machine_schema: String,
    /// Report-level status label from the tool.
    pub report_status: String,
    /// Per-probe `(family, status)` pairs.
    pub probe_statuses: Vec<(String, String)>,
    /// Bounded warnings.
    pub warnings: Vec<String>,
    /// Skip reason when disposition is `Skipped`.
    pub skipped_reason: Option<String>,
}

/// One staged diagnostic execution record for the run-level index.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticExecutionRecord {
    /// Diagnostic request identity.
    pub id: String,
    /// Lifecycle slot executed (`pre_workload` / `post_workload`).
    pub phase: String,
    /// Whether the request was required.
    pub required: bool,
    /// Target service name.
    pub target: String,
    /// Requested probe families.
    pub probes: Vec<String>,
    /// Per-diagnostic timeout in milliseconds.
    pub timeout_ms: u64,
    /// Typed execution disposition (`positive` / `negative` / `failed` / `skipped`).
    pub disposition: String,
    /// Tool report status label.
    pub report_status: String,
    /// Bundle-relative artifact path (`diagnostics/<phase>/<id>.json`).
    pub artifact: String,
    /// SHA-256 of the raw report bytes.
    pub artifact_sha256: String,
    /// Producer version.
    pub producer_version: String,
    /// Producer executable SHA-256.
    pub executable_sha256: String,
    /// Machine schema version.
    pub machine_schema: String,
    /// Bounded warnings.
    #[serde(default)]
    pub warnings: Vec<String>,
    /// Skip reason, when skipped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skipped_reason: Option<String>,
}

/// Run-level `diagnostics.json` index (schema v1).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticsIndex {
    /// Index schema version (1).
    pub schema_version: eggbench_core::SchemaVersion,
    /// Canonical diagnostic driver name (`eggprobe`).
    pub driver: String,
    /// Eggbench adapter version.
    pub adapter_version: String,
    /// Observed tool version.
    pub executable_version: String,
    /// SHA-256 of the selected executable.
    pub executable_sha256: String,
    /// Accepted machine schema (`0.3`).
    pub machine_schema: String,
    /// One record per diagnostic execution, in plan order (pre before post).
    pub executions: Vec<DiagnosticExecutionRecord>,
}

impl DiagnosticsIndex {
    /// Validate the index contract before staging.
    ///
    /// When every execution was skipped (cancellation before any diagnostic
    /// ran), no producer provenance exists and empty version/digest fields
    /// are accepted; the skipped records still prove what was requested.
    ///
    /// # Errors
    /// Returns [`eggbench_core::BundleError`] when the contract is violated.
    pub fn validate_contract(&self) -> Result<(), eggbench_core::BundleError> {
        if self.schema_version != eggbench_core::SchemaVersion(1) {
            return Err(eggbench_core::BundleError::InvalidManifest(
                "diagnostics index schema mismatch",
            ));
        }
        let all_skipped = !self.executions.is_empty()
            && self
                .executions
                .iter()
                .all(|execution| execution.disposition == "skipped");
        let provenance_ok = if all_skipped {
            self.executable_version.len() <= 128
                && (self.executable_sha256.is_empty() || self.executable_sha256.len() == 64)
        } else {
            !self.executable_version.is_empty()
                && self.executable_version.len() <= 128
                && self.executable_sha256.len() == 64
        };
        if self.driver != "eggprobe"
            || self.adapter_version.is_empty()
            || self.adapter_version.len() > 128
            || !provenance_ok
            || self.machine_schema != "0.3"
            || self.executions.len() > 64
        {
            return Err(eggbench_core::BundleError::InvalidManifest(
                "diagnostics index contract is invalid",
            ));
        }
        Ok(())
    }
}

/// Manifest role label for `diagnostics.json` and per-diagnostic artifacts.
///
/// # Panics
///
/// Never panics at runtime; the static label is valid by construction.
#[must_use]
pub fn diagnostics_role_label() -> eggbench_core::Name {
    eggbench_core::Name::new("diagnostics").expect("static role label")
}

/// Object-safe asynchronous diagnostic executor.
pub trait DiagnosticExecutor: Send {
    /// Canonical source label (e.g. `eggprobe`).
    fn source(&self) -> &str;

    /// Execute one diagnostic outside measured timing.
    fn execute<'a>(
        &'a mut self,
        context: DiagnosticContext,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<DiagnosticOutput, crate::orchestration::FailureCategory>>
                + Send
                + 'a,
        >,
    >;
}

/// Explicit registry of diagnostic executors keyed by source.
#[derive(Default)]
pub struct DiagnosticRegistry {
    executors: BTreeMap<String, Arc<tokio::sync::Mutex<Box<dyn DiagnosticExecutor>>>>,
}

impl DiagnosticRegistry {
    /// Empty registry (no diagnostics execute; plans requesting them fail
    /// closed at orchestration preflight).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an executor for its source label.
    pub fn register(&mut self, executor: Box<dyn DiagnosticExecutor>) {
        self.executors.insert(
            executor.source().to_owned(),
            Arc::new(tokio::sync::Mutex::new(executor)),
        );
    }

    /// Look up an executor by source label.
    #[must_use]
    pub fn lookup(
        &self,
        source: &str,
    ) -> Option<Arc<tokio::sync::Mutex<Box<dyn DiagnosticExecutor>>>> {
        self.executors.get(source).cloned()
    }

    /// True when no executor is registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.executors.is_empty()
    }
}

impl std::fmt::Debug for DiagnosticRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiagnosticRegistry")
            .field(
                "sources",
                &self.executors.keys().cloned().collect::<Vec<_>>(),
            )
            .finish()
    }
}

/// Deterministic fake diagnostic executor for runner qualification.
///
/// Returns a canned [`DiagnosticOutput`] per diagnostic ID, or a canned
/// failure when the ID is listed in `fail_ids` / `negative_ids`.
#[derive(Debug, Clone, Default)]
pub struct FakeDiagnosticExecutor {
    /// Source label this fake serves.
    pub source: String,
    /// Diagnostic IDs that report a negative outcome.
    pub negative_ids: Vec<String>,
    /// Diagnostic IDs that return an operational failure.
    pub fail_ids: Vec<String>,
    /// Executed diagnostic IDs in call order.
    pub executed: Vec<String>,
    /// Observed lifecycle slots in call order.
    pub phases: Vec<DiagnosticPhase>,
}

impl FakeDiagnosticExecutor {
    /// Fake executor serving `source`.
    #[must_use]
    pub fn new(source: &str) -> Self {
        Self {
            source: source.to_owned(),
            negative_ids: Vec::new(),
            fail_ids: Vec::new(),
            executed: Vec::new(),
            phases: Vec::new(),
        }
    }
}

impl DiagnosticExecutor for FakeDiagnosticExecutor {
    fn source(&self) -> &str {
        &self.source
    }

    fn execute<'a>(
        &'a mut self,
        context: DiagnosticContext,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<DiagnosticOutput, crate::orchestration::FailureCategory>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            self.executed.push(context.diagnostic_id.clone());
            self.phases.push(context.phase);
            if self.fail_ids.contains(&context.diagnostic_id) {
                return Err(crate::orchestration::FailureCategory::WorkloadFailed);
            }
            let negative = self.negative_ids.contains(&context.diagnostic_id);
            let report = serde_json::json!({
                "schema_version": "0.3",
                "tool": {"name": self.source, "version": "0.1.1"},
                "execution_id": format!("fake-{}", context.diagnostic_id),
                "target": {"summary": context.target},
                "route": {"kind": "direct"},
                "status": if negative { "fail" } else { "pass" },
                "probes": [],
                "findings": [],
                "warnings": [],
            });
            Ok(DiagnosticOutput {
                raw_report: serde_json::to_vec(&report).unwrap_or_default(),
                disposition: if negative {
                    DiagnosticDisposition::Negative
                } else {
                    DiagnosticDisposition::Positive
                },
                producer: self.source.clone(),
                producer_version: "0.1.1".to_owned(),
                executable_sha256: "ab".repeat(32),
                machine_schema: "0.3".to_owned(),
                report_status: if negative {
                    "fail".to_owned()
                } else {
                    "pass".to_owned()
                },
                probe_statuses: Vec::new(),
                warnings: Vec::new(),
                skipped_reason: None,
            })
        })
    }
}
