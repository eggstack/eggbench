//! Thin CLI binary: parses args, dispatches commands, writes JSON to stdout
//! when requested, and human progress/diagnostics to stderr.

use clap::{Parser, Subcommand};
use eggbench_cli::{
    CliEnvelope, CliFailure, Command, CommandOptions, ExitCode, InputFormat, execute,
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
    let cli_label = command_label(&cli.command);
    let options = CommandOptions {
        json: cli.json,
        quiet: cli.quiet,
    };
    let command = build_command(cli.command);

    let envelope = match execute(command, options).await {
        Ok(envelope) => envelope,
        Err(failure) => return exit_with_failure(&cli_label, &failure.into_failure(), options),
    };
    present(&envelope, options);
    StdExitCode::SUCCESS
}

fn command_label(command: &CliCommand) -> String {
    match command {
        CliCommand::Validate { .. } => "validate".to_owned(),
        CliCommand::Doctor { .. } => "doctor".to_owned(),
        CliCommand::Run { .. } => "run".to_owned(),
        CliCommand::Inspect { .. } => "inspect".to_owned(),
    }
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

fn present(envelope: &CliEnvelope, options: CommandOptions) {
    if options.json {
        let body = match envelope.to_pretty_json() {
            Ok(json) => json,
            Err(error) => {
                let failure = CliFailure::new(
                    "internal",
                    format!("could not serialize JSON envelope: {error}"),
                    ExitCode::Internal,
                );
                let _ = writeln!(std::io::stderr(), "eggbench: {}", failure.detail);
                std::process::exit(failure.exit_code.code());
            }
        };
        println!("{body}");
        return;
    }

    if envelope.ok {
        if !options.quiet {
            let _ = writeln!(std::io::stderr(), "eggbench: {} ok", envelope.command);
        }
        std::process::exit(ExitCode::Success.code());
    } else if let Some(error) = &envelope.error {
        let _ = writeln!(
            std::io::stderr(),
            "eggbench: {} failed [{}] {}",
            envelope.command,
            error.category,
            error.detail
        );
        std::process::exit(ExitCode::CapabilityPreflight.code());
    } else {
        std::process::exit(ExitCode::Internal.code());
    }
}

fn exit_with_failure(command: &str, failure: &CliFailure, options: CommandOptions) -> StdExitCode {
    let envelope = CliEnvelope::fail(command, failure);
    present(&envelope, options);
    let code = u8::try_from(failure.exit_code.code()).unwrap_or(1);
    StdExitCode::from(code)
}
