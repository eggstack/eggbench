//! `eggbench validate <plan>` command.

use crate::InputFormat;
use crate::envelope::{CliEnvelope, CliOutput};
use crate::error::CliError;
use crate::plan_input::load_plan;
use std::path::Path;

/// Parse and validate an experiment plan.
///
/// # Errors
/// Returns [`CliError`] when input reading or plan validation fails.
pub fn run(plan: &Path, input_format: Option<InputFormat>) -> Result<CliEnvelope, CliError> {
    let input = load_plan(plan, input_format)?;
    let schema_version = input.plan.schema_version.0;
    let experiment = input.plan.experiment.as_str().to_owned();
    Ok(CliEnvelope::ok(
        "validate",
        CliOutput::Validate {
            experiment: Some(experiment),
            schema_version,
        },
    ))
}
