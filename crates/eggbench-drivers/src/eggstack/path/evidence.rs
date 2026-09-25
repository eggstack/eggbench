//! Network-path evidence retained per run.

#![allow(clippy::missing_errors_doc)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::missing_panics_doc)]

use eggbench_core::{
    ArtifactRole, BundleError, BundleReader, Name, ResolvedDriver, RouteRequest, SchemaVersion,
    StreamFaultPlanRequest,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Read;
use std::sync::Mutex;

use crate::eggstack::path::MAX_ROUTE_FAILURE_KIND_BUCKETS;
use crate::eggstack::path::route::{parse_route_chain, redacted_chain_text};
use sha2::Digest;

/// Schema version of [`NetworkPathEvidence`].
pub const NETWORK_PATH_EVIDENCE_SCHEMA_VERSION: SchemaVersion = SchemaVersion(1);
/// Maximum retained `network-path.json` byte count.
pub const NETWORK_PATH_EVIDENCE_MAX_BYTES: usize = 128 * 1024;
const MAX_ROUTE_FAILURE_KEY_BYTES: usize = 256;

/// Run-level network-path evidence artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkPathEvidence {
    /// Evidence artifact schema version.
    pub schema_version: SchemaVersion,
    /// Eggbench adapter implementation version.
    pub adapter_version: String,
    /// Selected route driver provenance.
    pub route_driver: NetworkPathDriverEvidence,
    /// Exact resolved `eggress-outbound` version from `Cargo.lock`.
    pub eggress_outbound_version: String,
    /// Exact resolved `eggress-uri` parser version from `Cargo.lock`.
    pub eggress_uri_version: String,
    /// Selected stream-fault driver provenance, when requested.
    pub fault_driver: Option<NetworkPathDriverEvidence>,
    /// Stable ordering, layer, and direction semantics.
    pub semantics: PathSemantics,
    /// Credential-free route request.
    pub route: RouteRequest,
    /// Canonical credential-free chain text, when applicable.
    pub redacted_chain: Option<String>,
    /// SHA-256 of the canonical chain text, when applicable.
    pub chain_config_digest: Option<String>,
    /// Configured proxy hop count; zero for a direct route.
    pub configured_hop_count: u16,
    /// Static deterministic stream-fault plan, when requested.
    pub stream_faults: Option<StreamFaultEvidence>,
    /// Bounded run-level physical-dial diagnostics.
    pub diagnostics: PathDiagnosticsSnapshot,
    /// Static policy lifetime marker.
    pub policy_mode: PathPolicyMode,
}

/// Resolved route or fault driver provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkPathDriverEvidence {
    /// Driver name.
    pub name: String,
    /// Eggbench adapter version.
    pub adapter_version: String,
    /// Sibling crate or executable name.
    pub upstream_name: String,
    /// Exact sibling version, when known.
    pub upstream_version: Option<String>,
}

/// Stable route/fault composition semantics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathSemantics {
    /// Comparison-critical ordering semantics identity.
    pub ordering_version: String,
    /// Route-before-fault ordering marker.
    pub ordering: PathOrdering,
    /// User-space stream impairment marker.
    pub fault_layer: FaultLayer,
    /// Upstream direction identity.
    pub upstream: StreamDirection,
    /// Downstream direction identity.
    pub downstream: StreamDirection,
}

/// Route-before-fault ordering marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathOrdering {
    /// Eggress route first, Eggchaos stream second.
    RouteFirstFaultSecond,
}

/// User-space stream-fault layer marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FaultLayer {
    /// Accepted byte-stream impairment, never packet/datagram loss.
    UserSpaceStream,
}

/// Final-target-relative stream direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamDirection {
    /// Workload client to final target.
    ClientToTarget,
    /// Final target to workload client.
    TargetToClient,
}

/// Static policy lifetime marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathPolicyMode {
    /// No live mutation during the run.
    Static,
}

/// Static Eggchaos configuration and deterministic identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StreamFaultEvidence {
    /// Exact ordered upstream and downstream fault requests.
    pub request: StreamFaultPlanRequest,
    /// Explicit experiment seed used as the Eggchaos namespace.
    pub seed_namespace: Option<u64>,
    /// Stable Eggchaos RNG identity.
    pub rng_version: String,
}

/// Bounded run-level physical-dial diagnostics.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathDiagnosticsSnapshot {
    /// Physical dial attempts.
    pub physical_dial_attempts: u64,
    /// Successful routed physical dials.
    pub successful_dials: u64,
    /// Successful physical dials wrapped by Eggchaos.
    pub fault_wrapped_connections: u64,
    /// Eggchaos wrapper-construction failures after successful route establishment.
    pub fault_wrapper_construction_failures: u64,
    /// Number of route-failure events omitted after the bounded bucket limit.
    pub route_failure_buckets_dropped: u64,
    /// Stable Eggress failure kind/stage/protocol counts.
    pub route_failures: BTreeMap<String, u64>,
    /// Observed configured-hop-count distribution.
    pub hop_count_distribution: BTreeMap<u64, u64>,
    /// Maximum observed configured hop count.
    pub max_observed_hop_count: u64,
    /// Lowest successful connection ordinal.
    pub connection_ordinal_min: u64,
    /// Highest successful connection ordinal.
    pub connection_ordinal_max: u64,
    /// Number of successful connection ordinals represented.
    pub connection_ordinal_count: u64,
}

fn safe_route_failure_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= MAX_ROUTE_FAILURE_KEY_BYTES
        && label.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, ':' | '=' | '_' | '-' | '.')
        })
        && !label.to_ascii_lowercase().contains("secret")
        && !label.to_ascii_lowercase().contains("password")
        && !label.to_ascii_lowercase().contains("token")
}

fn subtract_string_map(
    after: &BTreeMap<String, u64>,
    before: &BTreeMap<String, u64>,
) -> BTreeMap<String, u64> {
    after
        .iter()
        .filter_map(|(key, value)| {
            let delta = value.saturating_sub(before.get(key).copied().unwrap_or(0));
            (delta > 0).then(|| (key.clone(), delta))
        })
        .collect()
}

fn subtract_hop_map(after: &BTreeMap<u64, u64>, before: &BTreeMap<u64, u64>) -> BTreeMap<u64, u64> {
    after
        .iter()
        .filter_map(|(key, value)| {
            let delta = value.saturating_sub(before.get(key).copied().unwrap_or(0));
            (delta > 0).then_some((*key, delta))
        })
        .collect()
}

#[derive(Debug, Clone, Default)]
struct PathDiagnosticsState {
    physical_dial_attempts: u64,
    successful_dials: u64,
    fault_wrapped_connections: u64,
    fault_wrapper_construction_failures: u64,
    route_failure_buckets_dropped: u64,
    max_observed_hop_count: u64,
    connection_ordinal_min: u64,
    connection_ordinal_max: u64,
    invocation_start: Option<u64>,
    invocation_ordinal_min: Option<u64>,
    invocation_ordinal_max: Option<u64>,
    route_failures: BTreeMap<String, u64>,
    hop_count_distribution: BTreeMap<u64, u64>,
}

/// Live counters shared by one workload executor and its evidence snapshot.
#[derive(Debug, Default)]
pub struct SharedPathDiagnostics {
    state: Mutex<PathDiagnosticsState>,
}

impl SharedPathDiagnostics {
    /// Construct zeroed diagnostics for one run.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Mutex::new(PathDiagnosticsState::default()),
        }
    }

    /// Record one physical route attempt.
    pub fn record_dial_attempt(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.physical_dial_attempts = state.physical_dial_attempts.saturating_add(1);
        }
    }

    /// Record one successful route establishment.
    pub fn record_successful_dial(&self, hop_count: u64, ordinal: u64) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        state.successful_dials = state.successful_dials.saturating_add(1);
        state.connection_ordinal_min = if state.successful_dials == 1 {
            ordinal
        } else {
            state.connection_ordinal_min.min(ordinal)
        };
        state.connection_ordinal_max = state.connection_ordinal_max.max(ordinal);
        if state.invocation_start.is_some_and(|start| ordinal >= start) {
            state.invocation_ordinal_min = Some(
                state
                    .invocation_ordinal_min
                    .map_or(ordinal, |current| current.min(ordinal)),
            );
            state.invocation_ordinal_max = Some(
                state
                    .invocation_ordinal_max
                    .map_or(ordinal, |current| current.max(ordinal)),
            );
        }
        state.max_observed_hop_count = state.max_observed_hop_count.max(hop_count);
        *state.hop_count_distribution.entry(hop_count).or_default() += 1;
    }

    /// Record one bounded stable route-failure category.
    pub fn record_route_failure(&self, key: String) {
        if key.len() > MAX_ROUTE_FAILURE_KEY_BYTES || key.chars().any(char::is_control) {
            if let Ok(mut state) = self.state.lock() {
                state.route_failure_buckets_dropped =
                    state.route_failure_buckets_dropped.saturating_add(1);
            }
            return;
        }
        if let Ok(mut state) = self.state.lock() {
            if state.route_failures.len() < MAX_ROUTE_FAILURE_KIND_BUCKETS
                || state.route_failures.contains_key(&key)
            {
                *state.route_failures.entry(key).or_default() += 1;
            } else {
                state.route_failure_buckets_dropped =
                    state.route_failure_buckets_dropped.saturating_add(1);
            }
        }
    }

    /// Record one successful Eggchaos wrapper activation.
    pub fn record_fault_wrapped_connection(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.fault_wrapped_connections = state.fault_wrapped_connections.saturating_add(1);
        }
    }

    /// Record one Eggchaos wrapper-construction failure.
    pub fn record_fault_wrapper_construction_failure(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.fault_wrapper_construction_failures =
                state.fault_wrapper_construction_failures.saturating_add(1);
        }
    }

    /// Begin one invocation and return the pre-invocation run snapshot.
    #[must_use]
    pub fn begin_invocation(&self, first_ordinal: u64) -> PathDiagnosticsSnapshot {
        let mut state = self.state.lock().expect("path diagnostics lock");
        state.invocation_start = Some(first_ordinal);
        state.invocation_ordinal_min = None;
        state.invocation_ordinal_max = None;
        snapshot_from_state(&state)
    }

    /// Return one invocation's coherent diagnostics delta.
    #[must_use]
    pub fn invocation_delta(&self, before: &PathDiagnosticsSnapshot) -> PathDiagnosticsSnapshot {
        let state = self.state.lock().expect("path diagnostics lock");
        let current = snapshot_from_state(&state);
        let hop_count_distribution = subtract_hop_map(
            &current.hop_count_distribution,
            &before.hop_count_distribution,
        );
        let successful_dials = current
            .successful_dials
            .saturating_sub(before.successful_dials);
        PathDiagnosticsSnapshot {
            physical_dial_attempts: current
                .physical_dial_attempts
                .saturating_sub(before.physical_dial_attempts),
            successful_dials,
            fault_wrapped_connections: current
                .fault_wrapped_connections
                .saturating_sub(before.fault_wrapped_connections),
            fault_wrapper_construction_failures: current
                .fault_wrapper_construction_failures
                .saturating_sub(before.fault_wrapper_construction_failures),
            route_failure_buckets_dropped: current
                .route_failure_buckets_dropped
                .saturating_sub(before.route_failure_buckets_dropped),
            route_failures: subtract_string_map(&current.route_failures, &before.route_failures),
            max_observed_hop_count: hop_count_distribution.keys().copied().max().unwrap_or(0),
            hop_count_distribution,
            connection_ordinal_min: if successful_dials == 0 {
                0
            } else {
                state.invocation_ordinal_min.unwrap_or_default()
            },
            connection_ordinal_max: if successful_dials == 0 {
                0
            } else {
                state.invocation_ordinal_max.unwrap_or_default()
            },
            connection_ordinal_count: successful_dials,
        }
    }

    /// Snapshot the current run-level counters.
    #[must_use]
    pub fn snapshot(&self) -> PathDiagnosticsSnapshot {
        self.state
            .lock()
            .map(|state| snapshot_from_state(&state))
            .unwrap_or_default()
    }
}

fn snapshot_from_state(state: &PathDiagnosticsState) -> PathDiagnosticsSnapshot {
    let (min, max) = if state.successful_dials == 0 {
        (0, 0)
    } else {
        (state.connection_ordinal_min, state.connection_ordinal_max)
    };
    PathDiagnosticsSnapshot {
        physical_dial_attempts: state.physical_dial_attempts,
        successful_dials: state.successful_dials,
        fault_wrapped_connections: state.fault_wrapped_connections,
        fault_wrapper_construction_failures: state.fault_wrapper_construction_failures,
        route_failure_buckets_dropped: state.route_failure_buckets_dropped,
        route_failures: state.route_failures.clone(),
        hop_count_distribution: state.hop_count_distribution.clone(),
        max_observed_hop_count: state.max_observed_hop_count,
        connection_ordinal_min: min,
        connection_ordinal_max: max,
        connection_ordinal_count: state.successful_dials,
    }
}

/// Build final run evidence from the resolved path and live diagnostics.
#[must_use]
pub fn build_evidence(
    resolved: &eggbench_core::ResolvedNetworkPath,
    redacted_chain: Option<String>,
    chain_config_digest: Option<String>,
    configured_hop_count: u16,
    seed_namespace: Option<u64>,
    diagnostics: PathDiagnosticsSnapshot,
) -> NetworkPathEvidence {
    let stream_faults = resolved.stream_faults.as_ref().map(|faults| {
        let active = !faults.request.upstream.is_empty() || !faults.request.downstream.is_empty();
        StreamFaultEvidence {
            request: faults.request.clone(),
            seed_namespace: active.then_some(seed_namespace).flatten(),
            rng_version: faults.rng_version.clone(),
        }
    });
    NetworkPathEvidence {
        schema_version: NETWORK_PATH_EVIDENCE_SCHEMA_VERSION,
        adapter_version: super::EGGSTACK_PATH_ADAPTER_VERSION.to_owned(),
        route_driver: driver_evidence(&resolved.route_driver),
        eggress_outbound_version: super::EGGRESS_OUTBOUND_VERSION.to_owned(),
        eggress_uri_version: super::EGGRESS_URI_VERSION.to_owned(),
        fault_driver: resolved
            .stream_faults
            .as_ref()
            .map(|faults| driver_evidence(&faults.fault_driver)),
        semantics: PathSemantics {
            ordering_version: resolved.semantics_version.clone(),
            ordering: PathOrdering::RouteFirstFaultSecond,
            fault_layer: FaultLayer::UserSpaceStream,
            upstream: StreamDirection::ClientToTarget,
            downstream: StreamDirection::TargetToClient,
        },
        route: resolved.route.clone(),
        redacted_chain,
        chain_config_digest,
        configured_hop_count,
        stream_faults,
        diagnostics,
        policy_mode: PathPolicyMode::Static,
    }
}

fn driver_evidence(driver: &ResolvedDriver) -> NetworkPathDriverEvidence {
    NetworkPathDriverEvidence {
        name: driver.descriptor.name.to_string(),
        adapter_version: driver.descriptor.adapter_version.clone(),
        upstream_name: driver.descriptor.upstream_name.clone(),
        upstream_version: driver.descriptor.upstream_version.clone(),
    }
}

/// Load a manifest-listed `network-path.json` artifact from a verified bundle.
///
/// # Errors
/// Returns [`BundleError`] when the artifact is ambiguous, unbounded, malformed, or invalid.
pub fn load_network_path_evidence(
    reader: &BundleReader,
) -> Result<Option<NetworkPathEvidence>, BundleError> {
    let mut records = reader
        .manifest()
        .artifacts
        .iter()
        .filter(|record| {
            matches!(&record.role, ArtifactRole::Other { label } if label.as_str() == "network-path")
        });
    let Some(record) = records.next() else {
        return Ok(None);
    };
    if records.next().is_some() {
        return Err(BundleError::InvalidManifest(
            "bundle contains multiple network-path evidence artifacts",
        ));
    }
    if record.byte_size > NETWORK_PATH_EVIDENCE_MAX_BYTES as u64 {
        return Err(BundleError::BoundExceeded("network-path evidence bytes"));
    }
    let mut file = reader.open_artifact(&record.path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| BundleError::Io {
            path: std::path::PathBuf::from(record.path.as_str()),
            source: error,
        })?;
    let evidence: NetworkPathEvidence = serde_json::from_slice(&bytes)
        .map_err(|error| BundleError::ManifestParse(error.to_string()))?;
    evidence.validate().map_err(BundleError::ManifestParse)?;
    Ok(Some(evidence))
}

impl NetworkPathEvidence {
    /// Validate schema and bounded diagnostic invariants.
    ///
    /// # Errors
    /// Returns a bounded reason when the artifact is invalid.
    #[allow(clippy::too_many_lines)] // Cross-field evidence invariants stay auditable in one pass.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != NETWORK_PATH_EVIDENCE_SCHEMA_VERSION {
            return Err("unsupported network-path evidence schema version".to_owned());
        }
        let request = eggbench_core::NetworkPathRequest {
            route: self.route.clone(),
            stream_faults: self
                .stream_faults
                .as_ref()
                .map(|faults| faults.request.clone()),
        };
        eggbench_core::validate_network_path_contract(&request)
            .map_err(|error| error.to_string())?;
        if self.adapter_version.is_empty()
            || self.adapter_version.len() > 128
            || self.route_driver.name != super::ROUTE_DRIVER_NAME
            || self.route_driver.adapter_version.is_empty()
            || self.route_driver.adapter_version.len() > 128
            || self.route_driver.upstream_name != "eggress-outbound"
            || self
                .route_driver
                .upstream_version
                .as_deref()
                .is_none_or(|version| version.is_empty() || version.len() > 128)
            || self.eggress_outbound_version.is_empty()
            || self.eggress_outbound_version.len() > 128
            || self.eggress_uri_version.is_empty()
            || self.eggress_uri_version.len() > 128
            || self.route.driver.as_str() != super::ROUTE_DRIVER_NAME
        {
            return Err("network-path route provenance is invalid".to_owned());
        }
        if self.semantics.ordering_version != eggbench_core::NETWORK_PATH_SEMANTICS_VERSION
            || !matches!(self.semantics.ordering, PathOrdering::RouteFirstFaultSecond)
            || !matches!(self.semantics.fault_layer, FaultLayer::UserSpaceStream)
            || !matches!(self.semantics.upstream, StreamDirection::ClientToTarget)
            || !matches!(self.semantics.downstream, StreamDirection::TargetToClient)
            || !matches!(self.policy_mode, PathPolicyMode::Static)
        {
            return Err("network-path semantics are invalid".to_owned());
        }

        let (expected_chain, expected_digest, expected_hops) = match &self.route.mode {
            eggbench_core::RouteMode::Direct => (None, None, 0),
            eggbench_core::RouteMode::ProxyChain { chain } => {
                let canonical = redacted_chain_text(chain)?;
                let digest = format!("{:x}", sha2::Sha256::digest(canonical.as_bytes()));
                let hops = u16::try_from(parse_route_chain(chain)?.hops.len())
                    .map_err(|_| "network-path hop count exceeds evidence bound".to_owned())?;
                (Some(canonical), Some(digest), hops)
            }
        };
        if self.redacted_chain != expected_chain
            || self.chain_config_digest != expected_digest
            || self.configured_hop_count != expected_hops
        {
            return Err("network-path canonical route identity is inconsistent".to_owned());
        }

        match (&self.stream_faults, &self.fault_driver) {
            (None, None) => {}
            (Some(faults), Some(driver)) => {
                if faults.request.driver.as_str() != super::FAULT_DRIVER_NAME
                    || driver.name != super::FAULT_DRIVER_NAME
                    || driver.adapter_version.is_empty()
                    || driver.adapter_version.len() > 128
                    || driver.upstream_name != "eggchaos-core"
                    || driver
                        .upstream_version
                        .as_deref()
                        .is_none_or(|version| version.is_empty() || version.len() > 128)
                    || faults.rng_version != eggbench_core::NETWORK_PATH_RNG_VERSION
                    || ((!faults.request.upstream.is_empty()
                        || !faults.request.downstream.is_empty())
                        && faults.seed_namespace.is_none())
                {
                    return Err("network-path fault provenance is invalid".to_owned());
                }
            }
            _ => return Err("network-path fault request and driver disagree".to_owned()),
        }

        let diagnostics = &self.diagnostics;
        let hop_total = diagnostics
            .hop_count_distribution
            .values()
            .try_fold(0_u64, |total, count| total.checked_add(*count));
        let expected_max_hop = diagnostics
            .hop_count_distribution
            .keys()
            .copied()
            .max()
            .unwrap_or(0);
        let faults_active = self.stream_faults.as_ref().is_some_and(|faults| {
            !faults.request.upstream.is_empty() || !faults.request.downstream.is_empty()
        });
        if diagnostics.route_failure_buckets_dropped > diagnostics.physical_dial_attempts
            || diagnostics.route_failures.len() > MAX_ROUTE_FAILURE_KIND_BUCKETS
            || diagnostics
                .route_failures
                .iter()
                .any(|(key, count)| !safe_route_failure_label(key) || *count == 0)
            || diagnostics
                .hop_count_distribution
                .iter()
                .any(|(hops, count)| *count == 0 || *hops > u64::from(self.configured_hop_count))
            || hop_total != Some(diagnostics.successful_dials)
            || diagnostics.max_observed_hop_count != expected_max_hop
            || diagnostics.successful_dials > diagnostics.physical_dial_attempts
            || diagnostics.fault_wrapped_connections > diagnostics.successful_dials
            || diagnostics.fault_wrapper_construction_failures > diagnostics.successful_dials
            || diagnostics
                .fault_wrapped_connections
                .checked_add(diagnostics.fault_wrapper_construction_failures)
                != if faults_active {
                    Some(diagnostics.successful_dials)
                } else {
                    Some(0)
                }
            || diagnostics.connection_ordinal_count != diagnostics.successful_dials
            || (diagnostics.successful_dials == 0
                && (diagnostics.connection_ordinal_min != 0
                    || diagnostics.connection_ordinal_max != 0
                    || diagnostics.max_observed_hop_count != 0))
            || (diagnostics.successful_dials > 0
                && diagnostics.connection_ordinal_min > diagnostics.connection_ordinal_max)
        {
            return Err("network-path diagnostics are invalid".to_owned());
        }
        Ok(())
    }
}

impl eggbench_runner::RunEvidenceContract for NetworkPathEvidence {
    const SCHEMA_VERSION: SchemaVersion = NETWORK_PATH_EVIDENCE_SCHEMA_VERSION;

    fn validate_contract(&self) -> Result<(), BundleError> {
        self.validate().map_err(BundleError::ManifestParse)
    }
}

/// Validate the stable role label used by the bundle manifest.
#[must_use]
pub fn network_path_role_label() -> Name {
    Name::new("network-path").expect("static role label")
}
