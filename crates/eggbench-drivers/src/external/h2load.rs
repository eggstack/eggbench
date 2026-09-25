//! `h2load` HTTP load workload adapter.
//!
//! h2load owns load generation and its human-readable stats semantics;
//! Eggbench owns resolution, version policy, argv construction, bounded
//! execution, raw retention, metric mapping, and evidence. h2load exposes
//! no machine output, so the parser anchors on the `finished in` marker
//! and requires the `requests:` and `time for request:` rows; anything
//! else fails closed. Exit status is not a failure signal (h2load exits 0
//! with every request failed); the request counts are.

use super::artifact::artifact_candidates;
use super::command::{ExternalCommandOutcome, ExternalCommandSpec, run_command};
use super::common::{
    check_min_version, driver_env, failure_category, finite_non_negative, metric_u64_as_f64,
    probe_failure_category, target_http_url,
};
use super::error::{DriverError, ErrorCategory};
use super::parser::{ExternalOutputParser, ParsedExternalOutput};
use super::resolver::{BinaryResolver, ResolvedExecutable};
use super::version::ToolVersion;
use eggbench_core::{
    Aggregation, Capability, DriverCategory, DriverDescriptor, HttpVersion, LoadMode, Name,
    RawMetricObservation, SchemaVersion, Workload,
};
use eggbench_runner::{
    DrainContext, FailureCategory, InvocationContext, WorkloadArtifact, WorkloadExecutor,
    WorkloadOutput,
};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Canonical workload driver name for the h2load adapter.
pub const H2LOAD_DRIVER_NAME: &str = "h2load";
/// Logical tool name resolved through the trusted substrate.
const H2LOAD_TOOL: &str = "h2load";
/// Versioned parser identifier for h2load text stats.
pub const H2LOAD_PARSER_ID: &str = "h2load-text/v1";
/// Minimum supported h2load release (text stats shape).
const H2LOAD_MIN_VERSION: (u64, u64, u64) = (1, 0, 0);
/// Diagnostic artifact with the status-code buckets.
pub const H2LOAD_STATUS_ARTIFACT: &str = "h2load-status.json";
/// Stdout retention cap: stats are small; the cap only guards pathologies.
const STDOUT_LIMIT: u64 = 1024 * 1024;
/// Stderr retention cap.
const STDERR_LIMIT: u64 = 256 * 1024;
/// Version-probe timeout.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// h2load workload executor: resolved binary plus pinned probe version.
pub struct H2loadWorkload {
    executable: ResolvedExecutable,
    version: Option<String>,
}

impl H2loadWorkload {
    /// Resolve the `h2load` binary through the trusted substrate.
    ///
    /// # Errors
    /// Returns a resolution category when no trusted executable is available.
    pub fn resolve() -> Result<ResolvedExecutable, DriverError> {
        BinaryResolver::resolve(H2LOAD_TOOL, None, None)
    }

    /// Probe `--version` with a bounded timeout.
    ///
    /// h2load prints `h2load nghttp2/<release>`, which the generic token
    /// extractor cannot isolate (digits precede the `/`), so the release
    /// after the `nghttp2/` marker is parsed explicitly.
    ///
    /// # Errors
    /// Returns a typed probe failure on timeout, nonzero exit, or missing
    /// version token.
    pub async fn probe(
        executable: &ResolvedExecutable,
        cancel: &CancellationToken,
    ) -> Result<ToolVersion, DriverError> {
        use super::command::ExternalCommandSpec;
        let outcome = run_command(
            &ExternalCommandSpec {
                executable: executable.clone(),
                args: vec![std::ffi::OsString::from("--version")],
                cwd: None,
                env: driver_env(),
                stdin_null: true,
                stdin_bytes: None,
                stdout_limit: 64 * 1024,
                stderr_limit: 64 * 1024,
                timeout: PROBE_TIMEOUT,
            },
            cancel,
        )
        .await
        .map_err(|error| {
            if matches!(
                error.category(),
                ErrorCategory::TimedOut | ErrorCategory::Cancelled
            ) {
                DriverError::probe(ErrorCategory::VersionProbeTimeout, error.to_string())
            } else {
                DriverError::probe(ErrorCategory::VersionProbeFailed, error.to_string())
            }
        })?;
        if outcome.exit_code != Some(0) {
            return Err(DriverError::probe(
                ErrorCategory::VersionProbeFailed,
                format!("h2load version probe exited with {:?}", outcome.exit_code),
            ));
        }
        let text = String::from_utf8_lossy(outcome.stdout.retained()).into_owned();
        let version = parse_h2load_version(&text).ok_or_else(|| {
            DriverError::probe(
                ErrorCategory::VersionProbeFailed,
                "h2load version output did not contain an nghttp2 release token",
            )
        })?;
        Ok(ToolVersion {
            tool: executable.logical_tool.clone(),
            executable: executable.clone(),
            stdout: outcome.stdout.clone(),
            stderr: outcome.stderr.clone(),
            exit_code: outcome.exit_code,
            version,
            parser_id: H2LOAD_PARSER_ID.to_owned(),
        })
    }

    /// Bind a probed binary to an executor, enforcing the version floor.
    ///
    /// # Errors
    /// Returns `unsupported_version` below h2load 1.0.0.
    pub fn new(executable: ResolvedExecutable, version: String) -> Result<Self, DriverError> {
        check_min_version(H2LOAD_TOOL, &version, H2LOAD_MIN_VERSION)?;
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
                .map_err(|error| probe_failure_category(&error, cancel))?;
            check_min_version(H2LOAD_TOOL, &probed.version, H2LOAD_MIN_VERSION)
                .map_err(|error| probe_failure_category(&error, cancel))?;
            self.version = Some(probed.version);
        }
        Ok(())
    }
}

impl std::fmt::Debug for H2loadWorkload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("H2loadWorkload")
            .field("driver", &H2LOAD_DRIVER_NAME)
            .field("version", &self.version)
            .finish_non_exhaustive()
    }
}

impl WorkloadExecutor for H2loadWorkload {
    fn execute<'a>(
        &'a mut self,
        context: InvocationContext,
    ) -> Pin<Box<dyn Future<Output = Result<WorkloadOutput, FailureCategory>> + Send + 'a>> {
        Box::pin(async move {
            self.ensure_probed(&context.cancellation).await?;
            let target = workload_target_name(&context.workload);
            let url = target_http_url(&context, target)?;
            let argv =
                h2load_argv(&context.workload, &url).map_err(|error| failure_category(&error))?;
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
            let report = parse_h2load_report(&outcome).map_err(|error| failure_category(&error))?;
            Ok(h2load_output(&outcome, &report))
        })
    }

    fn drain<'a>(
        &'a mut self,
        _context: DrainContext,
    ) -> Pin<Box<dyn Future<Output = Result<(), FailureCategory>> + Send + 'a>> {
        Box::pin(async move { Ok(()) })
    }
}

fn workload_target_name(workload: &Workload) -> &str {
    match workload {
        Workload::ClosedLoop { target, .. }
        | Workload::OpenLoop { target, .. }
        | Workload::FiniteCount { target, .. }
        | Workload::TimeBounded { target, .. }
        | Workload::SemanticReplay { target, .. } => target.as_str(),
    }
}

/// Build h2load argv from the plan workload (URL appended last).
///
/// Count-bound loops map to `-n/-c`, durations to `--duration`. `OpenLoop`
/// has no honest mapping (h2load offers no rate limiter) and fails closed.
/// Cleartext targets use `--h1`; anything but `http(s)` fails closed.
fn h2load_argv(workload: &Workload, url: &str) -> Result<Vec<OsString>, DriverError> {
    let unsupported = |detail: &str| {
        DriverError::execution(
            ErrorCategory::UnsupportedOption,
            format!("h2load workload: {detail}"),
        )
    };
    let scheme = url.split_once("://").map(|(s, _)| s).unwrap_or_default();
    let mut args: Vec<OsString> = Vec::new();
    match scheme {
        "http" => args.push("--h1".into()),
        "https" => {}
        _ => return Err(unsupported("target URL must be http or https")),
    }
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
                args.push(format!("--duration={}", duration_secs(duration.get())).into());
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
        Workload::OpenLoop { .. } => {
            return Err(unsupported("OpenLoop has no rate-limiter mapping"));
        }
        Workload::TimeBounded {
            duration_ms,
            mode,
            concurrency,
            ..
        } => match mode {
            LoadMode::ClosedLoop => {
                args.push(format!("--duration={}", duration_secs(duration_ms.get())).into());
                if let Some(concurrency) = concurrency {
                    args.push("-c".into());
                    args.push(concurrency.get().to_string().into());
                }
            }
            LoadMode::OpenLoop => {
                return Err(unsupported("OpenLoop has no rate-limiter mapping"));
            }
        },
        Workload::SemanticReplay { .. } => {
            return Err(unsupported(
                "SemanticReplay requires the eggreplay-semantic driver",
            ));
        }
    }
    args.push(url.into());
    Ok(args)
}

/// Format milliseconds as h2load `--duration` seconds.
///
/// Integer quotient/remainder preserves the exact pre-C002 textual output:
/// exact seconds emit a bare integer, other values emit three fractional
/// digits with trailing zeros trimmed (so 1500ms emits `1.5`).
fn duration_secs(duration_ms: u64) -> String {
    if duration_ms.is_multiple_of(1000) {
        (duration_ms / 1000).to_string()
    } else {
        let whole = duration_ms / 1000;
        let frac = duration_ms % 1000;
        format!("{whole}.{frac:03}")
            .trim_end_matches('0')
            .to_owned()
    }
}

/// Validated h2load stats: scalar inputs plus status buckets.
struct H2loadReport {
    total: u64,
    failed: u64,
    errored: u64,
    timed_out: u64,
    req_per_sec_mean: f64,
    request_min_ms: f64,
    request_mean_ms: f64,
    status_codes: Option<BTreeMap<String, u64>>,
}

/// Trivial parser adapter over [`parse_h2load_report`].
pub struct H2loadParser;

impl ExternalOutputParser for H2loadParser {
    fn parser_id(&self) -> &'static str {
        H2LOAD_PARSER_ID
    }

    fn parse(&self, outcome: &ExternalCommandOutcome) -> Result<ParsedExternalOutput, DriverError> {
        let _ = parse_h2load_report(outcome)?;
        Ok(ParsedExternalOutput {
            parser_id: H2LOAD_PARSER_ID.to_owned(),
            tool_version: String::new(),
            truncated: outcome.stdout.truncated(),
        })
    }
}

/// Parse anchored h2load stats after the `finished in` marker.
///
/// Progress chatter and the spawning preamble are ignored; the `requests:`
/// and `time for request:` rows are required.
fn parse_h2load_report(outcome: &ExternalCommandOutcome) -> Result<H2loadReport, DriverError> {
    let parse_failed = |detail: String| DriverError::parse(ErrorCategory::ParseFailed, detail);
    let text = String::from_utf8_lossy(outcome.stdout.retained());
    let stats: Vec<&str> = text
        .lines()
        .skip_while(|line| !line.starts_with("finished in "))
        .collect();
    if stats.is_empty() {
        return Err(parse_failed("missing `finished in` marker".to_owned()));
    }
    let row = |prefix: &str| {
        stats
            .iter()
            .find(|line| line.starts_with(prefix))
            .ok_or_else(|| parse_failed(format!("missing `{prefix}` row")))
    };
    let (total, failed, errored, timed_out) =
        parse_requests_row(row("requests: ")?).map_err(&parse_failed)?;
    let (request_min_ms, request_mean_ms) =
        parse_timing_row(row("time for request:")?, "time for request").map_err(&parse_failed)?;
    let req_per_sec_mean = parse_rate_row(row("req/s")?).map_err(&parse_failed)?;
    let status_codes = stats
        .iter()
        .find(|line| line.starts_with("status codes: "))
        .map(|line| parse_status_row(line))
        .transpose()
        .map_err(&parse_failed)?;
    Ok(H2loadReport {
        total,
        failed,
        errored,
        timed_out,
        req_per_sec_mean,
        request_min_ms,
        request_mean_ms,
        status_codes,
    })
}

/// Parse `requests: T total, S started, D done, O succeeded, F failed,
/// E errored, To timeout`.
fn parse_requests_row(line: &str) -> Result<(u64, u64, u64, u64), String> {
    let body = line
        .strip_prefix("requests: ")
        .ok_or("requests row prefix")?;
    let mut values: BTreeMap<&str, u64> = BTreeMap::new();
    for part in body.split(',') {
        let part = part.trim();
        let (count, label) = part
            .split_once(' ')
            .ok_or_else(|| format!("malformed requests cell: {part:?}"))?;
        let count: u64 = count
            .parse()
            .map_err(|_| format!("non-numeric requests cell: {part:?}"))?;
        values.insert(label.trim(), count);
    }
    let get = |label: &str| {
        values
            .get(label)
            .copied()
            .ok_or_else(|| format!("missing requests cell: {label}"))
    };
    Ok((
        get("total")?,
        get("failed")?,
        get("errored")?,
        get("timeout")?,
    ))
}

/// Parse `time for request: <min> <max> <mean> <sd> <pct>` durations.
fn parse_timing_row(line: &str, label: &str) -> Result<(f64, f64), String> {
    let body = line
        .split_once(':')
        .map(|(_, rest)| rest)
        .ok_or("timing row colon")?;
    let cells: Vec<&str> = body.split_whitespace().collect();
    if cells.len() < 3 {
        return Err(format!("short {label} row: {line:?}"));
    }
    Ok((parse_h2_duration(cells[0])?, parse_h2_duration(cells[2])?))
}

/// Extract the nghttp2 release from `h2load nghttp2/<release>` output.
fn parse_h2load_version(text: &str) -> Option<String> {
    let (_, rest) = text.split_once("nghttp2/")?;
    let token: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let token = token.trim_matches('.');
    if token.contains('.') && token.chars().any(|c| c.is_ascii_digit()) {
        Some(token.to_owned())
    } else {
        None
    }
}

/// Parse one h2load duration token (`631us`, `1.26ms`, `4.87ms`) to milliseconds.
fn parse_h2_duration(token: &str) -> Result<f64, String> {
    let (number, factor) = if let Some(number) = token.strip_suffix("us") {
        (number, 1.0 / 1000.0)
    } else if let Some(number) = token.strip_suffix("ms") {
        (number, 1.0)
    } else if let Some(number) = token.strip_suffix('s') {
        (number, 1000.0)
    } else if let Some(number) = token.strip_suffix('m') {
        (number, 60_000.0)
    } else {
        return Err(format!("unknown duration unit: {token:?}"));
    };
    let value: f64 = number
        .parse()
        .map_err(|_| format!("non-numeric duration: {token:?}"))?;
    finite_non_negative(value * factor, "duration")
}

/// Parse the `req/s` row mean (third numeric cell).
fn parse_rate_row(line: &str) -> Result<f64, String> {
    let body = line
        .split_once(':')
        .map(|(_, rest)| rest)
        .ok_or("req/s row colon")?;
    let cells: Vec<&str> = body.split_whitespace().collect();
    let mean = cells
        .get(2)
        .ok_or_else(|| format!("short req/s row: {line:?}"))?;
    let value: f64 = mean
        .parse()
        .map_err(|_| format!("non-numeric req/s mean: {mean:?}"))?;
    finite_non_negative(value, "req/s mean")
}

/// Parse `status codes: 10 2xx, 0 3xx, 0 4xx, 0 5xx`.
fn parse_status_row(line: &str) -> Result<BTreeMap<String, u64>, String> {
    let body = line
        .strip_prefix("status codes: ")
        .ok_or("status codes prefix")?;
    let mut out = BTreeMap::new();
    for part in body.split(',') {
        let part = part.trim();
        let (count, bucket) = part
            .split_once(' ')
            .ok_or_else(|| format!("malformed status cell: {part:?}"))?;
        out.insert(
            bucket.trim().to_owned(),
            count
                .parse()
                .map_err(|_| format!("non-numeric status cell: {part:?}"))?,
        );
    }
    if out.is_empty() {
        return Err("empty status codes row".to_owned());
    }
    Ok(out)
}

fn h2load_output(outcome: &ExternalCommandOutcome, report: &H2loadReport) -> WorkloadOutput {
    let mut artifacts = artifact_candidates(outcome, Some(H2LOAD_PARSER_ID));
    if let Some(status) = &report.status_codes {
        let status_json = serde_json::to_string_pretty(status).unwrap_or_else(|_| "{}".to_owned());
        artifacts.push(WorkloadArtifact {
            name: H2LOAD_STATUS_ARTIFACT.to_owned(),
            media_type: "application/json".to_owned(),
            bytes: status_json.into_bytes(),
        });
    }
    let raw = ["stdout.raw".to_owned()];
    let mut metrics = vec![
        observation(
            "throughput",
            "rps",
            report.req_per_sec_mean,
            Aggregation::Rate,
            "h2load.req_per_sec.mean",
            &raw,
        ),
        observation(
            "latency_min",
            "ms",
            report.request_min_ms,
            Aggregation::Minimum,
            "h2load.time_for_request.min",
            &raw,
        ),
        observation(
            "latency_mean",
            "ms",
            report.request_mean_ms,
            Aggregation::Mean,
            "h2load.time_for_request.mean",
            &raw,
        ),
    ];
    if report.total > 0 {
        metrics.push(observation(
            "error_rate",
            "ratio",
            metric_u64_as_f64(report.failed) / metric_u64_as_f64(report.total),
            Aggregation::Ratio,
            "h2load.requests.failed",
            &raw,
        ));
    }
    let mut error_counts = Vec::new();
    for (label, count) in [
        ("failed", report.failed),
        ("errored", report.errored),
        ("timeout", report.timed_out),
    ] {
        if count > 0 {
            error_counts.push((format!("h2load:{label}"), count));
        }
    }
    WorkloadOutput {
        artifacts,
        metrics,
        histograms: Vec::new(),
        error_counts,
        measurement_elapsed: None,
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

/// Production descriptor for the h2load workload driver.
///
/// # Panics
/// Never panics at runtime; the static names are valid by construction.
#[must_use]
pub fn h2load_descriptor() -> DriverDescriptor {
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
    capabilities.insert(Capability::ExternalBinary);
    DriverDescriptor {
        name: Name::new(H2LOAD_DRIVER_NAME).expect("static driver name"),
        adapter_version: env!("CARGO_PKG_VERSION").to_owned(),
        upstream_name: H2LOAD_TOOL.to_owned(),
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

    pub fn outcome_with_stdout(bytes: &[u8]) -> ExternalCommandOutcome {
        let total = bytes.len() as u64;
        ExternalCommandOutcome {
            executable: ResolvedExecutable {
                logical_tool: H2LOAD_TOOL.to_owned(),
                selected_path: PathBuf::from("/tmp/h2load"),
                canonical_path: PathBuf::from("/tmp/h2load"),
                sha256_hex: "ab".repeat(32),
                file_size: 1,
                executable_class: "test".to_owned(),
            },
            argc: 4,
            exit_code: Some(0),
            stdout: CapturedStream::collect(bytes.to_vec(), total, total.max(1)),
            stderr: CapturedStream::collect(Vec::new(), 0, 1),
            duration: Duration::from_millis(1),
            cancelled: false,
            timed_out: false,
            cleanup_notes: Vec::new(),
        }
    }

    const SUCCESS: &str = "starting benchmark...\n\
        spawning thread #0: 2 total client(s). 20 total requests\n\
        Application protocol: http/1.1\n\
        progress: 50% done\n\
        progress: 100% done\n\
        \n\
        finished in 4.87ms, 2055.50 req/s, 124.45KB/s\n\
        requests: 20 total, 20 started, 20 done, 20 succeeded, 0 failed, 0 errored, 0 timeout\n\
        status codes: 20 2xx, 0 3xx, 0 4xx, 0 5xx\n\
        traffic: 620B (620) total, 1.17KB (1200) headers (space savings 0.00%), 6.79KB (6950) data\n\
                             min         max         mean         sd        +/- sd\n\
        time for request:      631us      1.26ms       856us       207us    60.00%\n\
        time for connect:       33us        35us        34us         1us   100.00%\n\
        time to 1st byte:      741us       843us       792us        72us   100.00%\n\
        req/s           :    1066.98     1078.59     1072.78        8.21   100.00%\n";

    const ALL_FAIL: &str = "starting benchmark...\n\
        spawning thread #0: 1 total client(s). 5 total requests\n\
        \n\
        finished in 225us, 0.00 req/s, 0B/s\n\
        requests: 5 total, 0 started, 0 done, 0 succeeded, 5 failed, 5 errored, 0 timeout\n\
        status codes: 0 2xx, 0 3xx, 0 4xx, 0 5xx\n\
        traffic: 0B (0) total, 0B (0) headers (space savings 0.00%), 0B (0) data\n\
                             min         max         mean         sd        +/- sd\n\
        time for request:        0us         0us         0us         0us     0.00%\n\
        time for connect:        0us         0us         0us         0us     0.00%\n\
        time to 1st byte:        0us         0us         0us         0us     0.00%\n\
        req/s           :       0.00        0.00        0.00        0.00   100.00%\n";

    #[test]
    fn success_stats_map_parity_metrics() {
        let report = parse_h2load_report(&outcome_with_stdout(SUCCESS.as_bytes())).unwrap();
        assert_eq!(
            (
                report.total,
                report.failed,
                report.errored,
                report.timed_out
            ),
            (20, 0, 0, 0)
        );
        assert_eq!(report.req_per_sec_mean.to_bits(), 1072.78_f64.to_bits());
        assert_eq!(report.request_min_ms.to_bits(), 0.631_f64.to_bits());
        assert_eq!(report.request_mean_ms.to_bits(), 0.856_f64.to_bits());
        let output = h2load_output(&outcome_with_stdout(SUCCESS.as_bytes()), &report);
        let rate = output
            .metrics
            .iter()
            .find(|m| m.name == "error_rate")
            .unwrap();
        assert_eq!(rate.value.to_bits(), 0.0_f64.to_bits());
        assert!(output.error_counts.is_empty());
        assert!(
            output
                .artifacts
                .iter()
                .any(|a| a.name == H2LOAD_STATUS_ARTIFACT)
        );
    }

    #[test]
    fn all_fail_stats_stay_visible_without_invalidating() {
        // h2load exits 0 here; failure is carried by counts, not status.
        let report = parse_h2load_report(&outcome_with_stdout(ALL_FAIL.as_bytes())).unwrap();
        assert_eq!(report.failed, 5);
        let output = h2load_output(&outcome_with_stdout(ALL_FAIL.as_bytes()), &report);
        let rate = output
            .metrics
            .iter()
            .find(|m| m.name == "error_rate")
            .unwrap();
        assert_eq!(rate.value.to_bits(), 1.0_f64.to_bits());
        assert!(
            output
                .error_counts
                .contains(&("h2load:failed".to_owned(), 5))
        );
        assert!(
            output
                .error_counts
                .contains(&("h2load:errored".to_owned(), 5))
        );
    }

    #[test]
    fn missing_marker_or_rows_fail_closed() {
        assert!(parse_h2load_report(&outcome_with_stdout(b"progress: 10% done\n")).is_err());
        let no_requests = SUCCESS.replace("requests: 20 total", "requests: 20 totals");
        assert!(parse_h2load_report(&outcome_with_stdout(no_requests.as_bytes())).is_err());
        let no_timing = SUCCESS
            .lines()
            .filter(|l| !l.contains("time for request:"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(parse_h2load_report(&outcome_with_stdout(no_timing.as_bytes())).is_err());
    }

    #[test]
    fn version_parses_nghttp2_release() {
        assert_eq!(
            parse_h2load_version("h2load nghttp2/1.59.0\n").as_deref(),
            Some("1.59.0")
        );
        assert_eq!(parse_h2load_version("h2load\n"), None);
        assert_eq!(parse_h2load_version(""), None);
    }

    #[test]
    fn argv_mapping_and_rejections() {
        use eggbench_core::PositiveCount;
        let target = Name::new("origin").unwrap();
        let closed = Workload::ClosedLoop {
            target: target.clone(),
            concurrency: PositiveCount::new(2).unwrap(),
            requests: Some(PositiveCount::new(20).unwrap()),
            duration_ms: None,
        };
        let argv = h2load_argv(&closed, "http://127.0.0.1:9/").unwrap();
        let flat: Vec<String> = argv
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(flat, ["--h1", "-n", "20", "-c", "2", "http://127.0.0.1:9/"]);
        let tls = h2load_argv(&closed, "https://127.0.0.1:9/").unwrap();
        let flat_tls: Vec<String> = tls
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(!flat_tls.contains(&"--h1".to_owned()));
        let open = Workload::OpenLoop {
            target,
            rate_milli_rps: eggbench_core::RateMilliRps::new(1000).unwrap(),
            requests: Some(PositiveCount::new(5).unwrap()),
            duration_ms: None,
        };
        assert!(h2load_argv(&open, "http://127.0.0.1:9/").is_err());
    }

    #[test]
    fn duration_secs_preserves_pre_c002_text() {
        // Pre-C002 authority: float-formatted three decimals with trailing
        // zeros trimmed; exact seconds emit a bare integer.
        for (ms, expected) in [
            (1_u64, "0.001"),
            (999, "0.999"),
            (1000, "1"),
            (1001, "1.001"),
            (1010, "1.01"),
            (1100, "1.1"),
            (1500, "1.5"),
            (2501, "2.501"),
        ] {
            assert_eq!(duration_secs(ms), expected, "ms={ms}");
        }
    }

    #[test]
    fn debug_redacts_executable_identity() {
        use std::path::PathBuf;
        let workload = H2loadWorkload {
            executable: ResolvedExecutable {
                logical_tool: H2LOAD_TOOL.to_owned(),
                selected_path: PathBuf::from("/tmp/h2load"),
                canonical_path: PathBuf::from("/tmp/h2load"),
                sha256_hex: "ab".repeat(32),
                file_size: 1,
                executable_class: "test".to_owned(),
            },
            version: Some("1.59.0".to_owned()),
        };
        let rendered = format!("{workload:?}");
        assert!(rendered.contains(H2LOAD_DRIVER_NAME));
        assert!(rendered.contains("1.59.0"));
        assert!(!rendered.contains("/tmp/h2load"));
        assert!(!rendered.contains(&"ab".repeat(32)));
    }
}
