//! `oha` HTTP load workload adapter.
//!
//! oha owns load generation and its JSON report semantics; Eggbench owns
//! resolution, version policy, argv construction, bounded execution, raw
//! retention, metric mapping, and evidence. The parser validates the
//! upstream `schema.json` required members (`summary`,
//! `latencyPercentiles`, `statusCodeDistribution`, `errorDistribution`)
//! and fails closed on anything else.

use super::artifact::artifact_candidates;
use super::command::{ExternalCommandOutcome, ExternalCommandSpec, run_command};
use super::common::{
    check_min_version, driver_env, failure_category, finite_non_negative, probe_failure_category,
    target_http_url,
};
use super::error::{DriverError, ErrorCategory};
use super::parser::ExternalOutputParser;
use super::resolver::{BinaryResolver, ResolvedExecutable};
use super::version::{ToolVersion, VersionProbe, VersionProbeSpec};
use eggbench_core::{
    Aggregation, Capability, DriverCategory, DriverDescriptor, HttpVersion, LoadMode, Name,
    RawMetricObservation, SchemaVersion, Workload,
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
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Canonical workload driver name for the oha adapter.
pub const OHA_DRIVER_NAME: &str = "oha";
/// Logical tool name resolved through the trusted substrate.
const OHA_TOOL: &str = "oha";
/// Versioned parser identifier for oha `--output-format json`.
pub const OHA_PARSER_ID: &str = "oha-json/v1";
/// Minimum supported oha release (`--output-format json` stable).
const OHA_MIN_VERSION: (u64, u64, u64) = (1, 0, 0);
/// Diagnostic artifact with the status-code distribution.
pub const OHA_STATUS_ARTIFACT: &str = "oha-status.json";
/// Stdout retention cap: JSON reports are small; the cap only guards pathologies.
const STDOUT_LIMIT: u64 = 4 * 1024 * 1024;
/// Stderr retention cap.
const STDERR_LIMIT: u64 = 256 * 1024;
/// Version-probe timeout.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// oha workload executor: resolved binary plus pinned probe version.
///
/// The version is pinned at run preflight (before managed startup), never
/// re-probed per trial, and recorded in every trial's command metadata.
/// Executors built without a preflight probe self-probe once on first
/// execution so direct `execute_run` consumers get the same version floor.
pub struct OhaWorkload {
    executable: ResolvedExecutable,
    version: Option<String>,
}

impl OhaWorkload {
    /// Resolve the `oha` binary through the trusted substrate.
    ///
    /// # Errors
    /// Returns `binary_not_found` (or another resolution category) when no
    /// trusted executable is available. Called from run preflight, before
    /// managed startup.
    pub fn resolve() -> Result<ResolvedExecutable, DriverError> {
        BinaryResolver::resolve(OHA_TOOL, None, None)
    }

    /// Probe `--version` with a bounded timeout.
    ///
    /// # Errors
    /// Returns a typed probe failure on timeout, nonzero exit, or missing
    /// version token.
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
                parser_id: OHA_PARSER_ID.to_owned(),
            },
            cancel,
        )
        .await
    }

    /// Bind a probed binary to an executor, enforcing the version floor.
    ///
    /// # Errors
    /// Returns `unsupported_version` below oha 1.0.0.
    pub fn new(executable: ResolvedExecutable, version: String) -> Result<Self, DriverError> {
        check_min_version(OHA_TOOL, &version, OHA_MIN_VERSION)?;
        Ok(Self {
            executable,
            version: Some(version),
        })
    }

    /// Bind a resolved binary without a probe; the first execution probes
    /// once and enforces the version floor before any load runs.
    #[must_use]
    pub fn from_resolved(executable: ResolvedExecutable) -> Self {
        Self {
            executable,
            version: None,
        }
    }

    /// Pinned tool version, when probed.
    #[must_use]
    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    async fn ensure_probed(&mut self, cancel: &CancellationToken) -> Result<(), FailureCategory> {
        if self.version.is_none() {
            let probed = Self::probe(&self.executable, cancel)
                .await
                .map_err(|error| probe_failure_category(error, cancel))?;
            check_min_version(OHA_TOOL, &probed.version, OHA_MIN_VERSION)
                .map_err(|error| probe_failure_category(error, cancel))?;
            self.version = Some(probed.version);
        }
        Ok(())
    }
}

impl std::fmt::Debug for OhaWorkload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OhaWorkload")
            .field("driver", &OHA_DRIVER_NAME)
            .field("version", &self.version)
            .finish()
    }
}

impl WorkloadExecutor for OhaWorkload {
    fn execute<'a>(
        &'a mut self,
        context: InvocationContext,
    ) -> Pin<Box<dyn Future<Output = Result<WorkloadOutput, FailureCategory>> + Send + 'a>> {
        Box::pin(async move {
            self.ensure_probed(&context.cancellation).await?;
            let target = workload_target_name(&context.workload);
            let url = target_http_url(&context, target)?;
            let argv = oha_argv(&context.workload, &url).map_err(failure_category)?;
            let spec = ExternalCommandSpec {
                executable: self.executable.clone(),
                args: argv,
                cwd: None,
                env: driver_env(),
                stdin_null: true,
                stdout_limit: STDOUT_LIMIT,
                stderr_limit: STDERR_LIMIT,
                timeout: context.timeout,
            };
            let outcome = run_command(&spec, &context.cancellation)
                .await
                .map_err(failure_category)?;
            let report = parse_oha_report(&outcome).map_err(failure_category)?;
            Ok(oha_output(&outcome, &report))
        })
    }

    fn drain<'a>(
        &'a mut self,
        _context: DrainContext,
    ) -> Pin<Box<dyn Future<Output = Result<(), FailureCategory>> + Send + 'a>> {
        // No persistent state: every invocation spawns and reaps one child.
        Box::pin(async move { Ok(()) })
    }
}

fn workload_target_name(workload: &Workload) -> &str {
    match workload {
        Workload::ClosedLoop { target, .. }
        | Workload::OpenLoop { target, .. }
        | Workload::FiniteCount { target, .. }
        | Workload::TimeBounded { target, .. } => target.as_str(),
    }
}

/// Build oha argv from the plan workload (URL appended last).
///
/// ClosedLoop/FiniteCount counts map to `-n/-c`, durations to `-z`;
/// OpenLoop rates map to `-q/--latency-correction`. Mutually exclusive
/// count+duration pairs fail closed (oha silently ignores `-n` under `-z`).
fn oha_argv(workload: &Workload, url: &str) -> Result<Vec<OsString>, DriverError> {
    let unsupported = |detail: &str| {
        DriverError::execution(
            ErrorCategory::UnsupportedOption,
            format!("oha workload: {detail}"),
        )
    };
    let mut args: Vec<OsString> = vec!["--no-tui".into(), "--output-format".into(), "json".into()];
    match workload {
        Workload::ClosedLoop {
            concurrency,
            requests,
            duration_ms,
            ..
        } => match (requests, duration_ms) {
            (Some(requests), None) => {
                args.push("-n".into());
                args.push(requests.get().to_string().into());
                args.push("-c".into());
                args.push(concurrency.get().to_string().into());
            }
            (None, Some(duration)) => {
                args.push("-z".into());
                args.push(humantime(duration.get()).into());
                args.push("-c".into());
                args.push(concurrency.get().to_string().into());
            }
            _ => {
                return Err(unsupported(
                    "ClosedLoop needs exactly one of requests or duration_ms",
                ));
            }
        },
        Workload::FiniteCount {
            requests,
            concurrency,
            ..
        } => {
            args.push("-n".into());
            args.push(requests.get().to_string().into());
            args.push("-c".into());
            args.push(concurrency.get().to_string().into());
        }
        Workload::OpenLoop {
            rate_milli_rps,
            requests,
            duration_ms,
            ..
        } => {
            args.push("-q".into());
            args.push(rate_rps(rate_milli_rps.get()).into());
            args.push("--latency-correction".into());
            match (requests, duration_ms) {
                (Some(requests), None) => {
                    args.push("-n".into());
                    args.push(requests.get().to_string().into());
                }
                (None, Some(duration)) => {
                    args.push("-z".into());
                    args.push(humantime(duration.get()).into());
                }
                _ => {
                    return Err(unsupported(
                        "OpenLoop needs exactly one of requests or duration_ms",
                    ));
                }
            }
        }
        Workload::TimeBounded {
            duration_ms,
            mode,
            concurrency,
            rate_milli_rps,
            ..
        } => match mode {
            LoadMode::ClosedLoop => {
                args.push("-z".into());
                args.push(humantime(duration_ms.get()).into());
                if let Some(concurrency) = concurrency {
                    args.push("-c".into());
                    args.push(concurrency.get().to_string().into());
                }
            }
            LoadMode::OpenLoop => {
                let Some(rate) = rate_milli_rps else {
                    return Err(unsupported("OpenLoop TimeBounded needs rate_milli_rps"));
                };
                args.push("-q".into());
                args.push(rate_rps(rate.get()).into());
                args.push("--latency-correction".into());
                args.push("-z".into());
                args.push(humantime(duration_ms.get()).into());
            }
        },
    }
    args.push(url.into());
    Ok(args)
}

/// Format milli-rps as an oha `-q` rate, trimming trailing zeros.
fn rate_rps(milli_rps: u64) -> String {
    let whole = milli_rps / 1000;
    let frac = milli_rps % 1000;
    if frac == 0 {
        whole.to_string()
    } else {
        format!("{whole}.{:03}", frac)
            .trim_end_matches('0')
            .to_owned()
    }
}

/// Format a millisecond duration as oha `-z` humantime.
fn humantime(duration_ms: u64) -> String {
    if duration_ms % 1000 == 0 {
        format!("{}s", duration_ms / 1000)
    } else {
        format!("{duration_ms}ms")
    }
}

/// Validated oha JSON report: scalar inputs for normalization plus the
/// status distribution for the diagnostic artifact.
struct OhaReport {
    requests_per_sec: f64,
    success_rate: f64,
    fastest_ms: Option<f64>,
    average_ms: Option<f64>,
    percentile_ms: BTreeMap<&'static str, f64>,
    status_codes: BTreeMap<String, u64>,
    error_counts: Vec<(String, u64)>,
}

#[derive(Debug, Deserialize)]
struct OhaJson {
    summary: OhaSummary,
    #[serde(rename = "latencyPercentiles")]
    latency_percentiles: OhaPercentiles,
    #[serde(rename = "statusCodeDistribution")]
    status_code_distribution: BTreeMap<String, u64>,
    #[serde(rename = "errorDistribution")]
    error_distribution: BTreeMap<String, u64>,
}

#[derive(Debug, Deserialize)]
struct OhaSummary {
    #[serde(rename = "successRate")]
    success_rate: f64,
    #[serde(rename = "requestsPerSec")]
    requests_per_sec: f64,
    /// Slowest request; retained for schema validation, no parity metric name.
    #[allow(dead_code)]
    slowest: Option<f64>,
    fastest: Option<f64>,
    average: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct OhaPercentiles {
    /// Percentiles are null when no request completed (all-fail runs);
    /// missing latencies stay missing, never zero-filled.
    p50: Option<f64>,
    p95: Option<f64>,
    p99: Option<f64>,
}

/// Trivial parser adapter over [`parse_oha_report`] for the substrate contract.
pub struct OhaParser;

impl ExternalOutputParser for OhaParser {
    fn parser_id(&self) -> &'static str {
        OHA_PARSER_ID
    }

    fn parse(
        &self,
        outcome: &ExternalCommandOutcome,
    ) -> Result<super::parser::ParsedExternalOutput, DriverError> {
        let _ = parse_oha_report(outcome)?;
        Ok(super::parser::ParsedExternalOutput {
            parser_id: OHA_PARSER_ID.to_owned(),
            tool_version: String::new(),
            truncated: outcome.stdout.truncated(),
        })
    }
}

/// Parse and validate retained oha JSON stdout.
///
/// Exit status is deliberately not gated: oha exits 0 even when every
/// request fails, so success is determined from `successRate` and the
/// error distribution, never from the process status.
fn parse_oha_report(outcome: &ExternalCommandOutcome) -> Result<OhaReport, DriverError> {
    let parse_failed = |detail: String| DriverError::parse(ErrorCategory::ParseFailed, detail);
    let text = String::from_utf8_lossy(outcome.stdout.retained());
    let json: OhaJson = serde_json::from_str(&text)
        .map_err(|error| parse_failed(format!("invalid oha JSON: {error}")))?;
    let success_rate =
        finite_non_negative(json.summary.success_rate, "successRate").map_err(&parse_failed)?;
    if success_rate > 1.0 {
        return Err(parse_failed(format!(
            "successRate out of range: {success_rate}"
        )));
    }
    let requests_per_sec = finite_non_negative(json.summary.requests_per_sec, "requestsPerSec")
        .map_err(&parse_failed)?;
    let to_ms = |value: f64, field: &str| {
        finite_non_negative(value, field)
            .map(|v| v * 1000.0)
            .map_err(&parse_failed)
    };
    let fastest_ms = json
        .summary
        .fastest
        .map(|v| to_ms(v, "fastest"))
        .transpose()?;
    let average_ms = json
        .summary
        .average
        .map(|v| to_ms(v, "average"))
        .transpose()?;
    let mut percentile_ms = BTreeMap::new();
    for (name, value) in [
        ("p50", json.latency_percentiles.p50),
        ("p95", json.latency_percentiles.p95),
        ("p99", json.latency_percentiles.p99),
    ] {
        if let Some(value) = value {
            percentile_ms.insert(name, to_ms(value, name)?);
        }
    }
    let error_counts = json
        .error_distribution
        .into_iter()
        .map(|(message, count)| (format!("oha:{message}"), count))
        .collect();
    Ok(OhaReport {
        requests_per_sec,
        success_rate,
        fastest_ms,
        average_ms,
        percentile_ms,
        status_codes: json.status_code_distribution,
        error_counts,
    })
}

/// Assemble the trial output: substrate artifacts plus status diagnostic,
/// parity observations, and tool error counts.
fn oha_output(outcome: &ExternalCommandOutcome, report: &OhaReport) -> WorkloadOutput {
    let mut artifacts = artifact_candidates(outcome, Some(OHA_PARSER_ID));
    let status_json =
        serde_json::to_string_pretty(&report.status_codes).unwrap_or_else(|_| "{}".to_owned());
    artifacts.push(WorkloadArtifact {
        name: OHA_STATUS_ARTIFACT.to_owned(),
        media_type: "application/json".to_owned(),
        bytes: status_json.into_bytes(),
    });
    let raw = ["stdout.raw".to_owned()];
    let mut metrics = vec![
        observation(
            "throughput",
            "rps",
            report.requests_per_sec,
            Aggregation::Rate,
            "oha.summary.requests_per_sec",
            &raw,
        ),
        observation(
            "error_rate",
            "ratio",
            1.0 - report.success_rate,
            Aggregation::Ratio,
            "oha.summary.success_rate",
            &raw,
        ),
    ];
    if let Some(min) = report.fastest_ms {
        metrics.push(observation(
            "latency_min",
            "ms",
            min,
            Aggregation::Minimum,
            "oha.summary.fastest",
            &raw,
        ));
    }
    if let Some(mean) = report.average_ms {
        metrics.push(observation(
            "latency_mean",
            "ms",
            mean,
            Aggregation::Mean,
            "oha.summary.average",
            &raw,
        ));
    }
    for (name, basis_points, key) in [
        ("latency_p50", 5_000, "p50"),
        ("latency_p95", 9_500, "p95"),
        ("latency_p99", 9_900, "p99"),
    ] {
        if let Some(value) = report.percentile_ms.get(key) {
            metrics.push(observation(
                name,
                "ms",
                *value,
                Aggregation::Percentile { basis_points },
                "oha.latency_percentiles",
                &raw,
            ));
        }
    }
    WorkloadOutput {
        artifacts,
        metrics,
        histograms: Vec::new(),
        error_counts: report.error_counts.clone(),
    }
}

fn observation(
    name: &str,
    unit: &str,
    value: f64,
    aggregation: Aggregation,
    source_field: &str,
    raw_artifacts: &[String],
) -> RawMetricObservation {
    RawMetricObservation {
        name: name.to_owned(),
        unit: unit.to_owned(),
        value,
        aggregation,
        source_field: Some(source_field.to_owned()),
        producer: None,
        producer_version: None,
        raw_artifacts: raw_artifacts.to_vec(),
    }
}

/// Production descriptor for the oha workload driver.
///
/// # Panics
/// Never panics at runtime; the static names are valid by construction.
#[must_use]
pub fn oha_descriptor() -> DriverDescriptor {
    let mut capabilities = BTreeSet::new();
    capabilities.insert(Capability::HttpVersion {
        version: HttpVersion::Http11,
    });
    capabilities.insert(Capability::HttpVersion {
        version: HttpVersion::Http2,
    });
    capabilities.insert(Capability::LoadMode {
        mode: LoadMode::ClosedLoop,
    });
    capabilities.insert(Capability::LoadMode {
        mode: LoadMode::OpenLoop,
    });
    capabilities.insert(Capability::CorrectedLatency);
    capabilities.insert(Capability::ExternalBinary);
    DriverDescriptor {
        name: Name::new(OHA_DRIVER_NAME).expect("static driver name"),
        adapter_version: env!("CARGO_PKG_VERSION").to_owned(),
        upstream_name: OHA_TOOL.to_owned(),
        upstream_version: None,
        category: DriverCategory::Workload,
        capabilities,
        supported_platforms: BTreeSet::new(),
        machine_output_schema: Some(SchemaVersion(1)),
        external_process: true,
        default: false,
        compatible_service_types: BTreeSet::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::external::command::CapturedStream;
    use std::path::PathBuf;

    pub fn outcome_with_stdout(bytes: &[u8], exit_code: Option<i32>) -> ExternalCommandOutcome {
        let total = bytes.len() as u64;
        ExternalCommandOutcome {
            executable: ResolvedExecutable {
                logical_tool: OHA_TOOL.to_owned(),
                selected_path: PathBuf::from("/tmp/oha"),
                canonical_path: PathBuf::from("/tmp/oha"),
                sha256_hex: "ab".repeat(32),
                file_size: 1,
                executable_class: "test".to_owned(),
            },
            argc: 3,
            exit_code,
            stdout: CapturedStream::collect(bytes.to_vec(), total, total.max(1)),
            stderr: CapturedStream::collect(Vec::new(), 0, 1),
            duration: Duration::from_millis(1),
            cancelled: false,
            timed_out: false,
            cleanup_notes: Vec::new(),
        }
    }

    pub fn valid_oha_json() -> Vec<u8> {
        serde_json::json!({
            "summary": {
                "successRate": 1.0, "total": 0.01, "slowest": 0.002,
                "fastest": 0.001, "average": 0.0015, "requestsPerSec": 2000.0,
                "totalData": 100, "sizePerRequest": 10, "sizePerSec": 1000.0
            },
            "responseTimeHistogram": {"0.001": 10},
            "latencyPercentiles": {"p10": 0.001, "p25": 0.001, "p50": 0.0015,
                "p75": 0.0016, "p90": 0.0018, "p95": 0.0019, "p99": 0.002,
                "p99.9": 0.002, "p99.99": 0.002},
            "firstByteHistogram": {}, "firstBytePercentiles": {
                "p10": 0.0, "p25": 0.0, "p50": 0.0, "p75": 0.0, "p90": 0.0,
                "p95": 0.0, "p99": 0.0, "p99.9": 0.0, "p99.99": 0.0},
            "rps": {"mean": 2000.0, "stddev": 1.0, "max": 2001.0, "min": 1999.0,
                "percentiles": {"p10": 1.0, "p25": 1.0, "p50": 1.0, "p75": 1.0,
                "p90": 1.0, "p95": 1.0, "p99": 1.0, "p99.9": 1.0, "p99.99": 1.0}},
            "details": {"DNSDialup": {"average": 0.0, "fastest": 0.0, "slowest": 0.0},
                "DNSLookup": {"average": 0.0, "fastest": 0.0, "slowest": 0.0},
                "firstByte": {"average": 0.0, "fastest": 0.0, "slowest": 0.0}},
            "statusCodeDistribution": {"200": 10},
            "errorDistribution": {}
        })
        .to_string()
        .into_bytes()
    }

    #[test]
    fn valid_report_maps_parity_metrics() {
        let report = parse_oha_report(&outcome_with_stdout(&valid_oha_json(), Some(0))).unwrap();
        assert_eq!(report.requests_per_sec, 2000.0);
        assert_eq!(report.success_rate, 1.0);
        assert_eq!(report.fastest_ms, Some(1.0));
        assert_eq!(report.percentile_ms["p99"], 2.0);
        assert!(report.error_counts.is_empty());
        let output = oha_output(&outcome_with_stdout(&valid_oha_json(), Some(0)), &report);
        let names: Vec<&str> = output.metrics.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"throughput"));
        assert!(names.contains(&"error_rate"));
        assert!(names.contains(&"latency_min"));
        assert!(names.contains(&"latency_mean"));
        assert!(names.contains(&"latency_p50"));
        assert!(names.contains(&"latency_p95"));
        assert!(names.contains(&"latency_p99"));
        assert!(!names.contains(&"latency_p90"));
        assert!(
            output
                .artifacts
                .iter()
                .any(|a| a.name == OHA_STATUS_ARTIFACT)
        );
    }

    #[test]
    fn all_fail_report_parses_with_null_timings() {
        let bytes = serde_json::json!({
            "summary": {"successRate": 0.0, "total": 0.001, "slowest": null,
                "fastest": null, "average": null, "requestsPerSec": 8910.0,
                "totalData": 0, "sizePerRequest": null, "sizePerSec": 0.0},
            "latencyPercentiles": {"p10": null, "p25": null, "p50": null,
                "p75": null, "p90": null, "p95": null, "p99": null,
                "p99.9": null, "p99.99": null},
            "statusCodeDistribution": {},
            "errorDistribution": {"Connection refused (os error 111)": 5}
        })
        .to_string()
        .into_bytes();
        // Exit status is not the failure signal: oha exits 0 here.
        let report = parse_oha_report(&outcome_with_stdout(&bytes, Some(0))).unwrap();
        assert_eq!(report.success_rate, 0.0);
        assert_eq!(report.fastest_ms, None);
        assert!(report.percentile_ms.is_empty());
        let output = oha_output(&outcome_with_stdout(&bytes, Some(0)), &report);
        let error_rate = output
            .metrics
            .iter()
            .find(|m| m.name == "error_rate")
            .unwrap();
        assert_eq!(error_rate.value, 1.0);
        assert!(!output.metrics.iter().any(|m| m.name == "latency_min"));
        assert!(!output.metrics.iter().any(|m| m.name == "latency_p99"));
        assert_eq!(output.error_counts.len(), 1);
    }

    #[test]
    fn malformed_and_domain_invalid_reports_fail() {
        assert!(parse_oha_report(&outcome_with_stdout(b"not json", Some(0))).is_err());
        let missing = serde_json::json!({"summary": {}}).to_string().into_bytes();
        assert!(parse_oha_report(&outcome_with_stdout(&missing, Some(0))).is_err());
        let mut invalid = serde_json::from_slice::<serde_json::Value>(&valid_oha_json()).unwrap();
        invalid["summary"]["successRate"] = serde_json::json!(1.5);
        let bytes = serde_json::to_string(&invalid).unwrap().into_bytes();
        assert!(parse_oha_report(&outcome_with_stdout(&bytes, Some(0))).is_err());
    }

    #[test]
    fn argv_mapping_covers_load_models() {
        use eggbench_core::{DurationMs, PositiveCount, RateMilliRps};
        let target = Name::new("origin").unwrap();
        let closed = Workload::ClosedLoop {
            target: target.clone(),
            concurrency: PositiveCount::new(4).unwrap(),
            requests: Some(PositiveCount::new(100).unwrap()),
            duration_ms: None,
        };
        let argv = oha_argv(&closed, "http://127.0.0.1:1/").unwrap();
        let flat: Vec<String> = argv
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            flat,
            [
                "--no-tui",
                "--output-format",
                "json",
                "-n",
                "100",
                "-c",
                "4",
                "http://127.0.0.1:1/"
            ]
        );
        let open = Workload::OpenLoop {
            target,
            rate_milli_rps: RateMilliRps::new(2500).unwrap(),
            requests: Some(PositiveCount::new(10).unwrap()),
            duration_ms: None,
        };
        let argv = oha_argv(&open, "http://127.0.0.1:1/").unwrap();
        let flat: Vec<String> = argv
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(flat.windows(2).any(|w| w == ["-q", "2.5"]));
        assert!(flat.contains(&"--latency-correction".to_owned()));
        let both = Workload::ClosedLoop {
            target: Name::new("origin").unwrap(),
            concurrency: PositiveCount::new(1).unwrap(),
            requests: Some(PositiveCount::new(1).unwrap()),
            duration_ms: Some(DurationMs::new(1000).unwrap()),
        };
        assert!(oha_argv(&both, "http://127.0.0.1:1/").is_err());
    }
}
