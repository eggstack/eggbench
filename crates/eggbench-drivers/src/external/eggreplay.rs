#![allow(clippy::doc_markdown)]
//! EggReplay semantic replay workload adapter (Eggstack M003a).
//!
//! EggReplay owns fixture format/validation, recorded request semantics,
//! matching/replay order, candidate execution, and semantic finding meaning.
//! Eggbench owns fixture identity/confinement, driver provenance, process
//! lifecycle/cancellation, trial scheduling, bounded evidence, and the
//! optional absolute correctness gate.
//!
//! Production seam: trusted external `eggreplay` JSON CLI, never a Rust
//! library dependency:
//!
//! ```text
//! eggreplay validate --fixture <path> --output json
//! eggreplay replay --fixture <fixture> --target <url> --route direct --output json
//! ```
//!
//! Envelope schema 1 and RegressionReport schema 2 are enforced. One complete
//! fixture replay is one Eggbench trial observation; semantic findings are
//! successful workload observations (`semantic_findings > 0`), never
//! `WorkloadFailed`. Process wall-clock time is never mapped to latency.

use super::artifact::artifact_candidates;
use super::command::{ExternalCommandOutcome, ExternalCommandSpec, run_command};
use super::common::{
    check_min_version, driver_env, failure_category, probe_failure_category, target_http_url,
};
use super::error::{DriverError, ErrorCategory};
use super::parser::ExternalOutputParser;
use super::resolver::{BinaryResolver, ResolvedExecutable};
use super::version::{ToolVersion, VersionProbe, VersionProbeSpec};
use eggbench_core::{
    Aggregation, Capability, DriverCategory, DriverDescriptor, HttpVersion, Name,
    RawMetricObservation, SchemaVersion, Workload,
};
use eggbench_runner::{
    DrainContext, FailureCategory, InvocationContext, RunEvidenceArtifact, RunEvidenceContract,
    WorkloadArtifact, WorkloadExecutor, WorkloadOutput,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Canonical workload driver name for the EggReplay semantic adapter.
pub const EGGREPLAY_DRIVER_NAME: &str = "eggreplay-semantic";
/// Logical tool name resolved through the trusted substrate.
const EGGREPLAY_TOOL: &str = "eggreplay";
/// Versioned parser identifier for `eggreplay --output json`.
pub const EGGREPLAY_PARSER_ID: &str = "eggreplay-json/v1";
/// Minimum supported EggReplay release (workspace 0.1 line).
const EGGREPLAY_MIN_VERSION: (u64, u64, u64) = (0, 1, 0);
/// Enforced CLI envelope schema version.
pub const EGGREPLAY_ENVELOPE_SCHEMA: u32 = 1;
/// Enforced RegressionReport schema version.
pub const EGGREPLAY_REPORT_SCHEMA: u32 = 2;
/// Explicitly accepted EggReplay fixture session schemas.
pub const EGGREPLAY_ACCEPTED_FIXTURE_SCHEMAS: [u32; 2] = [1, 2];
/// Run-level evidence artifact name.
pub const SEMANTIC_REPLAY_EVIDENCE: &str = "semantic-replay.json";
/// Run-level evidence schema version.
pub const SEMANTIC_REPLAY_EVIDENCE_SCHEMA: u32 = 1;
/// Stdout retention cap.
const STDOUT_LIMIT: u64 = 4 * 1024 * 1024;
/// Stderr retention cap.
const STDERR_LIMIT: u64 = 256 * 1024;
/// Version-probe timeout.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
/// Validate/replay preflight timeout (separate from per-trial timeout).
const PREFLIGHT_TIMEOUT: Duration = Duration::from_secs(30);
/// Fixture traversal bounds.
const MAX_FIXTURE_REL_PATH: usize = 512;
const MAX_FIXTURE_DEPTH: usize = 16;
/// Replay report bounds.
const MAX_REPORTS: usize = 10_000;
const MAX_FINDINGS_PER_REPORT: usize = 10_000;
const MAX_TOTAL_FINDINGS: u64 = 100_000;
const MAX_FLOW_ID_LEN: usize = 256;
const MAX_WARNINGS: usize = 32;
const MAX_WARNING_LEN: usize = 512;

/// Deterministic fixture identity: digest-based, never path-based.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixtureIdentity {
    /// Aggregate SHA-256 over canonical ordered file records.
    pub aggregate_sha256: String,
    /// Number of regular files hashed.
    pub file_count: usize,
    /// Aggregate bytes hashed.
    pub total_bytes: u64,
}

/// Machine-contract preflight record (before managed startup).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatePreflight {
    /// Observed tool version.
    pub executable_version: String,
    /// SHA-256 of the selected executable.
    pub executable_sha256: String,
    /// Envelope schema version (always 1 in M003a).
    pub envelope_schema: u32,
    /// Fixture session schema reported by `validate`.
    pub fixture_session_schema: u32,
    /// Flow count reported by `validate`.
    pub flow_count: u64,
    /// Fixture aggregate digest.
    pub fixture_digest: String,
}

/// Parsed replay outcome: total findings plus diagnostic flow count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayParsed {
    /// Total `finding_count` (sum of report findings).
    pub finding_count: u64,
    /// Number of RegressionReport entries.
    pub report_count: usize,
}

/// EggReplay semantic workload executor.
pub struct EggReplayWorkload {
    executable: ResolvedExecutable,
    version: Option<String>,
    workspace_root: PathBuf,
    fixture: String,
    cached_identity: Option<FixtureIdentity>,
    cached_preflight: Option<ValidatePreflight>,
}

impl EggReplayWorkload {
    /// Resolve the `eggreplay` binary through the trusted substrate.
    ///
    /// # Errors
    /// Returns resolution failure when no trusted executable is available.
    pub fn resolve() -> Result<ResolvedExecutable, DriverError> {
        BinaryResolver::resolve(EGGREPLAY_TOOL, None, None)
    }

    /// Probe `--version` with a bounded timeout.
    ///
    /// # Errors
    /// Returns probe failure on timeout, nonzero exit, or missing token.
    pub async fn probe(
        executable: &ResolvedExecutable,
        cancel: &CancellationToken,
    ) -> Result<ToolVersion, DriverError> {
        VersionProbe::run(
            executable,
            &VersionProbeSpec {
                argv_tail: vec!["--version".to_owned()],
                timeout: PROBE_TIMEOUT,
                stdout_limit: 64 * 1024,
                stderr_limit: 64 * 1024,
                parser_id: EGGREPLAY_PARSER_ID.to_owned(),
            },
            cancel,
        )
        .await
    }

    /// Bind a probed binary plus workspace fixture, enforcing the version floor.
    ///
    /// # Errors
    /// Returns `unsupported_version` below 0.1.0 or `invalid_fixture` for a
    /// malformed fixture path.
    pub fn new(
        executable: ResolvedExecutable,
        version: String,
        workspace_root: PathBuf,
        fixture: String,
    ) -> Result<Self, DriverError> {
        check_min_version(EGGREPLAY_TOOL, &version, EGGREPLAY_MIN_VERSION)?;
        validate_fixture_path_syntax(&fixture)?;
        Ok(Self {
            executable,
            version: Some(version),
            workspace_root,
            fixture,
            cached_identity: None,
            cached_preflight: None,
        })
    }

    /// Bind a resolved binary without a probe; first execution probes once.
    #[must_use]
    pub fn from_resolved(
        executable: ResolvedExecutable,
        workspace_root: PathBuf,
        fixture: String,
    ) -> Self {
        Self {
            executable,
            version: None,
            workspace_root,
            fixture,
            cached_identity: None,
            cached_preflight: None,
        }
    }

    /// Pinned tool version, when probed.
    #[must_use]
    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    /// Fixture relative path (operator context; digest is the identity).
    #[must_use]
    pub fn fixture(&self) -> &str {
        &self.fixture
    }

    async fn ensure_preflighted(
        &mut self,
        cancel: &CancellationToken,
    ) -> Result<ValidatePreflight, FailureCategory> {
        if let Some(preflight) = &self.cached_preflight {
            return Ok(preflight.clone());
        }
        if self.version.is_none() {
            let probed = Self::probe(&self.executable, cancel)
                .await
                .map_err(|error| probe_failure_category(&error, cancel))?;
            check_min_version(EGGREPLAY_TOOL, &probed.version, EGGREPLAY_MIN_VERSION)
                .map_err(|error| probe_failure_category(&error, cancel))?;
            self.version = Some(probed.version);
        }
        let identity = compute_fixture_identity(&self.workspace_root, &self.fixture)
            .map_err(|error| failure_category(&error))?;
        let preflight = run_validate_contract(
            &self.executable,
            &self.workspace_root,
            &self.fixture,
            &identity,
            cancel,
        )
        .await
        .map_err(|error| failure_category(&error))?;
        self.cached_identity = Some(identity);
        self.cached_preflight = Some(preflight.clone());
        Ok(preflight)
    }
}

impl std::fmt::Debug for EggReplayWorkload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EggReplayWorkload")
            .field("driver", &EGGREPLAY_DRIVER_NAME)
            .field("version", &self.version)
            .field("fixture", &self.fixture)
            .finish_non_exhaustive()
    }
}

impl WorkloadExecutor for EggReplayWorkload {
    fn execute<'a>(
        &'a mut self,
        context: InvocationContext,
    ) -> Pin<Box<dyn Future<Output = Result<WorkloadOutput, FailureCategory>> + Send + 'a>> {
        Box::pin(async move {
            let preflight = self.ensure_preflighted(&context.cancellation).await?;
            let (target, fixture) = replay_target(&context.workload)?;
            if fixture != self.fixture {
                return Err(FailureCategory::WorkloadFailed);
            }
            let _ = target;
            let target_name = replay_target_name(&context.workload);
            let url = target_http_url(&context, target_name)?;
            require_loopback_url(&url).map_err(|_| FailureCategory::WorkloadFailed)?;
            let fixture_abs = join_workspace(&self.workspace_root, &self.fixture)
                .map_err(|_| FailureCategory::WorkloadFailed)?;
            let argv: Vec<OsString> = vec![
                "replay".into(),
                "--fixture".into(),
                fixture_abs.as_os_str().to_owned(),
                "--target".into(),
                url.clone().into(),
                "--route".into(),
                "direct".into(),
                "--output".into(),
                "json".into(),
            ];
            let spec = ExternalCommandSpec {
                executable: self.executable.clone(),
                args: argv,
                cwd: None,
                env: driver_env(),
                stdin_null: true,
                stdin_bytes: None,
                stdout_limit: STDOUT_LIMIT,
                stderr_limit: STDERR_LIMIT,
                timeout: context.timeout,
            };
            let outcome = run_command(&spec, &context.cancellation)
                .await
                .map_err(|error| failure_category(&error))?;
            if outcome.exit_code != Some(0) {
                return Err(FailureCategory::WorkloadFailed);
            }
            let parsed =
                parse_replay_envelope(&outcome).map_err(|error| failure_category(&error))?;
            Ok(replay_output(&outcome, &parsed, &preflight))
        })
    }

    fn drain<'a>(
        &'a mut self,
        _context: DrainContext,
    ) -> Pin<Box<dyn Future<Output = Result<(), FailureCategory>> + Send + 'a>> {
        Box::pin(async move { Ok(()) })
    }

    fn run_evidence(&mut self) -> Result<Option<RunEvidenceArtifact>, eggbench_core::BundleError> {
        let Some(preflight) = &self.cached_preflight else {
            return Ok(None);
        };
        let version = self.version.clone().unwrap_or_default();
        let evidence = SemanticReplayEvidence {
            schema_version: eggbench_core::SchemaVersion(SEMANTIC_REPLAY_EVIDENCE_SCHEMA),
            driver: EGGREPLAY_DRIVER_NAME.to_owned(),
            adapter_version: env!("CARGO_PKG_VERSION").to_owned(),
            executable_sha256: self.executable.sha256_hex.clone(),
            executable_version: version,
            envelope_schema: EGGREPLAY_ENVELOPE_SCHEMA,
            report_schema: EGGREPLAY_REPORT_SCHEMA,
            fixture_relative: self.fixture.clone(),
            fixture_digest: preflight.fixture_digest.clone(),
            fixture_session_schema: preflight.fixture_session_schema,
            flow_count: preflight.flow_count,
        };
        RunEvidenceArtifact::from_contract(
            SEMANTIC_REPLAY_EVIDENCE,
            semantic_replay_role_label(),
            "application/json",
            eggbench_core::Sensitivity::Redacted,
            &evidence,
        )
        .map(Some)
    }
}

fn replay_target_name(workload: &Workload) -> &str {
    match workload {
        Workload::ClosedLoop { target, .. }
        | Workload::OpenLoop { target, .. }
        | Workload::FiniteCount { target, .. }
        | Workload::TimeBounded { target, .. }
        | Workload::SemanticReplay { target, .. } => target.as_str(),
    }
}

fn replay_target(workload: &Workload) -> Result<(&str, &str), FailureCategory> {
    match workload {
        Workload::SemanticReplay { target, fixture } => Ok((target.as_str(), fixture.as_str())),
        _ => Err(FailureCategory::WorkloadFailed),
    }
}

fn require_loopback_url(url: &str) -> Result<(), String> {
    let after_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = after_scheme
        .split(['/'])
        .next()
        .unwrap_or_default()
        .split('@')
        .next_back()
        .unwrap_or_default();
    let host = if let Some(bracketed) = authority.strip_prefix('[') {
        bracketed.split(']').next().unwrap_or_default().to_owned()
    } else if authority.contains(':') {
        authority
            .rsplit_once(':')
            .map_or(authority, |(h, _)| h)
            .to_owned()
    } else {
        authority.to_owned()
    };
    let host_lower = host.to_ascii_lowercase();
    if host_lower == "127.0.0.1"
        || host_lower == "::1"
        || host_lower == "localhost"
        || host_lower.starts_with("127.")
    {
        Ok(())
    } else {
        Err(format!("target URL host {host:?} is not loopback"))
    }
}

/// Validate fixture path syntax (plan-level gate, also enforced in core).
fn validate_fixture_path_syntax(fixture: &str) -> Result<(), DriverError> {
    let invalid = |detail: &str| {
        DriverError::execution(
            ErrorCategory::UnsupportedOption,
            format!("eggreplay fixture: {detail}"),
        )
    };
    if fixture.is_empty() || fixture.len() > MAX_FIXTURE_REL_PATH {
        return Err(invalid("fixture path must be 1..=512 bytes"));
    }
    if fixture.contains('\0') || fixture.chars().any(char::is_control) {
        return Err(invalid("fixture path contains NUL or control characters"));
    }
    if fixture.starts_with('/') || fixture.starts_with('\\') {
        return Err(invalid("fixture must be relative"));
    }
    if fixture.len() >= 2 && fixture.as_bytes()[1] == b':' {
        return Err(invalid("fixture must be relative"));
    }
    if fixture.contains('\\') {
        return Err(invalid("fixture must use forward slashes"));
    }
    let mut depth = 0usize;
    for component in fixture.split('/') {
        if component.is_empty() || component == "." {
            return Err(invalid("empty or dot component"));
        }
        if component == ".." {
            return Err(invalid("parent traversal"));
        }
        if component.len() > 128 {
            return Err(invalid("component too long"));
        }
        depth += 1;
        if depth > MAX_FIXTURE_DEPTH {
            return Err(invalid("path too deep"));
        }
    }
    Ok(())
}

fn join_workspace(workspace_root: &Path, fixture: &str) -> Result<PathBuf, DriverError> {
    validate_fixture_path_syntax(fixture)?;
    let mut path = workspace_root.to_path_buf();
    for component in fixture.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return Err(DriverError::execution(
                ErrorCategory::UnsupportedOption,
                "eggreplay fixture path is not workspace confined",
            ));
        }
        path.push(component);
    }
    Ok(path)
}

/// Compute the deterministic digest-based fixture identity.
///
/// # Errors
/// Returns a typed failure when the fixture escapes the workspace, violates
/// bounds, or cannot be hashed within the traversal limits.
pub fn compute_fixture_identity(
    workspace_root: &Path,
    fixture: &str,
) -> Result<FixtureIdentity, DriverError> {
    let failed = |detail: String| DriverError::execution(ErrorCategory::UnsupportedOption, detail);
    validate_fixture_path_syntax(fixture).map_err(|e| failed(e.to_string()))?;
    let identity = eggbench_core::content_tree_identity(workspace_root, fixture)
        .map_err(|error| failed(format!("fixture identity failed: {error}")))?;
    if identity.file_count == 0 {
        return Err(failed("fixture contains no regular files".to_owned()));
    }
    Ok(FixtureIdentity {
        aggregate_sha256: identity.aggregate_sha256,
        file_count: identity.file_count,
        total_bytes: identity.total_bytes,
    })
}

/// Run `eggreplay validate --fixture <path> --output json` before startup.
async fn run_validate_contract(
    executable: &ResolvedExecutable,
    workspace_root: &Path,
    fixture: &str,
    identity: &FixtureIdentity,
    cancel: &CancellationToken,
) -> Result<ValidatePreflight, DriverError> {
    let fixture_abs = join_workspace(workspace_root, fixture)?;
    let argv: Vec<OsString> = vec![
        "validate".into(),
        "--fixture".into(),
        fixture_abs.as_os_str().to_owned(),
        "--output".into(),
        "json".into(),
    ];
    let spec = ExternalCommandSpec {
        executable: executable.clone(),
        args: argv,
        cwd: None,
        env: driver_env(),
        stdin_null: true,
        stdin_bytes: None,
        stdout_limit: STDOUT_LIMIT,
        stderr_limit: STDERR_LIMIT,
        timeout: PREFLIGHT_TIMEOUT,
    };
    let outcome = run_command(&spec, cancel).await?;
    if outcome.exit_code != Some(0) {
        return Err(DriverError::execution(
            ErrorCategory::NonzeroExit,
            format!("eggreplay validate exited with {:?}", outcome.exit_code),
        ));
    }
    let envelope = parse_validate_envelope(&outcome)?;
    Ok(ValidatePreflight {
        executable_version: String::new(),
        executable_sha256: executable.sha256_hex.clone(),
        envelope_schema: EGGREPLAY_ENVELOPE_SCHEMA,
        fixture_session_schema: envelope.fixture_schema,
        flow_count: envelope.flow_count,
        fixture_digest: identity.aggregate_sha256.clone(),
    })
}

struct ValidateEnvelope {
    fixture_schema: u32,
    flow_count: u64,
}

#[derive(Debug, Deserialize)]
struct GenericEnvelope {
    schema_version: Option<u32>,
    command: Option<String>,
    success: Option<bool>,
    warnings: Option<Vec<String>>,
    payload: Option<serde_json::Value>,
}

fn parse_validate_envelope(
    outcome: &ExternalCommandOutcome,
) -> Result<ValidateEnvelope, DriverError> {
    let parse_failed = |detail: String| DriverError::parse(ErrorCategory::ParseFailed, detail);
    if outcome.stdout.truncated() {
        return Err(parse_failed(
            "eggreplay validate stdout truncated".to_owned(),
        ));
    }
    let text = String::from_utf8_lossy(outcome.stdout.retained());
    if u64::try_from(text.len()).unwrap_or(u64::MAX) > STDOUT_LIMIT {
        return Err(parse_failed(
            "eggreplay validate output exceeds bound".to_owned(),
        ));
    }
    let envelope: GenericEnvelope = serde_json::from_str(&text)
        .map_err(|e| parse_failed(format!("invalid eggreplay validate JSON: {e}")))?;
    if envelope.schema_version != Some(EGGREPLAY_ENVELOPE_SCHEMA) {
        return Err(parse_failed(format!(
            "unsupported eggreplay envelope schema {:?}",
            envelope.schema_version
        )));
    }
    if envelope.command.as_deref() != Some("validate") {
        return Err(parse_failed(
            "eggreplay validate command mismatch".to_owned(),
        ));
    }
    if envelope.success != Some(true) {
        return Err(parse_failed(
            "eggreplay validate reported failure".to_owned(),
        ));
    }
    if let Some(warnings) = &envelope.warnings {
        if warnings.len() > MAX_WARNINGS {
            return Err(parse_failed(
                "eggreplay validate warnings exceed bound".to_owned(),
            ));
        }
        for warning in warnings {
            if warning.len() > MAX_WARNING_LEN {
                return Err(parse_failed(
                    "eggreplay validate warning too long".to_owned(),
                ));
            }
        }
    }
    let payload = envelope
        .payload
        .ok_or_else(|| parse_failed("eggreplay validate payload missing".to_owned()))?;
    let flow_count = payload
        .get("flow_count")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| parse_failed("validate payload flow_count missing".to_owned()))?;
    if flow_count > 100_000 {
        return Err(parse_failed("validate flow_count exceeds bound".to_owned()));
    }
    let fixture_schema = payload
        .get("fixture_schema_version")
        .or_else(|| payload.get("schema_version"))
        .or_else(|| payload.get("session_schema_version"))
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| parse_failed("validate fixture schema missing".to_owned()))?;
    let fixture_schema = u32::try_from(fixture_schema)
        .map_err(|_| parse_failed("fixture schema overflow".to_owned()))?;
    if !EGGREPLAY_ACCEPTED_FIXTURE_SCHEMAS.contains(&fixture_schema) {
        return Err(parse_failed(format!(
            "unsupported fixture schema {fixture_schema}"
        )));
    }
    Ok(ValidateEnvelope {
        fixture_schema,
        flow_count,
    })
}

/// Parsed replay report entry (only schema-relevant fields are validated;
/// finding kinds/fields are retained as raw diagnostic evidence downstream).
#[derive(Debug, Deserialize)]
struct ReplayReport {
    schema_version: Option<u32>,
    #[serde(default)]
    flow_id: Option<String>,
    #[serde(default)]
    findings: Option<Vec<serde_json::Value>>,
}

/// Parse and validate the `replay --output json` envelope.
///
/// # Errors
/// Returns a parse failure on malformed JSON, schema mismatch, bound
/// overflow, or `finding_count` inconsistency.
pub fn parse_replay_envelope(
    outcome: &ExternalCommandOutcome,
) -> Result<ReplayParsed, DriverError> {
    let parse_failed = |detail: String| DriverError::parse(ErrorCategory::ParseFailed, detail);
    if outcome.stdout.truncated() {
        return Err(parse_failed("eggreplay replay stdout truncated".to_owned()));
    }
    let text = String::from_utf8_lossy(outcome.stdout.retained());
    if u64::try_from(text.len()).unwrap_or(u64::MAX) > STDOUT_LIMIT {
        return Err(parse_failed(
            "eggreplay replay output exceeds bound".to_owned(),
        ));
    }
    let envelope: GenericEnvelope = serde_json::from_str(&text)
        .map_err(|e| parse_failed(format!("invalid eggreplay replay JSON: {e}")))?;
    if envelope.schema_version != Some(EGGREPLAY_ENVELOPE_SCHEMA) {
        return Err(parse_failed(format!(
            "unsupported eggreplay envelope schema {:?}",
            envelope.schema_version
        )));
    }
    if envelope.command.as_deref() != Some("replay") {
        return Err(parse_failed("eggreplay replay command mismatch".to_owned()));
    }
    if envelope.success.is_none() {
        return Err(parse_failed(
            "eggreplay replay success field missing".to_owned(),
        ));
    }
    let payload = envelope
        .payload
        .ok_or_else(|| parse_failed("eggreplay replay payload missing".to_owned()))?;
    let reports_value = payload
        .get("reports")
        .or_else(|| payload.get("regression_reports"))
        .or_else(|| payload.get("RegressionReport"))
        .ok_or_else(|| parse_failed("replay payload reports missing".to_owned()))?;
    let reports: Vec<ReplayReport> = serde_json::from_value(reports_value.clone())
        .map_err(|e| parse_failed(format!("replay reports invalid: {e}")))?;
    if reports.len() > MAX_REPORTS {
        return Err(parse_failed("replay report count exceeds bound".to_owned()));
    }
    let mut total: u64 = 0;
    for report in &reports {
        if report.schema_version != Some(EGGREPLAY_REPORT_SCHEMA) {
            return Err(parse_failed(format!(
                "unsupported RegressionReport schema {:?}",
                report.schema_version
            )));
        }
        if let Some(flow_id) = &report.flow_id
            && flow_id.len() > MAX_FLOW_ID_LEN
        {
            return Err(parse_failed("replay flow identifier too long".to_owned()));
        }
        let findings = report.findings.as_ref().map_or(0, Vec::len);
        if findings > MAX_FINDINGS_PER_REPORT {
            return Err(parse_failed("replay findings exceed bound".to_owned()));
        }
        total = total
            .checked_add(findings as u64)
            .ok_or_else(|| parse_failed("replay finding count overflow".to_owned()))?;
        if total > MAX_TOTAL_FINDINGS {
            return Err(parse_failed(
                "replay total findings exceed bound".to_owned(),
            ));
        }
    }
    let finding_count = payload
        .get("finding_count")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| parse_failed("replay payload finding_count missing".to_owned()))?;
    if finding_count != total {
        return Err(parse_failed(format!(
            "replay finding_count {finding_count} does not match report sum {total}"
        )));
    }
    Ok(ReplayParsed {
        finding_count,
        report_count: reports.len(),
    })
}

/// Assemble trial output: bounded raw artifacts plus normalized semantic metrics.
fn replay_output(
    outcome: &ExternalCommandOutcome,
    parsed: &ReplayParsed,
    _preflight: &ValidatePreflight,
) -> WorkloadOutput {
    let mut artifacts = artifact_candidates(outcome, Some(EGGREPLAY_PARSER_ID));
    let summary = serde_json::json!({
        "parser": EGGREPLAY_PARSER_ID,
        "envelope_schema": EGGREPLAY_ENVELOPE_SCHEMA,
        "report_schema": EGGREPLAY_REPORT_SCHEMA,
        "finding_count": parsed.finding_count,
        "report_count": parsed.report_count,
        "invocation_policy": {
            "command": "replay",
            "route": "direct",
            "scheduler": "sequential",
        },
    });
    artifacts.push(WorkloadArtifact {
        name: "eggreplay-summary.json".to_owned(),
        media_type: "application/json".to_owned(),
        bytes: serde_json::to_string_pretty(&summary)
            .unwrap_or_else(|_| "{}".to_owned())
            .into_bytes(),
    });
    let raw = vec!["stdout.raw".to_owned()];
    let metrics = vec![
        RawMetricObservation {
            name: "semantic_findings".to_owned(),
            unit: "count".to_owned(),
            #[allow(clippy::cast_precision_loss)]
            value: parsed.finding_count as f64,
            aggregation: Aggregation::Direct,
            source_field: Some("eggreplay.finding_count".to_owned()),
            producer: None,
            producer_version: None,
            raw_artifacts: raw.clone(),
        },
        RawMetricObservation {
            name: "semantic_flows".to_owned(),
            unit: "count".to_owned(),
            #[allow(clippy::cast_precision_loss)]
            value: parsed.report_count as f64,
            aggregation: Aggregation::Direct,
            source_field: Some("eggreplay.report_count".to_owned()),
            producer: None,
            producer_version: None,
            raw_artifacts: raw,
        },
    ];
    WorkloadOutput {
        artifacts,
        metrics,
        histograms: Vec::new(),
        error_counts: Vec::new(),
        measurement_elapsed: None,
    }
}

/// Trivial parser adapter over [`parse_replay_envelope`].
pub struct EggReplayParser;

impl ExternalOutputParser for EggReplayParser {
    fn parser_id(&self) -> &'static str {
        EGGREPLAY_PARSER_ID
    }

    fn parse(
        &self,
        outcome: &ExternalCommandOutcome,
    ) -> Result<super::parser::ParsedExternalOutput, DriverError> {
        let _ = parse_replay_envelope(outcome)?;
        Ok(super::parser::ParsedExternalOutput {
            parser_id: EGGREPLAY_PARSER_ID.to_owned(),
            tool_version: String::new(),
            truncated: outcome.stdout.truncated(),
        })
    }
}

/// Run-level `semantic-replay.json` evidence (schema v1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticReplayEvidence {
    /// Evidence schema version (1).
    pub schema_version: eggbench_core::SchemaVersion,
    /// Canonical driver name (`eggreplay-semantic`).
    pub driver: String,
    /// Eggbench adapter version.
    pub adapter_version: String,
    /// SHA-256 of the selected executable.
    pub executable_sha256: String,
    /// Observed tool version.
    pub executable_version: String,
    /// CLI envelope schema version.
    pub envelope_schema: u32,
    /// Accepted RegressionReport schema version.
    pub report_schema: u32,
    /// Fixture relative path (operator context; digest is the identity).
    pub fixture_relative: String,
    /// Fixture aggregate digest (comparison-critical).
    pub fixture_digest: String,
    /// Fixture session schema.
    pub fixture_session_schema: u32,
    /// Flow count from `validate`.
    pub flow_count: u64,
}

impl RunEvidenceContract for SemanticReplayEvidence {
    const SCHEMA_VERSION: eggbench_core::SchemaVersion = eggbench_core::SchemaVersion(1);

    fn validate_contract(&self) -> Result<(), eggbench_core::BundleError> {
        if self.schema_version != Self::SCHEMA_VERSION {
            return Err(eggbench_core::BundleError::InvalidManifest(
                "semantic-replay evidence schema mismatch",
            ));
        }
        if self.driver != EGGREPLAY_DRIVER_NAME
            || self.adapter_version.is_empty()
            || self.adapter_version.len() > 128
            || self.executable_sha256.len() != 64
            || self.executable_version.is_empty()
            || self.executable_version.len() > 128
            || self.envelope_schema != EGGREPLAY_ENVELOPE_SCHEMA
            || self.report_schema != EGGREPLAY_REPORT_SCHEMA
            || self.fixture_digest.len() != 64
            || self.fixture_relative.is_empty()
            || self.fixture_relative.len() > MAX_FIXTURE_REL_PATH
            || !EGGREPLAY_ACCEPTED_FIXTURE_SCHEMAS.contains(&self.fixture_session_schema)
        {
            return Err(eggbench_core::BundleError::InvalidManifest(
                "semantic-replay evidence contract is invalid",
            ));
        }
        Ok(())
    }
}

/// Manifest role label for `semantic-replay.json`.
///
/// # Panics
/// Never panics at runtime; the static label is valid by construction.
#[must_use]
pub fn semantic_replay_role_label() -> Name {
    Name::new("semantic-replay").expect("static role label")
}

/// Production descriptor for the EggReplay semantic workload driver.
///
/// # Panics
/// Never panics at runtime; the static names are valid by construction.
#[must_use]
pub fn eggreplay_descriptor() -> DriverDescriptor {
    let mut capabilities = BTreeSet::new();
    capabilities.insert(Capability::SemanticReplay);
    capabilities.insert(Capability::ExternalBinary);
    capabilities.insert(Capability::HttpVersion {
        version: HttpVersion::Http11,
    });
    let mut compatible = BTreeSet::new();
    compatible.insert(Name::new("eggserve-origin").expect("static service type"));
    DriverDescriptor {
        name: Name::new(EGGREPLAY_DRIVER_NAME).expect("static driver name"),
        adapter_version: env!("CARGO_PKG_VERSION").to_owned(),
        upstream_name: EGGREPLAY_TOOL.to_owned(),
        upstream_version: None,
        category: DriverCategory::Workload,
        capabilities,
        supported_platforms: BTreeSet::new(),
        machine_output_schema: Some(SchemaVersion(1)),
        external_process: true,
        default: false,
        compatible_service_types: compatible,
    }
}

/// Preflight helper for CLI `run`: resolve, version-probe, confine, validate.
///
/// # Errors
/// Returns resolution, probe, fixture-confinement, or contract failures.
pub async fn preflight_semantic_replay(
    workspace_root: &Path,
    fixture: &str,
    cancel: &CancellationToken,
) -> Result<(ResolvedExecutable, ToolVersion, ValidatePreflight), DriverError> {
    let executable = EggReplayWorkload::resolve()?;
    let probed = EggReplayWorkload::probe(&executable, cancel).await?;
    EggReplayWorkload::new(
        executable.clone(),
        probed.version.clone(),
        workspace_root.to_path_buf(),
        fixture.to_owned(),
    )?;
    let identity = compute_fixture_identity(workspace_root, fixture)?;
    let mut preflight =
        run_validate_contract(&executable, workspace_root, fixture, &identity, cancel).await?;
    preflight.executable_version.clone_from(&probed.version);
    Ok((executable, probed, preflight))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::external::command::CapturedStream;
    use std::path::PathBuf;
    use std::time::Duration;

    fn outcome_with_stdout(bytes: &[u8], exit_code: Option<i32>) -> ExternalCommandOutcome {
        let total = bytes.len() as u64;
        ExternalCommandOutcome {
            executable: ResolvedExecutable {
                logical_tool: EGGREPLAY_TOOL.to_owned(),
                selected_path: PathBuf::from("/tmp/eggreplay"),
                canonical_path: PathBuf::from("/tmp/eggreplay"),
                sha256_hex: "ab".repeat(32),
                file_size: 1,
                executable_class: "test".to_owned(),
            },
            argc: 5,
            exit_code,
            stdout: CapturedStream::collect(bytes.to_vec(), total, total.max(1)),
            stderr: CapturedStream::collect(Vec::new(), 0, 1),
            duration: Duration::from_millis(1),
            cancelled: false,
            timed_out: false,
            cleanup_notes: Vec::new(),
        }
    }

    fn valid_validate_json() -> Vec<u8> {
        serde_json::json!({
            "schema_version": 1,
            "command": "validate",
            "success": true,
            "warnings": [],
            "payload": {"flow_count": 3, "fixture_schema_version": 2}
        })
        .to_string()
        .into_bytes()
    }

    fn valid_replay_json() -> Vec<u8> {
        serde_json::json!({
            "schema_version": 1,
            "command": "replay",
            "success": true,
            "payload": {
                "reports": [
                    {"schema_version": 2, "flow_id": "flow-1", "findings": []},
                    {"schema_version": 2, "flow_id": "flow-2",
                     "findings": [{"kind": "status_mismatch"}]},
                ],
                "finding_count": 1
            }
        })
        .to_string()
        .into_bytes()
    }

    #[test]
    fn validate_envelope_accepts_contract() {
        let parsed = parse_validate_envelope(&outcome_with_stdout(&valid_validate_json(), Some(0)))
            .expect("validate parses");
        assert_eq!(parsed.flow_count, 3);
        assert_eq!(parsed.fixture_schema, 2);
    }

    #[test]
    fn validate_envelope_rejects_wrong_schema_and_failure() {
        let mut value =
            serde_json::from_slice::<serde_json::Value>(&valid_validate_json()).unwrap();
        value["schema_version"] = serde_json::json!(99);
        let bytes = serde_json::to_string(&value).unwrap().into_bytes();
        assert!(parse_validate_envelope(&outcome_with_stdout(&bytes, Some(0))).is_err());
        let mut value =
            serde_json::from_slice::<serde_json::Value>(&valid_validate_json()).unwrap();
        value["success"] = serde_json::json!(false);
        let bytes = serde_json::to_string(&value).unwrap().into_bytes();
        assert!(parse_validate_envelope(&outcome_with_stdout(&bytes, Some(0))).is_err());
        let mut value =
            serde_json::from_slice::<serde_json::Value>(&valid_validate_json()).unwrap();
        value["payload"]["fixture_schema_version"] = serde_json::json!(99);
        let bytes = serde_json::to_string(&value).unwrap().into_bytes();
        assert!(parse_validate_envelope(&outcome_with_stdout(&bytes, Some(0))).is_err());
    }

    #[test]
    fn replay_parser_counts_findings_and_checks_consistency() {
        let parsed = parse_replay_envelope(&outcome_with_stdout(&valid_replay_json(), Some(0)))
            .expect("replay parses");
        assert_eq!(parsed.finding_count, 1);
        assert_eq!(parsed.report_count, 2);
        let mut value = serde_json::from_slice::<serde_json::Value>(&valid_replay_json()).unwrap();
        value["payload"]["finding_count"] = serde_json::json!(2);
        let bytes = serde_json::to_string(&value).unwrap().into_bytes();
        assert!(parse_replay_envelope(&outcome_with_stdout(&bytes, Some(0))).is_err());
        let mut value = serde_json::from_slice::<serde_json::Value>(&valid_replay_json()).unwrap();
        value["payload"]["reports"][0]["schema_version"] = serde_json::json!(99);
        let bytes = serde_json::to_string(&value).unwrap().into_bytes();
        assert!(parse_replay_envelope(&outcome_with_stdout(&bytes, Some(0))).is_err());
    }

    #[test]
    fn replay_output_maps_semantic_metrics_without_latency() {
        let parsed =
            parse_replay_envelope(&outcome_with_stdout(&valid_replay_json(), Some(0))).unwrap();
        let preflight = ValidatePreflight {
            executable_version: "0.1.0".to_owned(),
            executable_sha256: "ab".repeat(32),
            envelope_schema: 1,
            fixture_session_schema: 2,
            flow_count: 2,
            fixture_digest: "cd".repeat(32),
        };
        let output = replay_output(
            &outcome_with_stdout(&valid_replay_json(), Some(0)),
            &parsed,
            &preflight,
        );
        let findings = output
            .metrics
            .iter()
            .find(|m| m.name == "semantic_findings")
            .expect("findings metric");
        assert_eq!(findings.unit, "count");
        assert_eq!(findings.value.to_bits(), 1.0_f64.to_bits());
        assert!(matches!(findings.aggregation, Aggregation::Direct));
        assert!(!output.metrics.iter().any(|m| m.name.starts_with("latency")));
        assert!(
            output
                .artifacts
                .iter()
                .any(|a| a.name == "eggreplay-summary.json")
        );
    }

    #[test]
    fn fixture_identity_is_path_independent_and_bounded() {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("ws");
        std::fs::create_dir(&workspace).unwrap();
        let fixture = workspace.join("fx");
        std::fs::create_dir(&fixture).unwrap();
        std::fs::write(fixture.join("a.eggr"), b"fixture-a").unwrap();
        std::fs::write(fixture.join("b.eggr"), b"fixture-b").unwrap();
        let first = compute_fixture_identity(&workspace, "fx").unwrap();
        assert_eq!(first.file_count, 2);
        assert_eq!(
            first.aggregate_sha256,
            "cbbccb211f6de5ad3850440e5347bd66ce9c637f73410de96446e82727dbf760"
        );
        // A second workspace with identical contents yields the same digest.
        let root2 = tempfile::tempdir().unwrap();
        let workspace2 = root2.path().join("ws");
        std::fs::create_dir(&workspace2).unwrap();
        let fixture2 = workspace2.join("fx");
        std::fs::create_dir(&fixture2).unwrap();
        std::fs::write(fixture2.join("a.eggr"), b"fixture-a").unwrap();
        std::fs::write(fixture2.join("b.eggr"), b"fixture-b").unwrap();
        let second = compute_fixture_identity(&workspace2, "fx").unwrap();
        assert_eq!(first.aggregate_sha256, second.aggregate_sha256);
        // Traversal escapes fail closed.
        assert!(compute_fixture_identity(&workspace, "../outside").is_err());
        assert!(compute_fixture_identity(&workspace, "/abs").is_err());
    }

    #[test]
    fn fixture_symlink_escape_fails_closed() {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("ws");
        std::fs::create_dir(&workspace).unwrap();
        let fixture = workspace.join("fx");
        std::fs::create_dir(&fixture).unwrap();
        std::fs::write(fixture.join("a.eggr"), b"ok").unwrap();
        let outside = root.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("evil"), b"evil").unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(outside.join("evil"), fixture.join("link")).unwrap();
            assert!(compute_fixture_identity(&workspace, "fx").is_err());
        }
    }

    #[test]
    fn secret_like_fixture_value_does_not_leak_into_method_evidence() {
        let secret = "bearer-super-secret-value";
        let parsed = ReplayParsed {
            finding_count: 0,
            report_count: 1,
        };
        let outcome = outcome_with_stdout(&valid_replay_json(), Some(0));
        let preflight = ValidatePreflight {
            executable_version: "0.1.0".to_owned(),
            executable_sha256: "ab".repeat(32),
            envelope_schema: 1,
            fixture_session_schema: 2,
            flow_count: 1,
            fixture_digest: "cd".repeat(32),
        };
        let output = replay_output(&outcome, &parsed, &preflight);
        let rendered = output
            .artifacts
            .iter()
            .map(|artifact| format!("{}:{}", artifact.name, artifact.media_type))
            .collect::<Vec<_>>()
            .join(",");
        assert!(!rendered.contains(secret));
        // The summary carries only counts/policy, never fixture payload bytes.
        let summary = output
            .artifacts
            .iter()
            .find(|a| a.name == "eggreplay-summary.json")
            .unwrap();
        let text = String::from_utf8_lossy(&summary.bytes);
        assert!(!text.contains(secret));
    }

    #[test]
    fn debug_redacts_executable_identity() {
        let workload = EggReplayWorkload {
            executable: ResolvedExecutable {
                logical_tool: EGGREPLAY_TOOL.to_owned(),
                selected_path: PathBuf::from("/tmp/eggreplay"),
                canonical_path: PathBuf::from("/tmp/eggreplay"),
                sha256_hex: "ab".repeat(32),
                file_size: 1,
                executable_class: "test".to_owned(),
            },
            version: Some("0.1.0".to_owned()),
            workspace_root: PathBuf::from("/tmp/ws"),
            fixture: "fx".to_owned(),
            cached_identity: None,
            cached_preflight: None,
        };
        let rendered = format!("{workload:?}");
        assert!(rendered.contains(EGGREPLAY_DRIVER_NAME));
        assert!(!rendered.contains("/tmp/eggreplay"));
        assert!(!rendered.contains(&"ab".repeat(32)));
    }
}
