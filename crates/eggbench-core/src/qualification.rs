//! Bounded, subject-neutral security qualification inputs.
#![allow(missing_docs)]
use crate::{ExperimentPlan, PlanError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};

pub const SECURITY_PROFILE_SCHEMA_VERSION: u32 = 1;
pub const HTTP_SECURITY_CORPUS_SCHEMA_VERSION: u32 = 1;
pub const QUALIFICATION_EXPANSION_POLICY: &str = "eggbench.security-profile-expansion.v1";
pub const CONTENT_TREE_MAX_FILES: usize = 1024;
pub const CONTENT_TREE_MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
pub const CONTENT_TREE_MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PROFILE_BYTES: u64 = 1024 * 1024;
const MAX_CORPUS_BYTES: u64 = 8 * 1024 * 1024;
const MAX_PLAN_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityQualificationProfileV1 {
    pub schema_version: u32,
    pub id: String,
    pub owner: String,
    pub scenarios: Vec<QualificationScenarioRef>,
    pub corpus: ContentInputRef,
    pub target_config: ContentInputRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationScenarioRef {
    pub id: String,
    pub plan: String,
    /// Explicit baseline bundle path; absent means absolute-only comparison.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_bundle: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentInputRef {
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpSecurityCorpusV1 {
    pub schema_version: u32,
    pub owner: String,
    pub corpus_id: String,
    pub cases: Vec<HttpSecurityCaseV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpSecurityCaseV1 {
    pub id: String,
    pub category: Option<String>,
    pub request: HttpCaseRequestV1,
    pub expectation: HttpObservableExpectationV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpCaseRequestV1 {
    pub method: String,
    pub path_and_query: String,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    #[serde(default)]
    pub body: HttpCaseBodyV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum HttpCaseBodyV1 {
    #[default]
    None,
    InlineUtf8(String),
    File(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum HttpObservableExpectationV1 {
    Exact { status_exact: u16 },
    AnyOf { status_any_of: Vec<u16> },
}

impl HttpObservableExpectationV1 {
    /// Whether a response status satisfies the predeclared expectation.
    #[must_use]
    pub fn matches(&self, status: u16) -> bool {
        match self {
            Self::Exact { status_exact } => status == *status_exact,
            Self::AnyOf { status_any_of } => status_any_of.contains(&status),
        }
    }

    fn validate(&self) -> bool {
        match self {
            Self::Exact { status_exact } => (100..=599).contains(status_exact),
            Self::AnyOf { status_any_of } => {
                !status_any_of.is_empty()
                    && status_any_of.len() <= 32
                    && status_any_of
                        .iter()
                        .all(|status| (100..=599).contains(status))
            }
        }
    }
}

/// Sanitized per-case projection persisted by the HTTP correctness executor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpCorpusCaseResultV1 {
    pub id: String,
    pub case_sha256: String,
    pub expectation: HttpObservableExpectationV1,
    pub observed_status: Option<u16>,
    pub disposition: HttpCorpusCaseDisposition,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Case-level correctness disposition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HttpCorpusCaseDisposition {
    Pass,
    Fail,
    Invalid,
}

/// Bounded and payload-free HTTP corpus result persisted as run evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpCorpusCheckResultV1 {
    pub schema_version: u32,
    pub id: String,
    pub source: String,
    pub family: String,
    pub target: String,
    pub corpus_id: String,
    pub corpus_sha256: String,
    pub adapter_semantic_version: String,
    pub eggfetch_version: String,
    pub cases: Vec<HttpCorpusCaseResultV1>,
}

impl HttpCorpusCheckResultV1 {
    /// Validate the sanitized result's bounds and recomputed case counts.
    ///
    /// # Errors
    /// Returns a stable reason code when the result violates the bounded v1 contract.
    #[allow(clippy::too_many_lines)]
    pub fn validate_contract(&self) -> Result<(), &'static str> {
        if self.schema_version != 1 || self.cases.is_empty() || self.cases.len() > 1024 {
            return Err("invalid_schema_or_case_count");
        }
        if validate_name(&self.id).is_err()
            || self.source != "eggbench-http-corpus"
            || self.family != "http_observable"
            || validate_name(&self.target).is_err()
            || validate_name(&self.corpus_id).is_err()
        {
            return Err("invalid_result_identity");
        }
        if self.corpus_sha256.len() != 64
            || !self
                .corpus_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("invalid_corpus_digest");
        }
        if self.adapter_semantic_version.is_empty()
            || self.adapter_semantic_version.len() > 128
            || self.eggfetch_version.is_empty()
            || self.eggfetch_version.len() > 128
            || self.adapter_semantic_version.chars().any(char::is_control)
            || self.eggfetch_version.chars().any(char::is_control)
        {
            return Err("invalid_producer_identity");
        }
        let mut seen = BTreeSet::new();
        for case in &self.cases {
            validate_name(&case.id).map_err(|_| "invalid_case_id")?;
            if !seen.insert(&case.id)
                || case.case_sha256.len() != 64
                || !case
                    .case_sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
                || case.reason.as_ref().is_some_and(|reason| {
                    !matches!(
                        reason.as_str(),
                        "transport_failure"
                            | "invalid_input"
                            | "unsupported_protocol"
                            | "check_timeout"
                    )
                })
                || !case.expectation.validate()
            {
                return Err("invalid_case_record");
            }
            if case.disposition == HttpCorpusCaseDisposition::Invalid
                && case.observed_status.is_some()
            {
                return Err("invalid_case_has_status");
            }
            if (case.disposition == HttpCorpusCaseDisposition::Invalid) != case.reason.is_some() {
                return Err("invalid_case_reason_mismatch");
            }
            if case.disposition != HttpCorpusCaseDisposition::Invalid
                && case.observed_status.is_none()
            {
                return Err("observed_case_missing_status");
            }
            if let Some(status) = case.observed_status {
                if !(100..=599).contains(&status) {
                    return Err("invalid_observed_status");
                }
                let matched = case.expectation.matches(status);
                if (case.disposition == HttpCorpusCaseDisposition::Pass) != matched
                    || case.disposition == HttpCorpusCaseDisposition::Invalid
                {
                    return Err("case_disposition_mismatch");
                }
            }
        }
        Ok(())
    }

    /// Number of cases by disposition, in (evaluated, passed, failed, invalid) order.
    #[must_use]
    pub fn counts(&self) -> (u32, u32, u32, u32) {
        let mut passed = 0;
        let mut failed = 0;
        let mut invalid = 0;
        for case in &self.cases {
            match case.disposition {
                HttpCorpusCaseDisposition::Pass => passed += 1,
                HttpCorpusCaseDisposition::Fail => failed += 1,
                HttpCorpusCaseDisposition::Invalid => invalid += 1,
            }
        }
        (
            u32::try_from(self.cases.len()).unwrap_or(u32::MAX),
            passed,
            failed,
            invalid,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentFileIdentity {
    pub path: String,
    pub length: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentTreeIdentity {
    pub aggregate_sha256: String,
    pub file_count: usize,
    pub total_bytes: u64,
    pub files: Vec<ContentFileIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QualificationExpansionV1 {
    pub schema_version: u32,
    pub expansion_policy: String,
    pub profile_id: String,
    pub profile_sha256: String,
    pub corpus_identity: ContentTreeIdentity,
    pub target_config_identity: ContentTreeIdentity,
    pub scenarios: Vec<ExpandedScenarioV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpandedScenarioV1 {
    pub id: String,
    pub source_plan_sha256: String,
    pub source_plan_schema: u32,
    pub source_plan_path_context: String,
    /// Frozen explicit baseline identity, resolved before candidate execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_identity: Option<crate::BundleIdentity>,
    /// Workspace-relative baseline path context, when one was declared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_path_context: Option<String>,
}

/// Final profile-level evidence referencing ordinary bundles and comparisons.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityQualificationReceiptV1 {
    pub schema_version: u32,
    pub policy_id: String,
    pub created_by_version: String,
    pub profile_id: String,
    pub profile_sha256: String,
    pub expansion_sha256: String,
    pub corpus_identity: ContentTreeIdentity,
    pub target_config_identity: ContentTreeIdentity,
    pub scenarios: Vec<QualificationScenarioRecordV1>,
    pub aggregate_verdict: crate::AggregateVerdict,
    pub execution_complete: bool,
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// One required scenario's state and ordinary evidence references.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationScenarioRecordV1 {
    pub id: String,
    pub source_plan_sha256: String,
    pub status: QualificationScenarioStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_bundle_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_bundle_identity: Option<crate::BundleIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_bundle_identity: Option<crate::BundleIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comparison_receipt_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comparison_receipt_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub performance_verdict: Option<crate::AggregateVerdict>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correctness_verdict: Option<crate::AggregateVerdict>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub combined_verdict: Option<crate::AggregateVerdict>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Finalized, invalid, cancelled, or not-started scenario state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationScenarioStatus {
    Completed,
    Invalid,
    Cancelled,
    NotRun,
}

/// Immutable qualification aggregate policy identifier.
pub const QUALIFICATION_RECEIPT_POLICY_V1: &str = "eggbench.security-qualification.v1";

/// SHA-256 hex digest used for qualification artifact references.
#[must_use]
pub fn qualification_sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Combine typed verdicts using Invalid > Fail > Inconclusive > Pass.
#[must_use]
pub fn aggregate_qualification_verdicts<I>(verdicts: I) -> crate::AggregateVerdict
where
    I: IntoIterator<Item = crate::AggregateVerdict>,
{
    let mut result = crate::AggregateVerdict::Pass;
    for value in verdicts {
        result = match (result, value) {
            (crate::AggregateVerdict::Invalid, _) | (_, crate::AggregateVerdict::Invalid) => {
                crate::AggregateVerdict::Invalid
            }
            (crate::AggregateVerdict::Fail, _) | (_, crate::AggregateVerdict::Fail) => {
                crate::AggregateVerdict::Fail
            }
            (crate::AggregateVerdict::Inconclusive, _)
            | (_, crate::AggregateVerdict::Inconclusive) => crate::AggregateVerdict::Inconclusive,
            _ => crate::AggregateVerdict::Pass,
        };
    }
    result
}

/// Aggregate required scenario records, treating incomplete/missing verdicts as Invalid.
#[must_use]
pub fn aggregate_qualification_scenarios(
    scenarios: &[QualificationScenarioRecordV1],
) -> crate::AggregateVerdict {
    if scenarios.is_empty()
        || scenarios.iter().any(|scenario| {
            scenario.status != QualificationScenarioStatus::Completed
                || scenario.combined_verdict.is_none()
        })
    {
        return crate::AggregateVerdict::Invalid;
    }
    aggregate_qualification_verdicts(
        scenarios
            .iter()
            .filter_map(|scenario| scenario.combined_verdict),
    )
}

#[cfg(test)]
mod suite_policy_tests {
    use super::aggregate_qualification_verdicts as aggregate;
    use crate::AggregateVerdict as V;

    #[test]
    fn qualification_aggregation_obeys_locked_precedence() {
        assert_eq!(aggregate([V::Pass, V::Pass]), V::Pass);
        assert_eq!(aggregate([V::Pass, V::Inconclusive]), V::Inconclusive);
        assert_eq!(aggregate([V::Fail, V::Inconclusive]), V::Fail);
        assert_eq!(aggregate([V::Fail, V::Invalid]), V::Invalid);
    }

    #[test]
    fn end_to_end_matrix_a_to_g_has_conservative_outcomes() {
        use super::{QualificationScenarioRecordV1 as R, QualificationScenarioStatus as S};
        let record = |status, verdict| R {
            id: "case".into(),
            source_plan_sha256: "a".repeat(64),
            status,
            candidate_bundle_path: None,
            candidate_bundle_identity: None,
            baseline_bundle_identity: None,
            comparison_receipt_path: None,
            comparison_receipt_sha256: None,
            performance_verdict: None,
            correctness_verdict: None,
            combined_verdict: verdict,
            reason: None,
        };
        let completed = |v| record(S::Completed, Some(v));
        assert_eq!(
            super::aggregate_qualification_scenarios(&[completed(V::Pass)]),
            V::Pass
        ); // A
        assert_eq!(
            super::aggregate_qualification_scenarios(&[completed(V::Fail)]),
            V::Fail
        ); // B/C
        assert_eq!(
            super::aggregate_qualification_scenarios(&[completed(V::Inconclusive)]),
            V::Inconclusive
        ); // D
        assert_eq!(
            super::aggregate_qualification_scenarios(&[completed(V::Invalid)]),
            V::Invalid
        ); // E
        assert_eq!(
            super::aggregate_qualification_scenarios(&[record(S::NotRun, None)]),
            V::Invalid
        ); // F
        assert_eq!(
            super::aggregate_qualification_scenarios(&[record(S::Cancelled, None)]),
            V::Invalid
        ); // G
    }
}

#[derive(Debug, thiserror::Error)]
pub enum QualificationInputError {
    #[error("invalid qualification input: {0}")]
    Invalid(String),
    #[error("unsafe content path: {0}")]
    UnsafePath(String),
    #[error("content bound exceeded: {0}")]
    Bound(String),
    #[error("content I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("plan reference invalid: {0}")]
    Plan(#[from] PlanError),
    #[error("serialization failed: {0}")]
    Json(#[from] serde_json::Error),
}

impl SecurityQualificationProfileV1 {
    /// Parse and validate a JSON profile.
    ///
    /// # Errors
    /// Returns an error when JSON or profile invariants are invalid.
    pub fn from_json(input: &str) -> Result<Self, QualificationInputError> {
        let value: Self = serde_json::from_str(input)?;
        value.validate()?;
        Ok(value)
    }
    /// Validate profile bounds, identifiers, and workspace-relative paths.
    ///
    /// # Errors
    /// Returns an error when any profile field violates the v1 contract.
    #[allow(clippy::too_many_lines)]
    pub fn validate(&self) -> Result<(), QualificationInputError> {
        if self.schema_version != 1 {
            return Err(QualificationInputError::Invalid(
                "unsupported profile schema".into(),
            ));
        }
        validate_name(&self.id)?;
        validate_text(&self.owner, 256, "owner")?;
        if !(1..=32).contains(&self.scenarios.len()) {
            return Err(QualificationInputError::Bound(
                "scenario count must be 1..=32".into(),
            ));
        }
        let mut ids = BTreeSet::new();
        let mut paths = BTreeSet::new();
        for s in &self.scenarios {
            validate_name(&s.id)?;
            validate_relative(&s.plan)?;
            if !ids.insert(&s.id) {
                return Err(QualificationInputError::Invalid(
                    "duplicate scenario ID".into(),
                ));
            }
            if !paths.insert(&s.plan) {
                return Err(QualificationInputError::Invalid(
                    "duplicate scenario plan".into(),
                ));
            }
        }
        validate_relative(&self.corpus.path)?;
        validate_relative(&self.target_config.path)?;
        Ok(())
    }
}

impl HttpSecurityCorpusV1 {
    /// Parse and validate a JSON corpus.
    ///
    /// # Errors
    /// Returns an error when JSON or corpus invariants are invalid.
    pub fn from_json(input: &str) -> Result<Self, QualificationInputError> {
        let value: Self = serde_json::from_str(input)?;
        value.validate()?;
        Ok(value)
    }
    /// Validate case bounds and normalized HTTP request fields.
    ///
    /// # Errors
    /// Returns an error when a case or expectation violates the v1 contract.
    #[allow(clippy::too_many_lines)]
    pub fn validate(&self) -> Result<(), QualificationInputError> {
        if self.schema_version != 1 {
            return Err(QualificationInputError::Invalid(
                "unsupported corpus schema".into(),
            ));
        }
        validate_text(&self.owner, 256, "owner")?;
        validate_name(&self.corpus_id)?;
        if !(1..=1024).contains(&self.cases.len()) {
            return Err(QualificationInputError::Bound(
                "case count must be 1..=1024".into(),
            ));
        }
        let mut ids = BTreeSet::new();
        for c in &self.cases {
            validate_name(&c.id)?;
            if !ids.insert(&c.id) {
                return Err(QualificationInputError::Invalid("duplicate case ID".into()));
            }
            if let Some(category) = &c.category {
                validate_text(category, 256, "category")?;
            }
            let r = &c.request;
            if r.method.is_empty() || r.method.len() > 32 || !r.method.bytes().all(is_token) {
                return Err(QualificationInputError::Invalid(
                    "invalid HTTP method".into(),
                ));
            }
            if r.path_and_query.is_empty()
                || r.path_and_query.len() > 8192
                || !r.path_and_query.starts_with('/')
                || r.path_and_query.starts_with("//")
                || r.path_and_query
                    .bytes()
                    .any(|b| b <= 0x20 || b == 0x7f || b == b'#' || b == b'\\')
            {
                return Err(QualificationInputError::Invalid(
                    "request target must be a bounded relative origin-form path".into(),
                ));
            }
            if r.headers.len() > 64 {
                return Err(QualificationInputError::Bound("header count".into()));
            }
            let mut header_bytes = 0usize;
            for (k, v) in &r.headers {
                header_bytes += k.len() + v.len();
                if !k.bytes().all(is_token) || v.bytes().any(|b| b == b'\r' || b == b'\n' || b == 0)
                {
                    return Err(QualificationInputError::Invalid(
                        "invalid HTTP header".into(),
                    ));
                }
                if matches!(
                    k.to_ascii_lowercase().as_str(),
                    "authorization"
                        | "proxy-authorization"
                        | "cookie"
                        | "set-cookie"
                        | "host"
                        | "connection"
                        | "proxy-connection"
                        | "transfer-encoding"
                        | "content-length"
                ) {
                    return Err(QualificationInputError::Invalid(
                        "credential-bearing header is forbidden".into(),
                    ));
                }
            }
            if header_bytes > 16 * 1024 {
                return Err(QualificationInputError::Bound("header bytes".into()));
            }
            match &r.body {
                HttpCaseBodyV1::InlineUtf8(s)
                    if u64::try_from(s.len()).unwrap_or(u64::MAX) > CONTENT_TREE_MAX_FILE_BYTES =>
                {
                    return Err(QualificationInputError::Bound("inline body bytes".into()));
                }
                HttpCaseBodyV1::File(p) => validate_relative(p)?,
                _ => {}
            }
            match &c.expectation {
                HttpObservableExpectationV1::Exact { status_exact }
                    if !(100..=599).contains(status_exact) =>
                {
                    return Err(QualificationInputError::Invalid(
                        "invalid exact status".into(),
                    ));
                }
                HttpObservableExpectationV1::AnyOf { status_any_of }
                    if status_any_of.is_empty()
                        || status_any_of.len() > 32
                        || status_any_of.iter().any(|s| !(100..=599).contains(s)) =>
                {
                    return Err(QualificationInputError::Invalid(
                        "invalid status set".into(),
                    ));
                }
                _ => {}
            }
        }
        Ok(())
    }
}

/// Compute bounded canonical identity for a workspace-confined file or tree.
///
/// # Errors
/// Returns an error for paths outside the workspace, symlinks, special files,
/// filesystem errors, or exceeded content limits.
pub fn content_tree_identity(
    workspace: &Path,
    requested: &str,
) -> Result<ContentTreeIdentity, QualificationInputError> {
    validate_relative(requested)?;
    let root = fs::canonicalize(workspace)?;
    let requested_path = root.join(requested);
    let mut cursor = root.clone();
    for component in Path::new(requested).components() {
        let Component::Normal(part) = component else {
            unreachable!("validated relative path")
        };
        cursor.push(part);
        if fs::symlink_metadata(&cursor)?.file_type().is_symlink() {
            return Err(QualificationInputError::UnsafePath(
                "symlink path components are not accepted".into(),
            ));
        }
    }
    let canonical = canonical_confined_path(&root, requested)?;
    if !canonical.starts_with(&root) {
        return Err(QualificationInputError::UnsafePath(requested.to_owned()));
    }
    let mut records = Vec::new();
    let mut total = 0u64;
    let meta = fs::symlink_metadata(&requested_path)?;
    if meta.file_type().is_symlink() {
        return Err(QualificationInputError::UnsafePath(
            "symlinks are not accepted".into(),
        ));
    }
    let tree_root = if meta.is_dir() {
        canonical.clone()
    } else {
        canonical.parent().unwrap_or(&root).to_path_buf()
    };
    if meta.is_file() {
        hash_file(
            &requested_path,
            canonical
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            &mut records,
            &mut total,
        )?;
    } else if meta.is_dir() {
        let mut stack = vec![(canonical, 0usize)];
        while let Some((dir, depth)) = stack.pop() {
            if depth > 16 {
                return Err(QualificationInputError::Bound("directory depth".into()));
            }
            for e in fs::read_dir(&dir)? {
                let e = e?;
                let p = e.path();
                let m = fs::symlink_metadata(&p)?;
                if m.file_type().is_symlink() {
                    return Err(QualificationInputError::UnsafePath(
                        "tree contains symlink".into(),
                    ));
                }
                if m.is_dir() {
                    stack.push((p, depth + 1));
                } else if m.is_file() {
                    let rel = p
                        .strip_prefix(&tree_root)
                        .map_err(|_| {
                            QualificationInputError::UnsafePath("path escaped tree".into())
                        })?
                        .to_string_lossy()
                        .replace('\\', "/");
                    if rel.len() > 512 {
                        return Err(QualificationInputError::Bound(
                            "relative path length".into(),
                        ));
                    }
                    hash_file(&p, rel, &mut records, &mut total)?;
                } else {
                    return Err(QualificationInputError::UnsafePath("special file".into()));
                }
            }
        }
    } else {
        return Err(QualificationInputError::Invalid(
            "input must be a regular file or directory".into(),
        ));
    }
    records.sort_by(|a: &ContentFileIdentity, b| a.path.cmp(&b.path));
    Ok(ContentTreeIdentity {
        aggregate_sha256: aggregate_content_files(&records),
        file_count: records.len(),
        total_bytes: total,
        files: records,
    })
}

/// Resolve a profile into deterministic, immutable input identities.
///
/// # Errors
/// Returns an error when profile, corpus, target configuration, or scenario
/// plans fail validation or confinement.
pub fn expand_qualification_profile(
    workspace: &Path,
    profile_path: &str,
) -> Result<QualificationExpansionV1, QualificationInputError> {
    validate_relative(profile_path)?;
    let root = fs::canonicalize(workspace)?;
    let canonical = canonical_confined_path(&root, profile_path)?;
    let bytes = read_bounded(&canonical, MAX_PROFILE_BYTES)?;
    let profile: SecurityQualificationProfileV1 = serde_json::from_slice(&bytes)?;
    profile.validate()?;
    let mut corpus = content_tree_identity(workspace, &profile.corpus.path)?;
    let corpus_file = root.join(&profile.corpus.path);
    if corpus_file.is_file() {
        let corpus_bytes = read_bounded(&corpus_file, MAX_CORPUS_BYTES)?;
        let corpus_model: HttpSecurityCorpusV1 = serde_json::from_slice(&corpus_bytes)?;
        corpus_model.validate()?;
        let corpus_parent = Path::new(&profile.corpus.path)
            .parent()
            .unwrap_or(Path::new("."));
        for case in &corpus_model.cases {
            if let HttpCaseBodyV1::File(body) = &case.request.body {
                let body_path = corpus_parent.join(body);
                let body_text = body_path.to_string_lossy().replace('\\', "/");
                let identity = content_tree_identity(workspace, &body_text)?;
                for mut record in identity.files {
                    record.path = format!("body/{}/{}", case.id, record.path);
                    corpus.total_bytes += record.length;
                    corpus.files.push(record);
                }
            }
        }
        corpus.files.sort_by(|a, b| a.path.cmp(&b.path));
        if corpus.total_bytes > CONTENT_TREE_MAX_TOTAL_BYTES {
            return Err(QualificationInputError::Bound(
                "corpus plus body aggregate bytes".into(),
            ));
        }
        if corpus.files.len() > CONTENT_TREE_MAX_FILES {
            return Err(QualificationInputError::Bound(
                "corpus plus body file count".into(),
            ));
        }
        corpus.file_count = corpus.files.len();
        let mut digest = Sha256::new();
        for record in &corpus.files {
            digest.update(record.path.as_bytes());
            digest.update([0]);
            digest.update(record.length.to_be_bytes());
            digest.update([0]);
            digest.update(record.sha256.as_bytes());
            digest.update([0]);
        }
        corpus.aggregate_sha256 = format!("{:x}", digest.finalize());
    }
    let config = content_tree_identity(workspace, &profile.target_config.path)?;
    let mut scenarios = Vec::new();
    for s in &profile.scenarios {
        let real = canonical_confined_path(&root, &s.plan)?;
        let data = read_bounded(&real, MAX_PLAN_BYTES)?;
        let text = std::str::from_utf8(&data)
            .map_err(|e| QualificationInputError::Invalid(e.to_string()))?;
        let parsed = match real.extension().and_then(|e| e.to_str()) {
            Some("toml") => ExperimentPlan::from_toml(text)?,
            _ => ExperimentPlan::from_json(text)?,
        };
        parsed.validate()?;
        let digest = format!("{:x}", Sha256::digest(&data));
        scenarios.push(ExpandedScenarioV1 {
            id: s.id.clone(),
            source_plan_sha256: digest,
            source_plan_schema: parsed.schema_version.0,
            source_plan_path_context: s.plan.clone(),
            baseline_identity: if let Some(path) = &s.baseline_bundle {
                validate_relative(path)?;
                let baseline = canonical_confined_path(&root, path)?;
                let (_, input) = crate::load_baseline_bundle(&baseline).map_err(|e| {
                    QualificationInputError::Invalid(format!("baseline is invalid: {e}"))
                })?;
                Some(input.identity)
            } else {
                None
            },
            baseline_path_context: s.baseline_bundle.clone(),
        });
    }
    Ok(QualificationExpansionV1 {
        schema_version: 1,
        expansion_policy: QUALIFICATION_EXPANSION_POLICY.into(),
        profile_id: profile.id,
        profile_sha256: format!("{:x}", Sha256::digest(&bytes)),
        corpus_identity: corpus,
        target_config_identity: config,
        scenarios,
    })
}

/// Load one corpus through a confined relative path, verify its frozen file
/// digest, and validate every referenced body file before a run starts.
///
/// # Errors
/// Returns an error when the reference escapes the workspace, content exceeds
/// configured bounds, or the corpus/content digest does not match.
pub fn load_http_security_corpus(
    workspace: &Path,
    relative: &str,
    expected_sha256: &str,
) -> Result<(HttpSecurityCorpusV1, PathBuf), QualificationInputError> {
    let root = fs::canonicalize(workspace)?;
    let corpus_path = canonical_confined_path(&root, relative)?;
    let bytes = read_bounded(&corpus_path, MAX_CORPUS_BYTES)?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|error| QualificationInputError::Invalid(error.to_string()))?;
    let corpus = HttpSecurityCorpusV1::from_json(text)?;
    let body_root = corpus_path
        .parent()
        .ok_or_else(|| QualificationInputError::UnsafePath(relative.to_owned()))?;
    let mut identity = content_tree_identity(workspace, relative)?;
    let corpus_parent = Path::new(relative).parent().unwrap_or(Path::new("."));
    for case in &corpus.cases {
        if let HttpCaseBodyV1::File(body_path) = &case.request.body {
            let body = canonical_confined_path(body_root, body_path)?;
            let metadata = fs::metadata(&body)?;
            if !metadata.is_file() {
                return Err(QualificationInputError::Invalid(
                    "corpus request body reference must name a regular file".into(),
                ));
            }
            if metadata.len() > CONTENT_TREE_MAX_FILE_BYTES {
                return Err(QualificationInputError::Bound(
                    "corpus request body exceeds per-file limit".into(),
                ));
            }
            let body_relative = corpus_parent
                .join(body_path)
                .to_string_lossy()
                .replace('\\', "/");
            let body_identity = content_tree_identity(workspace, &body_relative)?;
            identity.total_bytes = identity
                .total_bytes
                .saturating_add(body_identity.total_bytes);
            for mut file in body_identity.files {
                file.path = format!("body/{}/{}", case.id, file.path);
                identity.files.push(file);
            }
        }
    }
    identity
        .files
        .sort_by(|left, right| left.path.cmp(&right.path));
    identity.aggregate_sha256 = aggregate_content_files(&identity.files);
    if identity.total_bytes > CONTENT_TREE_MAX_TOTAL_BYTES
        || identity.files.len() > CONTENT_TREE_MAX_FILES
    {
        return Err(QualificationInputError::Bound(
            "corpus plus body aggregate limit".into(),
        ));
    }
    if !identity
        .aggregate_sha256
        .eq_ignore_ascii_case(expected_sha256)
    {
        return Err(QualificationInputError::Invalid(
            "corpus digest differs from the declared identity".into(),
        ));
    }
    Ok((corpus, body_root.to_path_buf()))
}

fn aggregate_content_files(files: &[ContentFileIdentity]) -> String {
    let mut hasher = Sha256::new();
    for file in files {
        hasher.update(file.path.as_bytes());
        hasher.update([0]);
        hasher.update(file.length.to_be_bytes());
        hasher.update([0]);
        hasher.update(file.sha256.as_bytes());
        hasher.update([0]);
    }
    format!("{:x}", hasher.finalize())
}

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, QualificationInputError> {
    let metadata = fs::metadata(path)?;
    if metadata.len() > limit {
        return Err(QualificationInputError::Bound("input file bytes".into()));
    }
    Ok(fs::read(path)?)
}

fn canonical_confined_path(
    root: &Path,
    relative: &str,
) -> Result<PathBuf, QualificationInputError> {
    validate_relative(relative)?;
    let mut cursor = root.to_path_buf();
    for component in Path::new(relative).components() {
        let Component::Normal(part) = component else {
            unreachable!("validated relative path")
        };
        cursor.push(part);
        if fs::symlink_metadata(&cursor)?.file_type().is_symlink() {
            return Err(QualificationInputError::UnsafePath(
                "symlink path components are not accepted".into(),
            ));
        }
    }
    let canonical = fs::canonicalize(&cursor)
        .map_err(|_| QualificationInputError::UnsafePath(relative.to_owned()))?;
    if !canonical.starts_with(root) {
        return Err(QualificationInputError::UnsafePath(relative.to_owned()));
    }
    Ok(canonical)
}

fn hash_file(
    path: &Path,
    rel: String,
    records: &mut Vec<ContentFileIdentity>,
    total: &mut u64,
) -> Result<(), QualificationInputError> {
    if records.len() >= CONTENT_TREE_MAX_FILES {
        return Err(QualificationInputError::Bound("file count".into()));
    }
    let mut f = fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = [0u8; 8192];
    let mut n = 0u64;
    loop {
        let read = f.read(&mut buf)?;
        if read == 0 {
            break;
        }
        n += read as u64;
        if n > CONTENT_TREE_MAX_FILE_BYTES {
            return Err(QualificationInputError::Bound("per-file bytes".into()));
        }
        h.update(&buf[..read]);
    }
    *total += n;
    if *total > CONTENT_TREE_MAX_TOTAL_BYTES {
        return Err(QualificationInputError::Bound("aggregate bytes".into()));
    }
    records.push(ContentFileIdentity {
        path: rel,
        length: n,
        sha256: format!("{:x}", h.finalize()),
    });
    Ok(())
}
fn validate_relative(s: &str) -> Result<(), QualificationInputError> {
    let p = Path::new(s);
    if s.is_empty()
        || p.is_absolute()
        || s.len() > 512
        || p.components().any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(QualificationInputError::UnsafePath(s.into()));
    }
    Ok(())
}
fn validate_name(s: &str) -> Result<(), QualificationInputError> {
    if s.is_empty()
        || s.len() > 64
        || !s
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
    {
        return Err(QualificationInputError::Invalid("invalid name".into()));
    }
    Ok(())
}
fn validate_text(s: &str, max: usize, label: &str) -> Result<(), QualificationInputError> {
    if s.is_empty() || s.len() > max || s.chars().any(char::is_control) {
        return Err(QualificationInputError::Invalid(format!("invalid {label}")));
    }
    Ok(())
}
fn is_token(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn content_identity_is_order_independent_and_content_sensitive() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("tree/z")).unwrap();
        fs::create_dir_all(dir.path().join("tree/a")).unwrap();
        fs::write(dir.path().join("tree/z/file"), b"z").unwrap();
        fs::write(dir.path().join("tree/a/file"), b"a").unwrap();
        let first = content_tree_identity(dir.path(), "tree").unwrap();
        let second = content_tree_identity(dir.path(), "tree").unwrap();
        assert_eq!(first, second);
        assert_eq!(first.files[0].path, "a/file");
        fs::write(dir.path().join("tree/a/file"), b"changed").unwrap();
        assert_ne!(
            first.aggregate_sha256,
            content_tree_identity(dir.path(), "tree")
                .unwrap()
                .aggregate_sha256
        );
    }

    #[test]
    fn path_escape_and_symlinks_are_rejected() {
        let dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        fs::write(outside.path().join("secret"), b"secret").unwrap();
        assert!(content_tree_identity(dir.path(), "../secret").is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(outside.path().join("secret"), dir.path().join("link"))
                .unwrap();
            assert!(content_tree_identity(dir.path(), "link").is_err());
        }
    }

    #[test]
    fn corpus_rejects_credentials_and_invalid_targets() {
        let mut corpus = HttpSecurityCorpusV1 {
            schema_version: 1,
            owner: "test".into(),
            corpus_id: "small".into(),
            cases: vec![HttpSecurityCaseV1 {
                id: "one".into(),
                category: None,
                request: HttpCaseRequestV1 {
                    method: "GET".into(),
                    path_and_query: "/".into(),
                    headers: vec![("Authorization".into(), "secret".into())],
                    body: HttpCaseBodyV1::None,
                },
                expectation: HttpObservableExpectationV1::Exact { status_exact: 200 },
            }],
        };
        assert!(corpus.validate().is_err());
        corpus.cases[0].request.headers.clear();
        corpus.cases[0].request.headers = vec![("Host".into(), "example.com".into())];
        assert!(corpus.validate().is_err());
        corpus.cases[0].request.headers.clear();
        corpus.cases[0].request.path_and_query = "http://example.com/".into();
        assert!(corpus.validate().is_err());
        corpus.cases[0].request.path_and_query = "/\\example.com/".into();
        assert!(corpus.validate().is_err());
    }

    #[test]
    fn corpus_result_recomputes_status_disposition_and_counts() {
        let result = HttpCorpusCheckResultV1 {
            schema_version: 1,
            id: "smoke".into(),
            source: "eggbench-http-corpus".into(),
            family: "http_observable".into(),
            target: "api".into(),
            corpus_id: "small".into(),
            corpus_sha256: "ab".repeat(32),
            adapter_semantic_version: "v1".into(),
            eggfetch_version: "0.2.0".into(),
            cases: vec![HttpCorpusCaseResultV1 {
                id: "one".into(),
                case_sha256: "cd".repeat(32),
                expectation: HttpObservableExpectationV1::Exact { status_exact: 200 },
                observed_status: Some(200),
                disposition: HttpCorpusCaseDisposition::Pass,
                reason: None,
            }],
        };
        assert!(result.validate_contract().is_ok());
        assert_eq!(result.counts(), (1, 1, 0, 0));
        let mut inconsistent = result;
        inconsistent.cases[0].observed_status = Some(403);
        assert_eq!(
            inconsistent.validate_contract(),
            Err("case_disposition_mismatch")
        );
    }

    #[test]
    fn corpus_loader_binds_body_file_content_into_profile_identity() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("request.txt"), b"request body").unwrap();
        fs::write(dir.path().join("target.json"), b"{}").unwrap();
        fs::write(
            dir.path().join("plan.json"),
            include_str!("../tests/fixtures/minimal.json"),
        )
        .unwrap();
        let corpus = HttpSecurityCorpusV1 {
            schema_version: 1,
            owner: "owner".into(),
            corpus_id: "with-body".into(),
            cases: vec![HttpSecurityCaseV1 {
                id: "post".into(),
                category: None,
                request: HttpCaseRequestV1 {
                    method: "POST".into(),
                    path_and_query: "/submit".into(),
                    headers: vec![],
                    body: HttpCaseBodyV1::File("request.txt".into()),
                },
                expectation: HttpObservableExpectationV1::Exact { status_exact: 200 },
            }],
        };
        fs::write(
            dir.path().join("corpus.json"),
            serde_json::to_vec(&corpus).unwrap(),
        )
        .unwrap();
        let profile = SecurityQualificationProfileV1 {
            schema_version: 1,
            id: "body-profile".into(),
            owner: "owner".into(),
            scenarios: vec![QualificationScenarioRef {
                id: "baseline".into(),
                plan: "plan.json".into(),
                baseline_bundle: None,
            }],
            corpus: ContentInputRef {
                path: "corpus.json".into(),
            },
            target_config: ContentInputRef {
                path: "target.json".into(),
            },
        };
        fs::write(
            dir.path().join("profile.json"),
            serde_json::to_vec(&profile).unwrap(),
        )
        .unwrap();
        let identity = expand_qualification_profile(dir.path(), "profile.json")
            .unwrap()
            .corpus_identity
            .aggregate_sha256;
        assert!(load_http_security_corpus(dir.path(), "corpus.json", &identity).is_ok());
        fs::write(dir.path().join("request.txt"), b"changed body").unwrap();
        assert!(load_http_security_corpus(dir.path(), "corpus.json", &identity).is_err());
    }
}
