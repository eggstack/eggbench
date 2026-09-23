//! `eggbench` CLI entry point and presentation logic.
//!
//! The CLI is a thin presentation adapter over `eggbench-core` and
//! `eggbench-runner`. It does not duplicate orchestration: every command
//! either calls into core contracts (parse/validate/resolve/inspect) or
//! delegates to the runner's bundle-preparation and execution seams.
//!
//! Machine output is a single JSON envelope on stdout when `--json` is set;
//! human progress and diagnostics go to stderr. Exit codes follow the
//! documented compact stable mapping.

#![forbid(unsafe_code)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::needless_pass_by_value)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]

mod commands;
mod envelope;
mod error;
mod plan_input;
mod workload_registry;

pub use envelope::{CliEnvelope, CliOutput, CliResult, CliWarnings, ExitCode};
pub use error::{CliError, CliFailure};
pub use workload_registry::{
    BuiltinWorkloadExecutor, DriverInventoryEntry, NoProductionAdapter, WorkloadDescriptor,
    WorkloadRegistry,
};

use std::path::PathBuf;

/// Top-level CLI invocation parsed by `clap`.
#[derive(Debug, Clone)]
pub enum Command {
    /// Parse and semantically validate an experiment plan.
    Validate {
        /// Plan input path or `-` for stdin.
        plan: PathBuf,
        /// Optional explicit input format (`toml` or `json`).
        input_format: Option<InputFormat>,
    },
    /// Run validate plus driver/environment/capability checks without starting processes.
    Doctor {
        /// Plan input path or `-` for stdin.
        plan: PathBuf,
        /// Optional explicit input format (`toml` or `json`).
        input_format: Option<InputFormat>,
    },
    /// Validate, resolve, prepare, and execute the experiment.
    Run {
        /// Plan input path or `-` for stdin.
        plan: PathBuf,
        /// Optional explicit input format (`toml` or `json`).
        input_format: Option<InputFormat>,
        /// Destination `.eggb` bundle path.
        bundle: PathBuf,
    },
    /// Open, verify, and summarize a finalized bundle.
    Inspect {
        /// Bundle path.
        bundle: PathBuf,
        /// Emit normalized manifest JSON to stdout.
        emit_manifest_json: bool,
    },
}

/// Explicit input format override for stdin or non-standard extensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputFormat {
    /// Parse as TOML.
    Toml,
    /// Parse as JSON.
    Json,
}

/// Shared command options.
#[derive(Debug, Clone, Copy)]
pub struct CommandOptions {
    /// Emit machine JSON envelope on stdout.
    pub json: bool,
    /// Suppress human diagnostics on stderr.
    pub quiet: bool,
}

impl CommandOptions {
    /// Defaults: human output to stderr, JSON not emitted.
    #[must_use]
    pub const fn human() -> Self {
        Self {
            json: false,
            quiet: false,
        }
    }
}

/// Execute a parsed CLI command, producing a [`CliEnvelope`] suitable for
/// presentation by the binary or by integration tests.
///
/// # Errors
/// Returns [`CliError`] when command execution fails.
pub async fn execute(command: Command, options: CommandOptions) -> Result<CliEnvelope, CliError> {
    match command {
        Command::Validate { plan, input_format } => commands::validate::run(&plan, input_format),
        Command::Doctor { plan, input_format } => {
            commands::doctor::run(&plan, input_format, options)
        }
        Command::Run {
            plan,
            input_format,
            bundle,
        } => commands::r#run::run(&plan, input_format, &bundle, options).await,
        Command::Inspect {
            bundle,
            emit_manifest_json,
        } => commands::inspect::run(&bundle, emit_manifest_json),
    }
}
