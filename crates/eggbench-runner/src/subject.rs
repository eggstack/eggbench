//! Subject identity snapshot evidence for M003.
//!
//! Subject snapshots live alongside the source plan, resolved plan, and
//! environment artifacts and are required evidence under the
//! `ArtifactRole::Subject` role. The snapshot records the resolved executable
//! path for managed commands, its observed SHA-256 digest, and the
//! declared/observed digest match. External and label subjects keep their
//! declared identity but never fabricate a binary digest.

use eggbench_core::{BundleError, Name, Subject};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::Path};

/// Maximum byte size of a single subject snapshot JSON artifact.
pub const MAX_SUBJECT_SNAPSHOT_BYTES: u64 = 4 * 1024 * 1024;
const HASH_BUFFER_BYTES: usize = 64 * 1024;

/// Versioned subject snapshot artifact written before managed startup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectSnapshot {
    /// Snapshot schema version.
    pub schema_version: SchemaVersion,
    /// Logical subject identity.
    pub subject: Subject,
    /// Resolved managed executable path when applicable.
    pub resolved_executable: Option<String>,
    /// SHA-256 digest of the managed executable when computed.
    pub executable_sha256: Option<String>,
    /// Declared subject revision hint when supplied.
    pub declared_revision: Option<String>,
    /// Declared subject digest hint when supplied.
    pub declared_digest: Option<String>,
    /// Whether the declared digest matched the observed executable digest.
    pub declared_digest_matches: Option<bool>,
    /// External or label identity label, when applicable.
    pub identity_label: Option<String>,
}

/// Schema version of [`SubjectSnapshot`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SchemaVersion(pub u32);

/// Current subject snapshot schema version.
pub const SUBJECT_SNAPSHOT_SCHEMA_VERSION: SchemaVersion = SchemaVersion(1);

impl SubjectSnapshot {
    /// Build a snapshot from a resolved plan and observed managed executable.
    ///
    /// The managed executable digest is hashed with bounded buffers; if no
    /// executable path was supplied, the digest fields stay `None`. For
    /// external or label subjects, the snapshot records declared identity
    /// only and never fabricates a digest.
    ///
    /// # Errors
    /// Returns a [`BundleError`] when the snapshot exceeds the artifact byte
    /// bound or the managed executable cannot be hashed.
    pub fn build(
        subject: &Subject,
        managed_executable: Option<&Path>,
    ) -> Result<Self, BundleError> {
        let mut snapshot = Self {
            schema_version: SUBJECT_SNAPSHOT_SCHEMA_VERSION,
            subject: subject.clone(),
            resolved_executable: None,
            executable_sha256: None,
            declared_revision: None,
            declared_digest: None,
            declared_digest_matches: None,
            identity_label: None,
        };
        match subject {
            Subject::ManagedCommand {
                revision, digest, ..
            } => {
                snapshot.declared_revision.clone_from(revision);
                snapshot.declared_digest.clone_from(digest);
                if let Some(path) = managed_executable {
                    snapshot.resolved_executable = Some(path.display().to_string());
                    let sha = hash_file(path)?;
                    let declared_matches = digest.as_deref() == Some(sha.as_str());
                    snapshot.executable_sha256 = Some(sha);
                    snapshot.declared_digest_matches = Some(declared_matches);
                }
            }
            Subject::External {
                target,
                revision,
                digest,
            } => {
                snapshot.identity_label = Some(target.as_str().to_owned());
                snapshot.declared_revision.clone_from(revision);
                snapshot.declared_digest.clone_from(digest);
            }
            Subject::Label { label } => {
                snapshot.identity_label = Some(label.as_str().to_owned());
            }
        }
        let bytes = serde_json::to_vec(&snapshot)
            .map_err(|error| BundleError::ManifestParse(error.to_string()))?;
        if bytes.len() as u64 > MAX_SUBJECT_SNAPSHOT_BYTES {
            return Err(BundleError::BoundExceeded("subject snapshot bytes"));
        }
        Ok(snapshot)
    }

    /// Serialize the snapshot as JSON bytes ready for artifact staging.
    ///
    /// # Errors
    /// Returns a [`BundleError`] when serialization fails.
    pub fn to_json_bytes(&self) -> Result<Vec<u8>, BundleError> {
        serde_json::to_vec_pretty(self)
            .map_err(|error| BundleError::ManifestParse(error.to_string()))
    }

    /// Validate the snapshot fields and boundedness.
    ///
    /// # Errors
    /// Returns a [`BundleError`] when the snapshot is malformed.
    pub fn validate(&self) -> Result<(), BundleError> {
        if self.schema_version != SUBJECT_SNAPSHOT_SCHEMA_VERSION {
            return Err(BundleError::InvalidManifest(
                "unsupported subject snapshot schema version",
            ));
        }
        if let Some(value) = &self.resolved_executable
            && (value.is_empty()
                || value.len() > usize::try_from(MAX_SUBJECT_SNAPSHOT_BYTES).unwrap_or(usize::MAX))
        {
            return Err(BundleError::InvalidManifest(
                "invalid resolved executable path",
            ));
        }
        if let Some(value) = &self.executable_sha256
            && (value.len() != 64
                || !value
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()))
        {
            return Err(BundleError::InvalidManifest(
                "invalid subject SHA-256 digest",
            ));
        }
        if let Some(value) = &self.declared_digest
            && value.len() > 128
        {
            return Err(BundleError::InvalidManifest(
                "declared subject digest exceeds bound",
            ));
        }
        if let Some(value) = &self.identity_label
            && value.is_empty()
        {
            return Err(BundleError::InvalidManifest(
                "identity label must not be empty",
            ));
        }
        if matches!(
            self.subject,
            Subject::External { .. } | Subject::Label { .. }
        ) && self.executable_sha256.is_some()
        {
            return Err(BundleError::InvalidManifest(
                "external or label subject must not fabricate an executable digest",
            ));
        }
        if matches!(self.subject, Subject::ManagedCommand { .. })
            && self.resolved_executable.is_none()
            && self.executable_sha256.is_some()
        {
            return Err(BundleError::InvalidManifest(
                "managed subject requires resolved executable when digest is recorded",
            ));
        }
        Ok(())
    }

    /// Report whether the declared subject digest matched the observed
    /// executable digest. `None` when not applicable.
    #[must_use]
    pub const fn declared_digest_matches(&self) -> Option<bool> {
        self.declared_digest_matches
    }

    /// Return the logical subject name for diagnostics.
    #[must_use]
    pub fn display_name(&self) -> String {
        match &self.subject {
            Subject::ManagedCommand { argv, .. } => argv
                .first()
                .cloned()
                .unwrap_or_else(|| "<managed>".to_owned()),
            Subject::External { target, .. } => target.as_str().to_owned(),
            Subject::Label { label } => label.as_str().to_owned(),
        }
    }

    /// Parse the logical subject name into a [`Name`] for stable identifier
    /// use where helpful.
    #[must_use]
    pub fn subject_name(&self) -> Option<Name> {
        match &self.subject {
            Subject::ManagedCommand { argv, .. } => {
                argv.first().and_then(|value| Name::new(value.clone()).ok())
            }
            Subject::External { target, .. } => Some(target.clone()),
            Subject::Label { label } => Some(label.clone()),
        }
    }
}

fn hash_file(path: &Path) -> Result<String, BundleError> {
    let mut file = File::open(path).map_err(|error| BundleError::Io {
        path: path.to_path_buf(),
        source: error,
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; HASH_BUFFER_BYTES].into_boxed_slice();
    loop {
        let read = file.read(&mut buffer).map_err(|error| BundleError::Io {
            path: path.to_path_buf(),
            source: error,
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(to_hex(&hasher.finalize()))
}

fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[(byte >> 4) as usize]));
        output.push(char::from(HEX[(byte & 0x0f) as usize]));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use eggbench_core::SecretRef;
    use std::{collections::BTreeMap, io::Write};

    fn managed_subject() -> Subject {
        Subject::ManagedCommand {
            argv: vec!["/bin/true".to_owned()],
            environment: BTreeMap::new(),
            revision: Some("abc".into()),
            digest: None,
        }
    }

    fn external_subject() -> Subject {
        Subject::External {
            target: Name::new("api").unwrap(),
            revision: Some("rev1".into()),
            digest: None,
        }
    }

    #[test]
    fn managed_subject_records_resolved_digest_and_mismatch() {
        let temp = tempfile::tempdir().unwrap();
        let binary = temp.path().join("subject-bin");
        let mut file = File::create(&binary).unwrap();
        file.write_all(b"binary-bytes").unwrap();
        file.sync_all().unwrap();

        let snapshot = SubjectSnapshot::build(&managed_subject(), Some(&binary)).unwrap();
        snapshot.validate().unwrap();
        assert_eq!(
            snapshot.resolved_executable.as_deref(),
            Some(binary.display().to_string()).as_deref()
        );
        let digest = snapshot.executable_sha256.clone().unwrap();
        assert_eq!(digest.len(), 64);
        assert_eq!(snapshot.declared_digest_matches, Some(false));
    }

    #[test]
    fn declared_digest_match_is_detected() {
        let temp = tempfile::tempdir().unwrap();
        let binary = temp.path().join("subject-bin");
        let mut file = File::create(&binary).unwrap();
        file.write_all(b"hello").unwrap();
        file.sync_all().unwrap();

        let digest = SubjectSnapshot::build(&managed_subject(), Some(&binary))
            .unwrap()
            .executable_sha256
            .unwrap();
        let mut subject = managed_subject();
        if let Subject::ManagedCommand { digest: d, .. } = &mut subject {
            *d = Some(digest.clone());
        }
        let snapshot = SubjectSnapshot::build(&subject, Some(&binary)).unwrap();
        assert_eq!(snapshot.declared_digest_matches, Some(true));
    }

    #[test]
    fn external_subject_has_no_executable_digest() {
        let snapshot = SubjectSnapshot::build(&external_subject(), None).unwrap();
        snapshot.validate().unwrap();
        assert!(snapshot.executable_sha256.is_none());
        assert_eq!(snapshot.identity_label.as_deref(), Some("api"));
    }

    #[test]
    fn label_subject_is_diagnostic_only() {
        let subject = Subject::Label {
            label: Name::new("baseline").unwrap(),
        };
        let snapshot = SubjectSnapshot::build(&subject, None).unwrap();
        snapshot.validate().unwrap();
        assert_eq!(snapshot.identity_label.as_deref(), Some("baseline"));
    }

    #[test]
    fn declared_digest_matches_returns_helper() {
        let snapshot = SubjectSnapshot::build(&external_subject(), None).unwrap();
        assert_eq!(snapshot.declared_digest_matches(), None);
    }

    #[test]
    fn secret_reference_debug_redacts() {
        let reference = SecretRef {
            reference: Name::new("API_TOKEN").unwrap(),
        };
        let debug = format!("{reference:?}");
        assert!(debug.contains("REDACTED"));
    }
}
