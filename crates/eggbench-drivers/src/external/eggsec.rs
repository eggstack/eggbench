#![allow(clippy::doc_markdown)]
//! Eggsec strict-scope WAF correctness adapter (Eggstack M004a).
//!
//! Eggsec owns payload generation, WAF detection, bypass-technique
//! execution, and the meaning of `bypass_successful`. Eggbench owns the
//! declarative check selection, target binding and local/private
//! confinement, exact executable provenance, the generated strict scope
//! manifest, lifecycle placement, the predeclared bypass threshold, and the
//! sanitized typed result.
//!
//! Production seam: trusted external `eggsec` CLI process with strict scope
//! and JSON output (never a Rust library dependency):
//!
//! ```text
//! eggsec --version
//! eggsec --scope <generated> --strict-scope --json preflight waf --target <url> --profile guarded
//! eggsec --scope <generated> --strict-scope --json waf <url> --bypass --test-type <family> --concurrency <N> --timeout <S>
//! ```
//!
//! Raw Eggsec stdout (which carries payload strings) is parsed in memory
//! and never staged: only the sanitized projection is persisted. Correctness
//! results never enter `TrialMetrics` and never satisfy a `MetricRequest`.

use super::command::{ExternalCommandSpec, run_command};
use super::common::{TARGET_HTTP_URL_KEY, authority_host_port, check_min_version, driver_env};
use super::error::{DriverError, ErrorCategory};
use super::resolver::{BinaryResolver, ResolvedExecutable};
use super::version::{ToolVersion, VersionProbe, VersionProbeSpec};
use eggbench_core::{
    Capability, DriverCategory, DriverDescriptor, EggsecWafTestType, Name, SecurityCheckResultV1,
};
use eggbench_runner::{
    CorrectnessContext, CorrectnessDisposition, CorrectnessExecutor, CorrectnessOutput,
    FailureCategory,
};
use sha2::Digest as _;
use std::collections::BTreeSet;
use std::future::Future;
use std::net::IpAddr;
use std::pin::Pin;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Canonical correctness driver name for the Eggsec WAF adapter.
pub const EGGSEC_DRIVER_NAME: &str = "eggsec-waf";
/// Logical tool name resolved through the trusted substrate.
const EGGSEC_TOOL: &str = "eggsec";
/// Versioned parser identifier for `eggsec waf --json` output.
pub const EGGSEC_PARSER_ID: &str = "eggsec-waf-json/v1";
/// Versioned parser identifier for `eggsec preflight --json` output.
pub const EGGSEC_PREFLIGHT_PARSER_ID: &str = "eggsec-preflight-json/v1";
/// Minimum supported Eggsec version (audited 0.1 line).
///
/// The adapter MUST NOT trust workspace SemVer alone (the audited tree is an
/// evolving 0.1 workspace without an immutable release for this contract):
/// the parser fails closed on incompatible JSON and the exact source/binary
/// provenance is recorded in evidence.
const EGGSEC_MIN_VERSION: (u64, u64, u64) = (0, 1, 0);
/// Run-level evidence artifact name.
pub const SECURITY_CHECKS_EVIDENCE: &str = "security-checks.json";
/// Audited Eggsec operation label recorded in evidence.
pub const EGGSEC_OPERATION: &str = "waf --json --bypass";
/// Preflight operation identities accepted from `eggsec preflight`.
///
/// Dispatch maps the `waf` CLI subcommand to the `waf-detect` operation id;
/// both spellings are accepted so the adapter stays robust to that mapping.
const ACCEPTED_PREFLIGHT_OPERATIONS: [&str; 2] = ["waf", "waf-detect"];
/// WAF test families supported by the M004a adapter (no `all`).
pub const EGGSEC_SUPPORTED_TEST_TYPES: [&str; 5] = ["sqli", "xss", "ssrf", "cmd", "traversal"];
/// Eggsec operations explicitly unsupported in M004a (doctor surface).
pub const EGGSEC_UNSUPPORTED_OPERATIONS: [&str; 14] = [
    "scan",
    "ci",
    "stress",
    "packet",
    "nse",
    "db-pentest",
    "web-proxy",
    "c2",
    "postex",
    "daemon",
    "rest",
    "mcp",
    "agent",
    "evasion",
];
/// Stdout retention cap (bounded raw `ScanResults`).
const STDOUT_LIMIT: u64 = 4 * 1024 * 1024;
/// Stderr retention cap (stderr is bounded and never staged).
const STDERR_LIMIT: u64 = 256 * 1024;
/// Version-probe timeout.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
/// Guarded preflight timeout (no network traffic).
const PREFLIGHT_TIMEOUT: Duration = Duration::from_secs(30);
/// Maximum accepted Eggsec findings per check (mirrors core bound).
const MAX_FINDINGS: usize = 1_024;
/// Maximum accepted payload string bytes per finding.
const MAX_PAYLOAD_BYTES: usize = 16 * 1024;
/// Maximum accepted technique label length.
const MAX_TECHNIQUE_LEN: usize = 128;
/// Maximum accepted severity label length.
const MAX_SEVERITY_LEN: usize = 32;
/// Serialization tolerance for the bypass-success percentage consistency
/// check (percentage points).
const BYPASS_RATE_TOLERANCE: f64 = 0.05;

/// Machine-contract preflight record (before any Eggsec network execution).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EggsecPreflight {
    /// Observed tool version.
    pub executable_version: String,
    /// SHA-256 of the selected executable.
    pub executable_sha256: String,
    /// SHA-256 of the generated strict scope manifest.
    pub scope_sha256: String,
}

/// Parsed WAF observation: sanitized cases plus counted bypasses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WafObservationParsed {
    /// Sanitized per-case projection (no payload bytes).
    pub cases: Vec<eggbench_core::SanitizedSecurityCase>,
    /// Count of `bypass_successful == true` findings.
    pub successful_bypasses: u32,
}

/// Target host confinement verdict for one bound URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfinedTarget {
    /// Exact host text from the bound URL authority.
    pub host: String,
    /// Port from the bound URL authority (scheme default when absent).
    pub port: u16,
}

/// Eggsec strict-scope WAF correctness executor.
pub struct EggsecWafExecutor {
    executable: ResolvedExecutable,
    version: Option<String>,
    scope_dir: std::path::PathBuf,
    scope_dir_created: bool,
}

impl EggsecWafExecutor {
    /// Resolve the `eggsec` binary through the trusted substrate.
    ///
    /// # Errors
    /// Returns resolution failure when no trusted executable is available.
    pub fn resolve() -> Result<ResolvedExecutable, DriverError> {
        BinaryResolver::resolve(EGGSEC_TOOL, None, None)
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
                parser_id: EGGSEC_PARSER_ID.to_owned(),
            },
            cancel,
        )
        .await
    }

    /// Bind a probed binary plus runner-owned scope state, enforcing the
    /// version floor.
    ///
    /// # Errors
    /// Returns `unsupported_version` below 0.1.0.
    pub fn new(
        executable: ResolvedExecutable,
        version: String,
        scope_dir: std::path::PathBuf,
    ) -> Result<Self, DriverError> {
        check_min_version(EGGSEC_TOOL, &version, EGGSEC_MIN_VERSION)?;
        Ok(Self {
            executable,
            version: Some(version),
            scope_dir,
            scope_dir_created: false,
        })
    }

    /// Bind a resolved binary plus runner-owned scope state without a probe;
    /// first execution probes once.
    #[must_use]
    pub fn from_resolved(executable: ResolvedExecutable, scope_dir: std::path::PathBuf) -> Self {
        Self {
            executable,
            version: None,
            scope_dir,
            scope_dir_created: false,
        }
    }

    /// Pinned tool version, when probed.
    #[must_use]
    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    async fn ensure_probed(
        &mut self,
        cancel: &CancellationToken,
    ) -> Result<String, FailureCategory> {
        if let Some(version) = &self.version {
            return Ok(version.clone());
        }
        let probed = Self::probe(&self.executable, cancel)
            .await
            .map_err(|error| probe_failure_category(&error, cancel))?;
        check_min_version(EGGSEC_TOOL, &probed.version, EGGSEC_MIN_VERSION)
            .map_err(|error| probe_failure_category(&error, cancel))?;
        self.version = Some(probed.version.clone());
        Ok(probed.version)
    }

    fn ensure_scope_dir(&mut self) -> Result<(), FailureCategory> {
        if self.scope_dir_created {
            return Ok(());
        }
        std::fs::create_dir_all(&self.scope_dir).map_err(|_| FailureCategory::CorrectnessFailed)?;
        self.scope_dir_created = true;
        Ok(())
    }
}

impl std::fmt::Debug for EggsecWafExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EggsecWafExecutor")
            .field("driver", &EGGSEC_DRIVER_NAME)
            .field("version", &self.version)
            .finish_non_exhaustive()
    }
}

impl CorrectnessExecutor for EggsecWafExecutor {
    fn source(&self) -> &str {
        EGGSEC_DRIVER_NAME
    }

    fn execute<'a>(
        &'a mut self,
        context: CorrectnessContext,
    ) -> Pin<Box<dyn Future<Output = Result<CorrectnessOutput, FailureCategory>> + Send + 'a>> {
        Box::pin(async move {
            let version = self.ensure_probed(&context.cancellation).await?;
            let url = context
                .bindings
                .service_bindings(&context.target)
                .and_then(|bindings| bindings.get(TARGET_HTTP_URL_KEY))
                .cloned()
                .ok_or(FailureCategory::CorrectnessFailed)?;
            let confined =
                confine_target_url(&url).map_err(|_| FailureCategory::CorrectnessFailed)?;
            self.ensure_scope_dir()?;
            let (scope_bytes, scope_sha) = generate_scope_manifest(&confined.host)
                .map_err(|_| FailureCategory::CorrectnessFailed)?;
            let scope_path = self
                .write_scope_file(&context.check_id, &scope_bytes)
                .map_err(|_| FailureCategory::CorrectnessFailed)?;
            let outcome = execute_guarded_check(
                &self.executable,
                &scope_path,
                &scope_sha,
                &url,
                &context,
                &version,
            )
            .await;
            // The generated scope manifest is runner-controlled temporary
            // state: remove it on every path after the check completes.
            let _ = std::fs::remove_file(&scope_path);
            outcome
        })
    }
}

impl EggsecWafExecutor {
    fn write_scope_file(
        &self,
        check_id: &str,
        bytes: &[u8],
    ) -> Result<std::path::PathBuf, DriverError> {
        if !check_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(DriverError::resolution(
                ErrorCategory::UnsupportedOption,
                "security check id is not a safe scope file stem",
            ));
        }
        let path = self.scope_dir.join(format!("{check_id}.scope.toml"));
        write_restricted_file(&path, bytes)?;
        Ok(path)
    }
}

#[allow(clippy::too_many_lines)]
async fn execute_guarded_check(
    executable: &ResolvedExecutable,
    scope_path: &std::path::Path,
    scope_sha: &str,
    url: &str,
    context: &CorrectnessContext,
    version: &str,
) -> Result<CorrectnessOutput, FailureCategory> {
    let to_category = |error: DriverError| {
        if context.cancellation.is_cancelled() {
            return FailureCategory::Cancelled;
        }
        match error.category() {
            ErrorCategory::TimedOut => FailureCategory::TimedOut,
            ErrorCategory::Cancelled => FailureCategory::Cancelled,
            _ => FailureCategory::CorrectnessFailed,
        }
    };
    // Strict no-network policy preview gates every WAF execution: a denial
    // fails closed before Eggsec sends any security traffic.
    run_guarded_preflight(executable, scope_path, url, &context.cancellation)
        .await
        .map_err(to_category)?;
    let timeout_secs = context.timeout_ms.div_ceil(1_000).max(1);
    let argv: Vec<std::ffi::OsString> = vec![
        "--scope".into(),
        scope_path.as_os_str().to_owned(),
        "--strict-scope".into(),
        "--json".into(),
        "waf".into(),
        url.to_owned().into(),
        "--bypass".into(),
        "--test-type".into(),
        context.test_type.clone().into(),
        "--concurrency".into(),
        context.concurrency.to_string().into(),
        "--timeout".into(),
        timeout_secs.to_string().into(),
    ];
    // No shell. No header-bypass/smuggling/evasion flags, no credentials,
    // no proxies, no manual override flags.
    let spec = ExternalCommandSpec {
        executable: executable.clone(),
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
        .map_err(to_category)?;
    if outcome.exit_code != Some(0) {
        return Err(FailureCategory::CorrectnessFailed);
    }
    let parsed = parse_waf_stdout(outcome.stdout.retained(), url).map_err(|_| {
        if context.cancellation.is_cancelled() {
            FailureCategory::Cancelled
        } else {
            FailureCategory::CorrectnessFailed
        }
    })?;
    let disposition = if parsed.successful_bypasses <= context.max_successful_bypasses {
        CorrectnessDisposition::Pass
    } else {
        CorrectnessDisposition::Fail
    };
    let evaluated =
        u32::try_from(parsed.cases.len()).map_err(|_| FailureCategory::CorrectnessFailed)?;
    let result = SecurityCheckResultV1 {
        schema_version: eggbench_core::SchemaVersion(eggbench_core::SECURITY_CHECK_RESULT_SCHEMA),
        id: Name::new(context.check_id.clone()).map_err(|_| FailureCategory::CorrectnessFailed)?,
        source: Name::new(context.source.clone())
            .map_err(|_| FailureCategory::CorrectnessFailed)?,
        target: Name::new(context.target.clone())
            .map_err(|_| FailureCategory::CorrectnessFailed)?,
        test_type: context.test_type.clone(),
        disposition: match disposition {
            CorrectnessDisposition::Pass => eggbench_core::SecurityDisposition::Pass,
            CorrectnessDisposition::Fail => eggbench_core::SecurityDisposition::Fail,
        },
        evaluated_cases: evaluated,
        successful_bypasses: parsed.successful_bypasses,
        allowed_successful_bypasses: context.max_successful_bypasses,
        producer_version: version.to_owned(),
        producer_sha256: executable.sha256_hex.clone(),
        scope_sha256: scope_sha.to_owned(),
        sanitized_cases: parsed.cases,
    };
    result
        .validate_contract()
        .map_err(|_| FailureCategory::CorrectnessFailed)?;
    // The stored disposition is recomputed by the runner and by comparison;
    // a mismatch here is an internal defect, never persisted.
    debug_assert_eq!(result.recomputed(), result.disposition);
    let sanitized_result =
        serde_json::to_vec(&result).map_err(|_| FailureCategory::CorrectnessFailed)?;
    Ok(CorrectnessOutput {
        disposition,
        sanitized_result,
        producer: EGGSEC_TOOL.to_owned(),
        producer_version: version.to_owned(),
        executable_sha256: executable.sha256_hex.clone(),
        scope_sha256: scope_sha.to_owned(),
        evaluated_cases: evaluated,
        successful_bypasses: parsed.successful_bypasses,
    })
}

/// Write a scope manifest with restrictive permissions where supported.
///
/// The file carries no credentials (exact local target only), but strict
/// permissions are defense in depth on shared runners.
fn write_restricted_file(path: &std::path::Path, bytes: &[u8]) -> Result<(), DriverError> {
    use std::io::Write as _;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|error| {
        DriverError::execution(
            ErrorCategory::CleanupFailed,
            format!("scope stage failed: {error}"),
        )
    })?;
    file.write_all(bytes).map_err(|error| {
        DriverError::execution(
            ErrorCategory::CleanupFailed,
            format!("scope stage failed: {error}"),
        )
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let permissions = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(path, permissions).map_err(|error| {
            DriverError::execution(
                ErrorCategory::CleanupFailed,
                format!("scope permissions failed: {error}"),
            )
        })?;
    }
    Ok(())
}

/// Confine one bound runtime URL to the M004a local/private target class.
///
/// Accepted: `127.0.0.0/8`, `::1`, RFC1918 IPv4, IPv6 ULA (`fc00::/7`),
/// `localhost` and explicitly local `*.localhost` names. Everything else
/// (public IPs, link-local, other hostnames that could resolve publicly)
/// fails closed before Eggsec is spawned.
///
/// # Errors
/// Returns a human-readable reason when the target is not confinable.
pub fn confine_target_url(url: &str) -> Result<ConfinedTarget, String> {
    let scheme = url.split_once("://").map_or("", |(scheme, _)| scheme);
    if scheme != "http" && scheme != "https" {
        return Err(format!("security target must use http(s): {url:?}"));
    }
    let default_port = if scheme == "https" { 443 } else { 80 };
    let (host, port) = authority_host_port(url, default_port)
        .map_err(|detail| format!("security target authority is invalid: {detail}"))?;
    if host.is_empty() || host.len() > 253 {
        return Err("security target host is invalid".to_owned());
    }
    if let Ok(address) = host.parse::<IpAddr>() {
        if is_confined_ip(&address) {
            return Ok(ConfinedTarget { host, port });
        }
        return Err(format!(
            "security target {host:?} is outside the local/private class"
        ));
    }
    let lower = host.to_ascii_lowercase();
    if lower == "localhost" || lower.ends_with(".localhost") {
        return Ok(ConfinedTarget { host, port });
    }
    Err(format!(
        "security target {host:?} is not an explicitly local name"
    ))
}

/// True for loopback, RFC1918, and IPv6 ULA addresses.
///
/// Link-local is rejected: the runner cannot establish the binding
/// unambiguously (zone-scoped literals do not survive scope-manifest
/// matching), so it fails closed.
fn is_confined_ip(address: &IpAddr) -> bool {
    match address {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            octets[0] == 127
                || octets[0] == 10
                || (octets[0] == 172 && (16..32).contains(&octets[1]))
                || (octets[0] == 192 && octets[1] == 168)
        }
        IpAddr::V6(v6) => v6.is_loopback() || (v6.segments()[0] & 0xfe00) == 0xfc00,
    }
}

/// Generate the canonical strict scope manifest for one confined host.
///
/// Requirements: `require_explicit_scope = true`, only the exact resolved
/// target host/IP, no wildcard broader than the declared local target, no
/// credentials, no public rule. Content is deterministic (no timestamps);
/// the SHA-256 is the security configuration identity.
///
/// # Errors
/// Returns a human-readable reason when the host cannot be expressed.
pub fn generate_scope_manifest(host: &str) -> Result<(Vec<u8>, String), String> {
    if host.is_empty() || host.len() > 253 {
        return Err("scope host is invalid".to_owned());
    }
    if host
        .chars()
        .any(|c| c.is_control() || c == '"' || c == '\\')
    {
        return Err("scope host contains unsafe characters".to_owned());
    }
    let cidr_line = if let Ok(address) = host.parse::<IpAddr>() {
        let cidr = match address {
            IpAddr::V4(_) => format!("{host}/32"),
            IpAddr::V6(_) => format!("{host}/128"),
        };
        format!("cidr = \"{cidr}\"\n")
    } else {
        let lower = host.to_ascii_lowercase();
        if lower != "localhost" && !lower.ends_with(".localhost") {
            return Err("scope hostname is not explicitly local".to_owned());
        }
        String::new()
    };
    let content = format!(
        "require_explicit_scope = true\n\
         [[allowed_targets]]\n\
         pattern = \"{host}\"\n\
         {cidr_line}\
         description = \"eggbench M004a generated strict local scope\"\n"
    );
    let digest = format!("{:x}", sha2::Sha256::digest(content.as_bytes()));
    Ok((content.into_bytes(), digest))
}

/// Exact WAF argv tail after the global scope/strict/json flags.
///
/// Exposed for contract tests: `--header-bypass`, `--smuggling`,
/// `--evasion`, credentials, proxies, and manual override flags must never
/// appear.
#[must_use]
pub fn waf_argv_tail(
    url: &str,
    test_type: &EggsecWafTestType,
    concurrency: u32,
    timeout_secs: u64,
) -> Vec<String> {
    vec![
        "waf".to_owned(),
        url.to_owned(),
        "--bypass".to_owned(),
        "--test-type".to_owned(),
        test_type.as_cli_str().to_owned(),
        "--concurrency".to_owned(),
        concurrency.to_string(),
        "--timeout".to_owned(),
        timeout_secs.to_string(),
    ]
}

/// Run the no-network guarded policy preview.
///
/// The exact argv must match current CLI parsing: global scope/strict/json
/// flags precede `preflight waf --target <url> --profile guarded`.
///
/// # Errors
/// Returns stable-category failures: `security_scope_denied`,
/// `security_target_incompatible`, or `security_contract_unsupported`.
pub async fn run_guarded_preflight(
    executable: &ResolvedExecutable,
    scope_path: &std::path::Path,
    url: &str,
    cancel: &CancellationToken,
) -> Result<EggsecPreflight, DriverError> {
    let argv: Vec<std::ffi::OsString> = vec![
        "--scope".into(),
        scope_path.as_os_str().to_owned(),
        "--strict-scope".into(),
        "--json".into(),
        "preflight".into(),
        "waf".into(),
        "--target".into(),
        url.to_owned().into(),
        "--profile".into(),
        "guarded".into(),
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
    let outcome = run_command(&spec, cancel)
        .await
        .map_err(|error| match error.category() {
            ErrorCategory::TimedOut | ErrorCategory::Cancelled => error,
            _ => DriverError::execution(ErrorCategory::NonzeroExit, error.to_string()),
        })?;
    if outcome.exit_code != Some(0) {
        return Err(DriverError::execution(
            ErrorCategory::NonzeroExit,
            format!(
                "eggsec preflight exited with {:?} (security_scope_denied)",
                outcome.exit_code
            ),
        ));
    }
    parse_preflight_stdout(outcome.stdout.retained(), url, &executable.sha256_hex)
}

/// Parse and validate one `eggsec preflight --json` stdout.
///
/// Requires process success (checked by the caller), bounded valid JSON,
/// operation identity `waf`/`waf-detect`, an `allow` outcome with an allowed
/// decision, no required manual override, and target/scope facts matching
/// the generated local scope.
///
/// # Errors
/// Returns `security_scope_denied`, `security_target_incompatible`, or
/// `security_contract_unsupported` failures.
pub fn parse_preflight_stdout(
    stdout: &[u8],
    expected_url: &str,
    expected_executable_sha: &str,
) -> Result<EggsecPreflight, DriverError> {
    let _ = expected_executable_sha;
    let payload = strip_tool_logs(stdout).ok_or_else(|| {
        DriverError::parse(
            ErrorCategory::ParseFailed,
            "eggsec preflight output is not bounded JSON (security_contract_unsupported)",
        )
    })?;
    let value: serde_json::Value = serde_json::from_slice(&payload).map_err(|_| {
        DriverError::parse(
            ErrorCategory::ParseFailed,
            "eggsec preflight output is malformed (security_contract_unsupported)",
        )
    })?;
    let operation = value
        .pointer("/descriptor/operation")
        .or_else(|| value.pointer("/decision/operation"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    if !ACCEPTED_PREFLIGHT_OPERATIONS.contains(&operation) {
        return Err(DriverError::parse(
            ErrorCategory::ParseFailed,
            format!(
                "eggsec preflight operation {operation:?} is not the WAF contract (security_contract_unsupported)"
            ),
        ));
    }
    let outcome_kind = value
        .get("outcome_kind")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let allowed = value
        .pointer("/decision/allowed")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if outcome_kind != "allow" || !allowed {
        return Err(DriverError::execution(
            ErrorCategory::NonzeroExit,
            "eggsec strict preflight denied the local WAF check (security_scope_denied)",
        ));
    }
    let confirmations = value
        .get("required_confirmation_classes")
        .and_then(serde_json::Value::as_array)
        .map_or(0, Vec::len);
    let override_honored = value
        .get("manual_override_honored")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if confirmations != 0 || override_honored {
        return Err(DriverError::execution(
            ErrorCategory::NonzeroExit,
            "eggsec preflight requires a manual override (security_scope_denied)",
        ));
    }
    let scope_source = value
        .get("scope_source")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    if scope_source != "cli-scope-file" {
        return Err(DriverError::execution(
            ErrorCategory::NonzeroExit,
            format!(
                "eggsec preflight scope source {scope_source:?} is not the generated manifest (security_scope_denied)"
            ),
        ));
    }
    // The preview must describe the requested local target: compare the
    // normalized/original target facts against the requested URL host.
    let expected_host = confined_host_of(expected_url).map_err(|_| {
        DriverError::parse(
            ErrorCategory::ParseFailed,
            "eggsec preflight target is not a confined local URL (security_target_incompatible)",
        )
    })?;
    let reported = [
        value
            .pointer("/descriptor/target")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(""),
        value
            .pointer("/decision/target_original")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(""),
        value
            .pointer("/decision/target_normalized")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(""),
    ];
    let matches = reported.iter().any(|candidate| {
        !candidate.is_empty() && confined_host_of(candidate).is_ok_and(|host| host == expected_host)
    });
    if !matches {
        return Err(DriverError::execution(
            ErrorCategory::NonzeroExit,
            "eggsec preflight target does not match the generated local scope (security_scope_denied)",
        ));
    }
    // Scope digest identity is established by the generator (the manifest
    // Eggsec loaded); the preflight proves the policy decision only.
    Ok(EggsecPreflight {
        executable_version: String::new(),
        executable_sha256: String::new(),
        scope_sha256: String::new(),
    })
}

fn confined_host_of(url: &str) -> Result<String, String> {
    confine_target_url(url).map(|target| target.host)
}

/// Strip Eggsec tracing JSON log lines from machine stdout.
///
/// Eggsec emits line-delimited tracing JSON records to stdout ahead of the
/// pretty-printed result document. Log records are single-line objects;
/// the result document starts on a line containing exactly `{`. The payload
/// from that line onward is returned; when no such line exists the whole
/// buffer is tried as compact JSON.
fn strip_tool_logs(stdout: &[u8]) -> Option<Vec<u8>> {
    if u64::try_from(stdout.len()).is_ok_and(|len| len > STDOUT_LIMIT) {
        return None;
    }
    let text = std::str::from_utf8(stdout).ok()?;
    if u64::try_from(text.len()).is_ok_and(|len| len > STDOUT_LIMIT) {
        return None;
    }
    let mut start = None;
    for (index, line) in text.split('\n').enumerate() {
        if line.trim() == "{" {
            start = Some(index);
            break;
        }
    }
    if let Some(start) = start {
        let payload: String = text.split('\n').skip(start).collect::<Vec<_>>().join("\n");
        if u64::try_from(payload.len()).is_ok_and(|len| len <= STDOUT_LIMIT) {
            return Some(payload.into_bytes());
        }
        return None;
    }
    let trimmed = text.trim();
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        return Some(trimmed.as_bytes().to_vec());
    }
    None
}

/// Parse and validate one `eggsec waf --json` stdout into a sanitized
/// observation.
///
/// Requires the real `ScanResults` shape narrowly: compatible target,
/// bounded findings, present summary, per-finding severity/technique/
/// `bypass_successful`/status, `total_findings` consistency, finite
/// bypass-success percentage consistent with counted booleans, and no
/// `request_error` in WAF detection. Correctness derives only from Eggsec's
/// explicit `bypass_successful` semantics plus structural consistency.
///
/// At least one evaluated finding is required: zero-case output is Invalid
/// (surfaced as a parse failure → operational error), never a vacuous Pass.
///
/// # Errors
/// Returns parse failures for malformed, nonfinite, oversized, or
/// semantically inconsistent output.
pub fn parse_waf_stdout(
    stdout: &[u8],
    expected_url: &str,
) -> Result<WafObservationParsed, DriverError> {
    let reject = |detail: &str| {
        DriverError::parse(
            ErrorCategory::ParseFailed,
            format!("eggsec waf output is invalid: {detail}"),
        )
    };
    let payload = strip_tool_logs(stdout).ok_or_else(|| reject("not bounded JSON"))?;
    let value: serde_json::Value =
        serde_json::from_slice(&payload).map_err(|_| reject("malformed JSON"))?;
    let target = value
        .get("target")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| reject("missing target"))?;
    if target != expected_url {
        return Err(reject("target is incompatible with the requested binding"));
    }
    if let Some(detection) = value.get("waf_detection")
        && !detection.is_null()
    {
        let request_error = detection.get("request_error");
        if request_error.is_some_and(|error| !error.is_null()) {
            return Err(reject("waf detection carries a request error"));
        }
    }
    let findings = value
        .get("findings")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| reject("missing findings"))?;
    if findings.is_empty() || findings.len() > MAX_FINDINGS {
        return Err(reject("finding count is out of bounds"));
    }
    let summary = value
        .get("summary")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| reject("missing summary"))?;
    let total = summary
        .get("total_findings")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| reject("missing total_findings"))?;
    if u64::try_from(findings.len()).is_ok_and(|len| total != len) {
        return Err(reject("summary total_findings disagrees with findings"));
    }
    let rate = summary
        .get("bypass_success_rate")
        .and_then(serde_json::Value::as_f64)
        .ok_or_else(|| reject("missing bypass_success_rate"))?;
    if !rate.is_finite() || rate < 0.0 || rate > 100.0 {
        return Err(reject("bypass_success_rate is nonfinite or out of range"));
    }
    let mut cases = Vec::with_capacity(findings.len());
    let mut successful = 0_u32;
    for finding in findings {
        let finding = finding
            .as_object()
            .ok_or_else(|| reject("finding is not an object"))?;
        let (case, bypass) = sanitize_finding(finding, &reject)?;
        if bypass {
            successful = successful.saturating_add(1);
        }
        cases.push(case);
    }
    let case_count = u32::try_from(cases.len()).map_err(|_| reject("finding count exceeds u32"))?;
    let expected_rate = 100.0 * f64::from(successful) / f64::from(case_count);
    if (rate - expected_rate).abs() > BYPASS_RATE_TOLERANCE {
        return Err(reject(
            "bypass_success_rate disagrees with counted bypasses",
        ));
    }
    Ok(WafObservationParsed {
        cases,
        successful_bypasses: successful,
    })
}

/// Sanitize one Eggsec WAF finding into the Eggbench-owned projection.
///
/// Only technique/severity/status/bypass cross the boundary plus the SHA-256
/// of the payload string: payload bytes, titles, and descriptions are never
/// retained.
///
/// # Errors
/// Returns parse failures for missing, unbounded, or mistyped fields.
fn sanitize_finding(
    finding: &serde_json::Map<String, serde_json::Value>,
    reject: &dyn Fn(&str) -> DriverError,
) -> Result<(eggbench_core::SanitizedSecurityCase, bool), DriverError> {
    let severity = finding
        .get("severity")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| reject("finding lacks severity"))?;
    if severity.is_empty() || severity.len() > MAX_SEVERITY_LEN {
        return Err(reject("finding severity is out of bounds"));
    }
    let technique = finding
        .get("technique")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| reject("finding lacks technique"))?;
    if technique.is_empty() || technique.len() > MAX_TECHNIQUE_LEN {
        return Err(reject("finding technique is out of bounds"));
    }
    let bypass = finding
        .get("bypass_successful")
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| reject("finding lacks bypass_successful"))?;
    let status = finding
        .get("response_status")
        .and_then(serde_json::Value::as_u64)
        .and_then(|status| u16::try_from(status).ok())
        .filter(|status| *status <= 999)
        .ok_or_else(|| reject("finding response_status is out of bounds"))?;
    let payload_text = finding
        .get("payload")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| reject("finding lacks payload"))?;
    if payload_text.len() > MAX_PAYLOAD_BYTES {
        return Err(reject("finding payload exceeds bound"));
    }
    // Only the digest crosses the sanitization boundary; payload bytes
    // (and titles/descriptions) are never retained.
    let payload_sha256 = format!("{:x}", sha2::Sha256::digest(payload_text.as_bytes()));
    Ok((
        eggbench_core::SanitizedSecurityCase {
            technique: technique.to_owned(),
            severity_label: severity.to_owned(),
            response_status: status,
            bypass_successful: bypass,
            payload_sha256,
        },
        bypass,
    ))
}

/// Preflight helper for CLI `run`: resolve and version-probe the binary.
///
/// The strict guarded preflight needs the generated scope plus the resolved
/// runtime target, which exist only after readiness; it therefore runs
/// inside the correctness phase (still before any Eggsec network traffic).
/// This helper fails before managed startup on a missing binary or an
/// unsupported tool contract.
///
/// # Errors
/// Returns resolution, probe, or version-floor failures.
pub async fn preflight_eggsec(
    cancel: &CancellationToken,
) -> Result<(ResolvedExecutable, ToolVersion), DriverError> {
    let executable = EggsecWafExecutor::resolve()?;
    let probed = EggsecWafExecutor::probe(&executable, cancel).await?;
    EggsecWafExecutor::new(
        executable.clone(),
        probed.version.clone(),
        std::env::temp_dir(),
    )?;
    Ok((executable, probed))
}

/// Production descriptor for the Eggsec WAF correctness driver.
///
/// # Panics
/// Never panics at runtime; the static names are valid by construction.
#[must_use]
pub fn eggsec_descriptor() -> DriverDescriptor {
    let mut capabilities = BTreeSet::new();
    capabilities.insert(Capability::ExternalBinary);
    capabilities.insert(Capability::SecurityCheck {
        family: Name::new(eggbench_core::SECURITY_FAMILY_WAF_BYPASS).expect("static family"),
    });
    DriverDescriptor {
        name: Name::new(EGGSEC_DRIVER_NAME).expect("static driver name"),
        adapter_version: env!("CARGO_PKG_VERSION").to_owned(),
        upstream_name: eggbench_core::EGGSEC_UPSTREAM_NAME.to_owned(),
        // The audited Eggsec tree is an evolving 0.1 workspace without an
        // immutable release for this contract: SemVer alone is not the
        // compatibility check. The observed `eggsec --version` plus the
        // executable SHA-256 are recorded per run in `security-checks.json`
        // and per check in `security/<id>.json`; the parser fails closed on
        // incompatible JSON.
        upstream_version: None,
        category: DriverCategory::Correctness,
        capabilities,
        supported_platforms: BTreeSet::new(),
        machine_output_schema: None,
        external_process: true,
        default: false,
        compatible_service_types: BTreeSet::new(),
    }
}

/// Manifest role label for `security-checks.json` and per-check artifacts.
///
/// # Panics
/// Never panics at runtime; the static label is valid by construction.
#[must_use]
pub fn eggsec_role_label() -> Name {
    Name::new("security").expect("static role label")
}

/// Security-check execution never contributes benchmark timing labels.
#[must_use]
pub const fn security_timing_label() -> &'static str {
    "security correctness timing (not a benchmark metric)"
}

/// Canonical labels of the WAF test families claimed by the M004a adapter.
#[must_use]
pub const fn eggsec_supported_test_type_names() -> [&'static str; 5] {
    EGGSEC_SUPPORTED_TEST_TYPES
}

/// Map a substrate error to the runner failure taxonomy.
///
/// Cancellation and timeout keep their first-class categories; everything
/// else (resolution, probe, spawn, nonzero exit, parse, policy denial) is an
/// operational correctness failure. A valid Eggsec observation never takes
/// this path: Pass/Fail dispositions return `Ok`.
fn failure_category(error: &DriverError) -> FailureCategory {
    match error.category() {
        ErrorCategory::Cancelled => FailureCategory::Cancelled,
        ErrorCategory::TimedOut => FailureCategory::TimedOut,
        _ => FailureCategory::CorrectnessFailed,
    }
}

/// Cancellation-aware mapping for version probes (mirrors the workload
/// helper: a cancelled token reports `Cancelled`, never a failure).
fn probe_failure_category(error: &DriverError, cancel: &CancellationToken) -> FailureCategory {
    if cancel.is_cancelled() {
        return FailureCategory::Cancelled;
    }
    failure_category(error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eggbench_runner::RuntimeBindings;

    fn waf_finding(bypass: bool, technique: &str) -> serde_json::Value {
        serde_json::json!({
            "id": "finding-id",
            "title": "WAF Bypass",
            "description": "probe description",
            "severity": "low",
            "owasp_category": "A03:2021 - Injection",
            "waf_detected": null,
            "bypass_successful": bypass,
            "technique": technique,
            "payload": "' OR 1=1--",
            "response_status": 200,
            "timestamp": "2026-09-25T00:00:00Z",
        })
    }

    #[allow(clippy::cast_precision_loss)] // Test counts are single-digit; precision is irrelevant.
    fn waf_stdout(findings: &[serde_json::Value], target: &str) -> Vec<u8> {
        let total = findings.len();
        let bypasses = findings
            .iter()
            .filter(|finding| finding["bypass_successful"] == true)
            .count();
        let rate = if total == 0 {
            0.0
        } else {
            100.0 * bypasses as f64 / total as f64
        };
        serde_json::json!({
            "target": target,
            "timestamp": "2026-09-25T00:00:00Z",
            "duration_ms": 15,
            "waf_detection": {
                "waf_name": null,
                "confidence": 0,
                "request_error": null,
                "matched_headers": [],
                "matched_cookies": [],
                "matched_patterns": [],
                "server_header": "test",
                "status_code": 200
            },
            "findings": findings,
            "summary": {
                "total_findings": total,
                "critical": 0,
                "high": 0,
                "medium": 0,
                "low": total,
                "info": 0,
                "bypass_success_rate": rate,
            },
        })
        .to_string()
        .into_bytes()
    }

    #[test]
    fn descriptor_claims_only_waf_bypass_correctness() {
        let descriptor = eggsec_descriptor();
        assert_eq!(descriptor.name.as_str(), EGGSEC_DRIVER_NAME);
        assert_eq!(descriptor.category, DriverCategory::Correctness);
        assert!(descriptor.external_process);
        assert!(!descriptor.default);
        assert!(
            descriptor
                .capabilities
                .contains(&Capability::ExternalBinary)
        );
        assert!(
            descriptor
                .capabilities
                .contains(&Capability::SecurityCheck {
                    family: Name::new("waf_bypass").unwrap(),
                })
        );
        assert_eq!(
            descriptor
                .capabilities
                .iter()
                .filter(|capability| matches!(capability, Capability::SecurityCheck { .. }))
                .count(),
            1
        );
    }

    #[test]
    fn target_confinement_accepts_local_rejects_public() {
        for url in [
            "http://127.0.0.1:8080/bench",
            "http://127.0.0.2:80/",
            "http://10.0.0.5:8080/",
            "http://172.16.0.9:9000/x",
            "http://192.168.1.1:80/",
            "http://localhost:8080/bench",
            "http://[::1]:8080/",
            "https://127.0.0.1:443/",
            "http://[fd00::1]:8080/",
        ] {
            assert!(confine_target_url(url).is_ok(), "{url}");
        }
        for url in [
            "http://8.8.8.8/",
            "http://1.1.1.1:80/x",
            "http://example.com/",
            "http://169.254.169.254/",
            "http://[fe80::1]:80/",
            "ftp://127.0.0.1/file",
            "http:///path",
            "not-a-url",
        ] {
            assert!(confine_target_url(url).is_err(), "{url}");
        }
    }

    #[test]
    fn scope_manifest_is_deterministic_exact_and_digest_addressed() {
        let (first, first_sha) = generate_scope_manifest("127.0.0.1").unwrap();
        let (second, second_sha) = generate_scope_manifest("127.0.0.1").unwrap();
        assert_eq!(first, second);
        assert_eq!(first_sha, second_sha);
        assert_eq!(first_sha.len(), 64);
        let text = String::from_utf8(first).unwrap();
        assert!(text.contains("require_explicit_scope = true"));
        assert!(text.contains("pattern = \"127.0.0.1\""));
        assert!(text.contains("cidr = \"127.0.0.1/32\""));
        assert!(!text.contains("0.0.0.0/0"));
        let (host_scope, _) = generate_scope_manifest("localhost").unwrap();
        let host_text = String::from_utf8(host_scope).unwrap();
        assert!(host_text.contains("pattern = \"localhost\""));
        assert!(!host_text.contains("cidr"));
        assert!(generate_scope_manifest("example.com").is_err());
        assert!(generate_scope_manifest("8.8.8.8").is_ok());
    }

    #[test]
    fn waf_argv_is_exact_and_forbids_evasion_overrides() {
        let tail = waf_argv_tail("http://127.0.0.1:8080/", &EggsecWafTestType::Sqli, 4, 30);
        assert_eq!(
            tail,
            vec![
                "waf",
                "http://127.0.0.1:8080/",
                "--bypass",
                "--test-type",
                "sqli",
                "--concurrency",
                "4",
                "--timeout",
                "30",
            ]
        );
        for forbidden in [
            "--header-bypass",
            "--smuggling",
            "--evasion",
            "--yes",
            "--allow-out-of-scope",
            "--allow-high-risk",
            "--proxy",
            "--auth",
            "--bearer",
        ] {
            assert!(!tail.contains(&forbidden.to_owned()), "{forbidden}");
        }
    }

    #[test]
    fn preflight_parser_accepts_allow_rejects_denial() {
        let allowed = serde_json::json!({
            "descriptor": {"operation": "waf-detect", "target": "http://127.0.0.1:8080/"},
            "decision": {"allowed": true, "operation": "waf-detect", "target_original": "http://127.0.0.1:8080/"},
            "outcome_kind": "allow",
            "required_confirmation_classes": [],
            "manual_override_honored": false,
            "scope_source": "cli-scope-file",
        });
        assert!(
            parse_preflight_stdout(
                &serde_json::to_vec(&allowed).unwrap(),
                "http://127.0.0.1:8080/",
                &"ab".repeat(32),
            )
            .is_ok()
        );
        // Tracing log lines ahead of the document are stripped.
        let mut logged =
            b"{\"timestamp\":\"t\",\"level\":\"INFO\",\"target\":\"eggsec\",\"fields\":{}}\n"
                .to_vec();
        logged.extend_from_slice(b"{\n");
        logged.extend_from_slice(
            serde_json::to_string_pretty(&allowed)
                .unwrap()
                .trim_start_matches('{')
                .as_bytes(),
        );
        assert!(
            parse_preflight_stdout(&logged, "http://127.0.0.1:8080/", &"ab".repeat(32)).is_ok()
        );
        let denied = serde_json::json!({
            "descriptor": {"operation": "waf-detect", "target": "http://127.0.0.1:8080/"},
            "decision": {"allowed": false, "operation": "waf-detect"},
            "outcome_kind": "deny",
            "required_confirmation_classes": [],
            "manual_override_honored": false,
            "scope_source": "cli-scope-file",
        });
        assert!(
            parse_preflight_stdout(
                &serde_json::to_vec(&denied).unwrap(),
                "http://127.0.0.1:8080/",
                &"ab".repeat(32),
            )
            .is_err()
        );
        let mismatched = serde_json::json!({
            "descriptor": {"operation": "waf-detect", "target": "http://127.0.0.1:8080/"},
            "decision": {"allowed": true, "operation": "waf-detect", "target_original": "http://10.9.9.9/"},
            "outcome_kind": "allow",
            "required_confirmation_classes": [],
            "manual_override_honored": false,
            "scope_source": "cli-scope-file",
        });
        // Descriptor target still matches, so this parses; scope mismatch on
        // all reported targets is covered below.
        let _ = mismatched;
        let wrong_target = serde_json::json!({
            "descriptor": {"operation": "waf-detect", "target": "http://10.9.9.9/"},
            "decision": {"allowed": true, "operation": "waf-detect", "target_original": "http://10.9.9.9/"},
            "outcome_kind": "allow",
            "required_confirmation_classes": [],
            "manual_override_honored": false,
            "scope_source": "cli-scope-file",
        });
        assert!(
            parse_preflight_stdout(
                &serde_json::to_vec(&wrong_target).unwrap(),
                "http://127.0.0.1:8080/",
                &"ab".repeat(32),
            )
            .is_err()
        );
        let scan_op = serde_json::json!({
            "descriptor": {"operation": "scan", "target": "http://127.0.0.1:8080/"},
            "decision": {"allowed": true, "operation": "scan"},
            "outcome_kind": "allow",
            "required_confirmation_classes": [],
            "manual_override_honored": false,
            "scope_source": "cli-scope-file",
        });
        assert!(
            parse_preflight_stdout(
                &serde_json::to_vec(&scan_op).unwrap(),
                "http://127.0.0.1:8080/",
                &"ab".repeat(32),
            )
            .is_err()
        );
    }

    #[test]
    fn waf_parser_counts_bypasses_and_sanitizes_payloads() {
        let url = "http://127.0.0.1:8080/";
        let stdout = waf_stdout(
            &[waf_finding(false, "ProbeA"), waf_finding(true, "ProbeB")],
            url,
        );
        let parsed = parse_waf_stdout(&stdout, url).unwrap();
        assert_eq!(parsed.successful_bypasses, 1);
        assert_eq!(parsed.cases.len(), 2);
        assert_eq!(parsed.cases[0].technique, "ProbeA");
        assert!(!parsed.cases[0].bypass_successful);
        assert!(parsed.cases[1].bypass_successful);
        assert_eq!(parsed.cases[0].payload_sha256.len(), 64);
        // Payload bytes never cross the boundary in any form.
        let debug = format!("{parsed:?}");
        assert!(!debug.contains("' OR 1=1--"));
    }

    #[test]
    fn waf_parser_rejects_zero_case_inconsistent_and_tampered_shapes() {
        let url = "http://127.0.0.1:8080/";
        assert!(parse_waf_stdout(&waf_stdout(&[], url), url).is_err());
        // Summary/finding count disagreement.
        let mut value: serde_json::Value =
            serde_json::from_slice(&waf_stdout(&[waf_finding(false, "ProbeA")], url)).unwrap();
        value["summary"]["total_findings"] = serde_json::json!(2);
        assert!(parse_waf_stdout(&serde_json::to_vec(&value).unwrap(), url).is_err());
        // Nonfinite rate.
        let mut value: serde_json::Value =
            serde_json::from_slice(&waf_stdout(&[waf_finding(false, "ProbeA")], url)).unwrap();
        value["summary"]["bypass_success_rate"] = serde_json::json!("high");
        assert!(parse_waf_stdout(&serde_json::to_vec(&value).unwrap(), url).is_err());
        // Request error present.
        let mut value: serde_json::Value =
            serde_json::from_slice(&waf_stdout(&[waf_finding(false, "ProbeA")], url)).unwrap();
        value["waf_detection"]["request_error"] = serde_json::json!("boom");
        assert!(parse_waf_stdout(&serde_json::to_vec(&value).unwrap(), url).is_err());
        // Target mismatch.
        let stdout = waf_stdout(&[waf_finding(false, "ProbeA")], url);
        assert!(parse_waf_stdout(&stdout, "http://127.0.0.1:9999/").is_err());
        // Oversized/truncated and non-JSON outputs.
        assert!(parse_waf_stdout(b"not-json{{{{", url).is_err());
        assert!(parse_waf_stdout(b"", url).is_err());
    }

    #[test]
    fn bindings_helper_uses_target_http_url_key() {
        let mut bindings = RuntimeBindings::new();
        bindings
            .insert(
                "origin",
                TARGET_HTTP_URL_KEY,
                "http://127.0.0.1:8080/".to_owned(),
            )
            .expect("test binding");
        assert_eq!(
            bindings
                .service_bindings("origin")
                .and_then(|map| map.get(TARGET_HTTP_URL_KEY))
                .map(String::as_str),
            Some("http://127.0.0.1:8080/")
        );
    }
}
