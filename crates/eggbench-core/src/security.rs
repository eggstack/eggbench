//! Eggstack M004a security-correctness evidence contract.
//!
//! Eggsec owns payload generation, WAF detection, bypass-technique
//! execution, and the meaning of `bypass_successful`. Eggbench owns the
//! declarative check selection, target binding, strict-scope preflight,
//! lifecycle placement, the predeclared bypass threshold, and this
//! sanitized typed result. Security results are categorical correctness
//! evidence and never enter `TrialMetrics`.

use crate::{Name, SchemaVersion};
use serde::{Deserialize, Serialize};

/// Run-level `security-checks.json` index schema version.
pub const SECURITY_CHECKS_INDEX_SCHEMA: u32 = 1;
/// Per-check sanitized result schema version.
pub const SECURITY_CHECK_RESULT_SCHEMA: u32 = 1;
/// Canonical Eggsec WAF correctness driver name.
pub const EGGSEC_WAF_DRIVER_NAME: &str = "eggsec-waf";
/// Canonical Eggsec upstream tool name.
pub const EGGSEC_UPSTREAM_NAME: &str = "eggsec";
/// Initial M004a correctness family.
pub const SECURITY_FAMILY_WAF_BYPASS: &str = "waf_bypass";
/// Initial M004a correctness source label.
pub const SECURITY_SOURCE_EGGSEC_WAF: &str = "eggsec-waf";
/// Maximum security checks per run (M004a §5).
pub const MAX_SECURITY_CHECKS: usize = 16;
/// Maximum accepted Eggsec findings per check (bounds parser + threshold).
pub const MAX_SECURITY_CASES: u32 = 1_024;
/// Minimum/maximum security-check concurrency (M004a §5).
pub const MIN_SECURITY_CONCURRENCY: u32 = 1;
/// Maximum security-check concurrency (M004a §5).
pub const MAX_SECURITY_CONCURRENCY: u32 = 32;
/// Minimum security-check timeout in milliseconds (1s, M004a §5).
pub const MIN_SECURITY_TIMEOUT_MS: u64 = 1_000;
/// Maximum security-check timeout in milliseconds (120s, M004a §5).
pub const MAX_SECURITY_TIMEOUT_MS: u64 = 120_000;
/// Correctness adapter semantic version carried in comparability identity.
pub const CORRECTNESS_ADAPTER_SEMANTIC_VERSION: &str = "1";

/// Typed disposition of one sanitized security check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityDisposition {
    /// Observed bypasses within the predeclared allowance.
    Pass,
    /// Observed bypasses exceeded the predeclared allowance.
    Fail,
    /// No trustworthy observation could be produced.
    Invalid,
}

impl SecurityDisposition {
    /// Lowercase label used in evidence and CLI output.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Invalid => "invalid",
        }
    }
}

/// One sanitized Eggsec case: technique/severity/status/bypass only.
///
/// Payload bytes, descriptions, and titles are never persisted. When a
/// payload digest is needed for diagnosis, only its SHA-256 is retained.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SanitizedSecurityCase {
    /// Eggsec technique label (bounded).
    pub technique: String,
    /// Lowercase severity label (bounded).
    pub severity_label: String,
    /// Observed HTTP response status.
    pub response_status: u16,
    /// Eggsec-owned bypass verdict for this case.
    pub bypass_successful: bool,
    /// SHA-256 of the Eggsec payload string (never the payload itself).
    pub payload_sha256: String,
}

/// Versioned Eggbench-owned sanitized security-check result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityCheckResultV1 {
    /// Result schema version (1).
    pub schema_version: SchemaVersion,
    /// Check identity from the plan.
    pub id: Name,
    /// Correctness source (`eggsec-waf`).
    pub source: Name,
    /// Declared target binding.
    pub target: Name,
    /// Requested WAF test family (`sqli`, `xss`, `ssrf`, `cmd`, `traversal`).
    pub test_type: String,
    /// Typed disposition.
    pub disposition: SecurityDisposition,
    /// Number of evaluated Eggsec cases (always >= 1 for pass/fail).
    pub evaluated_cases: u32,
    /// Count of `bypass_successful == true` findings.
    pub successful_bypasses: u32,
    /// Predeclared allowance from the plan.
    pub allowed_successful_bypasses: u32,
    /// Observed Eggsec version string.
    pub producer_version: String,
    /// SHA-256 of the Eggsec executable.
    pub producer_sha256: String,
    /// SHA-256 of the generated strict scope manifest.
    pub scope_sha256: String,
    /// Sanitized per-case projection (no payload bytes).
    pub sanitized_cases: Vec<SanitizedSecurityCase>,
}

impl SecurityCheckResultV1 {
    /// Recompute the disposition from observed counts and the predeclared
    /// allowance. Stored dispositions MUST match this relation.
    #[must_use]
    pub const fn recomputed_disposition(
        evaluated_cases: u32,
        successful_bypasses: u32,
        allowed: u32,
    ) -> SecurityDisposition {
        if evaluated_cases == 0 || successful_bypasses > evaluated_cases {
            return SecurityDisposition::Invalid;
        }
        if successful_bypasses <= allowed {
            SecurityDisposition::Pass
        } else {
            SecurityDisposition::Fail
        }
    }

    /// Validate the result contract before staging or comparison.
    ///
    /// # Errors
    /// Returns a static reason when the contract is violated.
    pub fn validate_contract(&self) -> Result<(), &'static str> {
        if self.schema_version != SchemaVersion(SECURITY_CHECK_RESULT_SCHEMA) {
            return Err("security result schema mismatch");
        }
        if self.source.as_str() != SECURITY_SOURCE_EGGSEC_WAF {
            return Err("security result source mismatch");
        }
        if self.test_type.is_empty()
            || self.test_type.len() > 32
            || !self
                .test_type
                .chars()
                .all(|c| c.is_ascii_lowercase() || c == '_')
        {
            return Err("security result test type is invalid");
        }
        if self.evaluated_cases == 0 || self.evaluated_cases > MAX_SECURITY_CASES {
            return Err("security result evaluated case count is invalid");
        }
        if self.successful_bypasses > self.evaluated_cases {
            return Err("security result bypass count exceeds evaluated cases");
        }
        if self.allowed_successful_bypasses > MAX_SECURITY_CASES {
            return Err("security result allowance exceeds bound");
        }
        if self.sanitized_cases.len() != self.evaluated_cases as usize {
            return Err("security result case projection length mismatch");
        }
        if self.recomputed() != self.disposition {
            return Err("security result disposition disagrees with recomputation");
        }
        if self.producer_version.is_empty() || self.producer_version.len() > 128 {
            return Err("security result producer version is invalid");
        }
        if self.producer_sha256.len() != 64
            || !self.producer_sha256.chars().all(|c| c.is_ascii_hexdigit())
        {
            return Err("security result producer digest is invalid");
        }
        if self.scope_sha256.len() != 64
            || !self.scope_sha256.chars().all(|c| c.is_ascii_hexdigit())
        {
            return Err("security result scope digest is invalid");
        }
        for case in &self.sanitized_cases {
            if case.technique.is_empty() || case.technique.len() > 128 {
                return Err("security result case technique is invalid");
            }
            if case.severity_label.is_empty() || case.severity_label.len() > 32 {
                return Err("security result case severity is invalid");
            }
            if case.payload_sha256.len() != 64
                || !case.payload_sha256.chars().all(|c| c.is_ascii_hexdigit())
            {
                return Err("security result case payload digest is invalid");
            }
        }
        Ok(())
    }

    /// Recompute this result's disposition from its own counts.
    #[must_use]
    pub const fn recomputed(&self) -> SecurityDisposition {
        Self::recomputed_disposition(
            self.evaluated_cases,
            self.successful_bypasses,
            self.allowed_successful_bypasses,
        )
    }
}

/// One row of the run-level security-checks index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityCheckIndexRecord {
    /// Check identity.
    pub id: Name,
    /// Correctness source.
    pub source: Name,
    /// Declared target binding.
    pub target: Name,
    /// Requested WAF test family.
    pub test_type: String,
    /// Typed disposition label (`pass` / `fail` / `invalid`).
    pub disposition: String,
    /// Evaluated case count.
    pub evaluated_cases: u32,
    /// Observed successful bypasses.
    pub successful_bypasses: u32,
    /// Predeclared allowance.
    pub allowed_successful_bypasses: u32,
    /// Bundle-relative artifact path (`security/<id>.json`).
    pub artifact: String,
    /// SHA-256 of the staged per-check artifact bytes.
    pub artifact_sha256: String,
}

/// Run-level `security-checks.json` index (schema v1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityChecksIndex {
    /// Index schema version (1).
    pub schema_version: SchemaVersion,
    /// Canonical correctness driver name (`eggsec-waf`).
    pub driver: String,
    /// Eggbench adapter version.
    pub adapter_version: String,
    /// Observed Eggsec version.
    pub executable_version: String,
    /// SHA-256 of the Eggsec executable.
    pub executable_sha256: String,
    /// Audited Eggsec operation (`waf --json` bypass check).
    pub operation: String,
    /// SHA-256 of the generated strict scope manifest.
    pub scope_sha256: String,
    /// Correctness lifecycle placement version.
    pub lifecycle_placement: String,
    /// One record per requested check, in plan order.
    pub checks: Vec<SecurityCheckIndexRecord>,
}

impl SecurityChecksIndex {
    /// Validate the index contract before staging.
    ///
    /// # Errors
    /// Returns a static reason when the contract is violated.
    pub fn validate_contract(&self) -> Result<(), &'static str> {
        if self.schema_version != SchemaVersion(SECURITY_CHECKS_INDEX_SCHEMA) {
            return Err("security index schema mismatch");
        }
        if self.driver != EGGSEC_WAF_DRIVER_NAME {
            return Err("security index driver mismatch");
        }
        if self.adapter_version.is_empty() || self.adapter_version.len() > 128 {
            return Err("security index adapter version is invalid");
        }
        if self.executable_version.is_empty() || self.executable_version.len() > 128 {
            return Err("security index executable version is invalid");
        }
        if self.executable_sha256.len() != 64 {
            return Err("security index executable digest is invalid");
        }
        if self.operation.is_empty() || self.operation.len() > 256 {
            return Err("security index operation is invalid");
        }
        if self.scope_sha256.len() != 64 {
            return Err("security index scope digest is invalid");
        }
        if self.lifecycle_placement.is_empty() || self.lifecycle_placement.len() > 128 {
            return Err("security index lifecycle placement is invalid");
        }
        if self.checks.len() > MAX_SECURITY_CHECKS {
            return Err("security index check count exceeds bound");
        }
        Ok(())
    }
}

/// Comparison-critical security configuration identity (M004a §19).
///
/// Result values (pass/fail/counts) are NOT configuration identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityConfigIdentity {
    /// Ordered check identities.
    pub check_ids: Vec<String>,
    /// Canonical per-check configuration rows.
    pub checks: Vec<SecurityCheckConfigRow>,
    /// Eggsec executable version.
    pub producer_version: String,
    /// Eggsec executable SHA-256.
    pub producer_sha256: String,
    /// Generated scope digest.
    pub scope_sha256: String,
    /// Correctness adapter semantic version.
    pub adapter_semantic_version: String,
}

/// One comparison-critical per-check configuration row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityCheckConfigRow {
    /// Check identity.
    pub id: String,
    /// Correctness source.
    pub source: String,
    /// Declared target binding.
    pub target: String,
    /// Requested WAF test family.
    pub test_type: String,
    /// Predeclared bypass allowance.
    pub max_successful_bypasses: u32,
    /// Requested concurrency.
    pub concurrency: u32,
    /// Requested timeout in milliseconds.
    pub timeout_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result_with(disposition: SecurityDisposition) -> SecurityCheckResultV1 {
        let (evaluated, successful, allowed) = match disposition {
            SecurityDisposition::Pass => (3, 0, 0),
            SecurityDisposition::Fail => (3, 2, 0),
            SecurityDisposition::Invalid => (0, 0, 0),
        };
        SecurityCheckResultV1 {
            schema_version: SchemaVersion(SECURITY_CHECK_RESULT_SCHEMA),
            id: Name::new("check-1").unwrap(),
            source: Name::new(SECURITY_SOURCE_EGGSEC_WAF).unwrap(),
            target: Name::new("origin").unwrap(),
            test_type: "sqli".to_owned(),
            disposition,
            evaluated_cases: evaluated,
            successful_bypasses: successful,
            allowed_successful_bypasses: allowed,
            producer_version: "0.1.0".to_owned(),
            producer_sha256: "ab".repeat(32),
            scope_sha256: "cd".repeat(32),
            sanitized_cases: (0..evaluated)
                .map(|i| SanitizedSecurityCase {
                    technique: format!("technique-{i}"),
                    severity_label: "high".to_owned(),
                    response_status: 403,
                    bypass_successful: disposition == SecurityDisposition::Fail && i < 2,
                    payload_sha256: "ef".repeat(32),
                })
                .collect(),
        }
    }

    #[test]
    fn disposition_recomputation_matches_threshold() {
        assert_eq!(
            SecurityCheckResultV1::recomputed_disposition(3, 0, 0),
            SecurityDisposition::Pass
        );
        assert_eq!(
            SecurityCheckResultV1::recomputed_disposition(3, 2, 0),
            SecurityDisposition::Fail
        );
        assert_eq!(
            SecurityCheckResultV1::recomputed_disposition(0, 0, 0),
            SecurityDisposition::Invalid
        );
        assert_eq!(
            SecurityCheckResultV1::recomputed_disposition(2, 3, 5),
            SecurityDisposition::Invalid
        );
    }

    #[test]
    fn valid_results_pass_contract_and_mismatch_fails() {
        assert!(
            result_with(SecurityDisposition::Pass)
                .validate_contract()
                .is_ok()
        );
        assert!(
            result_with(SecurityDisposition::Fail)
                .validate_contract()
                .is_ok()
        );
        assert!(
            result_with(SecurityDisposition::Invalid)
                .validate_contract()
                .is_err()
        );
        let mut tampered = result_with(SecurityDisposition::Pass);
        tampered.disposition = SecurityDisposition::Fail;
        assert!(tampered.validate_contract().is_err());
    }
}
