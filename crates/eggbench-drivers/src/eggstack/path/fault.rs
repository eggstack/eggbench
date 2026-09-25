//! Static Eggchaos stream-fault lowering for M002.
//!
//! Each plan-level [`StreamFaultRequest`](eggbench_core::StreamFaultRequest)
//! lowers into a sibling [`eggchaos_core::FaultSpec`]. Activation probability
//! is exactly `1.0`; hard reset is rejected; no datagram faults; no live
//! mutation; no time-varying scenarios. Failures to lower are fail-closed.

#![allow(clippy::missing_errors_doc)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::needless_pass_by_value)]

use eggbench_core::{StreamFaultKind, StreamFaultRequest};
use eggchaos_core::{
    BandwidthConfig, BlackholeConfig, DisconnectConfig, FaultId, FaultKind, FaultPlan, FaultSpec,
    LatencyConfig, LimitDataConfig, Probability, SliceConfig, SlowCloseConfig,
};
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FaultLowerError {
    #[error("duplicate fault id {0}")]
    DuplicateFault(String),
    #[error("fault {0}: {1}")]
    Invalid(String, String),
}

/// Lower an ordered list of plan faults into a sibling `FaultPlan`.
///
/// `probability` is `1.0` for every fault; callers should not pass
/// per-connection probability fields. Hard resets are rejected.
pub fn build_fault_plan(requests: &[StreamFaultRequest]) -> Result<FaultPlan, FaultLowerError> {
    let mut specs = Vec::with_capacity(requests.len());
    for request in requests {
        let id = FaultId::new(request.id.as_str())
            .map_err(|error| FaultLowerError::Invalid(request.id.to_string(), error.to_string()))?;
        let kind = lower_kind(&request.kind, &request.id)?;
        specs.push(FaultSpec {
            id,
            probability: Probability::new(1.0).expect("1.0 is in the closed unit interval"),
            kind,
        });
    }
    FaultPlan::new(specs)
        .map_err(|error| FaultLowerError::Invalid("fault plan".into(), error.to_string()))
}

fn lower_kind(
    kind: &StreamFaultKind,
    fault_id: &eggbench_core::Name,
) -> Result<FaultKind, FaultLowerError> {
    match kind {
        StreamFaultKind::Latency {
            delay_ms,
            jitter_ms,
            max_buffer_bytes,
        } => {
            let delay = Duration::from_millis(delay_ms.get());
            let jitter = Duration::from_millis(jitter_ms.get());
            let max_buffer_bytes = std::num::NonZeroU64::try_from(u64::from(
                max_buffer_bytes.get(),
            ))
            .map_err(|_| {
                FaultLowerError::Invalid(
                    fault_id.to_string(),
                    "max_buffer_bytes must be positive".into(),
                )
            })?;
            Ok(FaultKind::Latency(LatencyConfig {
                delay,
                jitter,
                max_buffer_bytes,
            }))
        }
        StreamFaultKind::Bandwidth {
            bytes_per_second,
            burst_bytes,
        } => {
            let bytes_per_second = std::num::NonZeroU64::try_from(u64::from(
                bytes_per_second.get(),
            ))
            .map_err(|_| {
                FaultLowerError::Invalid(
                    fault_id.to_string(),
                    "bytes_per_second must be positive".into(),
                )
            })?;
            let burst_bytes = std::num::NonZeroU64::try_from(u64::from(burst_bytes.get()))
                .map_err(|_| {
                    FaultLowerError::Invalid(
                        fault_id.to_string(),
                        "burst_bytes must be positive".into(),
                    )
                })?;
            Ok(FaultKind::Bandwidth(BandwidthConfig {
                bytes_per_second,
                burst_bytes,
            }))
        }
        StreamFaultKind::Blackhole { close_after_ms } => {
            let close_after = close_after_ms.map(|value| Duration::from_millis(value.get()));
            Ok(FaultKind::Blackhole(BlackholeConfig { close_after }))
        }
        StreamFaultKind::LimitData { bytes } => {
            let bytes = std::num::NonZeroU64::try_from(u64::from(bytes.get())).map_err(|_| {
                FaultLowerError::Invalid(
                    fault_id.to_string(),
                    "limit-data bytes must be positive".into(),
                )
            })?;
            Ok(FaultKind::LimitData(LimitDataConfig { bytes }))
        }
        StreamFaultKind::SlowClose { delay_ms } => {
            let delay = Duration::from_millis(delay_ms.get());
            Ok(FaultKind::SlowClose(SlowCloseConfig { delay }))
        }
        StreamFaultKind::Slice {
            average_size,
            variation,
            delay_ms,
        } => {
            let average_size = std::num::NonZeroU64::try_from(u64::from(average_size.get()))
                .map_err(|_| {
                    FaultLowerError::Invalid(
                        fault_id.to_string(),
                        "slice average_size must be positive".into(),
                    )
                })?;
            let delay = Duration::from_millis(delay_ms.get());
            Ok(FaultKind::Slice(SliceConfig {
                average_size,
                variation: *variation,
                delay,
            }))
        }
        StreamFaultKind::Disconnect { after_ms } => {
            let after = Duration::from_millis(after_ms.get());
            Ok(FaultKind::Disconnect(DisconnectConfig {
                after,
                hard_reset: false,
            }))
        }
    }
}
