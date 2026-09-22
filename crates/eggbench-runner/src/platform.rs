//! Narrow platform adapter for process-tree ownership.
//!
//! Unix implementations place each managed child in its own process group so
//! graceful termination and forced cleanup reach descendants. Windows process
//! trees are reported as an unsupported capability until Job Object semantics
//! are qualified. Only platforms with tested descendant cleanup are
//! advertised as supported.

use std::fmt;

/// Process-tree capability advertised by an adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformSupport {
    /// Descendant cleanup is tested on this platform.
    Supported,
    /// Managed spawn remains disabled until platform-specific qualification passes.
    Unqualified,
    /// Managed spawn must fail with a structured capability error.
    Unsupported,
}

/// Small OS-specific ownership boundary behind the session.
pub trait PlatformAdapter: Send + Sync + fmt::Debug {
    /// Capability advertised by this adapter.
    fn support(&self) -> PlatformSupport;

    /// Human-readable platform label for diagnostics.
    fn label(&self) -> &'static str;

    /// Returns true when the process identifier is currently alive.
    fn is_alive(&self, pid: u32) -> bool;

    /// Request graceful termination of a process group or process.
    ///
    /// # Errors
    /// Returns a redaction-safe message when the signal cannot be delivered.
    fn terminate_group(&self, pid: u32) -> Result<(), String>;

    /// Force-kill a process group or process and its descendants.
    ///
    /// # Errors
    /// Returns a redaction-safe message when cleanup cannot be completed.
    fn kill_group(&self, pid: u32) -> Result<(), String>;
}

/// Unix adapter using one process group per managed child.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnixPlatform;

impl PlatformAdapter for UnixPlatform {
    fn support(&self) -> PlatformSupport {
        if cfg!(target_os = "linux") || cfg!(target_os = "macos") {
            PlatformSupport::Supported
        } else if cfg!(unix) {
            PlatformSupport::Unqualified
        } else {
            PlatformSupport::Unsupported
        }
    }

    fn label(&self) -> &'static str {
        if cfg!(target_os = "linux") {
            "linux"
        } else if cfg!(target_os = "macos") {
            "macos"
        } else {
            "other"
        }
    }

    fn is_alive(&self, pid: u32) -> bool {
        is_process_alive(pid)
    }

    fn terminate_group(&self, pid: u32) -> Result<(), String> {
        #[cfg(unix)]
        {
            signal_process_group(pid, nix::sys::signal::Signal::SIGTERM)
        }
        #[cfg(not(unix))]
        {
            let _ = pid;
            Err("managed descendant cleanup is unsupported on this platform".to_owned())
        }
    }

    fn kill_group(&self, pid: u32) -> Result<(), String> {
        #[cfg(unix)]
        {
            signal_process_group(pid, nix::sys::signal::Signal::SIGKILL)
        }
        #[cfg(not(unix))]
        {
            let _ = pid;
            Err("managed descendant cleanup is unsupported on this platform".to_owned())
        }
    }
}

/// Adapter that always reports managed execution unsupported.
///
/// Used on Windows until Job Object semantics are qualified, and in tests
/// that need a deterministic capability failure.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedPlatform;

impl PlatformAdapter for UnsupportedPlatform {
    fn support(&self) -> PlatformSupport {
        PlatformSupport::Unsupported
    }

    fn label(&self) -> &'static str {
        "unsupported"
    }

    fn is_alive(&self, _pid: u32) -> bool {
        false
    }

    fn terminate_group(&self, _pid: u32) -> Result<(), String> {
        Err("managed descendant cleanup is unsupported on this platform".to_owned())
    }

    fn kill_group(&self, _pid: u32) -> Result<(), String> {
        Err("managed descendant cleanup is unsupported on this platform".to_owned())
    }
}

/// Returns true when the process identifier refers to a live process.
///
/// Signal zero performs error checking without delivering a signal; a
/// permission error still proves the process exists.
#[must_use]
pub fn is_process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let pid = nix::unistd::Pid::from_raw(i32::try_from(pid).unwrap_or(i32::MAX));
        match nix::sys::signal::kill(pid, None) {
            Ok(()) | Err(nix::errno::Errno::EPERM) => true,
            Err(_) => false,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

#[cfg(unix)]
fn signal_process_group(pid: u32, signal: nix::sys::signal::Signal) -> Result<(), String> {
    let pid = nix::unistd::Pid::from_raw(i32::try_from(pid).unwrap_or(i32::MAX));
    match nix::sys::signal::killpg(pid, signal) {
        Ok(()) | Err(nix::errno::Errno::ESRCH) => Ok(()),
        Err(error) => Err(format!("signal {} failed: {error}", signal.as_str())),
    }
}
