//! Thin CLI binary: parses args, dispatches commands, writes JSON to stdout
//! when requested, and human progress/diagnostics to stderr.
//!
//! Production `eggbench` registers no workload adapter: `run` fails before
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
    },
    /// Open, verify, and summarize a finalized bundle.
    Inspect {
        /// Bundle path.
        bundle: PathBuf,
        /// Emit normalized manifest JSON to stdout.
        #[arg(long)]
        manifest_json: bool,
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
    let command = build_command(cli.command);

    let presented = execute(command, options).await;
    present(&presented, options)
}

fn build_command(command: CliCommand) -> Command {
    match command {
        CliCommand::Validate { plan, input_format } => Command::Validate {
            plan,
            input_format: input_format.map(Into::into),
        },
        CliCommand::Doctor { plan, input_format } => Command::Doctor {
            plan,
            input_format: input_format.map(Into::into),
        },
        CliCommand::Run {
            plan,
            input_format,
            bundle,
        } => Command::Run {
            plan,
            input_format: input_format.map(Into::into),
            bundle,
        },
        CliCommand::Inspect {
            bundle,
            manifest_json,
        } => Command::Inspect {
            bundle,
            emit_manifest_json: manifest_json,
        },
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
