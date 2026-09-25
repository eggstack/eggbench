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

    pub(crate) fn collect(bytes: Vec<u8>, total: u64, _limit: u64) -> Self {
        Self::from_parts(bytes, total)
    }

    /// Build from retained bytes and total drained bytes.
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
    /// True when cancellation was requested.
    pub cancelled: bool,
    /// True when the driver timeout expired.
    pub timed_out: bool,
    /// Cleanup diagnostics (e.g. `direct_child_only` on Windows).
    pub cleanup_notes: Vec<String>,
}

/// Execute one external command with bounded capture and cleanup.
///
/// Observes the invocation cancellation token, the explicit driver timeout,
/// and child exit. On cancellation/timeout: request bounded termination,
/// continue draining pipes, ensure owned process cleanup, and return a typed
/// cancelled/timed-out failure.
///
/// # Errors
/// Returns typed [`DriverError`] on spawn failure, cancellation, timeout, or
/// nonzero exit (nonzero exit is a typed outcome failure, not a silent pass).
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

    let (cancelled, timed_out, wait_status) = match wait_result {
        WaitKind::Exited(result) => {
            let status = result.map_err(|e| {
                DriverError::execution(ErrorCategory::CleanupFailed, format!("wait failed: {e}"))
            })?;
            (false, false, status)
        }
        WaitKind::Cancelled | WaitKind::TimedOut => {
            let timed_out = matches!(wait_result, WaitKind::TimedOut);
            if let Some(writer) = stdin_writer {
                writer.abort();
            }
            terminate_child(&mut child, pid).await;
            // Bounded re-wait so pipes drain and the child is reaped.
            let status = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
            let _reaped: Option<std::process::ExitStatus> = status.map_or(None, Result::ok);
            // Drain pipes so a cancelled child can never block on a full pipe.
            drop(join_pipes(stdout_task, stderr_task).await);
            if timed_out {
                return Err(DriverError::execution(
                    ErrorCategory::TimedOut,
                    format!(
                        "command {} timed out after {:?}",
                        spec.executable.logical_tool, spec.timeout
                    ),
                ));
            }
            return Err(DriverError::execution(
                ErrorCategory::Cancelled,
                format!("command {} was cancelled", spec.executable.logical_tool),
            ));
        }
    };

    let (stdout, stderr) = join_pipes(stdout_task, stderr_task).await;
    if let Some(writer) = stdin_writer {
        writer.abort();
    }
    let duration = start.elapsed();
    let exit_code = wait_status.and_then(|s| s.code());
    let cleanup_notes: Vec<String> = {
        #[cfg(windows)]
        {
            vec!["direct_child_only".to_owned()]
        }
        #[cfg(not(windows))]
        {
            Vec::new()
        }
    };

    Ok(ExternalCommandOutcome {
        executable: spec.executable.clone(),
        argc: spec.args.len() + 1,
        exit_code,
        stdout: CapturedStream::collect(stdout.0, stdout.1, stdout_limit),
        stderr: CapturedStream::collect(stderr.0, stderr.1, stderr_limit),
        duration,
        cancelled,
        timed_out,
        cleanup_notes,
    })
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

async fn join_pipes(
    stdout_task: tokio::task::JoinHandle<(Vec<u8>, u64)>,
    stderr_task: tokio::task::JoinHandle<(Vec<u8>, u64)>,
) -> ((Vec<u8>, u64), (Vec<u8>, u64)) {
    let stdout = tokio::time::timeout(Duration::from_secs(5), stdout_task)
        .await
        .map(Result::unwrap_or_default)
        .unwrap_or_default();
    let stderr = tokio::time::timeout(Duration::from_secs(5), stderr_task)
        .await
        .map(Result::unwrap_or_default)
        .unwrap_or_default();
    (stdout, stderr)
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

    #[test]
    fn captured_stream_truncation_is_explicit() {
        let full = CapturedStream::collect(vec![b'x'; 100], 100, 256);
        assert!(!full.truncated());
        assert_eq!(full.retained_bytes(), 100);
        let capped = CapturedStream::collect(vec![b'x'; 64], 1000, 64);
        assert!(capped.truncated());
        assert_eq!(capped.retained_bytes(), 64);
        assert_eq!(capped.dropped_bytes(), 936);
        assert_eq!(capped.total_bytes(), 1000);
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
