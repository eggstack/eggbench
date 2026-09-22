//! Runner-owned process specifications prepared from a resolved plan.
//!
//! Preparation revalidates the plan boundary, orders managed services by
//! dependency, resolves working directories against an explicit workspace
//! root, resolves subject secret references through the injected provider,
//! and rejects unsupported service kinds before any process starts. Process
//! identifiers never enter these specs; they are diagnostics only.

use crate::error::RunnerError;
use crate::platform::{PlatformAdapter, PlatformSupport};
use crate::secret::SecretProvider;
use eggbench_core::{Lifecycle, Readiness, ResolvedPlan, ServiceKind, Shutdown, Subject};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Default retained log bytes for the subject, which carries no core log bound.
pub const DEFAULT_SUBJECT_LOG_LIMIT_BYTES: u64 = 1024 * 1024;
/// Default graceful shutdown allowance when no policy is declared.
pub const DEFAULT_GRACE_MS: u64 = 5_000;
/// Identity used for the managed subject process.
pub const SUBJECT_IDENTITY: &str = "subject";

/// Runner-owned spawn request with redacted debug output.
#[derive(Clone, PartialEq, Eq)]
pub struct ProcessSpec {
    /// Service name, or `subject` for a managed subject command.
    pub identity: String,
    /// Program and arguments spawned directly without a shell.
    pub argv: Vec<String>,
    /// Resolved working directory inside the workspace root.
    pub cwd: PathBuf,
    /// Resolved non-secret environment plus injected secret values.
    ///
    /// Values are live process inputs only and are redacted from `Debug`.
    pub env: BTreeMap<String, String>,
    /// Secret reference names resolved for this spec, for auditing.
    pub secret_references: Vec<String>,
    /// Maximum retained bytes per output stream.
    pub log_limit_bytes: u64,
    /// Declared readiness request.
    pub readiness: Option<Readiness>,
    /// Declared graceful shutdown policy.
    pub shutdown: Option<Shutdown>,
    /// Whether this spec is the experiment subject.
    pub is_subject: bool,
}

impl fmt::Debug for ProcessSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProcessSpec")
            .field("identity", &self.identity)
            .field("argv", &self.argv)
            .field("cwd", &self.cwd)
            .field("env", &redacted_env(&self.env))
            .field("secret_references", &self.secret_references)
            .field("log_limit_bytes", &self.log_limit_bytes)
            .field("readiness", &self.readiness)
            .field("shutdown", &self.shutdown)
            .field("is_subject", &self.is_subject)
            .finish()
    }
}

fn redacted_env(env: &BTreeMap<String, String>) -> BTreeMap<String, &'static str> {
    env.keys().map(|key| (key.clone(), "[REDACTED]")).collect()
}

/// Ordered spawn plan: subject first when managed, then services in
/// dependency order.
#[derive(Debug, Clone)]
pub struct SpawnPlan {
    /// Specs in spawn order.
    pub specs: Vec<ProcessSpec>,
}

impl SpawnPlan {
    /// Identities in spawn order.
    #[must_use]
    pub fn order(&self) -> Vec<String> {
        self.specs
            .iter()
            .map(|spec| spec.identity.clone())
            .collect()
    }

    /// Identities in teardown order (reverse spawn order).
    #[must_use]
    pub fn teardown_order(&self) -> Vec<String> {
        self.order().into_iter().rev().collect()
    }
}

/// Caller-supplied preparation inputs.
pub struct PrepareOptions {
    /// Explicit workspace root for relative working directories.
    pub workspace_root: PathBuf,
    /// Injected secret values keyed by reference.
    pub secrets: Arc<dyn SecretProvider>,
    /// Platform adapter used for capability gating.
    pub platform: Arc<dyn PlatformAdapter>,
}

impl fmt::Debug for PrepareOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PrepareOptions")
            .field("workspace_root", &self.workspace_root)
            .field("secrets", &self.secrets)
            .field("platform", &self.platform)
            .finish()
    }
}

/// Revalidate the resolved-plan boundary and prepare ordered spawn specs.
///
/// # Errors
/// Returns [`RunnerError`] for schema mismatches, unsupported kinds, unknown
/// working directories, missing secrets, cycles, or unsupported platforms.
pub fn prepare(
    resolved: &ResolvedPlan,
    options: &PrepareOptions,
) -> Result<SpawnPlan, RunnerError> {
    if resolved.schema_version != eggbench_core::RESOLVED_PLAN_SCHEMA_VERSION {
        return Err(RunnerError::InvalidPlan {
            detail: format!("unsupported resolved schema {}", resolved.schema_version.0),
        });
    }
    ensure_platform(resolved, options.platform.as_ref())?;
    let workspace_root = normalize_workspace_root(&options.workspace_root)?;
    let mut specs = Vec::new();
    if let Subject::ManagedCommand {
        argv, environment, ..
    } = &resolved.subject
    {
        if argv.is_empty() || argv[0].trim().is_empty() {
            return Err(RunnerError::EmptyArgv {
                service: SUBJECT_IDENTITY.to_owned(),
            });
        }
        let argv = resolve_executable(SUBJECT_IDENTITY, argv, &workspace_root)?;
        let mut env = BTreeMap::new();
        let mut references = Vec::new();
        for (key, secret_ref) in environment {
            let value = options
                .secrets
                .resolve(&secret_ref.reference)
                .ok_or_else(|| RunnerError::MissingSecret {
                    service: SUBJECT_IDENTITY.to_owned(),
                    reference: secret_ref.reference.as_str().to_owned(),
                })?;
            env.insert(key.clone(), value);
            references.push(secret_ref.reference.as_str().to_owned());
        }
        references.sort();
        specs.push(ProcessSpec {
            identity: SUBJECT_IDENTITY.to_owned(),
            argv,
            cwd: workspace_root.clone(),
            env,
            secret_references: references,
            log_limit_bytes: DEFAULT_SUBJECT_LOG_LIMIT_BYTES,
            readiness: None,
            shutdown: None,
            is_subject: true,
        });
    }
    let ordered = topological_order(&resolved.topology)?;
    for service in ordered {
        if service.lifecycle != Lifecycle::Managed {
            continue;
        }
        let mut argv = match &service.kind {
            ServiceKind::Command { argv } => argv.clone(),
            ServiceKind::Named { service_type } => {
                return Err(RunnerError::UnsupportedService {
                    service: service.name.as_str().to_owned(),
                    detail: format!(
                        "named service type {} needs a future adapter",
                        service_type.as_str()
                    ),
                });
            }
        };
        if argv.is_empty() || argv[0].trim().is_empty() {
            return Err(RunnerError::EmptyArgv {
                service: service.name.as_str().to_owned(),
            });
        }
        let cwd = resolve_working_directory(
            service.name.as_str(),
            service.working_directory.as_deref(),
            &workspace_root,
        )?;
        argv = resolve_executable(service.name.as_str(), &argv, &cwd)?;
        specs.push(ProcessSpec {
            identity: service.name.as_str().to_owned(),
            argv,
            cwd,
            env: BTreeMap::new(),
            secret_references: Vec::new(),
            log_limit_bytes: service.log_limit_bytes,
            readiness: service.readiness.clone(),
            shutdown: service.shutdown.clone(),
            is_subject: false,
        });
    }
    Ok(SpawnPlan { specs })
}

fn ensure_platform(
    resolved: &ResolvedPlan,
    platform: &dyn PlatformAdapter,
) -> Result<(), RunnerError> {
    if needs_managed_spawn(resolved) {
        match platform.support() {
            PlatformSupport::Supported => {}
            PlatformSupport::Unqualified => {
                return Err(RunnerError::UnsupportedPlatform {
                    detail: format!("managed execution is unqualified on {}", platform.label()),
                });
            }
            PlatformSupport::Unsupported => {
                return Err(RunnerError::UnsupportedPlatform {
                    detail: format!("managed execution is unsupported on {}", platform.label()),
                });
            }
        }
    }
    Ok(())
}

fn needs_managed_spawn(resolved: &ResolvedPlan) -> bool {
    matches!(resolved.subject, Subject::ManagedCommand { .. })
        || resolved
            .topology
            .iter()
            .any(|service| service.lifecycle == Lifecycle::Managed)
}

fn normalize_workspace_root(root: &Path) -> Result<PathBuf, RunnerError> {
    if !root.is_absolute() {
        return Err(RunnerError::InvalidWorkingDirectory {
            service: "<workspace>".to_owned(),
            detail: "workspace root must be absolute".to_owned(),
        });
    }
    let canonical = fs::canonicalize(root).map_err(|_| RunnerError::InvalidWorkingDirectory {
        service: "<workspace>".to_owned(),
        detail: "workspace root must exist and be a directory".to_owned(),
    })?;
    if !canonical.is_dir() {
        return Err(RunnerError::InvalidWorkingDirectory {
            service: "<workspace>".to_owned(),
            detail: "workspace root must exist and be a directory".to_owned(),
        });
    }
    Ok(canonical)
}

fn resolve_working_directory(
    service: &str,
    requested: Option<&str>,
    root: &Path,
) -> Result<PathBuf, RunnerError> {
    let Some(requested) = requested else {
        return Ok(root.to_path_buf());
    };
    let requested_path = Path::new(requested);
    if requested_path.is_absolute() {
        return Err(RunnerError::InvalidWorkingDirectory {
            service: service.to_owned(),
            detail: "absolute working directories are rejected".to_owned(),
        });
    }
    let requested_path = root.join(requested_path);
    let resolved =
        fs::canonicalize(&requested_path).map_err(|_| RunnerError::InvalidWorkingDirectory {
            service: service.to_owned(),
            detail: "working directory must exist and resolve inside the workspace root".to_owned(),
        })?;
    if !resolved.is_dir() {
        return Err(RunnerError::InvalidWorkingDirectory {
            service: service.to_owned(),
            detail: "working directory must be a directory".to_owned(),
        });
    }
    if !resolved.starts_with(root) {
        return Err(RunnerError::InvalidWorkingDirectory {
            service: service.to_owned(),
            detail: "working directory resolves outside the workspace root".to_owned(),
        });
    }
    Ok(resolved)
}

fn resolve_executable(
    service: &str,
    argv: &[String],
    cwd: &Path,
) -> Result<Vec<String>, RunnerError> {
    let program = argv.first().ok_or_else(|| RunnerError::EmptyArgv {
        service: service.to_owned(),
    })?;
    let requested = Path::new(program);
    let has_path_separator = program.contains('/') || program.contains(std::path::MAIN_SEPARATOR);
    if !requested.is_absolute() && !has_path_separator {
        return Err(RunnerError::InvalidExecutablePath {
            service: service.to_owned(),
            detail: "program must be an absolute path or a relative path containing a separator"
                .to_owned(),
        });
    }
    let candidate = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        cwd.join(requested)
    };
    let resolved =
        fs::canonicalize(&candidate).map_err(|_| RunnerError::InvalidExecutablePath {
            service: service.to_owned(),
            detail: "program path must resolve to an existing file".to_owned(),
        })?;
    if !resolved.is_file() {
        return Err(RunnerError::InvalidExecutablePath {
            service: service.to_owned(),
            detail: "program path must resolve to a file".to_owned(),
        });
    }
    let mut resolved_argv = argv.to_vec();
    resolved_argv[0] = resolved.to_string_lossy().into_owned();
    Ok(resolved_argv)
}

fn topological_order(
    services: &[eggbench_core::Service],
) -> Result<Vec<&eggbench_core::Service>, RunnerError> {
    let mut by_name = BTreeMap::<&str, &eggbench_core::Service>::new();
    for service in services {
        by_name.insert(service.name.as_str(), service);
    }
    let mut indegree = BTreeMap::<&str, usize>::new();
    let mut dependents = BTreeMap::<&str, Vec<&str>>::new();
    for service in services {
        indegree.entry(service.name.as_str()).or_insert(0);
        let mut seen = BTreeSet::new();
        for dependency in &service.depends_on {
            if !by_name.contains_key(dependency.as_str()) {
                return Err(RunnerError::InvalidPlan {
                    detail: format!(
                        "service {} depends on unknown service {dependency}",
                        service.name.as_str()
                    ),
                });
            }
            if seen.insert(dependency.as_str()) {
                *indegree.entry(service.name.as_str()).or_insert(0) += 1;
                dependents
                    .entry(dependency.as_str())
                    .or_default()
                    .push(service.name.as_str());
            }
        }
    }
    let mut ready: VecDeque<&str> = indegree
        .iter()
        .filter_map(|(name, degree)| (*degree == 0).then_some(*name))
        .collect();
    let mut ordered_names: Vec<&str> = Vec::with_capacity(services.len());
    while let Some(name) = ready.pop_front() {
        ordered_names.push(name);
        if let Some(next) = dependents.remove(name) {
            for dependent in next {
                let entry = indegree.entry(dependent).or_insert(1);
                *entry = entry.saturating_sub(1);
                if *entry == 0 {
                    ready.push_back(dependent);
                }
            }
        }
    }
    if ordered_names.len() != services.len() {
        return Err(RunnerError::InvalidPlan {
            detail: "service dependency cycle detected".to_owned(),
        });
    }
    let position: BTreeMap<&str, usize> = ordered_names
        .iter()
        .enumerate()
        .map(|(index, name)| (*name, index))
        .collect();
    let mut ordered: Vec<&eggbench_core::Service> = services.iter().collect();
    ordered.sort_by_key(|service| position[service.name.as_str()]);
    let _ = by_name;
    Ok(ordered)
}
