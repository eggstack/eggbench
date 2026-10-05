//! Argv-only external command execution with bounded capture,
//! cancellation/timeout cleanup, and typed outcomes.
//!
//! Environment policy: `env_clear` plus explicit driver environment only
//! (deterministic `LC_ALL=C`/`LANG=C` on Unix helpers; Windows preserves only
//! caller-supplied variables). The user environment is never inherited
//! wholesale. Stdin defaults to null. No shell, glob expansion, or implicit
//! cwd is ever used.
//!
//! Unix cleanup uses a dedicated process group (TERM then KILL with a bounded
//! wait). Windows reports direct-child-only cleanup explicitly and never
//! claims Job Object process-tree semantics.

use super::error::{DriverError, ErrorCategory};
use super::resolver::ResolvedExecutable;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

/// Maximum stdin payload accepted by the substrate (64 KiB). Generated
/// machine plans (for example Eggprobe schema-0.3 diagnostic plans) are
/// small deterministic JSON documents far below this cap; anything larger
/// fails before spawn rather than streaming unbounded input.
pub const MAX_STDIN_BYTES: u64 = 64 * 1024;

/// Protocol-neutral external command specification.
#[derive(Debug, Clone)]
pub struct ExternalCommandSpec {
    /// Resolved executable (argv[0]).
    pub executable: ResolvedExecutable,
    /// Arguments passed as argv (no shell interpretation).
    pub args: Vec<OsString>,
    /// Explicit working directory; `None` means the caller current directory
    /// is inherited explicitly by the driver (never an implicit secret cwd).
    pub cwd: Option<PathBuf>,
    /// Explicit driver environment only.
    pub env: BTreeMap<OsString, OsString>,
    /// When true stdin is null.
    pub stdin_null: bool,
    /// Optional bounded stdin payload. When `Some`, stdin is piped, the
    /// payload is written once, and the pipe closes so the child observes
    /// EOF (used for `eggprobe run -` plan delivery). Takes precedence over
    /// `stdin_null`. Payloads above [`MAX_STDIN_BYTES`] fail before spawn.
    pub stdin_bytes: Option<Vec<u8>>,
    /// Stdout retention cap in bytes.
    pub stdout_limit: u64,
    /// Stderr retention cap in bytes.
    pub stderr_limit: u64,
    /// Explicit driver command timeout.
    pub timeout: Duration,
}

/// Bounded captured byte stream: draining continues after the cap so the
/// child can never block on a full pipe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedStream {
    retained: Vec<u8>,
    retained_bytes: u64,
    dropped_bytes: u64,
    total_bytes: u64,
    truncated: bool,
}

impl CapturedStream {
    /// Retained leading bytes.
    #[must_use]
    pub fn retained(&self) -> &[u8] {
        &self.retained
    }

    /// Retained byte count.
    #[must_use]
    pub const fn retained_bytes(&self) -> u64 {
        self.retained_bytes
    }

    /// Discarded byte count beyond the cap.
    #[must_use]
    pub const fn dropped_bytes(&self) -> u64 {
        self.dropped_bytes
    }

    /// Total bytes drained.
    #[must_use]
    pub const fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    /// True when output exceeded the cap.
    #[must_use]
    pub const fn truncated(&self) -> bool {
        self.truncated
    }

    /// Build from retained bytes and total drained bytes.
    ///
    /// The retention cap is applied by [`drain_stream`] while the pipe is
    /// read, so this constructor only needs the retained prefix and the
    /// total drained: `truncated` follows from `total > retained`.
    pub(crate) fn from_parts(bytes: Vec<u8>, total: u64) -> Self {
        let retained_bytes = bytes.len() as u64;
        let dropped_bytes = total.saturating_sub(retained_bytes);
        Self {
            retained: bytes,
            retained_bytes,
            dropped_bytes,
            total_bytes: total,
            truncated: dropped_bytes > 0,
        }
    }
}

/// Typed external command outcome.
#[derive(Debug, Clone)]
pub struct ExternalCommandOutcome {
    /// Executable identity.
    pub executable: ResolvedExecutable,
    /// Redaction-safe argv length (values themselves are not echoed).
    pub argc: usize,
    /// Exit code, if the child was reaped.
    pub exit_code: Option<i32>,
    /// Bounded stdout.
    pub stdout: CapturedStream,
    /// Bounded stderr.
    pub stderr: CapturedStream,
    /// Monotonic wall duration.
    pub duration: Duration,
    /// True when the invocation was cancelled.
    ///
    /// Cancellation normally returns a typed [`ErrorCategory::Cancelled`]
    /// failure; it is also recorded here when the token fires while the
    /// child's output is still draining, because the captured bytes are then
    /// incomplete (see [`ExternalCommandOutcome::timed_out`]).
    pub cancelled: bool,
    /// True when the driver deadline expired during this invocation.
    ///
    /// Derived from the actual deadline outcome, not from the success-only
    /// construction site: the deadline is recorded both when it fires first
    /// (which returns [`ErrorCategory::TimedOut`]) and when it passes while
    /// the child's output is still draining. In the latter case the child
    /// was reaped with an exit status but the captured bytes are
    /// incomplete, so the invocation is reported as timed out instead of as
    /// a clean success that silently lost output.
    pub timed_out: bool,
    /// Cleanup diagnostics (e.g. `direct_child_only` on Windows,
    /// `forced_termination` after a cancel/timeout kill).
    pub cleanup_notes: Vec<String>,
}

/// Bound on the post-kill re-wait for a force-terminated child.
const REAP_BOUND: Duration = Duration::from_secs(5);

/// Bound on draining each output pipe after the child is reaped.
const DRAIN_BOUND: Duration = Duration::from_secs(5);

/// Cleanup note recorded when the child had to be force-terminated.
const FORCED_TERMINATION_NOTE: &str = "forced_termination";

/// Cleanup note recorded when the driver deadline passed while the child's
/// output was still being drained.
const DEADLINE_ELAPSED_NOTE: &str = "deadline_elapsed_during_drain";

/// Cleanup note recorded when the invocation was cancelled while the child's
/// output was still being drained.
const CANCELLED_DURING_DRAIN_NOTE: &str = "cancelled_during_drain";

/// Windows cleanup limitation: only the direct child is terminated, so a
/// grandchild can outlive the invocation.
#[cfg(windows)]
const DIRECT_CHILD_ONLY_NOTE: &str = "direct_child_only";

/// Execute one external command with bounded capture and cleanup.
///
/// Observes the invocation cancellation token, the explicit driver timeout,
/// and child exit. On cancellation/timeout: request bounded termination,
/// continue draining pipes, ensure owned process cleanup, and return a typed
/// cancelled/timed-out failure whose detail carries the cleanup record.
///
/// A child that is reaped while its output is still draining (a descendant
/// holding the inherited pipe) yields an outcome that records the real
/// `cancelled`/`timed_out` state and a `cleanup_notes` entry, because the
/// exit status alone does not make the captured bytes complete.
///
/// # Errors
/// Returns typed [`DriverError`] on spawn failure, cancellation, timeout, a
/// drain that does not complete within its bound, or nonzero exit (nonzero
/// exit is a typed outcome failure, not a silent pass).
#[allow(clippy::too_many_lines)] // One auditable spawn/drain/cancel/reap sequence.
pub async fn run_command(
    spec: &ExternalCommandSpec,
    cancel: &CancellationToken,
) -> Result<ExternalCommandOutcome, DriverError> {
    if spec
        .stdin_bytes
        .as_ref()
        .is_some_and(|payload| u64::try_from(payload.len()).unwrap_or(u64::MAX) > MAX_STDIN_BYTES)
    {
        return Err(DriverError::execution(
            ErrorCategory::UnsupportedOption,
            format!("stdin payload exceeds bound ({MAX_STDIN_BYTES} bytes)"),
        ));
    }
    let start = Instant::now();
    let mut command = tokio::process::Command::new(&spec.executable.canonical_path);
    command.args(&spec.args);
    command.env_clear();
    for (key, value) in &spec.env {
        command.env(key, value);
    }
    if let Some(cwd) = &spec.cwd {
        command.current_dir(cwd);
    }
    if spec.stdin_bytes.is_some() {
        command.stdin(Stdio::piped());
    } else if spec.stdin_null {
        command.stdin(Stdio::null());
    }
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    #[cfg(unix)]
    {
        // Dedicated process group so descendants can be signalled together
        // (same safe mechanism as the runner session; no second signal
        // implementation).
        use std::os::unix::process::CommandExt as _;
        command.as_std_mut().process_group(0);
    }
    // Kill-on-drop ensures the owned child cannot leak if we forget to reap.
    command.kill_on_drop(true);

    let mut child = command.spawn().map_err(|e| {
        DriverError::execution(
            ErrorCategory::SpawnFailed,
            format!("spawn failed for {}: {e}", spec.executable.logical_tool),
        )
    })?;

    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();
    let stdout_limit = spec.stdout_limit;
    let stderr_limit = spec.stderr_limit;

    // Bounded one-shot stdin delivery: write the payload once, then shut
    // down so the child observes EOF. The writer is aborted once the child
    // is reaped (or during cancellation/timeout cleanup).
    let stdin_writer = spec.stdin_bytes.clone().map(|payload| {
        let stdin = child.stdin.take();
        tokio::spawn(async move {
            if let Some(mut stdin) = stdin {
                let _ = stdin.write_all(&payload).await;
                let _ = stdin.shutdown().await;
            }
        })
    });

    let stdout_task = tokio::spawn(async move {
        let Some(pipe) = stdout_pipe.take() else {
            return (Vec::new(), 0_u64);
        };
        drain_stream(pipe, stdout_limit).await
    });
    let stderr_task = tokio::spawn(async move {
        let Some(pipe) = stderr_pipe.take() else {
            return (Vec::new(), 0_u64);
        };
        drain_stream(pipe, stderr_limit).await
    });

    let pid = child.id();
    let deadline = tokio::time::sleep(spec.timeout);
    tokio::pin!(deadline);

    let wait_result = tokio::select! {
        status = child.wait() => WaitKind::Exited(status.map(Some)),
        () = cancel.cancelled() => WaitKind::Cancelled,
        () = &mut deadline => WaitKind::TimedOut,
    };

    let mut cancelled = false;
    let mut timed_out = false;
    let wait_status = match wait_result {
        WaitKind::Exited(result) => result.map_err(|e| {
            DriverError::execution(ErrorCategory::CleanupFailed, format!("wait failed: {e}"))
        })?,
        WaitKind::Cancelled => {
            cancelled = true;
            None
        }
        WaitKind::TimedOut => {
            timed_out = true;
            None
        }
    };

    // Cleanup diagnostics are gathered before any early return so the
    // cancellation and timeout paths record the same orphan-risk evidence as
    // the success path — on exactly the platform (Windows
    // `direct_child_only`) where a grandchild is least likely to be reaped.
    let mut cleanup_notes = platform_cleanup_notes();

    if cancelled || timed_out {
        cleanup_notes.push(FORCED_TERMINATION_NOTE.to_owned());
        if let Some(writer) = stdin_writer {
            writer.abort();
        }
        terminate_child(&mut child, pid).await;
        // Bounded re-wait so pipes drain and the child is reaped.
        let _reaped: Option<std::process::ExitStatus> =
            tokio::time::timeout(REAP_BOUND, child.wait())
                .await
                .map_or(None, Result::ok);
        // Drain pipes so a killed child can never block on a full pipe. The
        // captured bytes are discarded: the invocation already failed, and
        // the cleanup record travels with the error instead.
        let _ = join_pipes(stdout_task, stderr_task).await;
        let (category, detail) = if timed_out {
            (
                ErrorCategory::TimedOut,
                format!(
                    "command {} timed out after {:?}",
                    spec.executable.logical_tool, spec.timeout
                ),
            )
        } else {
            (
                ErrorCategory::Cancelled,
                format!("command {} was cancelled", spec.executable.logical_tool),
            )
        };
        return Err(DriverError::execution(
            category,
            with_cleanup_notes(detail, &cleanup_notes),
        ));
    }

    // A drain that does not complete is a failure, never empty output: the
    // captured bytes would otherwise look like a tool that printed nothing.
    let (stdout, stderr) = join_pipes(stdout_task, stderr_task).await?;
    if let Some(writer) = stdin_writer {
        writer.abort();
    }
    let duration = start.elapsed();
    // `wait` reports the child's exit as soon as it is reaped, which can
    // happen while its output is still draining (a descendant that inherited
    // the output handle keeps the pipe open). The deadline and the
    // cancellation token can both fire in that window: the exit status is
    // known but the captured evidence is incomplete, so the outcome records
    // the real cancellation/timeout state instead of a clean success.
    if duration > spec.timeout {
        timed_out = true;
        cleanup_notes.push(DEADLINE_ELAPSED_NOTE.to_owned());
    }
    if cancel.is_cancelled() {
        cancelled = true;
        cleanup_notes.push(CANCELLED_DURING_DRAIN_NOTE.to_owned());
    }
    let exit_code = wait_status.and_then(|s| s.code());

    Ok(ExternalCommandOutcome {
        executable: spec.executable.clone(),
        argc: spec.args.len() + 1,
        exit_code,
        stdout: CapturedStream::from_parts(stdout.0, stdout.1),
        stderr: CapturedStream::from_parts(stderr.0, stderr.1),
        duration,
        cancelled,
        timed_out,
        cleanup_notes,
    })
}

/// Platform cleanup diagnostics recorded on every invocation, before any
/// early return, so cancellation and timeout failures carry them too.
fn platform_cleanup_notes() -> Vec<String> {
    #[cfg(windows)]
    {
        vec![DIRECT_CHILD_ONLY_NOTE.to_owned()]
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// Append the cleanup record to a failure detail.
///
/// The outcome struct only exists on the success path, so a cancelled or
/// timed-out invocation carries its cleanup notes in the error detail
/// instead of dropping them.
fn with_cleanup_notes(detail: String, notes: &[String]) -> String {
    if notes.is_empty() {
        return detail;
    }
    format!("{detail} [cleanup: {}]", notes.join(", "))
}

/// How one command wait resolved.
enum WaitKind {
    /// Child exited (or wait failed).
    Exited(std::io::Result<Option<std::process::ExitStatus>>),
    /// Invocation cancellation fired first.
    Cancelled,
    /// Driver timeout expired first.
    TimedOut,
}

/// Drain one child pipe to completion, retaining at most `limit` bytes.
///
/// Draining continues after the cap so the child can never block on a full
/// pipe; excess bytes are counted as dropped.
async fn drain_stream(mut pipe: impl tokio::io::AsyncRead + Unpin, limit: u64) -> (Vec<u8>, u64) {
    let mut retained = Vec::new();
    let mut total: u64 = 0;
    let mut buf = [0_u8; 8192];
    loop {
        match pipe.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                total += u64::try_from(n).unwrap_or(u64::MAX);
                let room = usize::try_from(
                    limit.saturating_sub(u64::try_from(retained.len()).unwrap_or(u64::MAX)),
                )
                .unwrap_or(usize::MAX);
                if room > 0 {
                    let take = room.min(n);
                    retained.extend_from_slice(&buf[..take]);
                }
            }
        }
    }
    (retained, total)
}

/// Join both pipe drains within [`DRAIN_BOUND`].
async fn join_pipes(
    stdout_task: tokio::task::JoinHandle<(Vec<u8>, u64)>,
    stderr_task: tokio::task::JoinHandle<(Vec<u8>, u64)>,
) -> Result<((Vec<u8>, u64), (Vec<u8>, u64)), DriverError> {
    join_pipes_within(stdout_task, stderr_task, DRAIN_BOUND).await
}

/// Join both pipe drains within an explicit bound.
///
/// A drain that does not finish is a typed failure, never empty output:
/// `(Vec::new(), 0)` would be indistinguishable from a tool that printed
/// nothing, and a parser would then report "malformed output" where the
/// truth is "we stopped draining".
///
/// The category is [`ErrorCategory::CleanupFailed`] — the bounded post-reap
/// drain is part of owned process cleanup, and the invocation did not fail
/// because the *driver* timeout expired (`TimedOut` would claim a deadline
/// that never fired) nor because the tool produced more than the retention
/// cap (`OutputTruncated`, which the bounded stream reports itself). No new
/// public error variant is introduced.
async fn join_pipes_within(
    stdout_task: tokio::task::JoinHandle<(Vec<u8>, u64)>,
    stderr_task: tokio::task::JoinHandle<(Vec<u8>, u64)>,
    bound: Duration,
) -> Result<((Vec<u8>, u64), (Vec<u8>, u64)), DriverError> {
    let stdout = join_pipe("stdout", stdout_task, bound).await?;
    let stderr = join_pipe("stderr", stderr_task, bound).await?;
    Ok((stdout, stderr))
}

/// Join one pipe drain, failing closed on a drain timeout or a panicked
/// drain task.
async fn join_pipe(
    stream: &str,
    task: tokio::task::JoinHandle<(Vec<u8>, u64)>,
    bound: Duration,
) -> Result<(Vec<u8>, u64), DriverError> {
    match tokio::time::timeout(bound, task).await {
        Ok(Ok(drained)) => Ok(drained),
        Ok(Err(join_error)) => Err(DriverError::execution(
            ErrorCategory::CleanupFailed,
            format!("{stream} drain task failed: {join_error}"),
        )),
        Err(_elapsed) => Err(DriverError::execution(
            ErrorCategory::CleanupFailed,
            format!("{stream} drain did not complete within {bound:?}"),
        )),
    }
}

#[cfg(unix)]
async fn terminate_child(child: &mut tokio::process::Child, child_pid: Option<u32>) {
    use nix::sys::signal::{Signal, killpg};
    use nix::unistd::Pid;
    if let Some(raw) = child_pid {
        let group = Pid::from_raw(raw.cast_signed());
        let _ = killpg(group, Signal::SIGTERM);
        tokio::time::sleep(Duration::from_millis(500)).await;
        // Liveness races make a conditional kill unreliable; send KILL to the
        // group unconditionally (idempotent) then let the caller reap.
        let _ = killpg(group, Signal::SIGKILL);
    }
    let _ = child.kill().await;
}

#[cfg(not(unix))]
async fn terminate_child(child: &mut tokio::process::Child, _pid: Option<u32>) {
    let _ = child.kill().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Env marker that turns a spawned copy of this test binary into a child
    /// process. It is absent in a normal run, so the two child-mode tests
    /// below return immediately when the suite itself executes them.
    const CHILD_ENV: &str = "EGGBENCH_TEST_COMMAND_CHILD";

    /// Child sleep duration in milliseconds (`CHILD_ENV` must also be set).
    const CHILD_SLEEP_MS_ENV: &str = "EGGBENCH_TEST_COMMAND_CHILD_SLEEP_MS";

    /// libtest filter selecting [`long_lived_child_helper`].
    const LONG_LIVED_FILTER: &str = "long_lived_child_helper";

    /// libtest filter selecting [`exit_with_held_pipe_helper`].
    const HELD_PIPE_FILTER: &str = "exit_with_held_pipe_helper";

    #[test]
    fn captured_stream_truncation_follows_total_minus_retained() {
        // `from_parts` owns the bound: the retention cap is applied while the
        // pipe is drained, so `truncated` is exactly "drained more than was
        // retained".
        let under_bound = CapturedStream::from_parts(vec![b'x'; 100], 100);
        assert!(!under_bound.truncated());
        assert_eq!(under_bound.retained_bytes(), 100);
        assert_eq!(under_bound.dropped_bytes(), 0);

        let over_bound = CapturedStream::from_parts(vec![b'x'; 64], 1000);
        assert!(over_bound.truncated());
        assert_eq!(over_bound.retained_bytes(), 64);
        assert_eq!(over_bound.dropped_bytes(), 936);
        assert_eq!(over_bound.total_bytes(), 1000);
    }

    #[test]
    fn command_spec_is_argv_only_by_construction() {
        // No shell string exists anywhere in the spec: args are argv entries
        // and metacharacters stay literal.
        let spec = ExternalCommandSpec {
            executable: dummy_executable(),
            args: vec!["; rm -rf /".into(), "$(evil)".into()],
            cwd: None,
            env: BTreeMap::new(),
            stdin_null: true,
            stdin_bytes: None,
            stdout_limit: 1024,
            stderr_limit: 1024,
            timeout: Duration::from_secs(1),
        };
        assert_eq!(spec.args.len(), 2);
        assert!(spec.env.is_empty());
    }

    /// Long-lived child mode: sleeps so a parent's deadline/cancellation
    /// always has something still running to terminate.
    #[test]
    fn long_lived_child_helper() {
        if std::env::var_os(CHILD_ENV).is_none() {
            return;
        }
        let millis = std::env::var(CHILD_SLEEP_MS_ENV)
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(60_000);
        std::thread::sleep(Duration::from_millis(millis));
    }

    /// Child mode that exits 0 immediately while a descendant keeps the
    /// inherited stdout handle open, so the parent observes an exit whose
    /// output has not finished draining. Deterministic: the descendant holds
    /// the pipe for a fixed duration, well inside [`DRAIN_BOUND`].
    #[test]
    fn exit_with_held_pipe_helper() {
        if std::env::var_os(CHILD_ENV).is_none() {
            return;
        }
        let mut holder = std::process::Command::new(std::env::current_exe().expect("test binary"));
        holder.arg(LONG_LIVED_FILTER).arg("--nocapture");
        holder.env(CHILD_ENV, "1");
        holder.env(CHILD_SLEEP_MS_ENV, "3000");
        holder.stdout(std::process::Stdio::inherit());
        let _ = holder.spawn();
        std::process::exit(0);
    }

    /// The child's exit status is known, but its output finished draining
    /// only after the driver deadline: the outcome must say so instead of
    /// reporting a clean success over incomplete evidence.
    #[tokio::test]
    async fn deadline_during_drain_reports_timed_out_outcome() {
        let cancel = CancellationToken::new();
        let spec = child_spec(HELD_PIPE_FILTER, Duration::from_millis(1_500));
        let outcome = run_command(&spec, &cancel).await.unwrap();

        assert_eq!(outcome.exit_code, Some(0));
        assert!(outcome.timed_out, "deadline passed during the drain");
        assert!(!outcome.cancelled);
        assert!(
            outcome
                .cleanup_notes
                .contains(&DEADLINE_ELAPSED_NOTE.to_owned()),
            "cleanup notes: {:?}",
            outcome.cleanup_notes
        );
    }

    /// A timeout is a typed failure whose detail carries the cleanup record:
    /// the notes are computed before the early return, so the forced kill and
    /// the platform orphan-risk note are never dropped.
    #[tokio::test]
    async fn timed_out_invocation_carries_cleanup_notes() {
        let spec = child_spec(LONG_LIVED_FILTER, Duration::from_millis(150));
        let err = run_command(&spec, &CancellationToken::new())
            .await
            .unwrap_err();

        assert_eq!(err.category(), ErrorCategory::TimedOut);
        let detail = err.to_string();
        assert!(
            detail.contains(FORCED_TERMINATION_NOTE),
            "cleanup record missing from: {detail}"
        );
        #[cfg(windows)]
        assert!(
            detail.contains(DIRECT_CHILD_ONLY_NOTE),
            "orphan-risk note missing from: {detail}"
        );
    }

    /// Same for cancellation: the cleanup record travels with the error.
    #[tokio::test]
    async fn cancelled_invocation_carries_cleanup_notes() {
        let cancel = CancellationToken::new();
        let trigger = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(150)).await;
            trigger.cancel();
        });
        let spec = child_spec(LONG_LIVED_FILTER, Duration::from_secs(30));
        let err = run_command(&spec, &cancel).await.unwrap_err();

        assert_eq!(err.category(), ErrorCategory::Cancelled);
        let detail = err.to_string();
        assert!(
            detail.contains(FORCED_TERMINATION_NOTE),
            "cleanup record missing from: {detail}"
        );
    }

    /// A drain that never finishes is a cleanup failure, never empty output.
    #[tokio::test]
    async fn stalled_drain_is_a_cleanup_failure_not_empty_output() {
        // Injected short bound: the same code path `run_command` takes when a
        // child floods a pipe past the drain window, without a five-second
        // wait.
        let stalled = tokio::spawn(async {
            std::future::pending::<()>().await;
            (Vec::new(), 0_u64)
        });
        let drained = tokio::spawn(async { (Vec::new(), 0_u64) });
        let err = join_pipes_within(stalled, drained, Duration::from_millis(50))
            .await
            .unwrap_err();

        assert_eq!(err.category(), ErrorCategory::CleanupFailed);
        assert!(
            err.to_string().contains("stdout drain did not complete"),
            "unexpected detail: {err}"
        );
    }

    /// A drain task that panicked is also a failure, not empty output.
    #[tokio::test]
    async fn failed_drain_task_is_a_cleanup_failure() {
        let panicked = tokio::spawn(async { panic!("drain task panicked") });
        let drained = tokio::spawn(async { (Vec::new(), 0_u64) });
        let err = join_pipes_within(panicked, drained, Duration::from_millis(500))
            .await
            .unwrap_err();

        assert_eq!(err.category(), ErrorCategory::CleanupFailed);
        assert!(
            err.to_string().contains("stdout drain task failed"),
            "unexpected detail: {err}"
        );
    }

    /// Spec whose argv re-executes this test binary in one of the child modes
    /// above. No external binary is provisioned: the child is the test binary
    /// itself, selected by a libtest filter.
    fn child_spec(filter: &str, timeout: Duration) -> ExternalCommandSpec {
        let mut env: BTreeMap<OsString, OsString> = BTreeMap::new();
        env.insert(CHILD_ENV.into(), "1".into());
        ExternalCommandSpec {
            executable: test_binary_executable(),
            args: vec![filter.into(), "--nocapture".into()],
            cwd: None,
            env,
            stdin_null: true,
            stdin_bytes: None,
            stdout_limit: 64 * 1024,
            stderr_limit: 64 * 1024,
            timeout,
        }
    }

    fn test_binary_executable() -> ResolvedExecutable {
        let path = std::env::current_exe().expect("test binary path");
        ResolvedExecutable {
            logical_tool: "eggbench-test-child".to_owned(),
            selected_path: path.clone(),
            canonical_path: path,
            sha256_hex: String::new(),
            file_size: 0,
            executable_class: "test".to_owned(),
        }
    }

    #[cfg(test)]
    fn dummy_executable() -> ResolvedExecutable {
        ResolvedExecutable {
            logical_tool: "dummy".to_owned(),
            selected_path: PathBuf::from("/tmp/dummy"),
            canonical_path: PathBuf::from("/tmp/dummy"),
            sha256_hex: "00".repeat(32),
            file_size: 1,
            executable_class: "test".to_owned(),
        }
    }
}
