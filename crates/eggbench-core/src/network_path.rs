//! Declarative network-path intent for experiment-plan schema v3.
//!
//! The schema-v3 plan may carry an optional `network_path` describing a
//! listener-free Eggress TCP route and a deterministic Eggchaos stream-fault
//! composition. The contract is bounded and redaction-safe so it can be
//! persisted in source plans, resolved plans, and run-level evidence
//! without leaking credentials.

use crate::{DurationMs, Name, PositiveCount};
use serde::{Deserialize, Serialize};

/// Stable route-first/fault-second semantics identity.
pub const NETWORK_PATH_SEMANTICS_VERSION: &str = "route-first-fault-second-v1";
/// Stable Eggchaos `SplitMix64` RNG identity.
pub const NETWORK_PATH_RNG_VERSION: &str = "splitmix64-v1";

/// Top-level network-path request attached to a schema-v3 plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkPathRequest {
    /// Required explicit route driver name.
    pub route: RouteRequest,
    /// Optional stream-fault composition (static, deterministic).
    #[serde(default)]
    pub stream_faults: Option<StreamFaultPlanRequest>,
}

/// Route request: explicit mode (`Direct` or named chain).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteRequest {
    /// Canonical route driver name (e.g. `eggress-route`).
    pub driver: Name,
    /// Direct connection or named proxy chain.
    pub mode: RouteMode,
}

/// Supported route modes for plan schema v3.
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RouteMode {
    /// Listener-free direct connection: no proxy hops are added.
    Direct,
    /// Native proxy chain URI understood by `eggress-uri::parse_proxy_chain`.
    ProxyChain {
        /// Connection chain text (no embedded credentials).
        chain: String,
    },
}

impl Serialize for RouteMode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::Error as _;

        match self {
            Self::Direct => {
                use serde::ser::SerializeStruct as _;
                let mut state = serializer.serialize_struct("RouteMode", 1)?;
                state.serialize_field("kind", "direct")?;
                state.end()
            }
            Self::ProxyChain { chain }
                if !chain.is_empty()
                    && chain.len() <= 1024
                    && !chain.chars().any(char::is_control)
                    && !chain.contains(['@', '%', '?', '#'])
                    && crate::plan::validate_proxy_chain_shape(chain).is_ok() =>
            {
                use serde::ser::SerializeStruct as _;
                let mut state = serializer.serialize_struct("RouteMode", 2)?;
                state.serialize_field("kind", "proxy_chain")?;
                state.serialize_field("chain", chain)?;
                state.end()
            }
            Self::ProxyChain { .. } => Err(S::Error::custom(
                "credential-bearing or invalid proxy chain cannot be serialized",
            )),
        }
    }
}

impl std::fmt::Debug for RouteMode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Direct => formatter.write_str("Direct"),
            Self::ProxyChain { .. } => formatter
                .debug_struct("ProxyChain")
                .field("chain", &"[REDACTED]")
                .finish(),
        }
    }
}

/// Stream-fault composition request: one driver, independent upstream and
/// downstream deterministic plans.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StreamFaultPlanRequest {
    /// Canonical fault driver name (e.g. `eggchaos-stream`).
    pub driver: Name,
    /// Ordered upstream faults (workload client → final target).
    #[serde(default)]
    pub upstream: Vec<StreamFaultRequest>,
    /// Ordered downstream faults (final target → workload client).
    #[serde(default)]
    pub downstream: Vec<StreamFaultRequest>,
}

/// One ordered stream-fault request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StreamFaultRequest {
    /// Stable fault identity (unique within the direction).
    pub id: Name,
    /// Fault kind and bounded configuration.
    pub kind: StreamFaultKind,
}

/// Bounded stream-fault kinds supported by plan schema v3.
///
/// These mirror the stable `eggchaos-core` subset (`latency`, `bandwidth`,
/// `blackhole`, `limit_data`, `slow_close`, `slice`, `disconnect`) without
/// exposing packet/datagram semantics or per-connection probabilities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum StreamFaultKind {
    /// Delay accepted bytes by `delay_ms` ± symmetric `jitter_ms`.
    Latency {
        /// Base delay in milliseconds (bounded 1ms..=1y).
        delay_ms: DurationMs,
        /// Symmetric jitter in milliseconds (bounded 1ms..=1y).
        jitter_ms: DurationMs,
        /// Per-fault buffer bound (positive integer, bytes).
        max_buffer_bytes: PositiveCount,
    },
    /// Rate-limit accepted bytes.
    Bandwidth {
        /// Steady throughput target (bytes per second, > 0).
        bytes_per_second: PositiveCount,
        /// Token-bucket burst (bytes, > 0).
        burst_bytes: PositiveCount,
    },
    /// Discard all accepted bytes; optionally close after a delay.
    Blackhole {
        /// Optional close-after delay (bounded 1ms..=1y).
        close_after_ms: Option<DurationMs>,
    },
    /// Stop forwarding after `bytes` are observed (bytes, > 0).
    LimitData {
        /// Maximum bytes before terminating the direction (bytes, > 0).
        bytes: PositiveCount,
    },
    /// Delay only the shutdown phase, not ordinary writes.
    SlowClose {
        /// Shutdown delay in milliseconds (bounded 1ms..=1y).
        delay_ms: DurationMs,
    },
    /// Split writes into bounded logical slices with an inter-slice delay.
    Slice {
        /// Average slice size (bytes, > 0).
        average_size: PositiveCount,
        /// Variation bound (`variation < average_size`).
        variation: u64,
        /// Inter-slice delay (bounded 1ms..=1y).
        delay_ms: DurationMs,
    },
    /// Graceful disconnect after the active contract boundary.
    Disconnect {
        /// Delay before issuing disconnect (bounded 1ms..=1y).
        after_ms: DurationMs,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_network_path_minimal() {
        let json = r#"{
            "route": {
                "driver": "eggress-route",
                "mode": { "kind": "direct" }
            }
        }"#;
        let request: NetworkPathRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.route.driver.as_str(), "eggress-route");
        assert!(matches!(request.route.mode, RouteMode::Direct));
        assert!(request.stream_faults.is_none());
    }

    #[test]
    fn parse_network_path_full() {
        let json = r#"{
            "route": {
                "driver": "eggress-route",
                "mode": { "kind": "proxy_chain", "chain": "http://hop1:8080__socks5://hop2:1080" }
            },
            "stream_faults": {
                "driver": "eggchaos-stream",
                "upstream": [
                    {
                        "id": "u-lat",
                        "kind": { "kind": "latency", "delay_ms": 50, "jitter_ms": 5, "max_buffer_bytes": 8192 }
                    }
                ],
                "downstream": [
                    {
                        "id": "d-bw",
                        "kind": { "kind": "bandwidth", "bytes_per_second": 1024, "burst_bytes": 4096 }
                    },
                    {
                        "id": "d-disc",
                        "kind": { "kind": "disconnect", "after_ms": 1000 }
                    }
                ]
            }
        }"#;
        let request: NetworkPathRequest = serde_json::from_str(json).unwrap();
        match &request.route.mode {
            RouteMode::ProxyChain { chain } => {
                assert!(chain.contains("hop1"));
            }
            RouteMode::Direct => panic!("expected proxy_chain mode"),
        }
        let faults = request.stream_faults.expect("stream_faults");
        assert_eq!(faults.upstream.len(), 1);
        assert_eq!(faults.downstream.len(), 2);
        assert_eq!(faults.upstream[0].id.as_str(), "u-lat");
    }

    #[test]
    fn unknown_fields_in_route_rejected() {
        let json = r#"{
            "driver": "eggress-route",
            "mode": { "kind": "direct" },
            "credential": "secret"
        }"#;
        let err = serde_json::from_str::<RouteRequest>(json).unwrap_err();
        assert!(err.to_string().contains("unknown field"));
    }

    #[test]
    fn credentialed_route_serialization_fails_without_exposing_input() {
        let mode = RouteMode::ProxyChain {
            chain: "trojan://secret-marker@proxy.example:443".to_owned(),
        };
        let error =
            serde_json::to_string(&mode).expect_err("credentialed route must not serialize");
        assert!(!error.to_string().contains("secret-marker"));
    }

    #[test]
    fn zero_or_zero_value_in_bandwidth_rejected_by_underlying() {
        let json = r#"{
            "kind": "bandwidth",
            "bytes_per_second": 0,
            "burst_bytes": 4096
        }"#;
        let err = serde_json::from_str::<StreamFaultKind>(json).unwrap_err();
        assert!(err.to_string().contains("positive"));
    }

    #[test]
    fn duration_bounds_are_shared() {
        let duration = DurationMs::new(50).unwrap();
        assert_eq!(duration.get(), 50);
        assert!(DurationMs::new(0).is_err());
        assert!(DurationMs::new(31_536_000_001).is_err());
    }

    #[test]
    fn positive_count_round_trips() {
        let count = PositiveCount::new(64).unwrap();
        assert_eq!(count.get(), 64);
        assert!(PositiveCount::new(0).is_err());
    }
}
