//! Bounded argv-only version probing.

use super::command::ExternalCommandSpec;
use super::command::{CapturedStream, run_command};
use super::error::{DriverError, ErrorCategory};
use super::resolver::ResolvedExecutable;
use std::collections::BTreeMap;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Version probe specification: bounded argv tail plus output caps.
#[derive(Debug, Clone)]
pub struct VersionProbeSpec {
    /// Arguments appended after argv[0] (e.g. `--version`).
    pub argv_tail: Vec<String>,
    /// Probe timeout.
    pub timeout: Duration,
    /// Stdout retention cap.
    pub stdout_limit: u64,
    /// Stderr retention cap.
    pub stderr_limit: u64,
    /// Parser identifier expected to interpret the output.
    pub parser_id: String,
}

/// Tool version provenance: bounded raw output plus parsed version.
#[derive(Debug, Clone)]
pub struct ToolVersion {
    /// Logical tool name.
    pub tool: String,
    /// Executable identity.
    pub executable: ResolvedExecutable,
    /// Bounded captured stdout.
    pub stdout: CapturedStream,
    /// Bounded captured stderr.
    pub stderr: CapturedStream,
    /// Exit code.
    pub exit_code: Option<i32>,
    /// Parsed version string.
    pub version: String,
    /// Parser identifier and version used.
    pub parser_id: String,
}

/// Version probe runner.
pub struct VersionProbe;

impl VersionProbe {
    /// Run the probe: argv[0] is the resolved executable, stdin null, bounded
    /// timeout, cancellable via the supplied token.
    ///
    /// # Errors
    /// Returns typed probe failures; nonzero exit is a failure unless the
    /// parsed output still yields a version and the caller accepts it via
    /// `allow_nonzero` (not used in M001: nonzero is always a probe failure).
    pub async fn run(
        executable: &ResolvedExecutable,
        spec: &VersionProbeSpec,
        cancel: &CancellationToken,
    ) -> Result<ToolVersion, DriverError> {
        let mut args = Vec::with_capacity(spec.argv_tail.len());
        for arg in &spec.argv_tail {
            args.push(std::ffi::OsString::from(arg));
        }
        let command = ExternalCommandSpec {
            executable: executable.clone(),
            args,
            cwd: None,
            env: default_probe_env(),
            stdin_null: true,
            stdout_limit: spec.stdout_limit,
            stderr_limit: spec.stderr_limit,
            timeout: spec.timeout,
        };
        let outcome = run_command(&command, cancel).await.map_err(|e| {
            if matches!(
                e.category(),
                ErrorCategory::TimedOut | ErrorCategory::Cancelled
            ) {
                DriverError::probe(ErrorCategory::VersionProbeTimeout, e.to_string())
            } else {
                DriverError::probe(ErrorCategory::VersionProbeFailed, e.to_string())
            }
        })?;
        if outcome.exit_code != Some(0) {
            return Err(DriverError::probe(
                ErrorCategory::VersionProbeFailed,
                format!("version probe exited with {:?}", outcome.exit_code),
            ));
        }
        let text = String::from_utf8_lossy(outcome.stdout.retained()).into_owned();
        let version = parse_version_token(&text).ok_or_else(|| {
            DriverError::probe(
                ErrorCategory::VersionProbeFailed,
                "version probe output did not contain a version token",
            )
        })?;
        // A parser must not pretend truncated output is complete unless the
        // required version token was retained; here the token was found in
        // retained bytes, so truncation elsewhere is acceptable and visible.
        Ok(ToolVersion {
            tool: executable.logical_tool.clone(),
            executable: executable.clone(),
            stdout: outcome.stdout.clone(),
            stderr: outcome.stderr.clone(),
            exit_code: outcome.exit_code,
            version,
            parser_id: spec.parser_id.clone(),
        })
    }
}

/// Deterministic locale for human/version output parsers.
fn default_probe_env() -> BTreeMap<std::ffi::OsString, std::ffi::OsString> {
    let mut env = BTreeMap::new();
    env.insert("LC_ALL".into(), "C".into());
    env.insert("LANG".into(), "C".into());
    env
}

/// Extract the first `N.N[.N...]`-style token from probe output.
fn parse_version_token(text: &str) -> Option<String> {
    for token in text.split(|c: char| c.is_whitespace() || c == ',') {
        let trimmed: String = token.chars().skip_while(|c| !c.is_ascii_digit()).collect();
        if trimmed.is_empty() {
            continue;
        }
        let candidate: String = trimmed
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        let candidate = candidate.trim_matches('.');
        if candidate.contains('.') && candidate.chars().any(|c| c.is_ascii_digit()) {
            return Some(candidate.to_owned());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_token_extraction_is_documented() {
        assert_eq!(
            parse_version_token("eggbench-fixture 1.2.3\n").as_deref(),
            Some("1.2.3")
        );
        assert_eq!(
            parse_version_token("oha 1.6.3 (abc)").as_deref(),
            Some("1.6.3")
        );
        assert_eq!(parse_version_token("no version here"), None);
        assert_eq!(parse_version_token("not-json{{{{"), None);
    }
}
