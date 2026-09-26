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
    /// Validate and expand a security qualification profile.
    Qualify {
        #[command(subcommand)]
        command: QualifyCommand,
    },
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
        /// Paired comparison of one paired bundle's arms under policy v2.
        #[arg(long)]
        paired: bool,
        /// Write the versioned comparison receipt JSON to this file.
        #[arg(long)]
        output: Option<PathBuf>,
        /// Explicit deterministic seed (default derives from bundle digests).
        #[arg(long)]
        seed: Option<u64>,
    },
}

#[derive(Subcommand, Debug)]
enum QualifyCommand {
    /// Validate the profile and its referenced corpus.
    Validate { profile: PathBuf },
    /// Resolve and emit the deterministic profile expansion.
    Expand { profile: PathBuf },
    /// Execute the profile serially and publish a qualification receipt.
    Run {
        profile: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Verify and summarize an immutable qualification receipt directory.
    Inspect { qualification_receipt: PathBuf },
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
    let command = match cli.command {
        CliCommand::Qualify { command } => return qualify(&command, options).await,
        other => match build_command(other) {
            Ok(command) => command,
            Err(detail) => {
                eprintln!("eggbench: compare failed [usage] {detail}");
                return StdExitCode::from(2);
            }
        },
    };

    let presented = execute(command, options).await;
    present(&presented, options)
}

async fn qualify(command: &QualifyCommand, options: CommandOptions) -> StdExitCode {
    if let QualifyCommand::Run { profile, output } = command {
        return qualify_run(profile, output, options).await;
    }
    if let QualifyCommand::Inspect {
        qualification_receipt,
    } = command
    {
        return qualify_inspect(qualification_receipt, options.json);
    }
    let json = options.json;
    let result = (|| -> Result<serde_json::Value, String> {
        let (QualifyCommand::Validate {
            profile: profile_path,
        }
        | QualifyCommand::Expand {
            profile: profile_path,
        }) = &command
        else {
            unreachable!()
        };
        let absolute = std::fs::canonicalize(profile_path)
            .map_err(|e| format!("profile is inaccessible: {e}"))?;
        let workspace = absolute.parent().ok_or("profile has no parent directory")?;
        let relative = absolute
            .file_name()
            .ok_or("profile has no file name")?
            .to_string_lossy();
        let expansion = eggbench_core::expand_qualification_profile(workspace, &relative)
            .map_err(|e| e.to_string())?;
        if matches!(command, QualifyCommand::Validate { .. }) {
            Ok(
                serde_json::json!({"ok":true,"command":"qualify_validate","profile_id":expansion.profile_id,"scenario_count":expansion.scenarios.len()}),
            )
        } else {
            serde_json::to_value(expansion).map_err(|e| e.to_string())
        }
    })();
    match result {
        Ok(value) => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".into())
                );
            } else {
                println!("{value}");
            }
            StdExitCode::SUCCESS
        }
        Err(error) => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({"ok":false,"error":{"category":"qualification_input","message":error}})
                );
            } else {
                eprintln!("eggbench qualify failed: {error}");
            }
            StdExitCode::from(2)
        }
    }
}

#[allow(clippy::too_many_lines)]
async fn qualify_run(
    profile: &std::path::Path,
    output: &std::path::Path,
    options: CommandOptions,
) -> StdExitCode {
    use eggbench_core::{
        QUALIFICATION_RECEIPT_POLICY_V1, QualificationScenarioRecordV1 as Record,
        QualificationScenarioStatus as State, SecurityQualificationReceiptV1,
    };
    let result: Result<(SecurityQualificationReceiptV1, PathBuf), String> = async {
        if output.exists() {
            return Err("qualification output already exists".into());
        }
        let profile_abs =
            std::fs::canonicalize(profile).map_err(|e| format!("profile inaccessible: {e}"))?;
        let workspace = profile_abs.parent().ok_or("profile has no parent")?;
        let rel = profile_abs
            .file_name()
            .ok_or("profile has no filename")?
            .to_string_lossy();
        let expansion = eggbench_core::expand_qualification_profile(workspace, &rel)
            .map_err(|e| e.to_string())?;
        let profile_bytes = std::fs::read(&profile_abs).map_err(|e| e.to_string())?;
        eggbench_core::SecurityQualificationProfileV1::from_json(
            std::str::from_utf8(&profile_bytes).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        if expansion.scenarios.is_empty() || expansion.scenarios.len() > 32 {
            return Err("scenario count must be between 1 and 32".into());
        }
        let parent = output
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(std::path::Path::new("."));
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let stage = parent.join(format!(
            ".qualification.staging-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::create_dir(&stage).map_err(|e| e.to_string())?;
        let work = async {
            let expansion_bytes =
                serde_json::to_vec_pretty(&expansion).map_err(|e| e.to_string())?;
            std::fs::write(stage.join("expansion.json"), &expansion_bytes)
                .map_err(|e| e.to_string())?;
            std::fs::create_dir(stage.join("scenarios")).map_err(|e| e.to_string())?;
            let mut records = Vec::new();
            let mut stopped = false;
            for scenario in &expansion.scenarios {
                if stopped {
                    records.push(Record {
                        id: scenario.id.clone(),
                        source_plan_sha256: scenario.source_plan_sha256.clone(),
                        status: State::NotRun,
                        candidate_bundle_path: None,
                        candidate_bundle_identity: None,
                        baseline_bundle_identity: scenario.baseline_identity.clone(),
                        comparison_receipt_path: None,
                        comparison_receipt_sha256: None,
                        performance_verdict: None,
                        correctness_verdict: None,
                        combined_verdict: None,
                        reason: Some(
                            "earlier scenario could not produce trustworthy evidence".into(),
                        ),
                    });
                    continue;
                }
                let plan = workspace.join(&scenario.source_plan_path_context);
                let bundle_rel = format!("scenarios/{}.eggb", scenario.id);
                let bundle = stage.join(&bundle_rel);
                let run = execute(
                    Command::Run {
                        plan,
                        input_format: None,
                        bundle: bundle.clone(),
                        workload_driver: None,
                    },
                    CommandOptions {
                        json: true,
                        quiet: true,
                    },
                )
                .await;
                let published = matches!(
                    run.envelope.result,
                    Some(eggbench_cli::CliOutput::Run {
                        bundle_published: true,
                        ..
                    })
                );
                if !published || !run.envelope.ok {
                    let cancelled = matches!(
                        &run.envelope.result,
                        Some(eggbench_cli::CliOutput::Run { execution_status, .. })
                            if execution_status.eq_ignore_ascii_case("cancelled")
                    );
                    records.push(Record {
                        id: scenario.id.clone(),
                        source_plan_sha256: scenario.source_plan_sha256.clone(),
                        status: if cancelled {
                            State::Cancelled
                        } else {
                            State::Invalid
                        },
                        candidate_bundle_path: published.then_some(bundle_rel),
                        candidate_bundle_identity: None,
                        baseline_bundle_identity: scenario.baseline_identity.clone(),
                        comparison_receipt_path: None,
                        comparison_receipt_sha256: None,
                        performance_verdict: None,
                        correctness_verdict: None,
                        combined_verdict: None,
                        reason: Some(
                            run.envelope
                                .error
                                .map_or("run did not produce a completed bundle".into(), |e| {
                                    e.detail
                                }),
                        ),
                    });
                    stopped = true;
                    continue;
                }
                let cmp_rel = format!("scenarios/{}.comparison.json", scenario.id);
                let cmp_path = stage.join(&cmp_rel);
                let baseline = scenario
                    .baseline_path_context
                    .as_ref()
                    .map(|p| workspace.join(p));
                let comparison = execute(
                    Command::Compare {
                        baseline,
                        candidate: bundle.clone(),
                        alias: None,
                        absolute_only: scenario.baseline_identity.is_none(),
                        paired: false,
                        output: Some(cmp_path.clone()),
                        seed: None,
                    },
                    CommandOptions {
                        json: true,
                        quiet: true,
                    },
                )
                .await;
                let Some(eggbench_cli::CliOutput::Compare { receipt, .. }) =
                    comparison.envelope.result
                else {
                    records.push(Record {
                        id: scenario.id.clone(),
                        source_plan_sha256: scenario.source_plan_sha256.clone(),
                        status: State::Invalid,
                        candidate_bundle_path: Some(bundle_rel),
                        candidate_bundle_identity: None,
                        baseline_bundle_identity: scenario.baseline_identity.clone(),
                        comparison_receipt_path: None,
                        comparison_receipt_sha256: None,
                        performance_verdict: None,
                        correctness_verdict: None,
                        combined_verdict: None,
                        reason: Some("comparison failed to produce a typed receipt".into()),
                    });
                    stopped = true;
                    continue;
                };
                if receipt.baseline_identity != scenario.baseline_identity {
                    return Err(format!(
                        "scenario {} baseline differs from frozen expansion",
                        scenario.id
                    ));
                }
                let receipt_bytes = std::fs::read(&cmp_path).map_err(|e| e.to_string())?;
                let correctness = receipt.correctness.as_ref().map(|c| c.aggregate_verdict);
                let perf = receipt.performance_verdict;
                let has_verdict = receipt.aggregate_verdict.is_some();
                records.push(Record {
                    id: scenario.id.clone(),
                    source_plan_sha256: scenario.source_plan_sha256.clone(),
                    status: if has_verdict {
                        State::Completed
                    } else {
                        State::Invalid
                    },
                    candidate_bundle_path: Some(bundle_rel),
                    candidate_bundle_identity: Some(receipt.candidate_identity.clone()),
                    baseline_bundle_identity: receipt.baseline_identity.clone(),
                    comparison_receipt_path: Some(cmp_rel),
                    comparison_receipt_sha256: Some(eggbench_core::qualification_sha256(
                        &receipt_bytes,
                    )),
                    performance_verdict: perf,
                    correctness_verdict: correctness,
                    combined_verdict: receipt.aggregate_verdict,
                    reason: (!has_verdict).then(|| "comparison produced no gated verdict".into()),
                });
            }
            let complete = records.iter().all(|r| r.status == State::Completed);
            let aggregate = eggbench_core::aggregate_qualification_scenarios(&records);
            let receipt = SecurityQualificationReceiptV1 {
                schema_version: 1,
                policy_id: QUALIFICATION_RECEIPT_POLICY_V1.into(),
                created_by_version: env!("CARGO_PKG_VERSION").into(),
                profile_id: expansion.profile_id.clone(),
                profile_sha256: expansion.profile_sha256.clone(),
                expansion_sha256: eggbench_core::qualification_sha256(&expansion_bytes),
                corpus_identity: expansion.corpus_identity.clone(),
                target_config_identity: expansion.target_config_identity.clone(),
                scenarios: records,
                aggregate_verdict: aggregate,
                execution_complete: complete,
                warnings: vec![],
            };
            let receipt_path = stage.join("qualification-receipt.json");
            std::fs::write(
                &receipt_path,
                format!(
                    "{}\n",
                    serde_json::to_string_pretty(&receipt).map_err(|e| e.to_string())?
                ),
            )
            .map_err(|e| e.to_string())?;
            Ok(receipt)
        }
        .await;
        let receipt = work?;
        std::fs::rename(&stage, output).map_err(|e| format!("atomic publication failed: {e}"))?;
        Ok((receipt, output.to_path_buf()))
    }
    .await;
    match result {
        Ok((receipt, path)) => {
            if options.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&receipt).unwrap_or_default()
                );
            } else {
                println!("Scenario                 Performance   Correctness   Combined");
                for scenario in &receipt.scenarios {
                    println!(
                        "{:<25} {:<13} {:<13} {:?}",
                        scenario.id,
                        scenario
                            .performance_verdict
                            .map_or("n/a".into(), |v| format!("{v:?}").to_lowercase()),
                        scenario
                            .correctness_verdict
                            .map_or("n/a".into(), |v| format!("{v:?}").to_lowercase()),
                        scenario.combined_verdict.map_or_else(
                            || format!("{:?}", scenario.status).to_lowercase(),
                            |v| format!("{v:?}").to_lowercase()
                        )
                    );
                }
                println!(
                    "\nQualification: {:?} ({})",
                    receipt.aggregate_verdict,
                    path.display()
                );
            }
            StdExitCode::from(match receipt.aggregate_verdict {
                eggbench_core::AggregateVerdict::Pass => 0,
                eggbench_core::AggregateVerdict::Fail => 6,
                eggbench_core::AggregateVerdict::Inconclusive => 7,
                eggbench_core::AggregateVerdict::Invalid => 8,
            })
        }
        Err(e) => {
            if options.json {
                println!("{{\"ok\":false,\"error\":{}}}", serde_json::json!(e));
            } else {
                eprintln!("eggbench qualify failed: {e}");
            }
            StdExitCode::from(8)
        }
    }
}

#[allow(clippy::too_many_lines)]
fn qualify_inspect(path: &std::path::Path, json: bool) -> StdExitCode {
    let result = (|| -> Result<eggbench_core::SecurityQualificationReceiptV1, String> {
        let raw = std::fs::read(path).map_err(|e| e.to_string())?;
        let receipt: eggbench_core::SecurityQualificationReceiptV1 =
            serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
        if receipt.schema_version != 1
            || receipt.policy_id != eggbench_core::QUALIFICATION_RECEIPT_POLICY_V1
        {
            return Err("unsupported qualification receipt".into());
        }
        let root = path.parent().ok_or("receipt has no parent")?;
        let expansion_raw =
            std::fs::read(root.join("expansion.json")).map_err(|e| e.to_string())?;
        if eggbench_core::qualification_sha256(&expansion_raw) != receipt.expansion_sha256 {
            return Err("expansion digest mismatch".into());
        }
        let expansion: eggbench_core::QualificationExpansionV1 =
            serde_json::from_slice(&expansion_raw).map_err(|e| e.to_string())?;
        if expansion.profile_id != receipt.profile_id
            || expansion.profile_sha256 != receipt.profile_sha256
            || expansion.corpus_identity != receipt.corpus_identity
            || expansion.target_config_identity != receipt.target_config_identity
        {
            return Err("receipt input identity differs from expansion".into());
        }
        if expansion.scenarios.len() != receipt.scenarios.len()
            || expansion
                .scenarios
                .iter()
                .zip(&receipt.scenarios)
                .any(|(a, b)| a.id != b.id || a.source_plan_sha256 != b.source_plan_sha256)
        {
            return Err("scenario order or source identity differs from expansion".into());
        }
        for (expanded, record) in expansion.scenarios.iter().zip(&receipt.scenarios) {
            if expanded.baseline_identity != record.baseline_bundle_identity {
                return Err("baseline identity differs from frozen profile reference".into());
            }
            if record.status != eggbench_core::QualificationScenarioStatus::Completed {
                continue;
            }
            let bp = root.join(
                record
                    .candidate_bundle_path
                    .as_ref()
                    .ok_or("completed scenario missing bundle path")?,
            );
            let bundle = eggbench_core::load_candidate_bundle(&bp).map_err(|e| e.to_string())?;
            if Some(&bundle.identity) != record.candidate_bundle_identity.as_ref() {
                return Err("candidate bundle identity mismatch".into());
            }
            let cp = root.join(
                record
                    .comparison_receipt_path
                    .as_ref()
                    .ok_or("completed scenario missing comparison path")?,
            );
            let bytes = std::fs::read(&cp).map_err(|e| e.to_string())?;
            if Some(eggbench_core::qualification_sha256(&bytes)) != record.comparison_receipt_sha256
            {
                return Err("comparison receipt digest mismatch".into());
            }
            let comparison =
                eggbench_core::parse_comparison_receipt(&bytes).map_err(|e| e.to_string())?;
            if comparison.candidate_identity != bundle.identity
                || comparison.baseline_identity != record.baseline_bundle_identity
                || comparison.performance_verdict != record.performance_verdict
                || comparison.correctness.as_ref().map(|c| c.aggregate_verdict)
                    != record.correctness_verdict
                || comparison.aggregate_verdict != record.combined_verdict
            {
                return Err("comparison identity or typed verdict mismatch".into());
            }
        }
        let complete = receipt
            .scenarios
            .iter()
            .all(|r| r.status == eggbench_core::QualificationScenarioStatus::Completed);
        if receipt.execution_complete != complete {
            return Err("execution_complete disagrees with scenario states".into());
        }
        let aggregate = eggbench_core::aggregate_qualification_scenarios(&receipt.scenarios);
        if aggregate != receipt.aggregate_verdict {
            return Err("aggregate verdict disagrees with typed scenario verdicts".into());
        }
        Ok(receipt)
    })();
    match result {
        Ok(value) => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&value).unwrap_or_default()
                );
            } else {
                println!("Qualification: {:?}", value.aggregate_verdict);
            }
            StdExitCode::SUCCESS
        }
        Err(error) => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({"ok":false,"error":{"category":"qualification_receipt","message":error}})
                );
            } else {
                eprintln!("eggbench qualify inspect failed: {error}");
            }
            StdExitCode::from(8)
        }
    }
}

fn build_command(command: CliCommand) -> Result<Command, String> {
    match command {
        CliCommand::Qualify { .. } => {
            unreachable!("qualification dispatch precedes standard command conversion")
        }
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
            paired,
            output,
            seed,
        } => {
            // One positional pair covers `<baseline> <candidate>`; single
            // positional covers `--alias <file> <candidate>`,
            // `--absolute-only <candidate>`, and `--paired <bundle>`.
            let (baseline_path, candidate_path) = match (baseline, candidate) {
                (Some(left), Some(right)) => (Some(left), right),
                (Some(only), None) if absolute_only || alias.is_some() || paired => (None, only),
                _ => {
                    return Err(
                        "provide <baseline> <candidate>, --alias <file> <candidate>, --absolute-only <candidate>, or --paired <bundle>"
                            .to_owned(),
                    );
                }
            };
            Ok(Command::Compare {
                baseline: baseline_path,
                candidate: candidate_path,
                alias,
                absolute_only,
                paired,
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
