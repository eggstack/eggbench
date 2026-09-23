//! Plan input parsing shared across CLI commands.
//!
//! Supports stdin (`-`), TOML (`.toml`), and JSON (`.json`) inputs. Unknown
//! extensions are rejected when no explicit format override is given.

use crate::InputFormat;
use crate::error::CliError;
use eggbench_core::ExperimentPlan;
use std::fs;
use std::io::Read;
use std::path::Path;

/// One parsed plan input with raw bytes preserved for evidence staging.
#[derive(Debug, Clone)]
pub struct PlanInput {
    /// Raw input bytes (canonical source for the `plan.json` artifact).
    pub bytes: Vec<u8>,
    /// Detected or explicit format.
    pub format: InputFormat,
    /// Parsed and validated plan.
    pub plan: ExperimentPlan,
}

/// Read, parse, and validate a plan input.
///
/// # Errors
/// Returns [`CliError`] when the file cannot be read, the extension is
/// ambiguous, the bytes cannot be parsed, or semantic validation fails.
pub fn load_plan(path: &Path, override_format: Option<InputFormat>) -> Result<PlanInput, CliError> {
    let bytes = read_plan_bytes(path)?;
    let format = detect_format(path, override_format)?;
    let plan = match format {
        InputFormat::Toml => parse_toml(&bytes),
        InputFormat::Json => parse_json(&bytes),
    }?;
    Ok(PlanInput {
        bytes,
        format,
        plan,
    })
}

fn read_plan_bytes(path: &Path) -> Result<Vec<u8>, CliError> {
    if path == Path::new("-") {
        let mut bytes = Vec::new();
        std::io::stdin()
            .read_to_end(&mut bytes)
            .map_err(|error| CliError::PlanIo(error.to_string()))?;
        Ok(bytes)
    } else {
        fs::read(path).map_err(|error| CliError::PlanIo(error.to_string()))
    }
}

fn detect_format(
    path: &Path,
    override_format: Option<InputFormat>,
) -> Result<InputFormat, CliError> {
    if let Some(format) = override_format {
        return Ok(format);
    }
    if path == Path::new("-") {
        return Err(CliError::PlanFormat(
            "stdin input requires --input-format".to_owned(),
        ));
    }
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("toml") => Ok(InputFormat::Toml),
        Some("json") => Ok(InputFormat::Json),
        Some(other) => Err(CliError::PlanFormat(format!(
            "unrecognized extension '.{other}'; pass --input-format toml|json"
        ))),
        None => Err(CliError::PlanFormat(
            "plan path has no extension; pass --input-format toml|json".to_owned(),
        )),
    }
}

fn parse_json(bytes: &[u8]) -> Result<ExperimentPlan, CliError> {
    ExperimentPlan::from_json(
        std::str::from_utf8(bytes)
            .map_err(|error| CliError::PlanFormat(format!("input is not valid UTF-8: {error}")))?,
    )
    .map_err(CliError::PlanValidation)
}

fn parse_toml(bytes: &[u8]) -> Result<ExperimentPlan, CliError> {
    ExperimentPlan::from_toml(
        std::str::from_utf8(bytes)
            .map_err(|error| CliError::PlanFormat(format!("input is not valid UTF-8: {error}")))?,
    )
    .map_err(CliError::PlanValidation)
}
