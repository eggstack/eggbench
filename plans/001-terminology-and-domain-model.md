# Eggbench Terminology and Domain Model

Status: canonical terminology

Companion documents:

- `plans/000-long-term-specification.md`
- `plans/002-long-term-roadmap.md`
- `plans/003-planning-process.md`

This document defines normative Eggbench terminology. Where implementation names differ during early development, this document controls the intended meaning.

## 1. Experiment

An **Experiment** is the complete reproducible performance question Eggbench is asked to answer.

An experiment includes the subject, topology, workload, testbed policy, lifecycle policy, repeated trials, telemetry, gates, and evidence requirements.

An experiment is not synonymous with one process invocation or one load-generator run.

## 2. Experiment Plan

An **Experiment Plan** is the user-authored, schema-versioned declaration of an experiment.

It may contain symbolic driver names, relative paths, defaults, and secret references.

The plan is input. It is not evidence that an experiment actually executed.

## 3. Resolved Plan

A **Resolved Plan** is the validated, concrete execution form produced before I/O begins.

It contains selected driver implementations, executable paths, versions, resolved non-secret defaults, normalized topology, explicit lifecycle policies, and stable identifiers.

The resolved plan MUST be retained in evidence.

## 4. Run

A **Run** is one execution of one resolved experiment plan.

A run may terminate as completed, failed, cancelled, or invalid.

One run normally contains multiple measured trials.

## 5. Phase

A **Phase** is one lifecycle segment of a run.

Canonical phases are:

- parse;
- validation/resolution;
- preflight;
- prepare;
- startup;
- readiness;
- warmup;
- measurement trial;
- cooldown/reset;
- drain;
- teardown;
- evidence finalization;
- comparison.

A phase boundary is part of experiment semantics whenever timing or telemetry interpretation depends on it.

## 6. Trial

A **Trial** is one bounded measured repetition inside a run.

Trials are the default statistical comparison unit.

One trial may contain many requests, packets, connections, transactions, or target-specific operations.

## 7. Warmup

**Warmup** is intentional pre-measurement activity used to establish steady-state conditions such as process initialization, connection pools, JIT-less cache warming, file caches, or target readiness.

Warmup observations MAY be retained diagnostically but MUST NOT enter measured-trial statistics unless a plan explicitly says startup/warm behavior is the subject.

## 8. Cooldown

**Cooldown** is a bounded interval after a trial intended to reduce carry-over such as queue backlog, transient resource saturation, or connection churn.

Cooldown is not assumed to restore a pristine state.

## 9. Reset

A **Reset** is an explicit operation intended to restore selected experiment state between trials.

A reset MUST declare what state it owns. Examples include restarting a subject, clearing a fixture, reloading a configuration, or resetting a fault plan.

A reset MUST NOT claim to clear operating-system caches or remote state unless it actually does so.

## 10. Subject

The **Subject** is the component whose performance or performance/correctness tradeoff is being evaluated.

Examples include SynVoid, Eggfetch, Eggress, EggServe, I2PR, a service binary, a library benchmark adapter, or a user application.

A subject may be managed by Eggbench or externally managed.

## 11. Candidate

A **Candidate** is the subject/configuration/build represented by the run being evaluated.

Candidate does not imply that it is newer or better.

## 12. Baseline

A **Baseline** is an immutable reference evidence bundle or an explicit absolute budget used for comparison.

A floating alias MAY identify a baseline, but the comparison MUST record the immutable object actually used.

## 13. Control

A **Control** is a deliberately simpler or bypass topology used to estimate harness, generator, origin, or environmental overhead.

A control is diagnostic unless a gate explicitly references it.

## 14. Testbed

A **Testbed** is the execution environment relevant to comparability.

It includes hardware, operating system, kernel where relevant, architecture, network arrangement, build/runtime toolchains, driver versions, and environment policy.

A testbed may contain one machine initially and several machines later.

## 15. Environment Fingerprint

An **Environment Fingerprint** is the machine-readable evidence describing a testbed for one run.

It is not necessarily a single hash. A stable digest MAY be computed from selected comparison-critical fields.

## 16. Topology

A **Topology** is the named set of services and network relationships active during an experiment.

The initial representation is a dependency-ordered service graph rather than a general network simulator.

## 17. Service

A **Service** is one managed or externally managed topology participant.

A service has a stable name, kind, configuration, readiness policy, ownership policy, and optional artifacts.

Examples: origin, proxy, WAF, replay server, chaos endpoint, target daemon.

## 18. Managed Service

A **Managed Service** is started and stopped by Eggbench for the run.

Eggbench owns bounded lifecycle cleanup for managed services.

## 19. External Service

An **External Service** is referenced by Eggbench but not lifecycle-owned by the runner.

Its identity and readiness may be checked, but Eggbench MUST NOT imply it performed teardown.

## 20. Driver

A **Driver** is an adapter that implements one bounded Eggbench integration contract.

Driver categories are deliberately distinct:

- SubjectDriver;
- ServiceDriver;
- WorkloadDriver;
- TelemetrySource;
- FaultDriver;
- DiagnosticDriver;
- ExecutionProvider.

A driver MUST declare capabilities rather than silently degrading unsupported options.

## 21. Workload

A **Workload** is the traffic or operations applied during a trial.

A workload includes the generation model, target, duration/count, concurrency or offered rate, request/operation template, and driver configuration.

## 22. Closed-Loop Workload

A **Closed-Loop Workload** issues new work as prior work completes, usually bounded by worker/concurrency count.

Observed throughput therefore affects future offered work.

## 23. Open-Loop Workload

An **Open-Loop Workload** schedules offered work independently of completion according to an arrival-rate model.

This distinction MUST be preserved in evidence because closed-loop load can hide queueing and coordinated-omission effects.

## 24. Offered Load

**Offered Load** is the workload rate requested from a generator.

It is distinct from achieved throughput.

A driver that cannot sustain offered load SHOULD report the shortfall.

## 25. Throughput

**Throughput** is successfully or totally completed work per unit time according to the metric definition.

The counted operation and success semantics MUST be explicit.

## 26. Latency Distribution

A **Latency Distribution** is the distribution of per-operation elapsed durations within a trial.

Percentiles are descriptive summaries of that distribution.

A request count in one latency distribution is not the default statistical sample size for candidate/baseline inference.

## 27. Observation

An **Observation** is one raw or normalized datum produced by a driver, telemetry source, runner phase, or target.

Observations carry source and unit provenance.

## 28. Metric

A **Metric** is a named quantitative contract derived from observations.

A metric defines unit, directionality, aggregation, and gate eligibility.

## 29. Primary Metric

A **Primary Metric** is explicitly selected before comparison as a metric eligible to determine the run verdict.

Primary metrics SHOULD be few enough to avoid accidental multiple-metric false-positive behavior.

## 30. Diagnostic Metric

A **Diagnostic Metric** is recorded and compared but does not independently fail a run.

## 31. Gate

A **Gate** is a predeclared acceptance rule over a metric or correctness assertion.

Gate families include:

- absolute budget;
- relative budget;
- statistical relative budget;
- correctness assertion;
- environment/comparability requirement.

## 32. Practical Threshold

A **Practical Threshold** is the magnitude of regression the experiment considers operationally meaningful.

It is not a confidence level or p-value.

## 33. Uncertainty Interval

An **Uncertainty Interval** is the comparison policy's estimate of uncertainty around a trial-level effect.

Its exact method is versioned by comparison policy.

## 34. Verdict

A **Verdict** is the normalized comparison outcome.

For a gated metric, the initial vocabulary is:

- pass;
- fail;
- inconclusive;
- invalid.

At run level, a failed correctness or regression gate produces failure; an invalid required measurement produces invalid; unresolved required gates may produce inconclusive.

## 35. Invalid Run

An **Invalid Run** is a run whose required measurement or comparability contract was violated.

Examples include failed readiness, insufficient trials, wrong driver version, major testbed mismatch under strict policy, generator saturation when prohibited, or missing required telemetry.

Invalid is distinct from regression.

## 36. Evidence Bundle

An **Evidence Bundle** is the immutable, portable directory artifact produced by a finalized run.

It contains a manifest plus typed and raw artifacts sufficient to inspect what happened.

The extension convention is `.eggb` for bundle directories and future packaged forms.

## 37. Manifest

A **Manifest** is the authoritative index of a completed evidence bundle.

It records schema versions, run identity, status, artifact paths, sizes, hashes, and compatibility metadata.

A directory without a valid finalized manifest is not a completed bundle.

## 38. Artifact

An **Artifact** is a retained file or typed object associated with a run or trial.

Examples include raw generator JSON, HDR histogram files, telemetry streams, logs, subject config, probe reports, and summaries.

## 39. Provenance

**Provenance** is the evidence linking a result to its plan, subject build, testbed, driver versions, source revision, configuration, and artifacts.

Provenance SHOULD be sufficient to answer what was actually measured.

## 40. Qualification Profile

A **Qualification Profile** is a reusable experiment template for one subject/capability class.

Examples include a SynVoid benign WAF H1 profile, Eggfetch streaming profile, or Eggress proxy-chain profile.

Profiles MUST expand into ordinary versioned experiment plans; they do not create a second execution model.

## 41. Security Correctness Gate

A **Security Correctness Gate** is an assertion owned by the security workload/subject semantics whose failure invalidates a claimed performance improvement.

Examples include expected detection, expected allow, protocol integrity, or WAF disposition.

## 42. Fault Plan

A **Fault Plan** is a deterministic impairment configuration applied by a fault driver.

Eggchaos stream faults and Linux netem packet/link impairments are distinct fault-plan families and MUST retain their layer semantics.

## 43. Telemetry Source

A **Telemetry Source** emits time-aligned resource or target state observations.

Telemetry is supporting evidence unless the plan declares its metrics as gated.

## 44. Diagnostic Probe

A **Diagnostic Probe** checks target/network behavior outside the primary benchmark measurement, normally before or after trials.

Eggprobe is the preferred Eggstack diagnostic owner.

## 45. Comparison Policy

A **Comparison Policy** is the versioned algorithm and parameter set that turns baseline/candidate trial observations into effect estimates, uncertainty intervals, and gate verdicts.

Changing policy semantics requires a new policy identifier.

## 46. Same-Testbed Comparison

A **Same-Testbed Comparison** satisfies the plan's declared comparability fields.

It does not require every environment field to be byte-identical; the policy decides which fields are critical, warning-only, or ignored.

## 47. Cross-Testbed Comparison

A **Cross-Testbed Comparison** compares evidence from materially different testbeds.

It is descriptive by default and MUST NOT silently inherit same-testbed regression gates.

## 48. Execution Provider

An **Execution Provider** owns process execution on a host.

The local runner is the initial provider.

A future Eggwork adapter may provide remote execution. The provider does not own Eggbench experiment semantics.

## 49. Repository planning terms

A **Subsystem Roadmap** defines one coherent ownership workstream.

A **Milestone Implementation Plan** is a bounded coding-agent handoff tied to a repository baseline.

A **Closure Record** records implementation evidence and determines whether a milestone is actually complete.

The planning process is normative in `plans/003-planning-process.md`.
