//! `eggbench compare` command: offline comparison of immutable bundles.
//!
//! The command never modifies any `.eggb` directory. It emits the
//! standalone versioned comparison receipt as machine JSON (stdout or
//! `--output <comparison.json>`) and a human summary on stderr. Exit codes
//! are additive: 6 comparison fail, 7 comparison inconclusive, 8 comparison
//! invalid; descriptive/no-verdict comparisons exit 0; evidence I/O retains
//! code 5. `--paired` compares the two arms of one paired bundle under
//! policy v2 instead of two bundles under policy v1.

use crate::envelope::{CliOutput, PathBufPayload, PresentedCommandResult};
use crate::error::CliError;
use eggbench_core::{
    BaselineReference, BaselineSide, ComparisonInput, ComparisonOptions, ComparisonReceipt,
    ComparisonRequest, compare, compare_paired, load_baseline_alias, load_baseline_bundle,
    load_candidate_bundle,
};
use std::path::{Path, PathBuf};

/// Compare two bundles, an alias plus a candidate, a candidate alone, or one
/// paired bundle's arms.
///
/// Exactly one mode applies: `baseline` path, `alias` file,
/// `absolute_only` candidate-only comparison, or `paired` single-bundle
/// paired comparison.
pub fn run(
    baseline: Option<&Path>,
    candidate: &Path,
    alias: Option<&Path>,
    absolute_only: bool,
    paired: bool,
    output: Option<&Path>,
    seed: Option<u64>,
) -> Result<PresentedCommandResult, CliError> {
    if paired {
        if absolute_only || baseline.is_some() || alias.is_some() {
            return Err(CliError::Internal(
                "--paired cannot be combined with a baseline, alias, or --absolute-only".to_owned(),
            ));
        }
        return run_paired(candidate, output, seed);
    }
    if absolute_only && (baseline.is_some() || alias.is_some()) {
        return Err(CliError::Internal(
            "--absolute-only cannot be combined with a baseline or alias".to_owned(),
        ));
    }
    if baseline.is_some() && alias.is_some() {
        return Err(CliError::Internal(
            "baseline path and alias file are mutually exclusive".to_owned(),
        ));
    }
    if !absolute_only && baseline.is_none() && alias.is_none() {
        return Err(CliError::Internal(
            "compare requires a baseline bundle, an alias file, or --absolute-only".to_owned(),
        ));
    }

    let candidate_input = load_candidate_bundle(candidate).map_err(map_comparison_error)?;
    let baseline_side: Option<(BaselineReference, ComparisonInput)> = if absolute_only {
        None
    } else if let Some(alias_file) = alias {
        Some(load_baseline_alias(alias_file).map_err(map_comparison_error)?)
    } else {
        let baseline_path = baseline.expect("baseline checked above");
        Some(load_baseline_bundle(baseline_path).map_err(map_comparison_error)?)
    };

    let request = ComparisonRequest {
        candidate: &candidate_input,
        baseline: baseline_side
            .as_ref()
            .map(|(reference, input)| BaselineSide {
                reference: reference.clone(),
                input,
            }),
    };
    let receipt = compare(&request, &ComparisonOptions { seed });

    present_receipt(&receipt, output)
}

/// Compare the baseline and candidate arms of one paired bundle.
fn run_paired(
    bundle: &Path,
    output: Option<&Path>,
    seed: Option<u64>,
) -> Result<PresentedCommandResult, CliError> {
    let input = load_candidate_bundle(bundle).map_err(map_comparison_error)?;
    let receipt = compare_paired(bundle, &input, &ComparisonOptions { seed });
    present_receipt(&receipt, output)
}

/// Emit the receipt payload and map the aggregate verdict to the locked
/// exit-code matrix. Shared by unpaired and paired comparison.
fn present_receipt(
    receipt: &ComparisonReceipt,
    output: Option<&Path>,
) -> Result<PresentedCommandResult, CliError> {
    let receipt_path = match output {
        Some(path) => {
            write_receipt(path, receipt)?;
            Some(
                PathBufPayload::from_path(path)
                    .unwrap_or_else(|| PathBufPayload::from_string(path.display().to_string())),
            )
        }
        None => None,
    };

    let aggregate_label = receipt
        .aggregate_verdict
        .map(|verdict| format!("{verdict:?}").to_lowercase());
    let output_payload = CliOutput::Compare {
        aggregate_verdict: aggregate_label,
        candidate_run_id: receipt.candidate_identity.run_id.to_string(),
        baseline_run_id: receipt
            .baseline_identity
            .as_ref()
            .map(|id| id.run_id.to_string()),
        comparability_match: !receipt.comparability.critical_mismatch,
        receipt_path,
        receipt: Box::new(receipt.clone()),
    };
    Ok(PresentedCommandResult::compare_verdict(
        "compare",
        output_payload,
        receipt.aggregate_verdict,
        comparison_detail(&receipt),
    ))
}

fn comparison_detail(receipt: &ComparisonReceipt) -> String {
    match receipt.aggregate_verdict {
        Some(eggbench_core::AggregateVerdict::Fail) => {
            "comparison failed a primary gate".to_owned()
        }
        Some(eggbench_core::AggregateVerdict::Inconclusive) => {
            "comparison is inconclusive for a primary gate".to_owned()
        }
        Some(eggbench_core::AggregateVerdict::Invalid) => {
            "comparison is invalid for a primary gate".to_owned()
        }
        Some(eggbench_core::AggregateVerdict::Pass) => "comparison passed".to_owned(),
        None => "comparison produced no gated primary verdict".to_owned(),
    }
}

fn write_receipt(path: &Path, receipt: &ComparisonReceipt) -> Result<(), CliError> {
    let body = serde_json::to_string_pretty(receipt)
        .map_err(|error| CliError::Internal(format!("could not serialize receipt: {error}")))?;
    std::fs::write(path, format!("{body}\n")).map_err(|error| {
        CliError::Bundle(eggbench_core::BundleError::Io {
            path: PathBuf::from(path),
            source: error,
        })
    })
}

fn map_comparison_error(error: eggbench_core::ComparisonError) -> CliError {
    use eggbench_core::ComparisonError as Source;
    match error {
        Source::Bundle(bundle) => CliError::Bundle(bundle),
        Source::Alias { category, detail } => CliError::BaselineAlias {
            category: category.to_owned(),
            detail,
        },
        Source::Unsupported(detail) => CliError::Internal(detail.to_owned()),
    }
}
