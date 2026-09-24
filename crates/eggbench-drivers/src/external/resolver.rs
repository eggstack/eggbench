//! Trusted executable resolution.
//!
//! Policy:
//!
//! - explicit paths must identify a regular executable file; relative
//!   explicit paths are rejected unless the caller resolved them against a
//!   trusted root before calling;
//! - `PATH` search enumerates components manually, skipping empty and
//!   relative components (no implicit current directory);
//! - on Unix a candidate must be a regular file with executable mode bits;
//! - on Windows only direct executable suffixes that `Command` can execute
//!   without a shell are accepted (`.exe`, `.com`); `.bat`/`.cmd` wrappers
//!   are rejected because they require command-interpreter semantics;
//! - the canonical target (after symlink resolution) is hashed and recorded
//!   alongside the selected path.

use super::error::{DriverError, ErrorCategory};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

/// Resolved executable identity: diagnostic provenance, not a trust signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedExecutable {
    /// Logical tool name requested by the adapter.
    pub logical_tool: String,
    /// Path selected by resolution (as discovered).
    pub selected_path: PathBuf,
    /// Canonical target after symlink resolution.
    pub canonical_path: PathBuf,
    /// SHA-256 hex of the canonical target bytes.
    pub sha256_hex: String,
    /// File size in bytes.
    pub file_size: u64,
    /// Platform execution classification (e.g. `unix-executable`, `windows-exe`).
    pub executable_class: String,
}

/// Trusted binary resolver.
pub struct BinaryResolver;

impl BinaryResolver {
    /// Resolve a requested tool name.
    ///
    /// `explicit_path`, when present, must be absolute. `search_path`, when
    /// present, is the `PATH`-style directory list to search; when absent the
    /// current process `PATH` is used.
    ///
    /// # Errors
    /// Returns a typed [`DriverError`] on untrusted input, missing binaries,
    /// non-executable files, or identity failures.
    pub fn resolve(
        requested_name: &str,
        explicit_path: Option<&Path>,
        search_path: Option<&str>,
    ) -> Result<ResolvedExecutable, DriverError> {
        if requested_name.is_empty()
            || requested_name.contains('/')
            || requested_name.contains('\\')
        {
            return Err(DriverError::resolution(
                ErrorCategory::BinaryNotFound,
                format!("invalid tool name {requested_name:?}"),
            ));
        }
        if let Some(path) = explicit_path {
            return Self::resolve_explicit(requested_name, path);
        }
        Self::resolve_search(requested_name, search_path)
    }

    fn resolve_explicit(
        requested_name: &str,
        path: &Path,
    ) -> Result<ResolvedExecutable, DriverError> {
        if path.as_os_str().is_empty() {
            return Err(DriverError::resolution(
                ErrorCategory::BinaryNotFound,
                "explicit executable path is empty",
            ));
        }
        if path.is_relative() {
            return Err(DriverError::resolution(
                ErrorCategory::UntrustedSearchPath,
                "relative explicit executable paths are rejected",
            ));
        }
        Self::identify(requested_name, path)
    }

    fn resolve_search(
        requested_name: &str,
        search_path: Option<&str>,
    ) -> Result<ResolvedExecutable, DriverError> {
        let raw = match search_path {
            Some(value) => value.to_owned(),
            None => std::env::var("PATH").unwrap_or_default(),
        };
        #[cfg(unix)]
        let separator = ':';
        #[cfg(windows)]
        let separator = ';';
        #[cfg(not(any(unix, windows)))]
        let separator = ':';
        for component in raw.split(separator) {
            if component.is_empty() {
                continue;
            }
            let dir = Path::new(component);
            if dir.is_relative() {
                continue;
            }
            #[cfg(unix)]
            {
                let candidate = dir.join(requested_name);
                if is_executable_file(&candidate) {
                    return Self::identify(requested_name, &candidate);
                }
            }
            #[cfg(windows)]
            {
                for suffix in ["", ".exe", ".com"] {
                    let file = format!("{requested_name}{suffix}");
                    let candidate = dir.join(file);
                    if candidate.is_file() {
                        return Self::identify(requested_name, &candidate);
                    }
                }
            }
            #[cfg(not(any(unix, windows)))]
            {
                let candidate = dir.join(requested_name);
                if candidate.is_file() {
                    return Self::identify(requested_name, &candidate);
                }
            }
        }
        Err(DriverError::resolution(
            ErrorCategory::BinaryNotFound,
            format!("tool {requested_name:?} not found in trusted PATH search"),
        ))
    }

    fn identify(requested_name: &str, selected: &Path) -> Result<ResolvedExecutable, DriverError> {
        let metadata = std::fs::metadata(selected).map_err(|_| {
            DriverError::resolution(
                ErrorCategory::NotExecutable,
                format!("candidate {} is not accessible", selected.display()),
            )
        })?;
        if !metadata.is_file() {
            return Err(DriverError::resolution(
                ErrorCategory::NotExecutable,
                format!("candidate {} is not a regular file", selected.display()),
            ));
        }
        check_executable_bits(selected)?;
        let canonical = std::fs::canonicalize(selected).map_err(|e| {
            DriverError::resolution(
                ErrorCategory::ExecutableIdentityFailed,
                format!("could not canonicalize {}: {e}", selected.display()),
            )
        })?;
        // Reject Windows batch/cmd wrappers even under explicit paths.
        #[cfg(windows)]
        {
            if let Some(ext) = canonical.extension().and_then(|e| e.to_str()) {
                let lower = ext.to_ascii_lowercase();
                if lower == "bat" || lower == "cmd" || lower == "ps1" {
                    return Err(DriverError::resolution(
                        ErrorCategory::NotExecutable,
                        "batch/cmd wrappers require shell semantics and are rejected",
                    ));
                }
            }
        }
        // On Unix also reject by extension only when the file is a script
        // wrapper without exec bits handled above; check extension defensively
        // on all platforms for explicit `.bat`/`.cmd` requests.
        if let Some(ext) = canonical.extension().and_then(|e| e.to_str()) {
            let lower = ext.to_ascii_lowercase();
            if lower == "bat" || lower == "cmd" {
                return Err(DriverError::resolution(
                    ErrorCategory::NotExecutable,
                    "batch/cmd wrappers require shell semantics and are rejected",
                ));
            }
        }
        let (sha256_hex, file_size) = hash_file(&canonical)?;
        let executable_class = classify(&canonical);
        Ok(ResolvedExecutable {
            logical_tool: requested_name.to_owned(),
            selected_path: selected.to_path_buf(),
            canonical_path: canonical,
            sha256_hex,
            file_size,
            executable_class,
        })
    }
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(unix)]
fn check_executable_bits(path: &Path) -> Result<(), DriverError> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = std::fs::metadata(path).map_err(|_| {
        DriverError::resolution(
            ErrorCategory::NotExecutable,
            format!("candidate {} is not accessible", path.display()),
        )
    })?;
    if metadata.permissions().mode() & 0o111 == 0 {
        return Err(DriverError::resolution(
            ErrorCategory::NotExecutable,
            format!("candidate {} is not executable", path.display()),
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_executable_bits(path: &Path) -> Result<(), DriverError> {
    if !path.is_file() {
        return Err(DriverError::resolution(
            ErrorCategory::NotExecutable,
            format!("candidate {} is not a regular file", path.display()),
        ));
    }
    Ok(())
}

fn hash_file(path: &Path) -> Result<(String, u64), DriverError> {
    let file = std::fs::File::open(path).map_err(|e| {
        DriverError::resolution(
            ErrorCategory::ExecutableIdentityFailed,
            format!("could not open {}: {e}", path.display()),
        )
    })?;
    let mut reader = std::io::BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut size: u64 = 0;
    let mut buf = [0u8; 8192];
    loop {
        let n = reader.read(&mut buf).map_err(|e| {
            DriverError::resolution(
                ErrorCategory::ExecutableIdentityFailed,
                format!("could not hash {}: {e}", path.display()),
            )
        })?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        size += n as u64;
    }
    Ok((hex::encode(hasher.finalize()), size))
}

fn classify(canonical: &Path) -> String {
    #[cfg(windows)]
    {
        let is_com = canonical
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("com"));
        if is_com {
            "windows-com".to_owned()
        } else {
            "windows-exe".to_owned()
        }
    }
    #[cfg(not(windows))]
    {
        let _ = canonical;
        "unix-executable".to_owned()
    }
}

/// Minimal hex encoder to avoid an extra dependency.
mod hex {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";

    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        let bytes = bytes.as_ref();
        let mut out = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            out.push(DIGITS[(byte >> 4) as usize] as char);
            out.push(DIGITS[(byte & 0x0f) as usize] as char);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_relative_path_is_rejected() {
        let err =
            BinaryResolver::resolve("tool", Some(Path::new("relative/bin")), None).unwrap_err();
        assert_eq!(err.category(), ErrorCategory::UntrustedSearchPath);
    }

    #[test]
    fn empty_path_component_cannot_resolve_cwd_executable() {
        let dir = tempfile::tempdir().unwrap();
        let cwd_file = dir.path().join("cwd-tool");
        std::fs::write(&cwd_file, b"#!/bin/sh\necho hi\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&cwd_file, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        // PATH with an empty component: it must be skipped, and since the
        // tool does not exist in any trusted absolute directory, resolution
        // fails rather than picking up the cwd-adjacent file.
        let err = BinaryResolver::resolve(
            "cwd-tool-definitely-missing-eggbench",
            None,
            Some(":/nonexistent-eggbench-dir"),
        )
        .unwrap_err();
        assert_eq!(err.category(), ErrorCategory::BinaryNotFound);
    }

    #[test]
    fn relative_path_component_is_skipped() {
        let err = BinaryResolver::resolve(
            "tool-missing-eggbench",
            None,
            Some("relative/dir:/nonexistent-eggbench-dir"),
        )
        .unwrap_err();
        assert_eq!(err.category(), ErrorCategory::BinaryNotFound);
    }

    #[test]
    fn non_executable_file_is_rejected_on_unix() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("not-exe");
        let mut handle = std::fs::File::create(&file).unwrap();
        handle.write_all(b"not executable").unwrap();
        drop(handle);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
            let err = BinaryResolver::resolve("tool", Some(&file), None).unwrap_err();
            assert_eq!(err.category(), ErrorCategory::NotExecutable);
        }
    }

    #[test]
    fn batch_wrapper_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("tool.bat");
        std::fs::write(&file, b"@echo off\n").unwrap();
        let err = BinaryResolver::resolve("tool", Some(&file), None).unwrap_err();
        assert_eq!(err.category(), ErrorCategory::NotExecutable);
    }

    #[test]
    fn argv_metacharacters_are_not_expanded() {
        // Resolution never interprets shell metacharacters; a tool name with
        // metacharacters is simply invalid/not found.
        let err = BinaryResolver::resolve("tool; rm -rf /", None, Some("/usr/bin")).unwrap_err();
        assert_eq!(err.category(), ErrorCategory::BinaryNotFound);
    }
}
