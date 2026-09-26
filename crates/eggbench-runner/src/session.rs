//! Library session owning managed local processes.
//!
//! A [`LocalSession`] prepares ordered spawn specs from a [`ResolvedPlan`],
//! starts managed commands in dependency order, applies declared readiness
//! bounds, drains bounded stdout/stderr continuously, and tears down the
//! owned process tree in reverse dependency order. External services are
//! observed but never started or stopped. Process identifiers are diagnostics
//! only and never durable identity.
//!
//! This milestone performs no load generation, trial scheduling, or
//! measurement. A lifecycle-only run records no trials.

use crate::error::{CleanupFailure, RunnerError};
use crate::platform::{PlatformAdapter, PlatformSupport};
use crate::probe::{ProbeContext, ProbeRegistry};
use crate::secret::SecretProvider;
use crate::service::{
    ManagedServiceHandle, RUNTIME_TOPOLOGY_SCHEMA_VERSION, RuntimeBindings, RuntimeTopology,
    ServiceAdapterRegistry, ServiceOwnership, ServiceStartRequest, ServiceTopologyEntry,
};
use crate::spec::{AdapterSpec, LaunchKind, PrepareOptions, ProcessSpec, SpawnPlan, prepare};
use eggbench_core::Readiness;
use eggbench_core::ResolvedPlan;
use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// Default poll interval while waiting for readiness or exit.
const POLL_INTERVAL: Duration = Duration::from_millis(10);
/// Bounded wait for a process group to disappear after forced kill.
const KILL_WAIT_LIMIT: Duration = Duration::from_secs(5);
/// Bounded wait for output-drain tasks to finish after a child exits.
const DRAIN_JOIN_LIMIT: Duration = Duration::from_secs(5);
/// Read chunk size for continuous pipe draining.
const DRAIN_CHUNK_BYTES: usize = 8 * 1024;

/// Caller-supplied session inputs.
pub struct RunnerOptions {
    /// Explicit workspace root for relative working directories.
    pub workspace_root: PathBuf,
    /// Injected secret values keyed by reference.
    pub secrets: Arc<dyn SecretProvider>,
    /// Named readiness probes.
    pub probes: ProbeRegistry,
    /// Process-tree ownership adapter.
    pub platform: Arc<dyn PlatformAdapter>,
    /// Registered named-service adapters.
    pub service_adapters: ServiceAdapterRegistry,
}

impl fmt::Debug for RunnerOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RunnerOptions")
            .field("workspace_root", &self.workspace_root)
            .field("secrets", &self.secrets)
            .field("probes", &self.probes)
            .field("platform", &self.platform)
            .field("service_adapters", &self.service_adapters)
            .finish()
    }
}

/// Lifecycle event kind for one owned process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleEventKind {
    /// Process spawned and draining started.
    Spawned,
    /// Readiness request satisfied.
    Ready,
    /// Graceful termination requested.
    Stopping,
    /// Process reaped after graceful or forced stop.
    Stopped,
}

/// Structured lifecycle event with a diagnostic process identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleEvent {
    /// Sequence number in session order.
    pub seq: u64,
    /// Service identity or `subject`.
    pub identity: String,
    /// Event kind.
    pub kind: LifecycleEventKind,
    /// Diagnostic OS process identifier, never durable identity.
    pub pid: Option<u32>,
    /// Monotonic milliseconds since session preparation; diagnostic only.
    pub elapsed_ms: u64,
}

/// Bounded stdout/stderr snapshot for one owned process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedOutput {
    /// Retained leading bytes up to the declared cap.
    pub data: Vec<u8>,
    /// Whether output beyond the cap was discarded while draining continued.
    pub truncated: bool,
    /// Retained byte count.
    pub retained_bytes: u64,
    /// Discarded byte count beyond the cap.
    pub dropped_bytes: u64,
    /// Total bytes drained from the pipe.
    pub total_bytes: u64,
}

/// Bounded logs for one owned process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceLogs {
    /// Bounded standard output.
    pub stdout: BoundedOutput,
    /// Bounded standard error.
    pub stderr: BoundedOutput,
}

/// Startup report for a fully ready session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupReport {
    /// Identities in spawn order.
    pub started: Vec<String>,
}

/// Teardown report; failures never replace a primary lifecycle outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShutdownReport {
    /// Identities in teardown attempt order.
    pub stopped_order: Vec<String>,
    /// Cleanup problems observed while stopping.
    pub failures: Vec<CleanupFailure>,
}

/// Full lifecycle outcome for a start-readiness-stop pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleOutcome {
    /// Identities in spawn order.
    pub started: Vec<String>,
    /// Identities in teardown attempt order.
    pub stopped_order: Vec<String>,
    /// Teardown problems observed after successful startup.
    pub cleanup: Vec<CleanupFailure>,
}

#[derive(Debug)]
struct SpoolState {
    retained: Vec<u8>,
    cap: u64,
    dropped: u64,
    total: u64,
}

impl SpoolState {
    fn new(cap: u64) -> Self {
        Self {
            retained: Vec::new(),
            cap,
            dropped: 0,
            total: 0,
        }
    }

    fn push(&mut self, chunk: &[u8]) {
        self.total += chunk.len() as u64;
        let retained_so_far = self.retained.len() as u64;
        if retained_so_far >= self.cap {
            self.dropped += chunk.len() as u64;
            return;
        }
        let room = usize::try_from(self.cap - retained_so_far).unwrap_or(usize::MAX);
        if chunk.len() <= room {
            self.retained.extend_from_slice(chunk);
        } else {
            self.retained.extend_from_slice(&chunk[..room]);
            self.dropped += (chunk.len() - room) as u64;
        }
    }

    fn snapshot(&self) -> BoundedOutput {
        BoundedOutput {
            data: self.retained.clone(),
            truncated: self.dropped > 0,
            retained_bytes: self.retained.len() as u64,
            dropped_bytes: self.dropped,
            total_bytes: self.total,
        }
    }
}

struct RunningProcess {
    spec: ProcessSpec,
    child: tokio::process::Child,
    pid: u32,
    stdout_spool: Arc<Mutex<SpoolState>>,
    stderr_spool: Arc<Mutex<SpoolState>>,
    drain_handles: Vec<tokio::task::JoinHandle<()>>,
}

struct RunningAdapter {
    identity: String,
    service_type: String,
    handle: Box<dyn ManagedServiceHandle>,
    bindings: RuntimeBindings,
    grace: Duration,
}

/// One runner-owned managed service: an OS process or an in-process adapter.
///
/// No service is ever represented as process-owned when it is actually
/// in-process: adapter services carry no PID and shut down through their
/// adapter handle.
enum RunningManagedService {
    /// Runner-owned OS process.
    Process(Box<RunningProcess>),
    /// Runner-owned in-process adapter service.
    Adapter(RunningAdapter),
}

impl RunningManagedService {
    fn identity(&self) -> &str {
        match self {
            Self::Process(service) => service.spec.identity.as_str(),
            Self::Adapter(service) => service.identity.as_str(),
        }
    }
}

#[derive(Debug)]
struct RetainedLogs {
    identity: String,
    stdout_spool: Arc<Mutex<SpoolState>>,
    stderr_spool: Arc<Mutex<SpoolState>>,
}

/// Library session owning managed local processes.
pub struct LocalSession {
    spawn_plan: SpawnPlan,
    options: RunnerOptions,
    external: Vec<String>,
    running: Vec<RunningManagedService>,
    retained: Vec<RetainedLogs>,
    events: Vec<LifecycleEvent>,
    prepared_at: Instant,
    next_seq: u64,
    /// Startup-established runtime bindings, retained through teardown for
    /// final evidence.
    bindings: RuntimeBindings,
}

impl fmt::Debug for LocalSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalSession")
            .field("spawn_plan", &self.spawn_plan)
            .field("options", &self.options)
            .field("external", &self.external)
            .field(
                "running",
                &self
                    .running
                    .iter()
                    .map(|service| service.identity().to_owned())
                    .collect::<Vec<_>>(),
            )
            .field(
                "retained",
                &self
                    .retained
                    .iter()
                    .map(|logs| logs.identity.clone())
                    .collect::<Vec<_>>(),
            )
            .field("events", &self.events)
            .field("prepared_at", &self.prepared_at)
            .field("next_seq", &self.next_seq)
            .field("bindings", &self.bindings)
            .finish()
    }
}

impl LocalSession {
    /// Revalidate the plan boundary and prepare ordered spawn specs.
    ///
    /// No process starts here; use [`Self::startup`] to spawn.
    ///
    /// # Errors
    /// Returns [`RunnerError`] for schema, capability, path, secret, or
    /// platform preflight failures before any spawn.
    pub fn prepare(resolved: &ResolvedPlan, options: RunnerOptions) -> Result<Self, RunnerError> {
        let prepare_options = PrepareOptions {
            workspace_root: options.workspace_root.clone(),
            secrets: Arc::clone(&options.secrets),
            platform: Arc::clone(&options.platform),
            service_adapters: options.service_adapters.clone(),
        };
        let spawn_plan = prepare(resolved, &prepare_options)?;
        let external_names = resolved
            .topology
            .iter()
            .filter(|service| service.lifecycle == eggbench_core::Lifecycle::External)
            .map(|service| service.name.as_str().to_owned())
            .collect();
        let mut bindings = RuntimeBindings::new();
        for service in &resolved.topology {
            if service.lifecycle == eggbench_core::Lifecycle::External
                && let Some(url) = &service.http_url
            {
                bindings
                    .insert(service.name.as_str(), "http_url", url.clone())
                    .map_err(|detail| RunnerError::InvalidPlan { detail })?;
            }
        }
        Ok(Self {
            spawn_plan,
            options,
            external: external_names,
            running: Vec::new(),
            retained: Vec::new(),
            events: Vec::new(),
            prepared_at: Instant::now(),
            next_seq: 0,
            bindings,
        })
    }

    /// Identities in spawn order (subject first when managed).
    #[must_use]
    pub fn spawn_order(&self) -> Vec<String> {
        self.spawn_plan.order()
    }

    /// Identities in teardown order (reverse spawn order).
    #[must_use]
    pub fn teardown_order(&self) -> Vec<String> {
        self.spawn_plan.teardown_order()
    }

    /// Externally managed identities; observed but never owned.
    #[must_use]
    pub fn external_services(&self) -> &[String] {
        &self.external
    }

    /// Structured lifecycle events recorded so far.
    #[must_use]
    pub fn events(&self) -> &[LifecycleEvent] {
        &self.events
    }

    /// Whether any owned process is currently running.
    #[must_use]
    pub fn is_running(&self) -> bool {
        !self.running.is_empty()
    }

    /// Bounded log snapshot for one identity, if it ever started.
    ///
    /// Drain tasks continue in the background while the process runs, so the
    /// snapshot reflects output retained up to the call point. In-process
    /// adapter services own no logs and always yield `None`.
    pub async fn logs(&self, identity: &str) -> Option<ServiceLogs> {
        if let Some(service) = self.running.iter().find_map(|service| match service {
            RunningManagedService::Process(process) if process.spec.identity == identity => {
                Some(process)
            }
            _ => None,
        }) {
            let stdout = service.stdout_spool.lock().await.snapshot();
            let stderr = service.stderr_spool.lock().await.snapshot();
            return Some(ServiceLogs { stdout, stderr });
        }
        if let Some(logs) = self.retained.iter().find(|logs| logs.identity == identity) {
            let stdout = logs.stdout_spool.lock().await.snapshot();
            let stderr = logs.stderr_spool.lock().await.snapshot();
            return Some(ServiceLogs { stdout, stderr });
        }
        None
    }

    /// Startup-established runtime bindings, merged across all started
    /// adapter services. Available through teardown for final evidence.
    #[must_use]
    pub fn runtime_bindings(&self) -> &RuntimeBindings {
        &self.bindings
    }

    /// Versioned runtime-topology evidence: one entry per launch-order
    /// identity plus externally managed services.
    #[must_use]
    pub fn runtime_topology(&self) -> RuntimeTopology {
        let mut services = Vec::new();
        for entry in &self.spawn_plan.launch_order {
            let retained: BTreeMap<String, String> = self
                .bindings
                .service_bindings(&entry.identity)
                .cloned()
                .unwrap_or_default();
            let running_adapter = self.running.iter().find_map(|service| match service {
                RunningManagedService::Adapter(adapter) if adapter.identity == entry.identity => {
                    Some(adapter)
                }
                _ => None,
            });
            let (ownership, service_type, bindings) = match entry.kind {
                LaunchKind::Process => (ServiceOwnership::Process, None, retained),
                LaunchKind::Adapter => {
                    let live: Option<BTreeMap<String, String>> =
                        running_adapter.and_then(|adapter| {
                            adapter.bindings.service_bindings(&entry.identity).cloned()
                        });
                    let service_type = self
                        .spawn_plan
                        .adapter(&entry.identity)
                        .map(|spec| spec.service_type.clone())
                        .or_else(|| running_adapter.map(|adapter| adapter.service_type.clone()));
                    (
                        ServiceOwnership::Adapter,
                        service_type,
                        live.unwrap_or(retained),
                    )
                }
            };
            services.push(ServiceTopologyEntry {
                identity: entry.identity.clone(),
                ownership,
                service_type,
                bindings,
            });
        }
        for identity in &self.external {
            services.push(ServiceTopologyEntry {
                identity: identity.clone(),
                ownership: ServiceOwnership::External,
                service_type: None,
                bindings: BTreeMap::new(),
            });
        }
        RuntimeTopology {
            schema_version: RUNTIME_TOPOLOGY_SCHEMA_VERSION,
            services,
        }
    }

    /// Spawn managed services in dependency order and apply readiness.
    ///
    /// Processes and named adapter services start in one unified dependency
    /// order and tear down in reverse order. On any spawn, readiness, or
    /// cancellation failure after the first successful start, already-started
    /// services are torn down in reverse dependency order and the initiating
    /// failure is preserved with cleanup problems attached.
    ///
    /// # Errors
    /// Returns [`RunnerError`] for spawn, readiness, timeout, early exit,
    /// cancellation, or capability failures.
    pub async fn startup(
        &mut self,
        cancel: &CancellationToken,
    ) -> Result<StartupReport, RunnerError> {
        self.check_probes_registered()?;
        if self.spawn_plan.launch_order.is_empty() {
            return Ok(StartupReport {
                started: Vec::new(),
            });
        }
        let mut started = Vec::new();
        for entry in self.spawn_plan.launch_order.clone() {
            if cancel.is_cancelled() {
                return self.cancel_after_start().await;
            }
            match entry.kind {
                LaunchKind::Process => {
                    let spec = self
                        .spawn_plan
                        .process(&entry.identity)
                        .cloned()
                        .ok_or_else(|| RunnerError::InvalidPlan {
                            detail: format!(
                                "launch order references unknown process {}",
                                entry.identity
                            ),
                        })?;
                    let running = match Self::spawn_one(&spec) {
                        Ok(running) => running,
                        Err(message) => {
                            let cleanup = self.teardown_running().await;
                            return Err(RunnerError::SpawnFailed {
                                service: spec.identity,
                                message,
                                cleanup,
                            });
                        }
                    };
                    let pid = running.pid;
                    self.push_event(&spec.identity, LifecycleEventKind::Spawned, Some(pid));
                    self.running
                        .push(RunningManagedService::Process(Box::new(running)));
                    started.push(spec.identity.clone());
                    if let Err(error) = self.apply_readiness(&spec, pid, cancel).await {
                        let cleanup = self.teardown_running().await;
                        return Err(attach_cleanup(error, cleanup));
                    }
                    self.push_event(&spec.identity, LifecycleEventKind::Ready, Some(pid));
                    if let Some(url) = &spec.http_url {
                        let mut declared = RuntimeBindings::new();
                        declared
                            .insert(&spec.identity, "http_url", url.clone())
                            .map_err(|detail| RunnerError::InvalidPlan { detail })?;
                        self.bindings
                            .merge_checked(&declared)
                            .map_err(|detail| RunnerError::InvalidPlan { detail })?;
                    }
                }
                LaunchKind::Adapter => {
                    let spec = self
                        .spawn_plan
                        .adapter(&entry.identity)
                        .cloned()
                        .ok_or_else(|| RunnerError::InvalidPlan {
                            detail: format!(
                                "launch order references unknown adapter {}",
                                entry.identity
                            ),
                        })?;
                    match self.start_adapter(&spec, cancel).await {
                        Ok(bindings) => {
                            self.push_event(&spec.identity, LifecycleEventKind::Spawned, None);
                            started.push(spec.identity.clone());
                            if let Err(error) = self.apply_adapter_readiness(&spec, cancel).await {
                                let cleanup = self.teardown_running().await;
                                return Err(attach_cleanup(error, cleanup));
                            }
                            self.bindings
                                .merge_checked(&bindings)
                                .map_err(|detail| RunnerError::InvalidPlan { detail })?;
                            self.push_event(&spec.identity, LifecycleEventKind::Ready, None);
                        }
                        Err(error) => {
                            if cancel.is_cancelled() {
                                return self.cancel_after_start().await;
                            }
                            let cleanup = self.teardown_running().await;
                            return Err(attach_cleanup(error, cleanup));
                        }
                    }
                }
            }
        }
        Ok(StartupReport { started })
    }

    /// Start one named adapter service and register its running handle.
    async fn start_adapter(
        &mut self,
        spec: &AdapterSpec,
        cancel: &CancellationToken,
    ) -> Result<RuntimeBindings, RunnerError> {
        let adapter = self
            .options
            .service_adapters
            .get(&spec.service_type)
            .cloned()
            .ok_or_else(|| RunnerError::UnsupportedService {
                service: spec.identity.clone(),
                detail: format!(
                    "named service type {} has no registered adapter",
                    spec.service_type
                ),
            })?;
        let request = ServiceStartRequest {
            service: spec.identity.clone(),
            service_type: spec.service_type.clone(),
            config: spec.config.clone(),
            grace: spec.grace,
        };
        let child = cancel.child_token();
        let handle = tokio::select! {
            () = cancel.cancelled() => {
                return Err(RunnerError::Cancelled { cleanup: Vec::new() });
            }
            started = adapter.start(request, child) => {
                started.map_err(|message| RunnerError::SpawnFailed {
                    service: spec.identity.clone(),
                    message,
                    cleanup: Vec::new(),
                })?
            }
        };
        // Adapter `start` returns after adapter-owned readiness; bindings are
        // captured here so workloads receive the startup-established map.
        // The handle moves into `running` so shutdown owns it.
        let mut bindings = handle.bindings();
        if let Some(url) = &spec.http_url {
            let mut declared = RuntimeBindings::new();
            declared
                .insert(&spec.identity, "http_url", url.clone())
                .map_err(|detail| RunnerError::InvalidPlan { detail })?;
            if let Err(detail) = bindings.merge_checked(&declared) {
                let mut handle = handle;
                let shutdown = handle.shutdown(spec.grace).await;
                let cleanup = shutdown.err().map_or_else(String::new, |message| {
                    format!("; adapter cleanup failed: {message}")
                });
                return Err(RunnerError::UnsupportedService {
                    service: spec.identity.clone(),
                    detail: format!("{detail}; adapter binding conflict{cleanup}"),
                });
            }
        }
        self.running
            .push(RunningManagedService::Adapter(RunningAdapter {
                identity: spec.identity.clone(),
                service_type: spec.service_type.clone(),
                handle,
                bindings: bindings.clone(),
                grace: spec.grace,
            }));
        Ok(bindings)
    }

    /// Apply plan-level readiness to an adapter service.
    ///
    /// Adapter start already waited for adapter-owned readiness. A
    /// plan-level `Probe` on an in-process service is rejected (no PID
    /// exists to probe); an optional post-ready delay remains supported
    /// outside measurement.
    async fn apply_adapter_readiness(
        &self,
        spec: &AdapterSpec,
        cancel: &CancellationToken,
    ) -> Result<(), RunnerError> {
        match &spec.readiness {
            None => Ok(()),
            Some(Readiness::Delay { after_ms }) => {
                let delay = Duration::from_millis(after_ms.get());
                tokio::select! {
                    () = cancel.cancelled() => Err(RunnerError::Cancelled {
                        cleanup: Vec::new(),
                    }),
                    () = tokio::time::sleep(delay) => Ok(()),
                }
            }
            Some(Readiness::Probe { probe, .. }) => Err(RunnerError::UnsupportedProbe {
                service: spec.identity.clone(),
                probe: probe.as_str().to_owned(),
            }),
        }
    }

    /// Stop owned services in reverse dependency order.
    ///
    /// Every owned service is attempted even when one stop fails; failures
    /// are collected, never thrown away, and never rewrite a primary outcome.
    /// Adapter shutdown failure becomes existing cleanup evidence, exactly
    /// like process teardown failure.
    pub async fn shutdown(&mut self) -> ShutdownReport {
        let mut stopped_order = Vec::new();
        let mut failures = Vec::new();
        while let Some(mut service) = self.running.pop() {
            let identity = service.identity().to_owned();
            stopped_order.push(identity.clone());
            match &mut service {
                RunningManagedService::Process(process) => {
                    let pid = process.pid;
                    self.push_event(&identity, LifecycleEventKind::Stopping, Some(pid));
                    let grace = shutdown_grace(&process.spec);
                    if let Err(reason) =
                        stop_process(process.as_mut(), &self.options.platform, grace).await
                    {
                        failures.push(CleanupFailure::new(identity.clone(), reason));
                    }
                    self.push_event(&identity, LifecycleEventKind::Stopped, Some(pid));
                    self.retained.push(RetainedLogs {
                        identity,
                        stdout_spool: Arc::clone(&process.stdout_spool),
                        stderr_spool: Arc::clone(&process.stderr_spool),
                    });
                }
                RunningManagedService::Adapter(adapter) => {
                    self.push_event(&identity, LifecycleEventKind::Stopping, None);
                    let grace = adapter.grace;
                    if let Err(reason) = adapter.handle.shutdown(grace).await {
                        failures.push(CleanupFailure::new(identity.clone(), reason));
                    }
                    self.push_event(&identity, LifecycleEventKind::Stopped, None);
                }
            }
        }
        ShutdownReport {
            stopped_order,
            failures,
        }
    }

    /// Run one start-readiness-stop lifecycle pass.
    ///
    /// Startup failures return the initiating error with cleanup attached;
    /// retained spools and events remain inspectable on the session. After
    /// successful startup the session shuts down and reports teardown
    /// problems in the outcome.
    ///
    /// # Errors
    /// Returns [`RunnerError`] when startup, readiness, or cancellation
    /// fails before the stop pass.
    pub async fn run(
        &mut self,
        cancel: &CancellationToken,
    ) -> Result<LifecycleOutcome, RunnerError> {
        let started = self.startup(cancel).await?.started;
        let report = self.shutdown().await;
        Ok(LifecycleOutcome {
            started,
            stopped_order: report.stopped_order,
            cleanup: report.failures,
        })
    }

    fn check_probes_registered(&self) -> Result<(), RunnerError> {
        for spec in &self.spawn_plan.specs {
            if let Some(Readiness::Probe { probe, .. }) = &spec.readiness
                && self.options.probes.get(probe).is_none()
            {
                return Err(RunnerError::UnsupportedProbe {
                    service: spec.identity.clone(),
                    probe: probe.as_str().to_owned(),
                });
            }
        }
        Ok(())
    }

    fn push_event(&mut self, identity: &str, kind: LifecycleEventKind, pid: Option<u32>) {
        self.events.push(LifecycleEvent {
            seq: self.next_seq,
            identity: identity.to_owned(),
            kind,
            pid,
            elapsed_ms: u64::try_from(self.prepared_at.elapsed().as_millis()).unwrap_or(u64::MAX),
        });
        self.next_seq += 1;
    }

    fn spawn_one(spec: &ProcessSpec) -> Result<RunningProcess, String> {
        let mut command = tokio::process::Command::new(&spec.argv[0]);
        if spec.argv.len() > 1 {
            command.args(&spec.argv[1..]);
        }
        command.current_dir(&spec.cwd);
        command.env_clear();
        command.envs(&spec.env);
        command.stdin(Stdio::null());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        command.kill_on_drop(false);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.as_std_mut().process_group(0);
        }
        let mut child = command.spawn().map_err(|error| error.to_string())?;
        let pid = child
            .id()
            .ok_or_else(|| "spawned process has no identifier".to_owned())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "spawned process has no stdout pipe".to_owned())?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "spawned process has no stderr pipe".to_owned())?;
        let stdout_spool = Arc::new(Mutex::new(SpoolState::new(spec.log_limit_bytes)));
        let stderr_spool = Arc::new(Mutex::new(SpoolState::new(spec.log_limit_bytes)));
        let drain_handles = vec![
            spawn_drain_task(stdout, Arc::clone(&stdout_spool)),
            spawn_drain_task(stderr, Arc::clone(&stderr_spool)),
        ];
        Ok(RunningProcess {
            spec: spec.clone(),
            child,
            pid,
            stdout_spool,
            stderr_spool,
            drain_handles,
        })
    }

    async fn apply_readiness(
        &mut self,
        spec: &ProcessSpec,
        pid: u32,
        cancel: &CancellationToken,
    ) -> Result<(), RunnerError> {
        match &spec.readiness {
            None => self.check_alive(spec, pid),
            Some(Readiness::Delay { after_ms }) => {
                let deadline = Duration::from_millis(after_ms.get());
                let outcome = {
                    let child = match self.running_mut(spec) {
                        Ok(child) => child,
                        Err(error) => {
                            let cleanup = self.teardown_running().await;
                            return Err(attach_cleanup(error, cleanup));
                        }
                    };
                    wait_for_exit_cancel(child, deadline, cancel).await
                };
                match outcome {
                    WaitOutcome::Cancelled => Err(RunnerError::Cancelled {
                        cleanup: Vec::new(),
                    }),
                    WaitOutcome::Exited(status) => Err(RunnerError::ProcessExitedEarly {
                        service: spec.identity.clone(),
                        message: format!("exited before delay elapsed: {status}"),
                        cleanup: Vec::new(),
                    }),
                    WaitOutcome::Elapsed => self.check_alive(spec, pid),
                }
            }
            Some(Readiness::Probe { probe, timeout_ms }) => {
                let probe_impl = self.options.probes.get(probe).ok_or_else(|| {
                    RunnerError::UnsupportedProbe {
                        service: spec.identity.clone(),
                        probe: probe.as_str().to_owned(),
                    }
                })?;
                let ctx = ProbeContext {
                    service: spec.identity.clone(),
                    pid: Some(pid),
                    alive: self.options.platform.is_alive(pid),
                };
                let timeout = Duration::from_millis(timeout_ms.get());
                tokio::select! {
                    () = cancel.cancelled() => {
                        Err(RunnerError::Cancelled { cleanup: Vec::new() })
                    }
                    result = tokio::time::timeout(timeout, probe_impl.check(&ctx)) => {
                        match result {
                            Err(_) => Err(RunnerError::ReadinessTimeout {
                                service: spec.identity.clone(),
                                timeout_ms: timeout_ms.get(),
                                cleanup: Vec::new(),
                            }),
                            Ok(Err(failure)) => Err(RunnerError::ReadinessFailed {
                                service: spec.identity.clone(),
                                message: failure.to_string(),
                                cleanup: Vec::new(),
                            }),
                            Ok(Ok(())) => self.check_alive(spec, pid),
                        }
                    }
                }
            }
        }
    }

    /// Confirm an owned process has not exited, reaping zombies first.
    ///
    /// Signal-zero liveness alone can mistake an unreaped zombie for a live
    /// process, so the owned handle is polled before consulting the
    /// platform adapter.
    /// Confirm an owned process has not exited, reaping zombies first.
    ///
    /// Signal-zero liveness alone can mistake an unreaped zombie for a live
    /// process, so the owned handle is polled before consulting the
    /// platform adapter.
    fn check_alive(&mut self, spec: &ProcessSpec, pid: u32) -> Result<(), RunnerError> {
        let exited = self
            .running
            .iter_mut()
            .rev()
            .find_map(|service| match service {
                RunningManagedService::Process(process)
                    if process.spec.identity == spec.identity =>
                {
                    Some(process)
                }
                _ => None,
            })
            .and_then(|process| process.child.try_wait().ok().flatten());
        if let Some(status) = exited {
            return Err(RunnerError::ProcessExitedEarly {
                service: spec.identity.clone(),
                message: format!("exited with status {status}"),
                cleanup: Vec::new(),
            });
        }
        if self.options.platform.is_alive(pid) {
            Ok(())
        } else {
            Err(RunnerError::ProcessExitedEarly {
                service: spec.identity.clone(),
                message: "process is not alive".to_owned(),
                cleanup: Vec::new(),
            })
        }
    }

    fn running_mut(
        &mut self,
        spec: &ProcessSpec,
    ) -> Result<&mut tokio::process::Child, RunnerError> {
        self.running
            .iter_mut()
            .rev()
            .find_map(|service| match service {
                RunningManagedService::Process(process)
                    if process.spec.identity == spec.identity =>
                {
                    Some(&mut process.child)
                }
                _ => None,
            })
            .ok_or_else(|| RunnerError::ProcessExitedEarly {
                service: spec.identity.clone(),
                message: "owned process handle is missing".to_owned(),
                cleanup: Vec::new(),
            })
    }

    async fn teardown_running(&mut self) -> Vec<CleanupFailure> {
        let mut failures = Vec::new();
        while let Some(mut service) = self.running.pop() {
            let identity = service.identity().to_owned();
            match &mut service {
                RunningManagedService::Process(process) => {
                    let pid = process.pid;
                    self.push_event(&identity, LifecycleEventKind::Stopping, Some(pid));
                    let grace = shutdown_grace(&process.spec);
                    if let Err(reason) =
                        stop_process(process.as_mut(), &self.options.platform, grace).await
                    {
                        failures.push(CleanupFailure::new(identity.clone(), reason));
                    }
                    self.push_event(&identity, LifecycleEventKind::Stopped, Some(pid));
                    self.retained.push(RetainedLogs {
                        identity,
                        stdout_spool: Arc::clone(&process.stdout_spool),
                        stderr_spool: Arc::clone(&process.stderr_spool),
                    });
                }
                RunningManagedService::Adapter(adapter) => {
                    self.push_event(&identity, LifecycleEventKind::Stopping, None);
                    let grace = adapter.grace;
                    if let Err(reason) = adapter.handle.shutdown(grace).await {
                        failures.push(CleanupFailure::new(identity.clone(), reason));
                    }
                    self.push_event(&identity, LifecycleEventKind::Stopped, None);
                }
            }
        }
        failures
    }

    async fn cancel_after_start(&mut self) -> Result<StartupReport, RunnerError> {
        if self.running.is_empty() {
            return Err(RunnerError::CancelledBeforeSpawn);
        }
        let cleanup = self.teardown_running().await;
        Err(RunnerError::Cancelled { cleanup })
    }
}

fn shutdown_grace(spec: &ProcessSpec) -> Duration {
    spec.shutdown.as_ref().map_or(
        Duration::from_millis(crate::spec::DEFAULT_GRACE_MS),
        |shutdown| Duration::from_millis(shutdown.grace_ms.get()),
    )
}

fn attach_cleanup(error: RunnerError, cleanup: Vec<CleanupFailure>) -> RunnerError {
    match error {
        RunnerError::SpawnFailed {
            service, message, ..
        } => RunnerError::SpawnFailed {
            service,
            message,
            cleanup,
        },
        RunnerError::ReadinessFailed {
            service, message, ..
        } => RunnerError::ReadinessFailed {
            service,
            message,
            cleanup,
        },
        RunnerError::ReadinessTimeout {
            service,
            timeout_ms,
            ..
        } => RunnerError::ReadinessTimeout {
            service,
            timeout_ms,
            cleanup,
        },
        RunnerError::ProcessExitedEarly {
            service, message, ..
        } => RunnerError::ProcessExitedEarly {
            service,
            message,
            cleanup,
        },
        RunnerError::Cancelled { .. } => RunnerError::Cancelled { cleanup },
        other => other,
    }
}

fn spawn_drain_task<R>(reader: R, spool: Arc<Mutex<SpoolState>>) -> tokio::task::JoinHandle<()>
where
    R: AsyncReadExt + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut reader = reader;
        let mut chunk = vec![0_u8; DRAIN_CHUNK_BYTES];
        loop {
            match reader.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    spool.lock().await.push(&chunk[..read]);
                }
            }
        }
    })
}

/// Cancellation-aware outcome while waiting for a child to exit.
#[derive(Debug)]
enum WaitOutcome {
    /// The child exited with this status.
    Exited(std::process::ExitStatus),
    /// Caller cancellation arrived first.
    Cancelled,
    /// The deadline elapsed with the child still alive.
    Elapsed,
}

/// Wait for a child to exit, observing caller cancellation throughout.
async fn wait_for_exit_cancel(
    child: &mut tokio::process::Child,
    deadline: Duration,
    cancel: &CancellationToken,
) -> WaitOutcome {
    let start = Instant::now();
    loop {
        if cancel.is_cancelled() {
            return WaitOutcome::Cancelled;
        }
        match child.try_wait() {
            Ok(Some(status)) => return WaitOutcome::Exited(status),
            Ok(None) => {}
            Err(_) => return WaitOutcome::Elapsed,
        }
        if start.elapsed() >= deadline {
            return WaitOutcome::Elapsed;
        }
        tokio::select! {
            () = cancel.cancelled() => return WaitOutcome::Cancelled,
            () = tokio::time::sleep(POLL_INTERVAL) => {}
        }
    }
}

async fn stop_process(
    service: &mut RunningProcess,
    platform: &Arc<dyn PlatformAdapter>,
    grace: Duration,
) -> Result<(), String> {
    if platform.support() == PlatformSupport::Unsupported {
        return Err("managed descendant cleanup is unsupported on this platform".to_owned());
    }
    // Best-effort graceful request; a missing group means already gone.
    if let Err(reason) = platform.terminate_group(service.pid) {
        // Continue to the wait/kill path so descendants are still reclaimed.
        let _ = reason;
    }
    if wait_for_child_exit(&mut service.child, grace).await {
        join_drains(service).await;
        return Ok(());
    }
    platform
        .kill_group(service.pid)
        .map_err(|reason| format!("forced process-tree cleanup failed: {reason}"))?;
    if wait_for_child_exit(&mut service.child, KILL_WAIT_LIMIT).await {
        join_drains(service).await;
        return Ok(());
    }
    Err(format!(
        "process {} did not exit after forced cleanup",
        service.pid
    ))
}

async fn wait_for_child_exit(child: &mut tokio::process::Child, limit: Duration) -> bool {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) => {}
            Err(_) => return false,
        }
        if start.elapsed() >= limit {
            return false;
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

async fn join_drains(service: &mut RunningProcess) {
    for handle in service.drain_handles.drain(..) {
        let _ = tokio::time::timeout(DRAIN_JOIN_LIMIT, handle).await;
    }
}
