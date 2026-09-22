# Experiment plans

An ExperimentPlan is a versioned request, not an execution script. JSON is the canonical machine representation and TOML is supported for hand editing. Both formats decode to the same typed model and are validated before use.

Schema v1 distinguishes closed-loop concurrency, open-loop offered rate, finite-count and time-bounded workloads. A workload target must name a declared service or the explicitly named external subject. A closed/open workload must specify exactly one of request count or duration. Time-bounded closed-loop plans require concurrency; open-loop plans require an offered rate.

Services have stable names and managed/external lifecycle intent. Dependencies must exist and form an acyclic graph. Trial count is positive; durations, rates, percentages, and counts have bounded typed representations. Metric direction and unit are explicit. Diagnostic/informational metrics cannot have a gate. This crate validates intent only; it does not measure or calculate statistical verdicts.

Secret material must be injected by reference (for example, an environment variable name or secret-manager reference). It does not belong in a plan snapshot. Artifact bounds set maximum count, per-artifact bytes, and total bytes for later evidence writing.

See [`minimal.json`](../crates/eggbench-core/tests/fixtures/minimal.json) and [`multi-service-open-loop.json`](../crates/eggbench-core/tests/fixtures/multi-service-open-loop.json) for representative plans.

