//! Strict-scope Eggsec load adapter for one reviewed HTTP corpus case.
//!
//! Eggsec owns request execution and report semantics. Eggbench builds a
//! bounded command from one immutable corpus case, enforces local scope,
//! validates the JSON report, and records Eggsec's Eggfetch-backed transport
//! as non-independent corroboration.

use super::artifact::artifact_candidates;
use super::command::{ExternalCommandOutcome, ExternalCommandSpec, run_command};
use super::common::{check_min_version, driver_env, metric_u64_as_f64, target_http_url};
use super::eggsec::{confine_target_url, generate_scope_manifest};
use super::error::{DriverError, ErrorCategory};
use super::resolver::{BinaryResolver, ResolvedExecutable};
use super::version::ToolVersion;
use super::version::{VersionProbe, VersionProbeSpec};
use eggbench_core::{
    Aggregation, Capability, DriverCategory, DriverDescriptor, HttpCaseBodyV1,
    HttpObservableExpectationV1, Name, RawMetricObservation, Workload, load_http_security_corpus,
};
use eggbench_runner::{
    DrainContext, FailureCategory, InvocationContext, WorkloadArtifact, WorkloadExecutor,
    WorkloadOutput,
};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Registered workload driver name.
pub const EGGSEC_LOAD_DRIVER_NAME: &str = "eggsec-load";
/// Versioned Eggsec load JSON parser contract.
pub const EGGSEC_LOAD_PARSER_ID: &str = "eggsec-load-json/v1";
const STDOUT_LIMIT: u64 = 4 * 1024 * 1024;
const STDERR_LIMIT: u64 = 256 * 1024;
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const PREFLIGHT_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_REQUEST_BODY: usize = 16 * 1024;
const MAX_REQUESTS: u64 = 1_000_000;
const MAX_CONCURRENCY: usize = 256;
const EGGSEC_TOOL: &str = "eggsec";
const EGGSEC_MIN_VERSION: (u64, u64, u64) = (0, 1, 0);
static SCOPE_NONCE: AtomicU64 = AtomicU64::new(0);

/// Resolved Eggsec workload executor.
pub struct EggsecLoadWorkload {
    executable: ResolvedExecutable,
    version: Option<String>,
}

impl EggsecLoadWorkload {
    /// Resolve Eggsec through the trusted executable substrate.
    ///
    /// # Errors
    /// Returns a resolution error when no trusted `eggsec` executable exists.
    pub fn resolve() -> Result<ResolvedExecutable, DriverError> {
        BinaryResolver::resolve(EGGSEC_TOOL, None, None)
    }

    /// Probe and validate the Eggsec version before managed service startup.
    ///
    /// # Errors
    /// Returns a probe failure or `unsupported_version` below 0.1.0.
    pub async fn probe(
        executable: &ResolvedExecutable,
        cancel: &CancellationToken,
    ) -> Result<ToolVersion, DriverError> {
        let probe = VersionProbe::run(
            executable,
            &VersionProbeSpec {
                argv_tail: vec!["--version".to_owned()],
                timeout: PROBE_TIMEOUT,
                stdout_limit: 64 * 1024,
                stderr_limit: 64 * 1024,
                parser_id: EGGSEC_LOAD_PARSER_ID.to_owned(),
            },
            cancel,
        )
        .await?;
        check_min_version(EGGSEC_TOOL, &probe.version, EGGSEC_MIN_VERSION)?;
        Ok(probe)
    }

    /// Construct an executor that probes the selected executable on first use.
    #[must_use]
    pub fn from_resolved(executable: ResolvedExecutable) -> Self {
        Self {
            executable,
            version: None,
        }
    }

    async fn ensure_probed(&mut self, cancel: &CancellationToken) -> Result<(), FailureCategory> {
        if self.version.is_some() {
            return Ok(());
        }
        let probe = Self::probe(&self.executable, cancel)
            .await
            .map_err(|error| failure(&error))?;
        self.version = Some(probe.version);
        Ok(())
    }
}

impl std::fmt::Debug for EggsecLoadWorkload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EggsecLoadWorkload")
            .field("version", &self.version)
            .finish_non_exhaustive()
    }
}

impl WorkloadExecutor for EggsecLoadWorkload {
    #[allow(clippy::too_many_lines)] // Preflight, scope, and workload share one fail-closed lifecycle.
    fn execute<'a>(
        &'a mut self,
        context: InvocationContext,
    ) -> Pin<Box<dyn Future<Output = Result<WorkloadOutput, FailureCategory>> + Send + 'a>> {
        Box::pin(async move {
            self.ensure_probed(&context.cancellation).await?;
            let version = self
                .version
                .clone()
                .ok_or(FailureCategory::WorkloadFailed)?;
            let Workload::HttpCorpus {
                target,
                corpus_ref,
                corpus_sha256,
                schedule,
                concurrency,
                ..
            } = &context.workload
            else {
                return Err(FailureCategory::WorkloadFailed);
            };
            if schedule.len() != 1 {
                return Err(FailureCategory::WorkloadFailed);
            }
            let requests = u64::from(schedule[0].count.get());
            let concurrency =
                usize::try_from(concurrency.get()).map_err(|_| FailureCategory::WorkloadFailed)?;
            if requests > MAX_REQUESTS || concurrency > MAX_CONCURRENCY {
                return Err(FailureCategory::WorkloadFailed);
            }
            let workspace = std::env::current_dir().map_err(|_| FailureCategory::WorkloadFailed)?;
            let (corpus, corpus_path) =
                load_http_security_corpus(&workspace, corpus_ref, corpus_sha256)
                    .map_err(|_| FailureCategory::WorkloadFailed)?;
            let case = corpus
                .cases
                .iter()
                .find(|case| case.id == schedule[0].case_id)
                .ok_or(FailureCategory::WorkloadFailed)?;
            let url = target_http_url(&context, target.as_str())?;
            let confined = confine_target_url(&url).map_err(|_| FailureCategory::WorkloadFailed)?;
            let (scope_bytes, scope_sha) = generate_scope_manifest(&confined.host)
                .map_err(|_| FailureCategory::WorkloadFailed)?;
            let scope_file = create_scope_file(&scope_sha, &scope_bytes)
                .map_err(|_| FailureCategory::WorkloadFailed)?;
            let scope_path = &scope_file.path;
            let preflight = run_load_preflight(
                &self.executable,
                scope_path,
                &url,
                &confined,
                &context.cancellation,
            )
            .await;
            if preflight.is_err() {
                return Err(FailureCategory::WorkloadFailed);
            }
            let (body, body_file) = load_case_body(&case.request.body, &corpus_path)
                .map_err(|_| FailureCategory::WorkloadFailed)?;
            let request_url = append_path(&url, &case.request.path_and_query)
                .ok_or(FailureCategory::WorkloadFailed)?;
            let args = load_argv(
                scope_path,
                &request_url,
                &case.request.method,
                &case.request.headers,
                body.as_deref(),
                requests,
                concurrency,
                context.timeout,
            )
            .map_err(|_| FailureCategory::WorkloadFailed)?;
            let outcome = run_command(
                &ExternalCommandSpec {
                    executable: self.executable.clone(),
                    args,
                    cwd: None,
                    env: driver_env(),
                    stdin_null: true,
                    stdin_bytes: None,
                    stdout_limit: STDOUT_LIMIT,
                    stderr_limit: STDERR_LIMIT,
                    timeout: context.timeout,
                },
                &context.cancellation,
            )
            .await
            .map_err(|error| failure(&error))?;
            let parsed = parse_load_report(&outcome, &request_url)?;
            let mut output = load_output(
                &outcome,
                parsed,
                &case.expectation,
                &version,
                &scope_sha,
                &case.id,
            );
            if body_file.is_some() {
                // Retain identity only; request bytes never enter evidence.
                output.artifacts.push(WorkloadArtifact {
                    name: "eggsec-load-request.json".to_owned(),
                    media_type: "application/json".to_owned(),
                    bytes: serde_json::to_vec(&serde_json::json!({
                        "case_id": case.id,
                        "body_file": body_file,
                        "body_bytes": body.as_ref().map_or(0, Vec::len),
                        "request_body_retained": false
                    }))
                    .unwrap_or_default(),
                });
            }
            Ok(output)
        })
    }

    fn drain<'a>(
        &'a mut self,
        _context: DrainContext,
    ) -> Pin<Box<dyn Future<Output = Result<(), FailureCategory>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}

struct ScopeFile {
    path: std::path::PathBuf,
    directory: std::path::PathBuf,
}

impl Drop for ScopeFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_dir(&self.directory);
    }
}

fn create_scope_file(digest: &str, bytes: &[u8]) -> Result<ScopeFile, std::io::Error> {
    use std::io::Write as _;
    let root = std::env::temp_dir();
    loop {
        let nonce = SCOPE_NONCE.fetch_add(1, Ordering::Relaxed);
        let directory = root.join(format!("eggbench-load-{}-{nonce}", std::process::id()));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt as _;
            builder.mode(0o700);
        }
        match builder.create(&directory) {
            Ok(()) => {
                let path = directory.join(format!("{digest}.toml"));
                let mut options = std::fs::OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt as _;
                    options.mode(0o600);
                }
                let scope_file = ScopeFile {
                    path: path.clone(),
                    directory,
                };
                let mut file = options.open(&path)?;
                file.write_all(bytes)?;
                return Ok(scope_file);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EggsecLoadReport {
    target_url: String,
    total_requests: u64,
    successful_requests: u64,
    failed_requests: u64,
    total_duration_ms: u64,
    requests_per_second: f64,
    latency_min_ms: f64,
    latency_max_ms: f64,
    latency_mean_ms: f64,
    latency_p50_ms: f64,
    latency_p90_ms: f64,
    latency_p95_ms: f64,
    latency_p99_ms: f64,
    status_codes: BTreeMap<u16, u64>,
    errors: Vec<String>,
    #[serde(default)]
    error_kinds: BTreeMap<String, u64>,
}

fn parse_load_report(
    outcome: &ExternalCommandOutcome,
    expected_url: &str,
) -> Result<EggsecLoadReport, FailureCategory> {
    if outcome.stdout.truncated() {
        return Err(FailureCategory::WorkloadFailed);
    }
    let text = std::str::from_utf8(outcome.stdout.retained())
        .map_err(|_| FailureCategory::WorkloadFailed)?;
    let mut report = serde_json::from_str::<EggsecLoadReport>(text).ok();
    if report.is_none() {
        for (index, line) in text.match_indices('{').rev() {
            if index > 0
                && text[..index]
                    .rsplit_once('\n')
                    .is_some_and(|(_, prefix)| prefix.trim().is_empty())
                && line == "{"
            {
                report = serde_json::from_str(&text[index..]).ok();
                if report.is_some() {
                    break;
                }
            }
        }
    }
    let report = report.ok_or(FailureCategory::WorkloadFailed)?;
    if report.target_url != expected_url
        || report.total_requests == 0
        || report.total_requests > MAX_REQUESTS
        || report.failed_requests > report.total_requests
        || report
            .successful_requests
            .saturating_add(report.failed_requests)
            != report.total_requests
        || !report.requests_per_second.is_finite()
        || report.requests_per_second < 0.0
        || [
            report.latency_min_ms,
            report.latency_max_ms,
            report.latency_mean_ms,
            report.latency_p50_ms,
            report.latency_p90_ms,
            report.latency_p95_ms,
            report.latency_p99_ms,
        ]
        .iter()
        .any(|value| !value.is_finite() || *value < 0.0)
        || report.errors.len() > 1_000
        || report.status_codes.values().copied().sum::<u64>() > report.total_requests
        || report.error_kinds.values().copied().sum::<u64>() > report.total_requests
    {
        return Err(FailureCategory::WorkloadFailed);
    }
    Ok(report)
}

fn load_output(
    outcome: &ExternalCommandOutcome,
    report: EggsecLoadReport,
    expected: &HttpObservableExpectationV1,
    version: &str,
    scope_sha: &str,
    case_id: &str,
) -> WorkloadOutput {
    let transport_errors = report.error_kinds.values().copied().sum::<u64>();
    let responses = report.status_codes.values().copied().sum::<u64>();
    let mismatches = report
        .status_codes
        .iter()
        .filter(|(status, _)| !expected.matches(**status))
        .map(|(_, count)| *count)
        .sum::<u64>();
    let raw = vec!["stdout.raw".to_owned()];
    let metric = |name: &str, unit: &str, value: f64, aggregation: Aggregation, source: &str| {
        RawMetricObservation {
            name: name.to_owned(),
            unit: unit.to_owned(),
            value,
            aggregation,
            source_field: Some(source.to_owned()),
            producer: Some(EGGSEC_LOAD_DRIVER_NAME.to_owned()),
            producer_version: Some(version.to_owned()),
            raw_artifacts: raw.clone(),
        }
    };
    let mut artifacts = artifact_candidates(outcome, Some(EGGSEC_LOAD_PARSER_ID));
    artifacts.push(WorkloadArtifact {
        name: "eggsec-load-status.json".to_owned(),
        media_type: "application/json".to_owned(),
        bytes: serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "status_codes": report.status_codes,
            "expected_outcome_mismatches": mismatches,
            "expected_statuses": expected_statuses(expected),
            "completed_responses": responses,
            "transport_errors": transport_errors,
            "transport_independent": false,
            "transport": "eggfetch"
        }))
        .unwrap_or_default(),
    });
    artifacts.push(WorkloadArtifact {
        name: "eggsec-load-method.json".to_owned(),
        media_type: "application/json".to_owned(),
        bytes: serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "driver": EGGSEC_LOAD_DRIVER_NAME,
            "eggsec_version": version,
            "executable_sha256": outcome.executable.sha256_hex,
            "scope_sha256": scope_sha,
            "case_id": case_id,
            "planned_requests": report.total_requests,
            "method": "Eggsec load via Eggfetch",
            "transport_independent": false
        }))
        .unwrap_or_default(),
    });
    let total = metric_u64_as_f64(report.total_requests);
    let mut metrics = vec![
        metric(
            "throughput",
            "rps",
            report.requests_per_second,
            Aggregation::Rate,
            "requests_per_second",
        ),
        metric(
            "latency_p95",
            "ms",
            report.latency_p95_ms,
            Aggregation::Percentile {
                basis_points: 9_500,
            },
            "latency_p95_ms",
        ),
        metric(
            "error_rate",
            "ratio",
            metric_u64_as_f64(transport_errors) / total,
            Aggregation::Ratio,
            "error_kinds",
        ),
    ];
    if responses > 0 {
        metrics.push(metric(
            "expected_outcome_mismatch_rate",
            "ratio",
            metric_u64_as_f64(mismatches) / metric_u64_as_f64(responses),
            Aggregation::Ratio,
            "status_codes",
        ));
    }
    WorkloadOutput {
        artifacts,
        metrics,
        histograms: Vec::new(),
        error_counts: report
            .error_kinds
            .into_iter()
            .map(|(kind, count)| (format!("eggsec:{kind}"), count))
            .collect(),
        measurement_elapsed: Some(Duration::from_millis(report.total_duration_ms)),
    }
}

fn expected_statuses(expected: &HttpObservableExpectationV1) -> Vec<u16> {
    match expected {
        HttpObservableExpectationV1::Exact { status_exact } => vec![*status_exact],
        HttpObservableExpectationV1::AnyOf { status_any_of } => status_any_of.clone(),
    }
}

#[allow(clippy::too_many_arguments)] // Mirrors the explicit reviewed request contract.
fn load_argv(
    scope: &std::path::Path,
    url: &str,
    method: &str,
    headers: &[(String, String)],
    body: Option<&[u8]>,
    requests: u64,
    concurrency: usize,
    timeout: Duration,
) -> Result<Vec<OsString>, DriverError> {
    if method.is_empty()
        || method.len() > 16
        || !method
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte == b'-')
        || requests == 0
        || requests > MAX_REQUESTS
        || concurrency == 0
        || concurrency > MAX_CONCURRENCY
        || headers.len() > 32
    {
        return Err(DriverError::execution(
            ErrorCategory::UnsupportedOption,
            "eggsec-load request shape rejected",
        ));
    }
    let mut args = vec![
        "--scope".into(),
        scope.as_os_str().to_owned(),
        "--strict-scope".into(),
        "--json".into(),
        "load".into(),
        url.into(),
        "--json".into(),
        "--quiet".into(),
        "--requests".into(),
        requests.to_string().into(),
        "--concurrency".into(),
        concurrency.to_string().into(),
        "--method".into(),
        method.into(),
        "--timeout".into(),
        timeout.as_secs().max(1).to_string().into(),
    ];
    for (key, value) in headers {
        let lower = key.to_ascii_lowercase();
        if key.is_empty()
            || key.len() > 128
            || value.len() > 2048
            || key
                .bytes()
                .any(|byte| !(byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)))
            || value.chars().any(char::is_control)
            || [
                "host",
                "content-length",
                "connection",
                "authorization",
                "cookie",
                "proxy-authorization",
            ]
            .contains(&lower.as_str())
        {
            return Err(DriverError::execution(
                ErrorCategory::UnsupportedOption,
                "eggsec-load header rejected",
            ));
        }
        args.extend(["--header".into(), format!("{key}:{value}").into()]);
    }
    if let Some(body) = body {
        if body.len() > MAX_REQUEST_BODY {
            return Err(DriverError::execution(
                ErrorCategory::UnsupportedOption,
                "eggsec-load body exceeds bound",
            ));
        }
        let body = std::str::from_utf8(body).map_err(|_| {
            DriverError::execution(
                ErrorCategory::UnsupportedOption,
                "eggsec-load body must be UTF-8",
            )
        })?;
        args.extend(["--body".into(), body.into()]);
    }
    Ok(args)
}

fn load_case_body(
    body: &HttpCaseBodyV1,
    corpus_path: &std::path::Path,
) -> Result<(Option<Vec<u8>>, Option<String>), DriverError> {
    match body {
        HttpCaseBodyV1::None => Ok((None, None)),
        HttpCaseBodyV1::InlineUtf8(body) => Ok((Some(body.as_bytes().to_vec()), None)),
        HttpCaseBodyV1::File(relative) => {
            let path = corpus_path
                .parent()
                .ok_or_else(|| {
                    DriverError::execution(
                        ErrorCategory::UnsupportedOption,
                        "corpus parent missing",
                    )
                })?
                .join(relative);
            let body_root = std::fs::canonicalize(corpus_path.parent().ok_or_else(|| {
                DriverError::execution(ErrorCategory::UnsupportedOption, "corpus parent missing")
            })?)
            .map_err(|_| {
                DriverError::execution(ErrorCategory::UnsupportedOption, "corpus parent unreadable")
            })?;
            let canonical = std::fs::canonicalize(path).map_err(|_| {
                DriverError::execution(ErrorCategory::UnsupportedOption, "corpus body missing")
            })?;
            if !canonical.starts_with(&body_root) {
                return Err(DriverError::execution(
                    ErrorCategory::UnsupportedOption,
                    "corpus body escaped its confined directory",
                ));
            }
            let metadata = std::fs::metadata(&canonical).map_err(|_| {
                DriverError::execution(ErrorCategory::UnsupportedOption, "corpus body unreadable")
            })?;
            if !metadata.is_file()
                || metadata.len() > u64::try_from(MAX_REQUEST_BODY).unwrap_or(u64::MAX)
            {
                return Err(DriverError::execution(
                    ErrorCategory::UnsupportedOption,
                    "eggsec-load body file exceeds bound or is not regular",
                ));
            }
            let bytes = std::fs::read(canonical).map_err(|_| {
                DriverError::execution(ErrorCategory::UnsupportedOption, "corpus body unreadable")
            })?;
            if bytes.len() > MAX_REQUEST_BODY {
                return Err(DriverError::execution(
                    ErrorCategory::UnsupportedOption,
                    "eggsec-load body exceeds bound",
                ));
            }
            std::str::from_utf8(&bytes).map_err(|_| {
                DriverError::execution(
                    ErrorCategory::UnsupportedOption,
                    "eggsec-load body must be UTF-8",
                )
            })?;
            Ok((Some(bytes), Some(relative.clone())))
        }
    }
}

fn append_path(base: &str, path: &str) -> Option<String> {
    if !path.starts_with('/')
        || path.starts_with("//")
        || path.chars().any(char::is_control)
        || path.contains('#')
    {
        return None;
    }
    let (origin, rest) = base.split_once("://")?;
    let authority = rest.split('/').next()?;
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    Some(format!("{origin}://{authority}{path}"))
}

#[allow(clippy::too_many_lines)] // One guarded no-network policy-contract validation.
async fn run_load_preflight(
    executable: &ResolvedExecutable,
    scope: &std::path::Path,
    url: &str,
    expected_target: &super::eggsec::ConfinedTarget,
    cancel: &CancellationToken,
) -> Result<(), DriverError> {
    let args = vec![
        "--scope".into(),
        scope.as_os_str().to_owned(),
        "--strict-scope".into(),
        "--json".into(),
        "preflight".into(),
        "load-test".into(),
        "--target".into(),
        url.into(),
        "--profile".into(),
        "guarded".into(),
    ];
    let outcome = run_command(
        &ExternalCommandSpec {
            executable: executable.clone(),
            args,
            cwd: None,
            env: driver_env(),
            stdin_null: true,
            stdin_bytes: None,
            stdout_limit: 256 * 1024,
            stderr_limit: STDERR_LIMIT,
            timeout: PREFLIGHT_TIMEOUT,
        },
        cancel,
    )
    .await?;
    if outcome.stdout.truncated() {
        return Err(DriverError::parse(
            ErrorCategory::ParseFailed,
            "Eggsec load preflight truncated",
        ));
    }
    let text = std::str::from_utf8(outcome.stdout.retained()).map_err(|_| {
        DriverError::parse(
            ErrorCategory::ParseFailed,
            "Eggsec load preflight invalid UTF-8",
        )
    })?;
    let value: serde_json::Value = serde_json::from_str(text).map_err(|_| {
        DriverError::parse(
            ErrorCategory::ParseFailed,
            "Eggsec load preflight invalid JSON",
        )
    })?;
    let operation = value
        .pointer("/descriptor/operation")
        .or_else(|| value.pointer("/decision/operation"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let allowed = value
        .pointer("/decision/allowed")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let outcome_kind = value
        .get("outcome_kind")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let scope_source = value
        .get("scope_source")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let confirmations = value
        .get("required_confirmation_classes")
        .and_then(serde_json::Value::as_array)
        .map_or(0, Vec::len);
    let override_honored = value
        .get("manual_override_honored")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let reported_targets = [
        value
            .pointer("/descriptor/target")
            .and_then(serde_json::Value::as_str),
        value
            .pointer("/decision/target_original")
            .and_then(serde_json::Value::as_str),
        value
            .pointer("/decision/target_normalized")
            .and_then(serde_json::Value::as_str),
    ];
    let target_matches = reported_targets.iter().flatten().any(|reported| {
        confine_target_url(reported).is_ok_and(|target| {
            target.host == expected_target.host && target.port == expected_target.port
        })
    });
    if operation != "load-test"
        || !allowed
        || outcome_kind != "allow"
        || scope_source != "cli-scope-file"
        || confirmations != 0
        || override_honored
        || !target_matches
    {
        return Err(DriverError::execution(
            ErrorCategory::NonzeroExit,
            "Eggsec strict load preflight denied or contract changed",
        ));
    }
    Ok(())
}

fn failure(error: &DriverError) -> FailureCategory {
    match error.category() {
        ErrorCategory::Cancelled => FailureCategory::Cancelled,
        ErrorCategory::TimedOut => FailureCategory::TimedOut,
        _ => FailureCategory::WorkloadFailed,
    }
}

/// Eggsec load workload descriptor.
///
/// # Panics
/// Never panics at runtime; the static driver name is valid by construction.
#[must_use]
pub fn eggsec_load_descriptor() -> DriverDescriptor {
    let mut capabilities = BTreeSet::new();
    capabilities.insert(Capability::LoadMode {
        mode: eggbench_core::LoadMode::ClosedLoop,
    });
    capabilities.insert(Capability::ExternalBinary);
    capabilities.insert(Capability::HttpCorpus);
    DriverDescriptor {
        name: Name::new(EGGSEC_LOAD_DRIVER_NAME).expect("static driver name"),
        adapter_version: env!("CARGO_PKG_VERSION").to_owned(),
        upstream_name: "eggsec".to_owned(),
        upstream_version: None,
        category: DriverCategory::Workload,
        capabilities,
        supported_platforms: BTreeSet::new(),
        machine_output_schema: Some(eggbench_core::SchemaVersion(1)),
        external_process: true,
        default: false,
        compatible_service_types: BTreeSet::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::external::command::CapturedStream;
    use crate::external::resolver::ResolvedExecutable;

    fn outcome(stdout: &str) -> ExternalCommandOutcome {
        let path = std::path::PathBuf::from("/tmp/eggsec");
        ExternalCommandOutcome {
            executable: ResolvedExecutable {
                logical_tool: "eggsec".to_owned(),
                selected_path: path.clone(),
                canonical_path: path,
                sha256_hex: "a".repeat(64),
                file_size: 1,
                executable_class: "test".to_owned(),
            },
            argc: 0,
            exit_code: Some(0),
            stdout: CapturedStream::from_parts(
                stdout.as_bytes().to_vec(),
                u64::try_from(stdout.len()).unwrap_or(u64::MAX),
            ),
            stderr: CapturedStream::from_parts(Vec::new(), 0),
            duration: Duration::ZERO,
            cancelled: false,
            timed_out: false,
            cleanup_notes: Vec::new(),
        }
    }

    #[test]
    fn parses_bounded_report_after_json_log_lines() {
        let report = r#"{"target_url":"http://127.0.0.1/","total_requests":4,"successful_requests":4,"failed_requests":0,"total_duration_ms":10,"requests_per_second":400.0,"latency_min_ms":1.0,"latency_max_ms":2.0,"latency_mean_ms":1.5,"latency_p50_ms":1.0,"latency_p90_ms":2.0,"latency_p95_ms":2.0,"latency_p99_ms":2.0,"status_codes":{"403":4},"errors":[],"error_kinds":{}}"#;
        let stdout = format!(
            "{{\"level\":\"INFO\"}}\n{{\n  \"total_requests\": 4,\n  \"failed_requests\": 0,\n  \"total_duration_ms\": 10,\n  \"requests_per_second\": 400.0,\n  \"latency_p95_ms\": 2.0,\n  \"status_codes\": {{\"403\": 4}},\n  \"error_kinds\": {{}}\n}}\n{report}"
        );
        let parsed = parse_load_report(&outcome(&stdout), "http://127.0.0.1/")
            .expect("machine report parses");
        assert_eq!(parsed.total_requests, 4);
        assert_eq!(parsed.status_codes.get(&403), Some(&4));
    }

    #[test]
    fn strict_argv_binds_body_and_rejects_sensitive_headers() {
        let path = std::path::Path::new("/tmp/scope.toml");
        let args = load_argv(
            path,
            "http://127.0.0.1:8080/submit",
            "POST",
            &[("content-type".to_owned(), "application/json".to_owned())],
            Some(br#"{"ok":true}"#),
            100,
            8,
            Duration::from_secs(10),
        )
        .expect("reviewed request shape is representable");
        let rendered = args
            .iter()
            .map(|arg| arg.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(rendered.contains("--strict-scope"));
        assert!(rendered.contains("--body {\"ok\":true}"));
        assert!(!rendered.contains("--allow"));
        assert!(
            load_argv(
                path,
                "http://127.0.0.1/",
                "POST",
                &[("Authorization".to_owned(), "secret".to_owned())],
                None,
                1,
                1,
                Duration::from_secs(1),
            )
            .is_err()
        );
    }

    #[test]
    fn expected_status_mismatch_is_not_a_transport_error() {
        let expected = HttpObservableExpectationV1::Exact { status_exact: 403 };
        let parsed = parse_load_report(&outcome(
            r#"{"target_url":"http://127.0.0.1/","total_requests":2,"successful_requests":2,"failed_requests":0,"total_duration_ms":10,"requests_per_second":200.0,"latency_min_ms":1.0,"latency_max_ms":2.0,"latency_mean_ms":1.5,"latency_p50_ms":1.0,"latency_p90_ms":2.0,"latency_p95_ms":2.0,"latency_p99_ms":2.0,"status_codes":{"200":1,"403":1},"errors":[],"error_kinds":{}}"#,
        ), "http://127.0.0.1/")
        .expect("report parses");
        let output = load_output(
            &outcome("{}"),
            parsed,
            &expected,
            "eggsec 0.1.0",
            &"a".repeat(64),
            "blocked",
        );
        let mismatch = output
            .metrics
            .iter()
            .find(|metric| metric.name == "expected_outcome_mismatch_rate")
            .expect("status mismatch observation");
        let transport = output
            .metrics
            .iter()
            .find(|metric| metric.name == "error_rate")
            .expect("transport error observation");
        assert!((mismatch.value - 0.5).abs() < f64::EPSILON);
        assert!(transport.value.abs() < f64::EPSILON);
    }
}
