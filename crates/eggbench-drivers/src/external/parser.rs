//! Versioned tool-parser contract, independent of process spawning.

use super::command::ExternalCommandOutcome;
use super::error::{DriverError, ErrorCategory};

/// Parser identifier for the M001 fixture parser.
pub const FIXTURE_PARSER_ID: &str = "eggbench-fixture/v1";

/// Driver-specific intermediate parse result.
///
/// Later adapters map this to `RawMetricObservation`, `RawHistogramInput`,
/// error-category counts, or additional raw artifacts.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedExternalOutput {
    /// Parser identifier and version.
    pub parser_id: String,
    /// Tool version string extracted from machine output.
    pub tool_version: String,
    /// Whether required output was truncated.
    pub truncated: bool,
}

/// Parser failure categories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExternalParseError {
    /// Tool version is unsupported.
    UnsupportedVersion(String),
    /// Required output was truncated.
    TruncatedRequiredOutput,
    /// Output is malformed machine output.
    MalformedOutput(String),
    /// A required field is missing.
    MissingField(String),
    /// Source data is nonfinite or domain-invalid.
    DomainInvalid(String),
}

/// Tool-parser contract: pure function of a command outcome.
pub trait ExternalOutputParser: Send + Sync {
    /// Stable parser identifier and version.
    fn parser_id(&self) -> &'static str;

    /// Parse a completed command outcome.
    ///
    /// # Errors
    /// Returns [`DriverError`] with category [`ErrorCategory::ParseFailed`]
    /// on unsupported versions, truncation, malformed output, missing
    /// fields, or domain-invalid data. A parser must never mutate the
    /// invocation into a different load/protocol semantic.
    fn parse(&self, outcome: &ExternalCommandOutcome) -> Result<ParsedExternalOutput, DriverError>;
}

/// Trivial fixture parser used only to qualify the contract.
///
/// Accepts retained stdout of the form `eggbench-fixture X.Y.Z` and requires
/// the version token to be present in retained bytes when truncation is set.
#[derive(Debug, Default, Clone, Copy)]
pub struct FixtureParser;

/// Fixture-qualified parse output (alias for documentation clarity).
pub type FixtureParsedOutput = ParsedExternalOutput;

impl ExternalOutputParser for FixtureParser {
    fn parser_id(&self) -> &'static str {
        FIXTURE_PARSER_ID
    }

    fn parse(&self, outcome: &ExternalCommandOutcome) -> Result<ParsedExternalOutput, DriverError> {
        if outcome.exit_code != Some(0) {
            return Err(DriverError::parse(
                ErrorCategory::ParseFailed,
                "fixture parser requires exit code 0",
            ));
        }
        let text = String::from_utf8_lossy(outcome.stdout.retained());
        let Some(version) = extract_fixture_version(&text) else {
            if outcome.stdout.truncated() {
                return Err(DriverError::parse(
                    ErrorCategory::ParseFailed,
                    "truncated required fixture output",
                ));
            }
            return Err(DriverError::parse(
                ErrorCategory::ParseFailed,
                "malformed fixture output",
            ));
        };
        Ok(ParsedExternalOutput {
            parser_id: Self::FIXTURE_PARSER_ID.to_owned(),
            tool_version: version,
            truncated: outcome.stdout.truncated(),
        })
    }
}

impl FixtureParser {
    /// Stable parser identifier.
    pub const FIXTURE_PARSER_ID: &'static str = FIXTURE_PARSER_ID;
}

fn extract_fixture_version(text: &str) -> Option<String> {
    for token in text.split_whitespace() {
        let candidate = token.trim_matches(',');
        if candidate.chars().filter(|c| *c == '.').count() >= 1
            && candidate.chars().any(|c| c.is_ascii_digit())
            && candidate.chars().all(|c| c.is_ascii_digit() || c == '.')
        {
            return Some(candidate.to_owned());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::external::command::CapturedStream;
    use crate::external::resolver::ResolvedExecutable;
    use std::time::Duration;

    fn outcome_with_stdout(
        bytes: &[u8],
        truncated: bool,
        exit_code: Option<i32>,
    ) -> ExternalCommandOutcome {
        ExternalCommandOutcome {
            executable: dummy_exe(),
            argc: 2,
            exit_code,
            stdout: captured(bytes.to_vec(), truncated),
            stderr: captured(Vec::new(), false),
            duration: Duration::from_millis(1),
            cancelled: false,
            timed_out: false,
            cleanup_notes: Vec::new(),
        }
    }

    fn captured(retained: Vec<u8>, truncated: bool) -> CapturedStream {
        if truncated {
            let keep = retained.len().saturating_sub(1);
            let kept = retained[..keep].to_vec();
            return CapturedStream::collect(kept, keep as u64 + 101, keep as u64);
        }
        let total = retained.len() as u64;
        let limit = total.max(1);
        CapturedStream::collect(retained, total, limit)
    }

    fn dummy_exe() -> ResolvedExecutable {
        ResolvedExecutable {
            logical_tool: "fixture".to_owned(),
            selected_path: std::path::PathBuf::from("/tmp/fixture"),
            canonical_path: std::path::PathBuf::from("/tmp/fixture"),
            sha256_hex: "00".repeat(32),
            file_size: 1,
            executable_class: "test".to_owned(),
        }
    }

    #[test]
    fn fixture_parser_accepts_version() {
        let parser = FixtureParser;
        let outcome = outcome_with_stdout(b"eggbench-fixture 1.2.3\n", false, Some(0));
        let parsed = parser.parse(&outcome).unwrap();
        assert_eq!(parsed.tool_version, "1.2.3");
        assert_eq!(parsed.parser_id, FIXTURE_PARSER_ID);
    }

    #[test]
    fn fixture_parser_rejects_malformed_output() {
        let parser = FixtureParser;
        let outcome = outcome_with_stdout(b"not-json{{{{\n", false, Some(0));
        let err = parser.parse(&outcome).unwrap_err();
        assert_eq!(err.category(), ErrorCategory::ParseFailed);
    }

    #[test]
    fn fixture_parser_rejects_nonzero_exit() {
        let parser = FixtureParser;
        let outcome = outcome_with_stdout(b"eggbench-fixture 1.2.3\n", false, Some(1));
        assert!(parser.parse(&outcome).is_err());
    }

    #[test]
    fn fixture_parser_rejects_truncated_required_output() {
        let parser = FixtureParser;
        let outcome = outcome_with_stdout(b"eggbench-fixt", true, Some(0));
        assert!(parser.parse(&outcome).is_err());
    }
}
