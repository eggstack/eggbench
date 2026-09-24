//! `iperf3` TCP throughput workload adapter.
//!
//! iperf3 owns byte-stream transfer and its `-J` report semantics; Eggbench
//! owns resolution, version policy, argv construction, bounded execution,
//! raw retention, metric mapping, and evidence. The server is always an
//! explicit configured host (loopback tests spawn `iperf3 -s -1` as a
//! fixture): Eggbench never manages an iperf3 server in M002.
//!
//! Mapping honesty: iperf3 has no request-count semantic, so only
//! duration-bound workloads map (`-t`); count-bound plans fail closed.
//! TCP only; parallel streams map from closed-loop concurrency.

use super::artifact::artifact_candidates;
use super::command::{ExternalCommandOutcome, ExternalCommandSpec, run_command};
use super::common::{
    DEFAULT_IPERF3_PORT, authority_host_port, check_min_version, driver_env, failure_category,
    finite_non_negative, metric_u64_as_f64, probe_failure_category, target_http_url,
};
use super::error::{DriverError, ErrorCategory};
use super::parser::{ExternalOutputParser, ParsedExternalOutput};
use super::resolver::{BinaryResolver, ResolvedExecutable};
use super::version::{ToolVersion, VersionProbe, VersionProbeSpec};
use eggbench_core::{
    Aggregation, Capability, DriverCategory, DriverDescriptor, LoadMode, Name,
    RawMetricObservation, SchemaVersion, Workload,
};
use eggbench_runner::{
    DrainContext, FailureCategory, InvocationContext, WorkloadExecutor, WorkloadOutput,
};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Canonical workload driver name for the iperf3 adapter.
pub const IPERF3_DRIVER_NAME: &str = "iperf3";
/// Logical tool name resolved through the trusted substrate.
const IPERF3_TOOL: &str = "iperf3";
/// Versioned parser identifier for iperf3 `-J`.
pub const IPERF3_PARSER_ID: &str = "iperf3-json/v1";
/// Minimum supported iperf3 release (`-J` output).
const IPERF3_MIN_VERSION: (u64, u64, u64) = (3, 1, 0);
/// Exact rounded value produced by `u64::MAX as f64` (2^64).
///
/// The pre-C002 parser compared `v > u64::MAX as f64` with the same `>`
/// operator; this named constant preserves that boundary bit-for-bit
/// without reintroducing a lossy cast at the comparison site.
const U64_MAX_AS_F64: f64 = 18_446_744_073_709_551_616.0;
/// Stdout retention cap.
const STDOUT_LIMIT: u64 = 4 * 1024 * 1024;
/// Stderr retention cap.
const STDERR_LIMIT: u64 = 256 * 1024;
/// Version-probe timeout.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// iperf3 workload executor: resolved binary plus pinned probe version.
pub struct Iperf3Workload {
    executable: ResolvedExecutable,
    version: Option<String>,
}

impl Iperf3Workload {
    /// Resolve the `iperf3` binary through the trusted substrate.
    ///
    /// # Errors
    /// Returns a resolution category when no trusted executable is available.
    pub fn resolve() -> Result<ResolvedExecutable, DriverError> {
        BinaryResolver::resolve(IPERF3_TOOL, None, None)
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
                parser_id: IPERF3_PARSER_ID.to_owned(),
            },
            cancel,
        )
        .await
    }

    /// Bind a probed binary to an executor, enforcing the version floor.
    ///
    /// # Errors
    /// Returns `unsupported_version` below iperf3 3.1.
    pub fn new(executable: ResolvedExecutable, version: String) -> Result<Self, DriverError> {
        check_min_version(IPERF3_TOOL, &version, IPERF3_MIN_VERSION)?;
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
            check_min_version(IPERF3_TOOL, &probed.version, IPERF3_MIN_VERSION)
                .map_err(|error| probe_failure_category(&error, cancel))?;
            self.version = Some(probed.version);
        }
        Ok(())
    }
}

impl std::fmt::Debug for Iperf3Workload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Iperf3Workload")
            .field("driver", &IPERF3_DRIVER_NAME)
            .field("version", &self.version)
            .finish_non_exhaustive()
    }
}

impl WorkloadExecutor for Iperf3Workload {
    fn execute<'a>(
        &'a mut self,
        context: InvocationContext,
    ) -> Pin<Box<dyn Future<Output = Result<WorkloadOutput, FailureCategory>> + Send + 'a>> {
        Box::pin(async move {
            self.ensure_probed(&context.cancellation).await?;
            let target = workload_target_name(&context.workload);
            let url = target_http_url(&context, target)?;
            let argv =
                iperf3_argv(&context.workload, &url).map_err(|error| failure_category(&error))?;
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
                .map_err(|error| failure_category(&error))?;
            let report = parse_iperf3_report(&outcome).map_err(|error| failure_category(&error))?;
            Ok(iperf3_output(&outcome, &report))
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
        | Workload::TimeBounded { target, .. } => target.as_str(),
    }
}

/// Build iperf3 client argv (host/port from the target URL authority).
///
/// Only duration-bound closed-loop workloads map; request counts have no
/// byte-stream equivalent and fail closed instead of being coerced.
fn iperf3_argv(workload: &Workload, url: &str) -> Result<Vec<OsString>, DriverError> {
    let unsupported = |detail: &str| {
        DriverError::execution(
            ErrorCategory::UnsupportedOption,
            format!("iperf3 workload: {detail}"),
        )
    };
    let (host, port) = authority_host_port(url, DEFAULT_IPERF3_PORT)
        .map_err(|detail| DriverError::execution(ErrorCategory::UnsupportedOption, detail))?;
    let (duration_ms, streams): (Option<u64>, Option<u32>) = match workload {
        Workload::ClosedLoop {
            concurrency,
            requests,
            duration_ms: duration,
            ..
        } => {
            if requests.is_some() {
                return Err(unsupported("request counts have no byte-stream mapping"));
            }
            (
                duration.map(eggbench_core::DurationMs::get),
                Some(concurrency.get()),
            )
        }
        Workload::TimeBounded {
            duration_ms: duration,
            mode,
            concurrency,
            ..
        } => match mode {
            LoadMode::ClosedLoop => (
                Some(duration.get()),
                concurrency.map(eggbench_core::PositiveCount::get),
            ),
            LoadMode::OpenLoop => {
                return Err(unsupported("OpenLoop rate has no TCP throughput mapping"));
            }
        },
        Workload::OpenLoop { .. } | Workload::FiniteCount { .. } => {
            return Err(unsupported(
                "only duration-bound closed-loop maps to iperf3",
            ));
        }
    };
    let Some(duration_ms) = duration_ms else {
        return Err(unsupported("iperf3 needs a duration-bound workload"));
    };
    let mut args: Vec<OsString> = vec![
        "-c".into(),
        host.into(),
        "-p".into(),
        port.to_string().into(),
        "-t".into(),
        duration_secs(duration_ms).into(),
        "-J".into(),
    ];
    if let Some(streams) = streams {
        args.push("-P".into());
        args.push(streams.to_string().into());
    }
    Ok(args)
}

/// Ceil milliseconds to whole `-t` seconds (minimum one).
fn duration_secs(duration_ms: u64) -> String {
    duration_ms.div_ceil(1000).max(1).to_string()
}

/// Validated iperf3 `-J` transfer summary.
#[derive(Debug)]
struct Iperf3Report {
    sent_bps: f64,
    received_bps: f64,
    sent_bytes: u64,
    received_bytes: u64,
    retransmits: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct Iperf3Json {
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    end: Option<Iperf3End>,
}

#[derive(Debug, Deserialize)]
struct Iperf3End {
    #[serde(default)]
    sum_sent: Option<Iperf3Sum>,
    #[serde(default)]
    sum_received: Option<Iperf3Sum>,
}

#[derive(Debug, Deserialize)]
struct Iperf3Sum {
    #[serde(default)]
    bytes: Option<f64>,
    #[serde(default)]
    bits_per_second: Option<f64>,
    #[serde(default)]
    retransmits: Option<f64>,
}

/// Trivial parser adapter over [`parse_iperf3_report`].
pub struct Iperf3Parser;

impl ExternalOutputParser for Iperf3Parser {
    fn parser_id(&self) -> &'static str {
        IPERF3_PARSER_ID
    }

    fn parse(&self, outcome: &ExternalCommandOutcome) -> Result<ParsedExternalOutput, DriverError> {
        let _ = parse_iperf3_report(outcome)?;
        Ok(ParsedExternalOutput {
            parser_id: IPERF3_PARSER_ID.to_owned(),
            tool_version: String::new(),
            truncated: outcome.stdout.truncated(),
        })
    }
}

/// Parse and validate retained iperf3 `-J` stdout.
///
/// A top-level `error` member (the refused-connection shape) is a tool
/// failure, never a zero-throughput observation.
fn parse_iperf3_report(outcome: &ExternalCommandOutcome) -> Result<Iperf3Report, DriverError> {
    let parse_failed = |detail: String| DriverError::parse(ErrorCategory::ParseFailed, detail);
    let text = String::from_utf8_lossy(outcome.stdout.retained());
    let json: Iperf3Json = serde_json::from_str(&text)
        .map_err(|error| parse_failed(format!("invalid iperf3 JSON: {error}")))?;
    if let Some(message) = json.error {
        return Err(parse_failed(format!("iperf3 reported error: {message}")));
    }
    let end = json
        .end
        .ok_or_else(|| parse_failed("missing `end` section".to_owned()))?;
    let sent = end
        .sum_sent
        .ok_or_else(|| parse_failed("missing `end.sum_sent`".to_owned()))?;
    let received = end
        .sum_received
        .ok_or_else(|| parse_failed("missing `end.sum_received`".to_owned()))?;
    let bps = |sum: &Iperf3Sum, field: &str| {
        sum.bits_per_second
            .ok_or_else(|| parse_failed(format!("missing {field}.bits_per_second")))
            .and_then(|v| finite_non_negative(v, field).map_err(parse_failed))
    };
    let bytes = |sum: &Iperf3Sum, field: &str| {
        sum.bytes
            .ok_or_else(|| parse_failed(format!("missing {field}.bytes")))
            .and_then(|v| finite_non_negative(v, field).map_err(parse_failed))
            .and_then(|v| {
                if v > U64_MAX_AS_F64 {
                    Err(parse_failed(format!("{field}.bytes out of range")))
                } else {
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    Ok(v as u64)
                }
            })
    };
    let retransmits = sent
        .retransmits
        .map(|v| {
            finite_non_negative(v, "retransmits")
                .map_err(&parse_failed)
                .and_then(|v| {
                    if v > U64_MAX_AS_F64 {
                        Err(parse_failed("retransmits out of range".to_owned()))
                    } else {
                        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                        Ok(v as u64)
                    }
                })
        })
        .transpose()?;
    Ok(Iperf3Report {
        sent_bps: bps(&sent, "sum_sent")?,
        received_bps: bps(&received, "sum_received")?,
        sent_bytes: bytes(&sent, "sum_sent")?,
        received_bytes: bytes(&received, "sum_received")?,
        retransmits,
    })
}

fn iperf3_output(outcome: &ExternalCommandOutcome, report: &Iperf3Report) -> WorkloadOutput {
    let artifacts = artifact_candidates(outcome, Some(IPERF3_PARSER_ID));
    let raw = ["stdout.raw".to_owned()];
    let count = metric_u64_as_f64;
    let mut metrics = vec![
        observation(
            "bits_per_sec_sent",
            "bps",
            report.sent_bps,
            Aggregation::Rate,
            "iperf3.end.sum_sent.bits_per_second",
            &raw,
        ),
        observation(
            "bits_per_sec_received",
            "bps",
            report.received_bps,
            Aggregation::Rate,
            "iperf3.end.sum_received.bits_per_second",
            &raw,
        ),
        observation(
            "bytes_sent",
            "bytes",
            count(report.sent_bytes),
            Aggregation::Sum,
            "iperf3.end.sum_sent.bytes",
            &raw,
        ),
        observation(
            "bytes_received",
            "bytes",
            count(report.received_bytes),
            Aggregation::Sum,
            "iperf3.end.sum_received.bytes",
            &raw,
        ),
    ];
    if let Some(retransmits) = report.retransmits {
        metrics.push(observation(
            "retransmits",
            "count",
            count(retransmits),
            Aggregation::Sum,
            "iperf3.end.sum_sent.retransmits",
            &raw,
        ));
    }
    WorkloadOutput {
        artifacts,
        metrics,
        histograms: Vec::new(),
        error_counts: Vec::new(),
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

/// Production descriptor for the iperf3 workload driver.
///
/// # Panics
/// Never panics at runtime; the static names are valid by construction.
#[must_use]
pub fn iperf3_descriptor() -> DriverDescriptor {
    let mut capabilities = BTreeSet::new();
    capabilities.insert(Capability::LoadMode {
        mode: LoadMode::ClosedLoop,
    });
    capabilities.insert(Capability::ExternalBinary);
    DriverDescriptor {
        name: Name::new(IPERF3_DRIVER_NAME).expect("static driver name"),
        adapter_version: env!("CARGO_PKG_VERSION").to_owned(),
        upstream_name: IPERF3_TOOL.to_owned(),
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
                logical_tool: IPERF3_TOOL.to_owned(),
                selected_path: PathBuf::from("/tmp/iperf3"),
                canonical_path: PathBuf::from("/tmp/iperf3"),
                sha256_hex: "ab".repeat(32),
                file_size: 1,
                executable_class: "test".to_owned(),
            },
            argc: 6,
            exit_code: Some(0),
            stdout: CapturedStream::collect(bytes.to_vec(), total, total.max(1)),
            stderr: CapturedStream::collect(Vec::new(), 0, 1),
            duration: Duration::from_millis(1),
            cancelled: false,
            timed_out: false,
            cleanup_notes: Vec::new(),
        }
    }

    fn valid_iperf3_json() -> Vec<u8> {
        serde_json::json!({
            "start": {"connected": [], "version": "iperf 3.16"},
            "intervals": [],
            "end": {
                "sum_sent": {"seconds": 2.0, "bytes": 1000.0,
                    "bits_per_second": 4000.0, "retransmits": 3.0},
                "sum_received": {"seconds": 2.0, "bytes": 990.0,
                    "bits_per_second": 3960.0}
            }
        })
        .to_string()
        .into_bytes()
    }

    #[test]
    fn valid_report_maps_transfer_metrics() {
        let report = parse_iperf3_report(&outcome_with_stdout(&valid_iperf3_json())).unwrap();
        assert_eq!(report.sent_bps.to_bits(), 4000.0_f64.to_bits());
        assert_eq!(report.received_bps.to_bits(), 3960.0_f64.to_bits());
        assert_eq!(report.retransmits, Some(3));
        let output = iperf3_output(&outcome_with_stdout(&valid_iperf3_json()), &report);
        let names: Vec<&str> = output.metrics.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"bits_per_sec_sent"));
        assert!(names.contains(&"bits_per_sec_received"));
        assert!(names.contains(&"bytes_sent"));
        assert!(names.contains(&"bytes_received"));
        assert!(names.contains(&"retransmits"));
        assert!(!names.contains(&"throughput"));
    }

    #[test]
    fn tool_error_member_is_failure_not_zero() {
        let bytes = serde_json::json!({
            "start": {"connected": []}, "intervals": [], "end": {},
            "error": "unable to connect to server: Connection refused"
        })
        .to_string()
        .into_bytes();
        let error = parse_iperf3_report(&outcome_with_stdout(&bytes)).unwrap_err();
        assert_eq!(error.category(), ErrorCategory::ParseFailed);
        assert!(error.to_string().contains("unable to connect"));
    }

    #[test]
    fn missing_sections_and_bad_numbers_fail() {
        assert!(parse_iperf3_report(&outcome_with_stdout(b"not json")).is_err());
        let no_end = serde_json::json!({"start": {}, "intervals": []})
            .to_string()
            .into_bytes();
        assert!(parse_iperf3_report(&outcome_with_stdout(&no_end)).is_err());
        let mut bad = serde_json::from_slice::<serde_json::Value>(&valid_iperf3_json()).unwrap();
        bad["end"]["sum_sent"]["bits_per_second"] = serde_json::json!(-1.0);
        let bytes = serde_json::to_string(&bad).unwrap().into_bytes();
        assert!(parse_iperf3_report(&outcome_with_stdout(&bytes)).is_err());
    }

    #[test]
    fn argv_mapping_and_count_rejection() {
        use eggbench_core::{DurationMs, PositiveCount};
        let target = Name::new("iperf-target").unwrap();
        let duration = Workload::ClosedLoop {
            target: target.clone(),
            concurrency: PositiveCount::new(2).unwrap(),
            requests: None,
            duration_ms: Some(DurationMs::new(2500).unwrap()),
        };
        let argv = iperf3_argv(&duration, "http://127.0.0.1:5201/").unwrap();
        let flat: Vec<String> = argv
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            flat,
            ["-c", "127.0.0.1", "-p", "5201", "-t", "3", "-J", "-P", "2"]
        );
        let default_port = iperf3_argv(&duration, "http://127.0.0.1/").unwrap();
        let flat_default: Vec<String> = default_port
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(flat_default.windows(2).any(|w| w == ["-p", "5201"]));
        let count = Workload::FiniteCount {
            target,
            requests: PositiveCount::new(10).unwrap(),
            concurrency: PositiveCount::new(1).unwrap(),
        };
        assert!(iperf3_argv(&count, "http://127.0.0.1:5201/").is_err());
    }

    #[test]
    fn duration_secs_ceiling_matches_pre_c002_mapping() {
        // Pre-C002 `((ms + 999) / 1000).max(1)` authority over the accepted
        // duration domain; `div_ceil` additionally avoids the `ms + 999`
        // overflow near `u64::MAX`.
        for (ms, expected) in [
            (0_u64, "1"),
            (1, "1"),
            (999, "1"),
            (1000, "1"),
            (1001, "2"),
            (1500, "2"),
            (2500, "3"),
            (2501, "3"),
        ] {
            assert_eq!(duration_secs(ms), expected, "ms={ms}");
        }
        assert_eq!(duration_secs(u64::MAX), ((u64::MAX / 1000) + 1).to_string());
    }

    #[allow(clippy::cast_precision_loss)]
    #[test]
    fn u64_max_constant_matches_historical_cast() {
        assert_eq!(
            U64_MAX_AS_F64.to_bits(),
            (u64::MAX as f64).to_bits(),
            "upper-bound constant must preserve the pre-C002 rounded comparison"
        );
    }

    #[test]
    fn byte_counter_boundary_parity() {
        // 0.0 and ordinary integer values are accepted.
        let mut json = serde_json::from_slice::<serde_json::Value>(&valid_iperf3_json()).unwrap();
        json["end"]["sum_sent"]["bytes"] = serde_json::json!(0.0);
        assert!(
            parse_iperf3_report(&outcome_with_stdout(
                serde_json::to_string(&json).unwrap().as_bytes()
            ))
            .is_ok()
        );
        // Non-integral positive values keep the historical truncation.
        let mut truncated =
            serde_json::from_slice::<serde_json::Value>(&valid_iperf3_json()).unwrap();
        truncated["end"]["sum_sent"]["bytes"] = serde_json::json!(1000.9);
        let report = parse_iperf3_report(&outcome_with_stdout(
            serde_json::to_string(&truncated).unwrap().as_bytes(),
        ))
        .unwrap();
        assert_eq!(report.sent_bytes, 1000);
        // The rounded upper bound is accepted; the next representable
        // value above it is rejected.
        let mut at_bound =
            serde_json::from_slice::<serde_json::Value>(&valid_iperf3_json()).unwrap();
        at_bound["end"]["sum_sent"]["bytes"] = serde_json::json!(U64_MAX_AS_F64);
        assert!(
            parse_iperf3_report(&outcome_with_stdout(
                serde_json::to_string(&at_bound).unwrap().as_bytes()
            ))
            .is_ok()
        );
        let above = f64::from_bits(U64_MAX_AS_F64.to_bits() + 1);
        assert!(above > U64_MAX_AS_F64);
        let mut over = serde_json::from_slice::<serde_json::Value>(&valid_iperf3_json()).unwrap();
        over["end"]["sum_sent"]["bytes"] = serde_json::json!(above);
        assert!(
            parse_iperf3_report(&outcome_with_stdout(
                serde_json::to_string(&over).unwrap().as_bytes()
            ))
            .is_err()
        );
        // Negative, NaN, and infinite counters are rejected.
        for bad in [
            serde_json::json!(-1.0),
            serde_json::json!(f64::NAN),
            serde_json::json!(f64::INFINITY),
        ] {
            let mut invalid =
                serde_json::from_slice::<serde_json::Value>(&valid_iperf3_json()).unwrap();
            invalid["end"]["sum_sent"]["bytes"] = bad;
            assert!(
                parse_iperf3_report(&outcome_with_stdout(
                    serde_json::to_string(&invalid).unwrap().as_bytes()
                ))
                .is_err()
            );
        }
    }

    #[test]
    fn debug_redacts_executable_identity() {
        use std::path::PathBuf;
        let workload = Iperf3Workload {
            executable: ResolvedExecutable {
                logical_tool: IPERF3_TOOL.to_owned(),
                selected_path: PathBuf::from("/tmp/iperf3"),
                canonical_path: PathBuf::from("/tmp/iperf3"),
                sha256_hex: "ab".repeat(32),
                file_size: 1,
                executable_class: "test".to_owned(),
            },
            version: Some("3.16".to_owned()),
        };
        let rendered = format!("{workload:?}");
        assert!(rendered.contains(IPERF3_DRIVER_NAME));
        assert!(rendered.contains("3.16"));
        assert!(!rendered.contains("/tmp/iperf3"));
        assert!(!rendered.contains(&"ab".repeat(32)));
    }
}
