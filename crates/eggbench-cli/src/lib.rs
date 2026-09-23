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

pub use envelope::{
    CliEnvelope, CliOutput, CliResult, CliWarnings, ExitCode, PresentedCommandResult,
};
pub use error::{CliError, CliFailure};
pub use workload_registry::{
    BuiltinWorkloadExecutor, DriverInventoryEntry, NoProductionAdapter, QualificationRuntime,
    WorkloadDescriptor, WorkloadRegistry, WorkloadRuntime,
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

/// Execute a parsed CLI command, producing a [`PresentedCommandResult`]
/// pairing the machine [`CliEnvelope`] with its process-exit [`ExitCode`].
///
/// The envelope remains the machine compatibility surface; the exit code is
/// process metadata. Presentation must use the attached code and never guess
/// it from `ok` alone. `CliError` failures are converted with the same
/// mapping so direct error conversion yields the identical structure.
pub async fn execute(command: Command, options: CommandOptions) -> PresentedCommandResult {
    let label = command_label(&command);
    let outcome: Result<PresentedCommandResult, CliError> = match command {
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
    };
    match outcome {
        Ok(presented) => presented,
        Err(error) => {
            let failure = error.into_failure();
            PresentedCommandResult::failure(label, &failure)
        }
    }
}

fn command_label(command: &Command) -> &'static str {
    match command {
        Command::Validate { .. } => "validate",
        Command::Doctor { .. } => "doctor",
        Command::Run { .. } => "run",
        Command::Inspect { .. } => "inspect",
    }
}

/// Qualification/test seam: `doctor` with an explicit driver inventory.
///
/// Production uses [`execute`]; qualification tests inject the fake inventory
/// explicitly. No public production flag selects the fake path.
pub fn commands_doctor_run_with_registry(
    plan: &std::path::Path,
    input_format: Option<InputFormat>,
    inventory: &[DriverInventoryEntry],
) -> Result<PresentedCommandResult, CliError> {
    commands::doctor::run_with_registry(plan, input_format, inventory)
}

/// Qualification/test seam: `run` with an explicitly injected fake workload.
///
/// Not used by the production binary. The `signal` future deterministically
/// triggers M002 cancellation in tests; production drivers would pass a
/// Ctrl-C listener instead.
pub async fn commands_run_with_qualification(
    plan: &std::path::Path,
    input_format: Option<InputFormat>,
    bundle: &std::path::Path,
    options: CommandOptions,
    fake: eggbench_runner::test_support::FakeWorkload,
    signal: impl std::future::Future<Output = ()> + Send + 'static,
) -> Result<PresentedCommandResult, CliError> {
    commands::r#run::run_with_qualification(plan, input_format, bundle, options, fake, signal).await
}

/// Test seam for the SIGINT forwarding helper.
pub async fn commands_run_forward_signal(
    cancel: tokio_util::sync::CancellationToken,
    signal: impl std::future::Future<Output = ()>,
) {
    commands::r#run::forward_signal(cancel, signal).await;
}
