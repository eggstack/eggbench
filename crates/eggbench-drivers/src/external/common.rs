//! Shared helpers for external-process workload adapters (oracles M002).
//!
//! Tool ownership recap: oha/h2load/iperf3 own load generation and their
//! machine-output semantics. Eggbench owns binary resolution, version
//! policy, argv construction from the plan workload, bounded execution,
//! raw retention, metric mapping, and evidence. No tool output format is
//! redefined here; parsers validate the documented shapes and fail closed.

use super::error::{DriverError, ErrorCategory};
use eggbench_runner::{FailureCategory, InvocationContext};
use std::collections::BTreeMap;
use std::ffi::OsString;

/// Runtime-bindings key carrying the target HTTP URL for managed services.
pub const TARGET_HTTP_URL_KEY: &str = "http_url";

/// Default iperf3 server port when the target URL carries no explicit port.
pub const DEFAULT_IPERF3_PORT: u16 = 5201;

/// Deterministic locale for tool execution (matches version probing).
pub fn driver_env() -> BTreeMap<OsString, OsString> {
    let mut env = BTreeMap::new();
    env.insert("LC_ALL".into(), "C".into());
    env.insert("LANG".into(), "C".into());
    env
}

/// Resolve the target HTTP URL from the invocation bindings snapshot.
pub fn target_http_url(
    context: &InvocationContext,
    target: &str,
) -> Result<String, FailureCategory> {
    context
        .bindings
        .service_bindings(target)
        .and_then(|bindings| bindings.get(TARGET_HTTP_URL_KEY))
        .cloned()
        .ok_or(FailureCategory::WorkloadFailed)
}

/// Split an `http(s)://authority[/…]` URL into `(host, port)`.
///
/// No new URL-parsing dependency is introduced: the adapter only needs the
/// authority section, and anything outside `host[:port]` (IPv6 brackets
/// honored) is rejected explicitly rather than guessed.
pub fn authority_host_port(url: &str, default_port: u16) -> Result<(String, u16), String> {
    let after_scheme = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    let authority = after_scheme
        .split(['/'])
        .next()
        .unwrap_or_default()
        .split('@')
        .next_back()
        .unwrap_or_default();
    if authority.is_empty() {
        return Err(format!("target URL has no authority section: {url:?}"));
    }
    if let Some(bracketed) = authority.strip_prefix('[') {
        let (host, rest) = bracketed
            .split_once(']')
            .ok_or_else(|| format!("malformed IPv6 authority: {authority:?}"))?;
        if host.is_empty() {
            return Err(format!("empty IPv6 host: {authority:?}"));
        }
        let port = match rest.strip_prefix(':') {
            Some(port_str) => parse_port(port_str)?,
            None if rest.is_empty() => default_port,
            None => return Err(format!("trailing authority text: {rest:?}")),
        };
        return Ok((host.to_owned(), port));
    }
    if authority.contains(':') {
        let (host, port_str) = authority.rsplit_once(':').expect("contains checked above");
        if host.is_empty() {
            return Err(format!("empty host: {authority:?}"));
        }
        return Ok((host.to_owned(), parse_port(port_str)?));
    }
    Ok((authority.to_owned(), default_port))
}

fn parse_port(text: &str) -> Result<u16, String> {
    text.parse::<u16>()
        .map_err(|_| format!("invalid port: {text:?}"))
}

/// Map a substrate error to the runner failure taxonomy.
///
/// Cancellation and timeout keep their first-class categories; everything
/// else (resolution, probe, spawn, nonzero exit, parse) is a workload
/// failure with the stable category preserved in the detail string.
#[must_use]
pub fn failure_category(error: DriverError) -> FailureCategory {
    match error.category() {
        ErrorCategory::Cancelled => FailureCategory::Cancelled,
        ErrorCategory::TimedOut => FailureCategory::TimedOut,
        _ => FailureCategory::WorkloadFailed,
    }
}

/// Cancellation-aware mapping for version probes.
///
/// [`super::version::VersionProbe`] folds cancellation into
/// `VersionProbeTimeout`, so a cancelled token must be re-checked: probe
/// errors under cancellation are `Cancelled`, never workload failures.
#[must_use]
pub fn probe_failure_category(
    error: DriverError,
    cancel: &tokio_util::sync::CancellationToken,
) -> FailureCategory {
    if cancel.is_cancelled() {
        return FailureCategory::Cancelled;
    }
    failure_category(error)
}

/// Enforce a minimum `major.minor.patch` tool version from a probe token.
///
/// The token comes from [`super::version::VersionProbe`] (first `N.N`
/// token); only the leading numeric `major.minor.patch` components are
/// compared, extras ignored. Unparseable tokens fail closed as
/// unsupported rather than being accepted on trust.
pub fn check_min_version(
    tool: &str,
    version: &str,
    minimum: (u64, u64, u64),
) -> Result<(), DriverError> {
    let parsed = parse_version_tuple(version);
    match parsed {
        Some(found)
            if found.0 > minimum.0
                || (found.0 == minimum.0 && found.1 > minimum.1)
                || (found.0 == minimum.0 && found.1 == minimum.1 && found.2 >= minimum.2) =>
        {
            Ok(())
        }
        _ => Err(DriverError::probe(
            ErrorCategory::UnsupportedVersion,
            format!(
                "{tool} version {version:?} is below minimum {}.{}.{}",
                minimum.0, minimum.1, minimum.2
            ),
        )),
    }
}

fn parse_version_tuple(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split('.');
    let major = parts
        .next()?
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse::<u64>()
        .ok()?;
    let minor = parts
        .next()
        .unwrap_or("0")
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse::<u64>()
        .ok()?;
    let patch = parts
        .next()
        .unwrap_or("0")
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse::<u64>()
        .ok()?;
    Some((major, minor, patch))
}

/// Require a finite, non-negative measurement.
pub fn finite_non_negative(value: f64, field: &str) -> Result<f64, String> {
    if value.is_finite() && value >= 0.0 {
        Ok(value)
    } else {
        Err(format!("nonfinite or negative {field}: {value}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authority_parsing_covers_loopback_forms() {
        assert_eq!(
            authority_host_port("http://127.0.0.1:18321/", DEFAULT_IPERF3_PORT).unwrap(),
            ("127.0.0.1".to_owned(), 18321)
        );
        assert_eq!(
            authority_host_port("http://127.0.0.1/", DEFAULT_IPERF3_PORT).unwrap(),
            ("127.0.0.1".to_owned(), 5201)
        );
        assert_eq!(
            authority_host_port("http://[::1]:5202/x", DEFAULT_IPERF3_PORT).unwrap(),
            ("::1".to_owned(), 5202)
        );
        assert!(authority_host_port("http:///path", DEFAULT_IPERF3_PORT).is_err());
        assert!(authority_host_port("http://host:notaport/", DEFAULT_IPERF3_PORT).is_err());
        assert!(authority_host_port("http://:8080/", DEFAULT_IPERF3_PORT).is_err());
    }

    #[test]
    fn version_floor_accepts_newer_rejects_older() {
        assert!(check_min_version("oha", "1.16.0", (1, 0, 0)).is_ok());
        assert!(check_min_version("oha", "1.0.0", (1, 0, 0)).is_ok());
        assert!(check_min_version("oha", "0.9.9", (1, 0, 0)).is_err());
        assert!(check_min_version("iperf3", "3.16", (3, 1, 0)).is_ok());
        assert!(check_min_version("iperf3", "3.0.9", (3, 1, 0)).is_err());
        assert!(check_min_version("oha", "not-a-version", (1, 0, 0)).is_err());
    }
}
