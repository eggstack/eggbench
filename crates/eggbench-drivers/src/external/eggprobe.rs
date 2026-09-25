#![allow(clippy::doc_markdown)]
//! Eggprobe pre/post workload diagnostic adapter (Eggstack M003b).
//!
//! Eggprobe owns DNS/TCP/TLS/HTTP diagnostic execution, route-safe machine
//! report semantics, and probe/assertion finding meaning. Eggbench owns when
//! diagnostics execute, target binding resolution, required/optional policy,
//! binary/schema provenance, bounded raw evidence, execution-status effect,
//! and comparison-critical diagnostic configuration.
//!
//! Production seam: trusted external `eggprobe` JSON CLI using the qualified
//! machine contract (schema `0.3`), never a Rust library dependency:
//!
//! ```text
//! eggprobe --version
//! eggprobe run -   # schema-0.3 plan JSON on stdin, ProbeReport JSON on stdout
//! ```
//!
//! Probe timings are diagnostic evidence only. They never enter
//! `TrialMetrics`, never satisfy a `MetricRequest`, and never gate
//! acceptance. Diagnostics run outside every measured workload interval.

use super::command::{ExternalCommandOutcome, ExternalCommandSpec, run_command};
use super::common::{TARGET_HTTP_URL_KEY, authority_host_port, check_min_version, driver_env};
use super::error::{DriverError, ErrorCategory};
use super::resolver::{BinaryResolver, ResolvedExecutable};
use super::version::{ToolVersion, VersionProbe, VersionProbeSpec};
use eggbench_core::{Capability, DiagnosticProbe, DriverCategory, DriverDescriptor, Name};
use eggbench_runner::{
    DiagnosticContext, DiagnosticDisposition, DiagnosticExecutor, DiagnosticOutput, FailureCategory,
};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Canonical diagnostic driver name for the Eggprobe adapter.
pub const EGGPROBE_DRIVER_NAME: &str = "eggprobe";
/// Logical tool name resolved through the trusted substrate.
const EGGPROBE_TOOL: &str = "eggprobe";
/// Versioned parser identifier for `eggprobe run -` JSON output.
pub const EGGPROBE_PARSER_ID: &str = "eggprobe-json/v1";
/// Minimum supported Eggprobe release (qualified v0.1.1 line).
const EGGPROBE_MIN_VERSION: (u64, u64, u64) = (0, 1, 1);
/// Enforced machine-contract schema version (dotted string, not SemVer).
pub const EGGPROBE_MACHINE_SCHEMA: &str = "0.3";
/// Probe families claimed by the M003b adapter (no ICMP/UDP/trace/PMTU).
pub const EGGPROBE_SUPPORTED_FAMILIES: [DiagnosticProbe; 4] = [
    DiagnosticProbe::Dns,
    DiagnosticProbe::Tcp,
    DiagnosticProbe::Tls,
    DiagnosticProbe::Http,
];
/// Probe families explicitly unsupported in M003b.
pub const EGGPROBE_UNSUPPORTED_FAMILIES: [&str; 4] = ["icmp", "udp", "trace", "pmtu"];
/// Run-level evidence artifact name.
pub const DIAGNOSTICS_EVIDENCE: &str = "diagnostics.json";
/// Stdout retention cap (bounded raw ProbeReport).
const STDOUT_LIMIT: u64 = 4 * 1024 * 1024;
/// Stderr retention cap.
const STDERR_LIMIT: u64 = 256 * 1024;
/// Version-probe timeout.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
/// Schema-handshake timeout (separate from per-diagnostic timeouts).
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
/// Report bounds.
const MAX_PROBES: usize = 16;
const MAX_FINDINGS: usize = 4096;
const MAX_WARNINGS: usize = 32;
const MAX_WARNING_LEN: usize = 512;
const MAX_EXECUTION_ID_LEN: usize = 256;
const MAX_STATUS_LEN: usize = 64;
const MAX_FAMILY_LEN: usize = 32;
const MAX_VERSION_LEN: usize = 128;
/// Per-diagnostic timeout cap enforced at execution (10 minutes). The core
/// `DurationMs` type already guarantees nonzero; this tighter cap keeps one
/// diagnostic from stalling lifecycle teardown.
pub const MAX_DIAGNOSTIC_TIMEOUT_MS: u64 = 600_000;

/// Lowered diagnostic target derived from startup-established bindings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoweredTarget {
    /// Target hostname or IP literal from the bound URL.
    pub host: String,
    /// Target port from the bound URL (or scheme default).
    pub port: u16,
    /// Whether TLS semantics were explicitly bound (`https_url`).
    pub use_tls: bool,
    /// Full bound HTTP URL (for the HTTP probe).
    pub http_url: String,
    /// False when the hostname is a literal IP (DNS is not applicable).
    pub dns_applicable: bool,
}

/// Machine-contract handshake record (before managed startup).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandshakeProof {
    /// Observed tool version.
    pub executable_version: String,
    /// SHA-256 of the selected executable.
    pub executable_sha256: String,
    /// Negotiated machine schema (always `0.3` in M003b).
    pub machine_schema: String,
}

/// Parsed ProbeReport outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeReportParsed {
    /// Report-level status label (`pass`, `fail`, or another bounded label).
    pub status: String,
    /// Per-probe `(family, status)` pairs in report order.
    pub probes: Vec<(DiagnosticProbe, String)>,
    /// Bounded report warnings.
    pub warnings: Vec<String>,
    /// Finding count (diagnostic evidence only, never a metric).
    pub finding_count: usize,
}

/// Eggprobe diagnostic executor (one-shot, sibling-neutral seam).
pub struct EggProbeExecutor {
    executable: ResolvedExecutable,
    version: String,
}

impl EggProbeExecutor {
    /// Resolve the `eggprobe` binary through the trusted substrate.
    ///
    /// # Errors
    /// Returns resolution failure when no trusted executable is available.
    pub fn resolve() -> Result<ResolvedExecutable, DriverError> {
        BinaryResolver::resolve(EGGPROBE_TOOL, None, None)
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
                parser_id: EGGPROBE_PARSER_ID.to_owned(),
            },
            cancel,
        )
        .await
    }

    /// Bind a probed binary, enforcing the version floor.
    ///
    /// # Errors
    /// Returns `unsupported_version` below 0.1.1.
    pub fn new(executable: ResolvedExecutable, version: String) -> Result<Self, DriverError> {
        check_min_version(EGGPROBE_TOOL, &version, EGGPROBE_MIN_VERSION)?;
        Ok(Self {
            executable,
            version,
        })
    }

    /// Bind a resolved binary without a probe; first execution probes once.
    #[must_use]
    pub fn from_resolved(executable: ResolvedExecutable, version: String) -> Self {
        Self {
            executable,
            version,
        }
    }

    /// Pinned tool version.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Executable SHA-256.
    #[must_use]
    pub fn executable_sha256(&self) -> &str {
        &self.executable.sha256_hex
    }
}

impl std::fmt::Debug for EggProbeExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EggProbeExecutor")
            .field("driver", &EGGPROBE_DRIVER_NAME)
            .field("version", &self.version)
            .finish_non_exhaustive()
    }
}

impl DiagnosticExecutor for EggProbeExecutor {
    fn source(&self) -> &str {
        EGGPROBE_DRIVER_NAME
    }

    fn execute<'a>(
        &'a mut self,
        context: DiagnosticContext,
    ) -> Pin<Box<dyn Future<Output = Result<DiagnosticOutput, FailureCategory>> + Send + 'a>> {
        Box::pin(async move { execute_probe(&self.executable, &self.version, context).await })
    }
}

async fn execute_probe(
    executable: &ResolvedExecutable,
    version: &str,
    context: DiagnosticContext,
) -> Result<DiagnosticOutput, FailureCategory> {
    let failed_output = |report_status: String, warnings: Vec<String>| DiagnosticOutput {
        raw_report: Vec::new(),
        disposition: DiagnosticDisposition::Failed,
        producer: EGGPROBE_DRIVER_NAME.to_owned(),
        producer_version: version.to_owned(),
        executable_sha256: executable.sha256_hex.clone(),
        machine_schema: EGGPROBE_MACHINE_SCHEMA.to_owned(),
        report_status,
        probe_statuses: Vec::new(),
        warnings,
        skipped_reason: None,
    };
    if context.timeout.as_millis() > u128::from(MAX_DIAGNOSTIC_TIMEOUT_MS) {
        return Ok(failed_output(
            "timeout_policy".to_owned(),
            vec!["diagnostic timeout exceeds M003b bound".to_owned()],
        ));
    }
    let Ok(lowered) = lower_target(&context) else {
        return Err(FailureCategory::DiagnosticFailed);
    };
    let (plan_value, effective_probes, lowering_warnings) = build_probe_plan(&context, &lowered);
    // Defensive: generated plans are <2 KiB; the substrate enforces 64 KiB.
    let plan_bytes =
        serde_json::to_vec(&plan_value).map_err(|_| FailureCategory::DiagnosticFailed)?;
    let spec = ExternalCommandSpec {
        executable: executable.clone(),
        args: vec!["run".into(), "-".into()],
        cwd: None,
        env: driver_env(),
        stdin_null: false,
        stdin_bytes: Some(plan_bytes),
        stdout_limit: STDOUT_LIMIT,
        stderr_limit: STDERR_LIMIT,
        timeout: context.timeout,
    };
    let outcome = match run_command(&spec, &context.cancellation).await {
        Ok(outcome) => outcome,
        Err(error) => match error.category() {
            ErrorCategory::Cancelled => return Err(FailureCategory::Cancelled),
            ErrorCategory::TimedOut => return Err(FailureCategory::TimedOut),
            _ => {
                return Ok(failed_output(
                    "execution_failed".to_owned(),
                    vec![format!("eggprobe execution failed: {}", error.category())],
                ));
            }
        },
    };
    if outcome.cancelled {
        return Err(FailureCategory::Cancelled);
    }
    if outcome.timed_out {
        return Err(FailureCategory::TimedOut);
    }
    let raw_report: Vec<u8> = outcome.stdout.retained().to_vec();
    let parsed = match parse_probe_report(&raw_report, &effective_probes) {
        Ok(parsed) => parsed,
        Err(detail) => {
            return Ok(parse_failure_output(
                executable, version, raw_report, &outcome, detail,
            ));
        }
    };
    let disposition = match classify_probe_exit(outcome.exit_code, Some(&parsed)) {
        "positive" => DiagnosticDisposition::Positive,
        "negative" => DiagnosticDisposition::Negative,
        "cancelled" => return Err(FailureCategory::Cancelled),
        _ => DiagnosticDisposition::Failed,
    };
    let mut warnings = lowering_warnings;
    warnings.extend(parsed.warnings.clone());
    let mut probe_statuses: Vec<(String, String)> = parsed
        .probes
        .iter()
        .map(|(family, status)| (probe_family_name(*family).to_owned(), status.clone()))
        .collect();
    for skipped in skipped_probe_labels(&context, &lowered) {
        probe_statuses.push(skipped);
    }
    Ok(DiagnosticOutput {
        raw_report,
        disposition,
        producer: EGGPROBE_DRIVER_NAME.to_owned(),
        producer_version: version.to_owned(),
        executable_sha256: executable.sha256_hex.clone(),
        machine_schema: EGGPROBE_MACHINE_SCHEMA.to_owned(),
        report_status: parsed.status.clone(),
        probe_statuses,
        warnings,
        skipped_reason: None,
    })
}

/// `DiagnosticOutput` for a valid process outcome whose report failed to
/// parse: the bounded raw bytes are preserved as evidence.
fn parse_failure_output(
    executable: &ResolvedExecutable,
    version: &str,
    raw_report: Vec<u8>,
    outcome: &ExternalCommandOutcome,
    detail: String,
) -> DiagnosticOutput {
    DiagnosticOutput {
        raw_report,
        disposition: DiagnosticDisposition::Failed,
        producer: EGGPROBE_DRIVER_NAME.to_owned(),
        producer_version: version.to_owned(),
        executable_sha256: executable.sha256_hex.clone(),
        machine_schema: EGGPROBE_MACHINE_SCHEMA.to_owned(),
        report_status: classify_probe_exit(outcome.exit_code, None).to_owned(),
        probe_statuses: Vec::new(),
        warnings: vec![detail],
        skipped_reason: None,
    }
}

/// Lower the diagnostic target from startup-established runtime bindings.
///
/// Requires the target service's `http_url` binding. TCP derives host/port
/// from that URL; DNS uses its hostname unless it is a literal IP (then DNS
/// is deterministically not applicable); TLS requires an explicit `https_url`
/// binding and is never inferred from the port number.
///
/// # Errors
/// Returns a redaction-safe reason when no bound URL exists, the URL has no
/// usable authority, or a required TLS probe has no HTTPS-capable binding.
pub fn lower_target(context: &DiagnosticContext) -> Result<LoweredTarget, String> {
    let bindings = context
        .bindings
        .service_bindings(&context.target)
        .ok_or_else(|| {
            format!(
                "diagnostic target {} has no runtime bindings",
                context.target
            )
        })?;
    let http_url = bindings.get(TARGET_HTTP_URL_KEY).cloned().ok_or_else(|| {
        format!(
            "diagnostic target {} publishes no {TARGET_HTTP_URL_KEY} binding",
            context.target
        )
    })?;
    require_loopback_url(&http_url)?;
    let default_port = if http_url.starts_with("https://") {
        443
    } else {
        80
    };
    let (host, port) = authority_host_port(&http_url, default_port)
        .map_err(|detail| format!("diagnostic target URL is not usable: {detail}"))?;
    let wants_tls = context.probes.contains(&DiagnosticProbe::Tls);
    let tls_url = bindings.get("https_url").cloned();
    if wants_tls {
        match tls_url {
            Some(url) => {
                require_loopback_url(&url)?;
                let (tls_host, tls_port) = authority_host_port(&url, 443)
                    .map_err(|detail| format!("diagnostic TLS URL is not usable: {detail}"))?;
                return Ok(LoweredTarget {
                    host: tls_host,
                    port: tls_port,
                    use_tls: true,
                    http_url,
                    dns_applicable: !is_literal_ip(&host),
                });
            }
            None => {
                if context.required {
                    return Err(format!(
                        "diagnostic {} requires TLS but target {} publishes no https_url binding",
                        context.diagnostic_id, context.target
                    ));
                }
                // Optional TLS without an HTTPS binding is recorded as
                // unavailable; remaining probes still execute.
            }
        }
    }
    Ok(LoweredTarget {
        host: host.clone(),
        port,
        use_tls: false,
        http_url,
        dns_applicable: !is_literal_ip(&host),
    })
}

/// Probes actually sent to Eggprobe after deterministic skip rules.
fn effective_probes(context: &DiagnosticContext, lowered: &LoweredTarget) -> Vec<DiagnosticProbe> {
    let mut out = Vec::new();
    for probe in &context.probes {
        match probe {
            DiagnosticProbe::Dns if !lowered.dns_applicable => {}
            DiagnosticProbe::Tls
                if context
                    .bindings
                    .service_bindings(&context.target)
                    .and_then(|bindings| bindings.get("https_url"))
                    .is_none() => {}
            _ => out.push(*probe),
        }
    }
    out
}

/// Deterministic skip labels for probes withheld from the generated plan.
fn skipped_probe_labels(
    context: &DiagnosticContext,
    lowered: &LoweredTarget,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if context.probes.contains(&DiagnosticProbe::Dns) && !lowered.dns_applicable {
        out.push(("dns".to_owned(), "not_applicable".to_owned()));
    }
    if context.probes.contains(&DiagnosticProbe::Tls)
        && context
            .bindings
            .service_bindings(&context.target)
            .and_then(|bindings| bindings.get("https_url"))
            .is_none()
    {
        out.push(("tls".to_owned(), "unavailable".to_owned()));
    }
    out
}

/// Build the deterministic schema-0.3 Eggprobe plan for one execution.
///
/// Returns the plan value, the effective (post-skip) probe set, and
/// lowering warnings. The plan carries no credentials: only the bound host,
/// port, TLS flag, and HTTP URL cross into the generated JSON.
fn build_probe_plan(
    context: &DiagnosticContext,
    lowered: &LoweredTarget,
) -> (serde_json::Value, Vec<DiagnosticProbe>, Vec<String>) {
    let effective = effective_probes(context, lowered);
    let mut warnings = Vec::new();
    if effective.len() != context.probes.len() {
        for (family, label) in skipped_probe_labels(context, lowered) {
            warnings.push(format!(
                "probe {family} {label}: withheld from generated plan"
            ));
        }
    }
    let probes: Vec<&str> = effective
        .iter()
        .map(|probe| probe_family_name(*probe))
        .collect();
    let deadline_ms =
        u64::try_from(context.timeout.as_millis().min(u128::from(u64::MAX))).unwrap_or(u64::MAX);
    let plan = serde_json::json!({
        "schema_version": EGGPROBE_MACHINE_SCHEMA,
        "target": {
            "host": lowered.host,
            "port": lowered.port,
            "tls": lowered.use_tls,
            "http_url": lowered.http_url,
        },
        "route": {"kind": "direct"},
        "probes": probes,
        "execution": {"repetitions": 1, "retries": 0, "deadline_ms": deadline_ms},
        "assertions": [],
    });
    (plan, effective, warnings)
}

/// Canonical family label (matches the core schema serialization).
fn probe_family_name(probe: DiagnosticProbe) -> &'static str {
    match probe {
        DiagnosticProbe::Dns => "dns",
        DiagnosticProbe::Tcp => "tcp",
        DiagnosticProbe::Tls => "tls",
        DiagnosticProbe::Http => "http",
    }
}

fn parse_probe_family(name: &str) -> Option<DiagnosticProbe> {
    match name {
        "dns" => Some(DiagnosticProbe::Dns),
        "tcp" => Some(DiagnosticProbe::Tcp),
        "tls" => Some(DiagnosticProbe::Tls),
        "http" => Some(DiagnosticProbe::Http),
        _ => None,
    }
}

fn is_literal_ip(host: &str) -> bool {
    host.parse::<std::net::IpAddr>().is_ok()
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

/// Classify an `eggprobe run -` exit code given the parsed report.
///
/// - `0`: valid report; disposition follows the report status.
/// - `1`: valid negative diagnostic outcome, never a process failure.
/// - `2`: invalid generated plan/invocation (adapter/contract error).
/// - `3`: Eggprobe internal failure (operational failure).
/// - `130`: cancellation.
/// - anything else: unsupported/malformed execution.
fn classify_probe_exit(exit_code: Option<i32>, parsed: Option<&ProbeReportParsed>) -> &'static str {
    match exit_code {
        Some(0) => match parsed.map(|report| report.status.as_str()) {
            Some("pass") => "positive",
            Some("fail") => "negative",
            _ => "failed",
        },
        Some(1) => {
            if parsed.is_some() {
                "negative"
            } else {
                "failed"
            }
        }
        Some(130) => "cancelled",
        _ => "failed",
    }
}

#[derive(Debug, Deserialize)]
struct ProbeReportWire {
    schema_version: Option<serde_json::Value>,
    tool: Option<ProbeToolWire>,
    execution_id: Option<serde_json::Value>,
    target: Option<serde_json::Value>,
    route: Option<ProbeRouteWire>,
    status: Option<serde_json::Value>,
    #[serde(default)]
    probes: Option<Vec<ProbeEntryWire>>,
    #[serde(default)]
    findings: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    warnings: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Deserialize)]
struct ProbeToolWire {
    name: Option<String>,
    version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProbeRouteWire {
    kind: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProbeEntryWire {
    family: Option<String>,
    status: Option<String>,
}

/// Parse and validate a schema-0.3 `ProbeReport`.
///
/// Accepts only schema `0.3`: a binary emitting schema `0.4` is incompatible
/// even when its version string is still `0.1.1`. Every reported probe must
/// correspond to a requested family, and every requested family must be
/// reported exactly once.
///
/// # Errors
/// Returns a redaction-safe reason on malformed JSON, schema mismatch,
/// bound overflow, or probe-set mismatch.
pub fn parse_probe_report(
    raw: &[u8],
    expected: &[DiagnosticProbe],
) -> Result<ProbeReportParsed, String> {
    if raw.len() as u64 > STDOUT_LIMIT {
        return Err("eggprobe report exceeds bound".to_owned());
    }
    let report: ProbeReportWire =
        serde_json::from_slice(raw).map_err(|e| format!("invalid eggprobe report JSON: {e}"))?;
    match report.schema_version {
        Some(serde_json::Value::String(version)) if version == EGGPROBE_MACHINE_SCHEMA => {}
        Some(serde_json::Value::String(version)) => {
            return Err(format!(
                "unsupported eggprobe machine schema {version:?} (M003b requires 0.3)"
            ));
        }
        _ => return Err("eggprobe report schema_version must be \"0.3\"".to_owned()),
    }
    let tool = report
        .tool
        .ok_or_else(|| "eggprobe report tool provenance missing".to_owned())?;
    if tool.name.as_deref() != Some(EGGPROBE_TOOL) {
        return Err("eggprobe report producer is not eggprobe".to_owned());
    }
    let version = tool.version.unwrap_or_default();
    if version.is_empty() || version.len() > MAX_VERSION_LEN {
        return Err("eggprobe report tool version is missing or unbounded".to_owned());
    }
    let execution_id = match report.execution_id {
        Some(serde_json::Value::String(id))
            if !id.is_empty() && id.len() <= MAX_EXECUTION_ID_LEN =>
        {
            id
        }
        _ => return Err("eggprobe report execution_id is missing or unbounded".to_owned()),
    };
    let _ = execution_id;
    if !report.target.is_some_and(|target| target.is_object()) {
        return Err("eggprobe report target summary missing".to_owned());
    }
    match report.route.and_then(|route| route.kind) {
        Some(kind) if kind == "direct" => {}
        _ => return Err("eggprobe report route must be direct in M003b".to_owned()),
    }
    let status = match report.status {
        Some(serde_json::Value::String(status))
            if !status.is_empty() && status.len() <= MAX_STATUS_LEN =>
        {
            status
        }
        _ => return Err("eggprobe report status is missing or unbounded".to_owned()),
    };
    let entries = report.probes.unwrap_or_default();
    if entries.len() > MAX_PROBES || entries.len() > expected.len() {
        return Err("eggprobe report probe count exceeds bound".to_owned());
    }
    let probes = validate_report_probes(&entries, expected)?;
    let findings = report.findings.unwrap_or_default();
    if findings.len() > MAX_FINDINGS {
        return Err("eggprobe report findings exceed bound".to_owned());
    }
    let raw_warnings = report.warnings.unwrap_or_default();
    if raw_warnings.len() > MAX_WARNINGS {
        return Err("eggprobe report warnings exceed bound".to_owned());
    }
    let mut warnings = Vec::with_capacity(raw_warnings.len());
    for warning in &raw_warnings {
        let text = match warning {
            serde_json::Value::String(text) => text.clone(),
            other => other.to_string(),
        };
        if text.len() > MAX_WARNING_LEN {
            return Err("eggprobe report warning too long".to_owned());
        }
        warnings.push(text);
    }
    Ok(ProbeReportParsed {
        status,
        probes,
        warnings,
        finding_count: findings.len(),
    })
}

fn family_name_len(entry: &ProbeEntryWire) -> usize {
    entry.family.as_ref().map_or(0, String::len)
}

/// Validate reported probe entries against the requested families: every
/// reported probe must be requested, every requested family must be
/// reported exactly once, and all labels stay bounded.
fn validate_report_probes(
    entries: &[ProbeEntryWire],
    expected: &[DiagnosticProbe],
) -> Result<Vec<(DiagnosticProbe, String)>, String> {
    let mut probes = Vec::with_capacity(entries.len());
    let mut seen = BTreeSet::new();
    for entry in entries {
        let family = entry
            .family
            .as_deref()
            .and_then(parse_probe_family)
            .ok_or_else(|| "eggprobe report probe family is unknown or unbounded".to_owned())?;
        if family_name_len(entry) > MAX_FAMILY_LEN {
            return Err("eggprobe report probe family is unbounded".to_owned());
        }
        if !expected.contains(&family) {
            return Err("eggprobe report probe was not requested".to_owned());
        }
        if !seen.insert(family) {
            return Err("eggprobe report repeats a probe family".to_owned());
        }
        let probe_status = entry.status.clone().unwrap_or_default();
        if probe_status.is_empty() || probe_status.len() > MAX_STATUS_LEN {
            return Err("eggprobe report probe status is missing or unbounded".to_owned());
        }
        probes.push((family, probe_status));
    }
    for required in expected {
        if !seen.contains(required) {
            return Err(format!(
                "eggprobe report omits requested probe {}",
                probe_family_name(*required)
            ));
        }
    }
    Ok(probes)
}

/// Run the schema-compatibility handshake before managed startup.
///
/// Delivers a minimal schema-0.3 plan on stdin (`eggprobe run -`) using
/// loopback with route direct and an empty probe list. When the qualified
/// schema rejects empty probe lists, one bounded no-external-network DNS
/// probe for `localhost` is used instead. Requires exit 0, a valid
/// schema-0.3 report, `eggprobe` provenance, and a direct route summary.
///
/// A binary emitting schema `0.4` is incompatible even when its version
/// string is still `0.1.1`.
///
/// # Errors
/// Returns `diagnostic_contract_unsupported` detail on schema mismatch and
/// other typed failures otherwise.
pub async fn handshake_eggprobe(
    executable: &ResolvedExecutable,
    cancel: &CancellationToken,
) -> Result<HandshakeProof, DriverError> {
    let empty_plan = serde_json::json!({
        "schema_version": EGGPROBE_MACHINE_SCHEMA,
        "target": {"host": "127.0.0.1", "port": 80, "tls": false},
        "route": {"kind": "direct"},
        "probes": [],
        "execution": {"repetitions": 1, "retries": 0, "deadline_ms": 5_000},
        "assertions": [],
    });
    match try_handshake_plan(executable, &empty_plan, &[], cancel).await {
        Ok(proof) => return Ok(proof),
        Err(error) if is_handshake_exit_2(&error) => {}
        Err(error) => return Err(error),
    }
    // The qualified schema may require at least one probe; retry once with
    // a bounded no-external-network DNS probe for localhost.
    let dns_plan = serde_json::json!({
        "schema_version": EGGPROBE_MACHINE_SCHEMA,
        "target": {"host": "localhost", "port": 80, "tls": false},
        "route": {"kind": "direct"},
        "probes": ["dns"],
        "execution": {"repetitions": 1, "retries": 0, "deadline_ms": 5_000},
        "assertions": [],
    });
    try_handshake_plan(executable, &dns_plan, &[DiagnosticProbe::Dns], cancel).await
}

/// True when a handshake attempt failed specifically with exit code 2
/// (invalid plan), licensing the single-probe retry.
fn is_handshake_exit_2(error: &DriverError) -> bool {
    error.to_string().contains("exited with Some(2)")
}

async fn try_handshake_plan(
    executable: &ResolvedExecutable,
    plan: &serde_json::Value,
    expected: &[DiagnosticProbe],
    cancel: &CancellationToken,
) -> Result<HandshakeProof, DriverError> {
    let contract =
        |detail: String| DriverError::execution(ErrorCategory::UnsupportedOption, detail);
    let outcome = run_handshake_plan(executable, plan, cancel).await?;
    if outcome.exit_code != Some(0) {
        return Err(contract(format!(
            "diagnostic_contract_unsupported: eggprobe handshake exited with {:?}",
            outcome.exit_code
        )));
    }
    let parsed = parse_probe_report(outcome.stdout.retained(), expected)
        .map_err(|detail| contract(format!("diagnostic_contract_unsupported: {detail}")))?;
    if parsed.status != "pass" {
        return Err(contract(
            "diagnostic_contract_unsupported: handshake report did not pass".to_owned(),
        ));
    }
    Ok(HandshakeProof {
        executable_version: String::new(),
        executable_sha256: executable.sha256_hex.clone(),
        machine_schema: EGGPROBE_MACHINE_SCHEMA.to_owned(),
    })
}

async fn run_handshake_plan(
    executable: &ResolvedExecutable,
    plan: &serde_json::Value,
    cancel: &CancellationToken,
) -> Result<ExternalCommandOutcome, DriverError> {
    let stdin_bytes = serde_json::to_vec(plan)
        .map_err(|e| DriverError::parse(ErrorCategory::ParseFailed, e.to_string()))?;
    let spec = ExternalCommandSpec {
        executable: executable.clone(),
        args: vec!["run".into(), "-".into()],
        cwd: None,
        env: driver_env(),
        stdin_null: false,
        stdin_bytes: Some(stdin_bytes),
        stdout_limit: STDOUT_LIMIT,
        stderr_limit: STDERR_LIMIT,
        timeout: HANDSHAKE_TIMEOUT,
    };
    run_command(&spec, cancel).await
}

/// Preflight helper for CLI `run`: resolve, version-probe, and handshake.
///
/// # Errors
/// Returns resolution, probe, version-floor, or contract failures. All fail
/// before managed startup.
pub async fn preflight_eggprobe(
    cancel: &CancellationToken,
) -> Result<(ResolvedExecutable, ToolVersion, HandshakeProof), DriverError> {
    let executable = EggProbeExecutor::resolve()?;
    let probed = EggProbeExecutor::probe(&executable, cancel).await?;
    EggProbeExecutor::new(executable.clone(), probed.version.clone())?;
    let mut proof = handshake_eggprobe(&executable, cancel).await?;
    proof.executable_version.clone_from(&probed.version);
    Ok((executable, probed, proof))
}

/// Production descriptor for the Eggprobe diagnostic driver.
///
/// # Panics
/// Never panics at runtime; the static names are valid by construction.
#[must_use]
pub fn eggprobe_descriptor() -> DriverDescriptor {
    let mut capabilities = BTreeSet::new();
    capabilities.insert(Capability::ExternalBinary);
    for probe in EGGPROBE_SUPPORTED_FAMILIES {
        capabilities.insert(Capability::DiagnosticProbe { probe });
    }
    DriverDescriptor {
        name: Name::new(EGGPROBE_DRIVER_NAME).expect("static driver name"),
        adapter_version: env!("CARGO_PKG_VERSION").to_owned(),
        upstream_name: EGGPROBE_TOOL.to_owned(),
        upstream_version: None,
        category: DriverCategory::Diagnostic,
        capabilities,
        supported_platforms: BTreeSet::new(),
        // The M003b machine contract is the dotted string "0.3", which has
        // no SchemaVersion(u32) representation; compatibility is enforced in
        // code by parse_probe_report/handshake_eggprobe instead.
        machine_output_schema: None,
        external_process: true,
        default: false,
        compatible_service_types: BTreeSet::new(),
    }
}

/// Manifest role label for `diagnostics.json`.
///
/// # Panics
/// Never panics at runtime; the static label is valid by construction.
#[must_use]
pub fn eggprobe_role_label() -> Name {
    Name::new("diagnostics").expect("static role label")
}

/// Diagnostic timings are evidence-only labels for CLI/docs surfaces.
#[must_use]
pub const fn diagnostic_timing_label() -> &'static str {
    "diagnostic timing (not a benchmark metric)"
}

/// Canonical labels of the probe families claimed by the M003b adapter.
#[must_use]
pub const fn eggprobe_supported_family_names() -> [&'static str; 4] {
    ["dns", "tcp", "tls", "http"]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::external::command::CapturedStream;
    use eggbench_runner::RuntimeBindings;
    use std::path::PathBuf;
    use std::time::Duration;

    fn test_executable() -> ResolvedExecutable {
        ResolvedExecutable {
            logical_tool: EGGPROBE_TOOL.to_owned(),
            selected_path: PathBuf::from("/tmp/eggprobe"),
            canonical_path: PathBuf::from("/tmp/eggprobe"),
            sha256_hex: "ab".repeat(32),
            file_size: 1,
            executable_class: "test".to_owned(),
        }
    }

    fn bindings_with_url(url: &str) -> RuntimeBindings {
        let mut bindings = RuntimeBindings::new();
        bindings
            .insert("origin", TARGET_HTTP_URL_KEY, url.to_owned())
            .expect("test binding");
        bindings
    }

    fn diagnostic_context(probes: Vec<DiagnosticProbe>, required: bool) -> DiagnosticContext {
        DiagnosticContext {
            run_id: eggbench_core::RunId::new(),
            diagnostic_id: "pre".to_owned(),
            phase: eggbench_core::DiagnosticPhase::PreWorkload,
            target: "origin".to_owned(),
            probes,
            required,
            bindings: bindings_with_url("http://localhost:18321/bench"),
            cancellation: CancellationToken::new(),
            timeout: Duration::from_secs(5),
        }
    }

    fn valid_report_json() -> Vec<u8> {
        serde_json::json!({
            "schema_version": "0.3",
            "tool": {"name": "eggprobe", "version": "0.1.1"},
            "execution_id": "exec-1",
            "target": {"summary": "127.0.0.1:18321"},
            "route": {"kind": "direct"},
            "status": "pass",
            "probes": [
                {"family": "dns", "status": "pass"},
                {"family": "tcp", "status": "pass"},
            ],
            "findings": [],
            "warnings": [],
        })
        .to_string()
        .into_bytes()
    }

    #[test]
    fn descriptor_claims_only_m003b_families() {
        let descriptor = eggprobe_descriptor();
        assert_eq!(descriptor.name.as_str(), EGGPROBE_DRIVER_NAME);
        assert_eq!(descriptor.category, DriverCategory::Diagnostic);
        assert!(descriptor.external_process);
        assert!(!descriptor.default);
        for probe in EGGPROBE_SUPPORTED_FAMILIES {
            assert!(
                descriptor
                    .capabilities
                    .contains(&Capability::DiagnosticProbe { probe })
            );
        }
        assert_eq!(
            descriptor
                .capabilities
                .iter()
                .filter(|capability| matches!(capability, Capability::DiagnosticProbe { .. }))
                .count(),
            4
        );
    }

    #[test]
    fn report_parser_accepts_schema_03_contract() {
        let parsed = parse_probe_report(
            &valid_report_json(),
            &[DiagnosticProbe::Dns, DiagnosticProbe::Tcp],
        )
        .expect("valid report parses");
        assert_eq!(parsed.status, "pass");
        assert_eq!(parsed.probes.len(), 2);
        assert_eq!(parsed.finding_count, 0);
    }

    #[test]
    fn report_parser_rejects_schema_04_despite_matching_semver() {
        let mut value = serde_json::from_slice::<serde_json::Value>(&valid_report_json()).unwrap();
        value["schema_version"] = serde_json::json!("0.4");
        let bytes = serde_json::to_string(&value).unwrap().into_bytes();
        let error = parse_probe_report(&bytes, &[DiagnosticProbe::Dns, DiagnosticProbe::Tcp])
            .expect_err("schema 0.4 must fail");
        assert!(error.contains("0.4") || error.contains("0.3"), "{error}");
    }

    #[test]
    fn report_parser_rejects_malformed_and_mismatched_reports() {
        // Malformed JSON.
        assert!(parse_probe_report(b"{not json", &[DiagnosticProbe::Dns]).is_err());
        // Wrong producer.
        let mut value = serde_json::from_slice::<serde_json::Value>(&valid_report_json()).unwrap();
        value["tool"]["name"] = serde_json::json!("other");
        let bytes = serde_json::to_string(&value).unwrap().into_bytes();
        assert!(parse_probe_report(&bytes, &[DiagnosticProbe::Dns, DiagnosticProbe::Tcp]).is_err());
        // Non-direct route.
        let mut value = serde_json::from_slice::<serde_json::Value>(&valid_report_json()).unwrap();
        value["route"] = serde_json::json!({"kind": "proxy"});
        let bytes = serde_json::to_string(&value).unwrap().into_bytes();
        assert!(parse_probe_report(&bytes, &[DiagnosticProbe::Dns, DiagnosticProbe::Tcp]).is_err());
        // Unrequested probe family.
        let mut value = serde_json::from_slice::<serde_json::Value>(&valid_report_json()).unwrap();
        value["probes"] = serde_json::json!([{"family": "http", "status": "pass"}]);
        let bytes = serde_json::to_string(&value).unwrap().into_bytes();
        assert!(parse_probe_report(&bytes, &[DiagnosticProbe::Dns]).is_err());
        // Missing requested probe.
        assert!(
            parse_probe_report(
                &valid_report_json(),
                &[
                    DiagnosticProbe::Dns,
                    DiagnosticProbe::Tcp,
                    DiagnosticProbe::Http
                ]
            )
            .is_err()
        );
    }

    #[test]
    fn exit_code_mapping_treats_one_as_negative_evidence() {
        let parsed = parse_probe_report(
            &valid_report_json(),
            &[DiagnosticProbe::Dns, DiagnosticProbe::Tcp],
        )
        .unwrap();
        assert_eq!(classify_probe_exit(Some(0), Some(&parsed)), "positive");
        assert_eq!(classify_probe_exit(Some(1), Some(&parsed)), "negative");
        assert_eq!(classify_probe_exit(Some(2), Some(&parsed)), "failed");
        assert_eq!(classify_probe_exit(Some(3), Some(&parsed)), "failed");
        assert_eq!(classify_probe_exit(Some(130), Some(&parsed)), "cancelled");
        assert_eq!(classify_probe_exit(Some(99), Some(&parsed)), "failed");
        assert_eq!(classify_probe_exit(None, Some(&parsed)), "failed");
        // Exit 1 without a parseable report is an operational failure.
        assert_eq!(classify_probe_exit(Some(1), None), "failed");
    }

    #[test]
    fn target_lowering_covers_http_tcp_dns_and_tls_policy() {
        let context = diagnostic_context(
            vec![
                DiagnosticProbe::Http,
                DiagnosticProbe::Tcp,
                DiagnosticProbe::Dns,
            ],
            true,
        );
        let lowered = lower_target(&context).expect("http/tcp/dns lower");
        assert_eq!(lowered.host, "localhost");
        assert_eq!(lowered.port, 18321);
        assert!(!lowered.use_tls);
        assert!(lowered.dns_applicable);

        // Literal-IP targets make DNS deterministically not applicable.
        let mut ip_context = diagnostic_context(vec![DiagnosticProbe::Dns], true);
        ip_context.bindings = bindings_with_url("http://127.0.0.1:80/");
        let lowered = lower_target(&ip_context).expect("literal ip lowers");
        assert!(!lowered.dns_applicable);
        assert!(effective_probes(&ip_context, &lowered).is_empty());

        // TLS requires an explicit https_url binding; required fails closed.
        let tls_context = diagnostic_context(vec![DiagnosticProbe::Tls], true);
        assert!(lower_target(&tls_context).is_err());
        // Optional TLS without https_url records unavailable, not failure.
        let optional_tls = diagnostic_context(vec![DiagnosticProbe::Tls], false);
        let lowered = lower_target(&optional_tls).expect("optional tls lowers");
        assert!(effective_probes(&optional_tls, &lowered).is_empty());
        assert_eq!(
            skipped_probe_labels(&optional_tls, &lowered),
            vec![("tls".to_owned(), "unavailable".to_owned())]
        );
    }

    #[test]
    fn generated_plan_is_direct_bounded_and_credential_free() {
        let mut context = diagnostic_context(
            vec![
                DiagnosticProbe::Dns,
                DiagnosticProbe::Tcp,
                DiagnosticProbe::Http,
            ],
            true,
        );
        // A secret-like value in an unrelated binding must never cross into
        // the generated plan because only host/port/URL are copied.
        context
            .bindings
            .insert("origin", "note", "bearer-super-secret-value".to_owned())
            .expect("test binding");
        let lowered = lower_target(&context).expect("lowers");
        let (plan, effective, warnings) = build_probe_plan(&context, &lowered);
        let text = serde_json::to_string(&plan).unwrap();
        assert!(!text.contains("bearer-super-secret-value"), "{text}");
        assert!(!text.contains("note"), "{text}");
        assert_eq!(plan["schema_version"], serde_json::json!("0.3"));
        assert_eq!(plan["route"]["kind"], serde_json::json!("direct"));
        assert_eq!(plan["assertions"], serde_json::json!([]));
        assert_eq!(plan["execution"]["repetitions"], serde_json::json!(1));
        assert_eq!(plan["execution"]["retries"], serde_json::json!(0));
        assert_eq!(effective.len(), 3);
        assert!(warnings.is_empty());
        assert!(text.len() as u64 <= super::super::command::MAX_STDIN_BYTES);
    }

    #[test]
    fn debug_redacts_executable_identity() {
        let executor = EggProbeExecutor::from_resolved(test_executable(), "0.1.1".to_owned());
        let rendered = format!("{executor:?}");
        assert!(rendered.contains(EGGPROBE_DRIVER_NAME));
        assert!(!rendered.contains("/tmp/eggprobe"));
        assert!(!rendered.contains(&"ab".repeat(32)));
    }

    #[test]
    fn outcome_helper_builds_bounded_streams() {
        let total = valid_report_json().len() as u64;
        let stream = CapturedStream::collect(valid_report_json(), total, total.max(1));
        assert!(!stream.truncated());
        assert_eq!(stream.retained_bytes(), total);
    }
}
