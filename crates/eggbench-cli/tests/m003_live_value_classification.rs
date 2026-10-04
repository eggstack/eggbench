//! M003c live-value classification: the consumer-side `--require-live-values`
//! claim, exercised against the real harness script.
//!
//! `--require-live-values` is a separate claim from the telemetry contract
//! shape: it asserts the subject *populated* the required series rather than
//! publishing inventory at zero. "Populated" is not the same question for every
//! series, so the script classifies metrics by kind:
//!
//! * `--positive-gauge` — a workload-derived gauge such as in-flight active
//!   connections only exists if it actually rose, so it must exceed zero;
//! * `--allow-zero-gauge` — a health gauge such as event-loop lag is populated
//!   when it reports a valid reading, and zero is the correct reading for an
//!   event loop that was never late;
//! * unclassified required gauges keep the strict must-exceed-zero rule, so
//!   refining the classification cannot silently weaken a series nobody
//!   reasoned about.
//!
//! Counters are classified by neither flag: a counter only has to be observed.
//!
//! These cases are deliberately built as bundles on disk and run through the
//! real script rather than re-implementing the rule, because the rule has
//! already been got wrong once: an early `continue` guard meant to skip
//! non-gauges also skipped every counter, so `seen_counter` was never set and a
//! correctly reporting subject was failed with "no delta". That defect was
//! invisible to a differential check that exercised only gauge combinations, so
//! the counter cases here are not redundant with the gauge cases and must stay.
//!
//! The synthetic bundles are harness inputs only. Nothing here asserts `SynVoid`
//! semantics, and no real `SynVoid` checkout is required.

use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Owner-published mapping digest the synthetic provenance records agree on.
const MAPPING_SHA256: &str = "622f6a13c4353cc7465cce39a57ed86fa0db2fe4114258e6f06226c1748d2d99";

fn script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/qualification/synvoid-m003/assert-subject-telemetry.py")
}

fn python() -> Option<&'static str> {
    for candidate in ["python3", "python"] {
        if Command::new(candidate)
            .arg("--version")
            .output()
            .is_ok_and(|out| out.status.success())
        {
            return Some(candidate);
        }
    }
    eprintln!("skipping M003c live-value classification: no python3 on PATH");
    None
}

/// One measured trial: the observations the subject published, or `None` for a
/// metric the subject did not export at all.
type Trial = Vec<(&'static str, Option<f64>)>;

fn observation(name: &str, value: Option<f64>) -> Value {
    match value {
        Some(value) => json!({
            "name": name,
            "state": {"state": "observed", "value": value},
        }),
        None => json!({
            "name": name,
            "state": {"state": "absent"},
        }),
    }
}

/// Build a bundle the script accepts structurally: every trial carries a
/// loopback provenance record with no scrape errors, no dropped samples, and
/// enough retained samples.
fn synthetic_bundle(dir: &Path, trials: &[Trial]) -> PathBuf {
    let bundle = dir.join("bundle.eggb");
    for (index, trial) in trials.iter().enumerate() {
        let staged = bundle.join("trials").join(format!("{:03}", index + 1));
        std::fs::create_dir_all(staged.join("telemetry")).expect("stage trial");

        let mut published = Vec::new();
        let mut requested = Vec::new();
        for (name, value) in trial {
            requested.push(json!({"name": name}));
            if let Some(value) = value {
                published.push(observation(name, Some(*value)));
            }
        }
        std::fs::write(
            staged.join("metrics.json"),
            serde_json::to_vec_pretty(&json!({
                "schema_version": 2,
                "observations": published,
            }))
            .expect("metrics json"),
        )
        .expect("write metrics");

        let provenance = json!({
            "schema_version": 1,
            "source": "prometheus-http",
            "exposition_format": "prometheus-text-scalar-v1",
            "endpoint_authority": "127.0.0.1:19090",
            "mapping_ref": "telemetry-mapping.json",
            "mapping_sha256": MAPPING_SHA256,
            "poll_interval_ms": 1000,
            "sample_count": 2,
            "poll_error_count": 0,
            "missing_field_observation_count": 0,
            "dropped_sample_count": 0,
            "requested_metrics": requested,
        });
        std::fs::write(
            staged
                .join("telemetry")
                .join("00-00-prometheus-provenance.json"),
            serde_json::to_vec_pretty(&provenance).expect("provenance json"),
        )
        .expect("write provenance");
    }
    bundle
}

fn run_assert(bundle: &Path, extra: &[&str]) -> Output {
    let mut args = vec![
        script().to_string_lossy().into_owned(),
        "--bundle".to_owned(),
        bundle.to_string_lossy().into_owned(),
        "--mapping-sha256".to_owned(),
        MAPPING_SHA256.to_owned(),
    ];
    args.extend(extra.iter().map(|a| (*a).to_owned()));
    Command::new(python().expect("python checked by caller"))
        .args(&args)
        .output()
        .expect("spawn the harness assertion script")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Assert the script's verdict, and require the reason it gives to name the
/// right kind of series so a stop is diagnosable from the harness log alone.
fn expect(output: &Output, code: i32, reason: &str) {
    assert_eq!(
        output.status.code(),
        Some(code),
        "expected exit {code}, got {:?}\nstdout: {}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        stderr(output),
    );
    assert!(
        stderr(output).contains(reason),
        "expected the stop to name {reason:?}\nstderr: {}",
        stderr(output),
    );
}

#[test]
fn a_healthy_subject_with_live_connections_passes() {
    let Some(_) = python() else { return };
    let temp = tempfile::tempdir().expect("temp");
    // The real m003c-13b shape: an idle loop is honestly reporting zero lag
    // while connections are in flight and the counter is advancing.
    let bundle = synthetic_bundle(
        temp.path(),
        &[vec![
            ("subject_active_connections", Some(8.0)),
            ("subject_event_loop_lag_ms", Some(0.0)),
            ("subject_requests_total", Some(1204.0)),
        ]],
    );
    let output = run_assert(
        &bundle,
        &[
            "--require-gauge",
            "subject_active_connections",
            "--positive-gauge",
            "subject_active_connections",
            "--require-gauge",
            "subject_event_loop_lag_ms",
            "--allow-zero-gauge",
            "subject_event_loop_lag_ms",
            "--require-counter",
            "subject_requests_total",
            "--require-live-values",
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "an honest zero-lag reading with live connections must pass\nstderr: {}",
        stderr(&output),
    );
}

#[test]
fn a_populated_counter_is_not_reported_as_having_no_delta() {
    let Some(_) = python() else { return };
    let temp = tempfile::tempdir().expect("temp");
    // Counters carry no classification flag. This case is the regression guard
    // for the early-`continue` defect that made the counter branch
    // unreachable, which failed a correctly reporting subject.
    let bundle = synthetic_bundle(
        temp.path(),
        &[vec![("subject_requests_total", Some(4096.0))]],
    );
    let output = run_assert(
        &bundle,
        &[
            "--require-counter",
            "subject_requests_total",
            "--require-live-values",
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "an advancing counter must not be reported as having no delta\nstderr: {}",
        stderr(&output),
    );
}

#[test]
fn a_counter_advanced_in_only_one_trial_is_still_populated() {
    let Some(_) = python() else { return };
    let temp = tempfile::tempdir().expect("temp");
    let bundle = synthetic_bundle(
        temp.path(),
        &[
            vec![("subject_requests_total", Some(0.0))],
            vec![("subject_requests_total", Some(17.0))],
        ],
    );
    let output = run_assert(
        &bundle,
        &[
            "--require-counter",
            "subject_requests_total",
            "--require-live-values",
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "one advancing trial populates the counter\nstderr: {}",
        stderr(&output),
    );
}

#[test]
fn a_counter_the_subject_never_exported_has_no_delta() {
    let Some(_) = python() else { return };
    let temp = tempfile::tempdir().expect("temp");
    let bundle = synthetic_bundle(
        temp.path(),
        &[vec![("subject_active_connections", Some(4.0))]],
    );
    let output = run_assert(
        &bundle,
        &[
            "--require-counter",
            "subject_requests_total",
            "--require-live-values",
        ],
    );
    expect(&output, 2, "no delta");
}

#[test]
fn an_in_flight_gauge_at_zero_is_not_populated() {
    let Some(_) = python() else { return };
    let temp = tempfile::tempdir().expect("temp");
    let bundle = synthetic_bundle(
        temp.path(),
        &[vec![("subject_active_connections", Some(0.0))]],
    );
    let output = run_assert(
        &bundle,
        &[
            "--require-gauge",
            "subject_active_connections",
            "--positive-gauge",
            "subject_active_connections",
            "--require-live-values",
        ],
    );
    expect(&output, 2, "at zero for every trial");
}

#[test]
fn a_health_gauge_is_populated_by_a_valid_reading_including_zero() {
    let Some(_) = python() else { return };
    let temp = tempfile::tempdir().expect("temp");
    let bundle = synthetic_bundle(
        temp.path(),
        &[vec![("subject_event_loop_lag_ms", Some(0.0))]],
    );
    let output = run_assert(
        &bundle,
        &[
            "--require-gauge",
            "subject_event_loop_lag_ms",
            "--allow-zero-gauge",
            "subject_event_loop_lag_ms",
            "--require-live-values",
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "zero lag is the honest reading for a never-late loop\nstderr: {}",
        stderr(&output),
    );
}

#[test]
fn a_health_gauge_with_no_reading_at_all_is_not_populated() {
    let Some(_) = python() else { return };
    let temp = tempfile::tempdir().expect("temp");
    let bundle = synthetic_bundle(temp.path(), &[vec![("subject_queue_depth", Some(3.0))]]);
    let output = run_assert(
        &bundle,
        &[
            "--require-gauge",
            "subject_event_loop_lag_ms",
            "--allow-zero-gauge",
            "subject_event_loop_lag_ms",
            "--require-live-values",
        ],
    );
    expect(&output, 2, "no valid reading");
}

#[test]
fn an_unclassified_gauge_keeps_the_strict_must_exceed_zero_rule() {
    let Some(_) = python() else { return };
    let temp = tempfile::tempdir().expect("temp");
    let bundle = synthetic_bundle(temp.path(), &[vec![("subject_something_new", Some(0.0))]]);
    // No classification flag names this series, so nobody has reasoned about
    // whether zero is valid for it and the strict rule stands.
    let output = run_assert(
        &bundle,
        &[
            "--require-gauge",
            "subject_something_new",
            "--require-live-values",
        ],
    );
    expect(&output, 2, "at zero for every trial");
}

#[test]
fn classifying_one_gauge_does_not_reclassify_its_neighbour() {
    let Some(_) = python() else { return };
    let temp = tempfile::tempdir().expect("temp");
    let bundle = synthetic_bundle(
        temp.path(),
        &[vec![
            ("subject_active_connections", Some(8.0)),
            ("subject_other_gauge", Some(0.0)),
        ]],
    );
    let output = run_assert(
        &bundle,
        &[
            "--require-gauge",
            "subject_active_connections",
            "--positive-gauge",
            "subject_active_connections",
            "--require-gauge",
            "subject_other_gauge",
            "--require-live-values",
        ],
    );
    expect(&output, 2, "subject_other_gauge");
}

#[test]
fn a_negative_reading_is_never_a_valid_populated_value() {
    let Some(_) = python() else { return };
    let temp = tempfile::tempdir().expect("temp");
    // A reset or wrapped counter can report a negative value. Neither class
    // treats that as populated, and the domain check upstream of this script
    // should have rejected it first.
    let bundle = synthetic_bundle(
        temp.path(),
        &[vec![
            ("subject_event_loop_lag_ms", Some(-1.0)),
            ("subject_requests_total", Some(-1.0)),
        ]],
    );
    let output = run_assert(
        &bundle,
        &[
            "--require-gauge",
            "subject_event_loop_lag_ms",
            "--allow-zero-gauge",
            "subject_event_loop_lag_ms",
            "--require-counter",
            "subject_requests_total",
            "--require-live-values",
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(2),
        "a negative reading must not satisfy the live-value claim\nstderr: {}",
        stderr(&output),
    );
}

#[test]
fn a_trial_with_a_dropped_sample_is_rejected_regardless_of_classification() {
    let Some(_) = python() else { return };
    let temp = tempfile::tempdir().expect("temp");
    let bundle = synthetic_bundle(
        temp.path(),
        &[vec![("subject_event_loop_lag_ms", Some(0.0))]],
    );
    // Classification only relaxes the *value* rule for a health gauge. It must
    // not become a way to pass a trial whose samples were dropped.
    let provenance = bundle.join("trials/001/telemetry/00-00-prometheus-provenance.json");
    let mut document: Value =
        serde_json::from_slice(&std::fs::read(&provenance).expect("read provenance"))
            .expect("provenance json");
    document["dropped_sample_count"] = json!(2);
    std::fs::write(
        &provenance,
        serde_json::to_vec_pretty(&document).expect("provenance json"),
    )
    .expect("write provenance");
    let output = run_assert(
        &bundle,
        &[
            "--require-gauge",
            "subject_event_loop_lag_ms",
            "--allow-zero-gauge",
            "subject_event_loop_lag_ms",
            "--require-live-values",
        ],
    );
    expect(&output, 2, "dropped_sample_count");
}
