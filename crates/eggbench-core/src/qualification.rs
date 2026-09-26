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
#[serde(untagged)]
pub enum HttpObservableExpectationV1 {
    Exact { status_exact: u16 },
    AnyOf { status_any_of: Vec<u16> },
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
                    .any(|b| b <= 0x20 || b == 0x7f || b == b'#')
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
                    "authorization" | "proxy-authorization" | "cookie" | "set-cookie"
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
    let mut h = Sha256::new();
    for r in &records {
        h.update(r.path.as_bytes());
        h.update([0]);
        h.update(r.length.to_be_bytes());
        h.update([0]);
        h.update(r.sha256.as_bytes());
        h.update([0]);
    }
    Ok(ContentTreeIdentity {
        aggregate_sha256: format!("{:x}", h.finalize()),
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
        corpus.cases[0].request.path_and_query = "http://example.com/".into();
        assert!(corpus.validate().is_err());
    }
}
