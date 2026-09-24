//! Thin CLI binary: parses args, dispatches commands, writes JSON to stdout
//! when requested, and human progress/diagnostics to stderr.
//!
//! Production `eggbench` resolves against the production catalog: the
//! external-process oracles always register while native adapters join per
//! feature. Without a unique default workload driver an explicit
//! `--workload-driver` selection is required; unresolvable runs fail before
//! managed startup with a stable capability category. Exit status exactly
//! matches the documented CLI result class in both JSON and human modes.

use clap::{Parser, Subcommand};
use eggbench_cli::{
    CliFailure, Command, CommandOptions, ExitCode, InputFormat, PresentedCommandResult, execute,
};
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode as StdExitCode;

#[derive(Parser, Debug)]
#[command(name = "eggbench", version, about = "Eggbench local-runner CLI")]
struct Cli {
    /// Emit a machine-readable JSON envelope on stdout.
    #[arg(long, global = true)]
    json: bool,
    /// Suppress human progress/diagnostic output on stderr.
    #[arg(long, global = true)]
    quiet: bool,
    #[command(subcommand)]
    command: CliCommand,
}

#[derive(Subcommand, Debug)]
enum CliCommand {
    /// Parse and semantically validate an experiment plan.
    Validate {
        /// Plan input path or `-` for stdin.
        plan: PathBuf,
        /// Optional explicit input format (`toml` or `json`).
        #[arg(long, value_enum)]
        input_format: Option<InputFormatArg>,
    },
    /// Run validate plus driver/environment/capability checks without starting processes.
    Doctor {
        /// Plan input path or `-` for stdin.
        plan: PathBuf,
        /// Optional explicit input format (`toml` or `json`).
        #[arg(long, value_enum)]
        input_format: Option<InputFormatArg>,
        /// Optional explicit workload driver name (default selection otherwise).
        #[arg(long)]
        workload_driver: Option<String>,
    },
    /// Validate, resolve, prepare, and execute the experiment.
    Run {
        /// Plan input path or `-` for stdin.
        plan: PathBuf,
        /// Optional explicit input format (`toml` or `json`).
        #[arg(long, value_enum)]
        input_format: Option<InputFormatArg>,
        /// Destination `.eggb` bundle path.
        bundle: PathBuf,
        /// Optional explicit workload driver name (default selection otherwise).
        #[arg(long)]
        workload_driver: Option<String>,
    },
    /// Open, verify, and summarize a finalized bundle.
    Inspect {
        /// Bundle path.
        bundle: PathBuf,
        /// Emit normalized manifest JSON to stdout.
        #[arg(long)]
        manifest_json: bool,
    },
    /// Compare two immutable bundles without modifying them.
    Compare {
        /// Baseline `.eggb` bundle path.
        baseline: Option<PathBuf>,
        /// Candidate `.eggb` bundle path.
        candidate: Option<PathBuf>,
        /// Baseline alias file (`*.eggbaseline.json`).
        #[arg(long)]
        alias: Option<PathBuf>,
        /// Candidate-only absolute-gate comparison without a baseline.
        #[arg(long)]
        absolute_only: bool,
        /// Write the versioned comparison receipt JSON to this file.
        #[arg(long)]
        output: Option<PathBuf>,
        /// Explicit deterministic seed (default derives from bundle digests).
        #[arg(long)]
        seed: Option<u64>,
    },
}

/// CLI-side wrapper around [`InputFormat`].
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum InputFormatArg {
    Toml,
    Json,
}

impl From<InputFormatArg> for InputFormat {
    fn from(value: InputFormatArg) -> Self {
        match value {
            InputFormatArg::Toml => Self::Toml,
            InputFormatArg::Json => Self::Json,
        }
    }
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> StdExitCode {
    let cli = Cli::parse();
    let options = CommandOptions {
        json: cli.json,
        quiet: cli.quiet,
    };
    let command = match build_command(cli.command) {
        Ok(command) => command,
        Err(detail) => {
            eprintln!("eggbench: compare failed [usage] {detail}");
            return StdExitCode::from(2);
        }
    };

    let presented = execute(command, options).await;
    present(&presented, options)
}

fn build_command(command: CliCommand) -> Result<Command, String> {
    match command {
        CliCommand::Validate { plan, input_format } => Ok(Command::Validate {
            plan,
            input_format: input_format.map(Into::into),
        }),
        CliCommand::Doctor {
            plan,
            input_format,
            workload_driver,
        } => Ok(Command::Doctor {
            plan,
            input_format: input_format.map(Into::into),
            workload_driver,
        }),
        CliCommand::Run {
            plan,
            input_format,
            bundle,
            workload_driver,
        } => Ok(Command::Run {
            plan,
            input_format: input_format.map(Into::into),
            bundle,
            workload_driver,
        }),
        CliCommand::Inspect {
            bundle,
            manifest_json,
        } => Ok(Command::Inspect {
            bundle,
            emit_manifest_json: manifest_json,
        }),
        CliCommand::Compare {
            baseline,
            candidate,
            alias,
            absolute_only,
            output,
            seed,
        } => {
            // One positional pair covers `<baseline> <candidate>`; single
            // positional covers `--alias <file> <candidate>` and
            // `--absolute-only <candidate>`.
            let (baseline_path, candidate_path) = match (baseline, candidate) {
                (Some(left), Some(right)) => (Some(left), right),
                (Some(only), None) if absolute_only || alias.is_some() => (None, only),
                _ => {
                    return Err(
                        "provide <baseline> <candidate>, --alias <file> <candidate>, or --absolute-only <candidate>"
                            .to_owned(),
                    );
                }
            };
            Ok(Command::Compare {
                baseline: baseline_path,
                candidate: candidate_path,
                alias,
                absolute_only,
                output,
                seed,
            })
        }
    }
}

/// Write command output and return the process status.
///
/// The numeric status is taken from [`PresentedCommandResult::exit_code`] in
/// both JSON and human modes so the same outcome yields the same code
/// regardless of presentation. Only `main` converts this into the final
/// process status, which keeps binary behavior testable.
fn present(presented: &PresentedCommandResult, options: CommandOptions) -> StdExitCode {
    let envelope = &presented.envelope;
    let code = u8::try_from(presented.exit_code.code()).unwrap_or(1);
    if options.json {
        match envelope.to_pretty_json() {
            Ok(body) => {
                println!("{body}");
            }
            Err(error) => {
                let failure = CliFailure::new(
                    "internal",
                    format!("could not serialize JSON envelope: {error}"),
                    ExitCode::Internal,
                );
                let _ = writeln!(std::io::stderr(), "eggbench: {}", failure.detail);
                return StdExitCode::from(1);
            }
        }
        return StdExitCode::from(code);
    }

    if envelope.ok {
        if !options.quiet {
            let _ = writeln!(std::io::stderr(), "eggbench: {} ok", envelope.command);
        }
    } else if let Some(error) = &envelope.error {
        let _ = writeln!(
            std::io::stderr(),
            "eggbench: {} failed [{}] {}",
            envelope.command,
            error.category,
            error.detail
        );
    } else {
        let _ = writeln!(
            std::io::stderr(),
            "eggbench: {} failed [internal] missing error payload",
            envelope.command
        );
        return StdExitCode::from(1);
    }
    StdExitCode::from(code)
}
