//! Sibling-neutral security-correctness execution seam (Eggstack M004a).
//!
//! Correctness checks are bounded one-shot lifecycle executions that run
//! after readiness (and after pre-workload diagnostics, when requested) but
//! before warmups. They are outside every measured workload interval and
//! never enter `TrialMetrics`.
//!
//! Semantics (mandatory distinction):
//!
//! - a valid Eggsec observation whose bypass count exceeds the predeclared
//!   allowance is `CorrectnessDisposition::Fail`: execution continues into
//!   warmups and measured trials and the run may still complete;
//! - an operational/process/policy/schema failure is a
//!   [`FailureCategory`](crate::orchestration::FailureCategory) error and
//!   follows the existing phase-failure taxonomy (invalid/failed), with
//!   mandatory cleanup through the common tail.
//!
//! This module is sibling-neutral: no Eggsec types cross here. Executors
//! receive a resolved [`CorrectnessContext`] and return a
//! [`CorrectnessOutput`] carrying the sanitized typed result plus
//! provenance. The Eggsec adapter lives in `eggbench-drivers`.

use eggbench_core::RunId;
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use crate::service::RuntimeBindings;

/// Context passed to one correctness-check execution.
#[derive(Debug, Clone)]
pub struct CorrectnessContext {
    /// Bundle/run identity.
    pub run_id: RunId,
    /// Check identity from the plan.
    pub check_id: String,
    /// Correctness source (`eggsec-waf` in M004a).
    pub source: String,
    /// Declared target service name from the resolved request.
    pub target: String,
    /// Requested WAF test family (`sqli`, `xss`, `ssrf`, `cmd`, `traversal`).
    pub test_type: String,
    /// Predeclared allowance for successful bypasses.
    pub max_successful_bypasses: u32,
    /// Requested Eggsec concurrency.
    pub concurrency: u32,
    /// Requested Eggsec timeout in milliseconds.
    pub timeout_ms: u64,
    /// Startup-established runtime bindings snapshot (read-only).
    pub bindings: RuntimeBindings,
    /// Cancellation token for this check.
    pub cancellation: CancellationToken,
    /// Per-check timeout (mirrors `timeout_ms` as a `Duration`).
    pub timeout: Duration,
}

/// Typed correctness observation for one check.
///
/// `Pass`/`Fail` are valid observations: the runner continues into
/// performance trials either way. There is no `Invalid` disposition here;
/// untrustworthy observations are operational errors surfaced as
/// [`FailureCategory`](crate::orchestration::FailureCategory).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorrectnessDisposition {
    /// Observed bypasses within the predeclared allowance.
    Pass,
    /// Observed bypasses exceeded the predeclared allowance.
    Fail,
}

/// Bounded correctness output for one check.
#[derive(Debug, Clone)]
pub struct CorrectnessOutput {
    /// Typed observation disposition.
    pub disposition: CorrectnessDisposition,
    /// Sanitized typed result bytes (`SecurityCheckResultV1` JSON).
    pub sanitized_result: Vec<u8>,
    /// Producer name (`eggsec`).
    pub producer: String,
    /// Producer version string.
    pub producer_version: String,
    /// SHA-256 of the selected executable.
    pub executable_sha256: String,
    /// SHA-256 of the generated strict scope manifest.
    pub scope_sha256: String,
    /// Number of evaluated Eggsec cases.
    pub evaluated_cases: u32,
    /// Observed successful bypasses.
    pub successful_bypasses: u32,
}

/// Object-safe asynchronous correctness executor.
pub trait CorrectnessExecutor: Send {
    /// Canonical source label (e.g. `eggsec-waf`).
    fn source(&self) -> &str;

    /// Execute one correctness check outside measured timing.
    fn execute<'a>(
        &'a mut self,
        context: CorrectnessContext,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<CorrectnessOutput, crate::orchestration::FailureCategory>>
                + Send
                + 'a,
        >,
    >;
}

/// Explicit registry of correctness executors keyed by source.
#[derive(Default)]
pub struct CorrectnessRegistry {
    executors: BTreeMap<String, Arc<tokio::sync::Mutex<Box<dyn CorrectnessExecutor>>>>,
}

impl CorrectnessRegistry {
    /// Empty registry (no correctness checks execute; plans requesting
    /// them fail closed at orchestration preflight).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an executor for its source label.
    pub fn register(&mut self, executor: Box<dyn CorrectnessExecutor>) {
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
    ) -> Option<Arc<tokio::sync::Mutex<Box<dyn CorrectnessExecutor>>>> {
        self.executors.get(source).cloned()
    }

    /// True when no executor is registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.executors.is_empty()
    }
}

impl std::fmt::Debug for CorrectnessRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CorrectnessRegistry")
            .field(
                "sources",
                &self.executors.keys().cloned().collect::<Vec<_>>(),
            )
            .finish()
    }
}

/// Lifecycle placement label recorded in `security-checks.json`.
#[must_use]
pub const fn correctness_timing_label() -> &'static str {
    "correctness after readiness, outside measured intervals (not a benchmark metric)"
}

/// Manifest role label for `security-checks.json` and per-check artifacts.
///
/// # Panics
///
/// Never panics at runtime; the static label is valid by construction.
#[must_use]
pub fn security_role_label() -> eggbench_core::Name {
    eggbench_core::Name::new("security").expect("static role label")
}

/// Audited Eggsec operation label recorded in `security-checks.json`.
#[must_use]
pub const fn security_operation_label() -> &'static str {
    "waf --json --bypass"
}

/// One staged correctness execution record for the run-level index.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorrectnessExecutionRecord {
    /// Check identity from the plan.
    pub id: String,
    /// Correctness source (`eggsec-waf`).
    pub source: String,
    /// Declared target binding.
    pub target: String,
    /// Requested WAF test family.
    pub test_type: String,
    /// Typed observation disposition (`pass` / `fail`).
    pub disposition: String,
    /// Evaluated Eggsec case count.
    pub evaluated_cases: u32,
    /// Observed successful bypasses.
    pub successful_bypasses: u32,
    /// Predeclared allowance.
    pub allowed_successful_bypasses: u32,
    /// Bundle-relative artifact path (`security/<id>.json`).
    pub artifact: String,
    /// SHA-256 of the staged per-check artifact bytes.
    pub artifact_sha256: String,
    /// Producer version.
    pub producer_version: String,
    /// Producer executable SHA-256.
    pub executable_sha256: String,
    /// Generated scope manifest SHA-256.
    pub scope_sha256: String,
}

/// Deterministic fake correctness executor for runner qualification.
///
/// Returns a canned [`CorrectnessOutput`] per check ID: IDs listed in
/// `fail_ids` observe a bypass above the allowance (`Fail`), IDs in
/// `invalid_ids` return an operational failure, all others pass.
#[derive(Debug, Clone, Default)]
pub struct FakeCorrectnessExecutor {
    /// Source label this fake serves.
    pub source: String,
    /// Check IDs that observe a failing bypass count.
    pub fail_ids: Vec<String>,
    /// Check IDs that return an operational failure.
    pub invalid_ids: Vec<String>,
    /// Executed check IDs in call order.
    pub executed: Vec<String>,
}

impl FakeCorrectnessExecutor {
    /// Fake executor serving `source`.
    #[must_use]
    pub fn new(source: &str) -> Self {
        Self {
            source: source.to_owned(),
            fail_ids: Vec::new(),
            invalid_ids: Vec::new(),
            executed: Vec::new(),
        }
    }
}

impl CorrectnessExecutor for FakeCorrectnessExecutor {
    fn source(&self) -> &str {
        &self.source
    }

    fn execute<'a>(
        &'a mut self,
        context: CorrectnessContext,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<CorrectnessOutput, crate::orchestration::FailureCategory>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            self.executed.push(context.check_id.clone());
            if self.invalid_ids.contains(&context.check_id) {
                return Err(crate::orchestration::FailureCategory::CorrectnessFailed);
            }
            let failing = self.fail_ids.contains(&context.check_id);
            let (successful, disposition) = if failing {
                (
                    context.max_successful_bypasses.saturating_add(1).max(1),
                    CorrectnessDisposition::Fail,
                )
            } else {
                (0, CorrectnessDisposition::Pass)
            };
            let evaluated = successful.max(1);
            let result = serde_json::json!({
                "schema_version": 1,
                "id": context.check_id,
                "source": context.source,
                "target": context.target,
                "test_type": context.test_type,
                "disposition": if failing { "fail" } else { "pass" },
                "evaluated_cases": evaluated,
                "successful_bypasses": successful,
                "allowed_successful_bypasses": context.max_successful_bypasses,
                "producer_version": "0.1.0",
                "producer_sha256": "ab".repeat(32),
                "scope_sha256": "cd".repeat(32),
                "sanitized_cases": (0..evaluated).map(|i| serde_json::json!({
                    "technique": format!("fake-technique-{i}"),
                    "severity_label": "low",
                    "response_status": 403,
                    "bypass_successful": failing && i < successful,
                    "payload_sha256": "ef".repeat(32),
                })).collect::<Vec<_>>(),
            });
            Ok(CorrectnessOutput {
                disposition,
                sanitized_result: serde_json::to_vec(&result).unwrap_or_default(),
                producer: "eggsec".to_owned(),
                producer_version: "0.1.0".to_owned(),
                executable_sha256: "ab".repeat(32),
                scope_sha256: "cd".repeat(32),
                evaluated_cases: evaluated,
                successful_bypasses: successful,
            })
        })
    }
}
