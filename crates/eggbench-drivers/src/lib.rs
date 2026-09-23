//! Shared production driver catalog and external-command substrate.
//!
//! `eggbench-drivers` is the sole production adapter/catalog ownership crate.
//! The CLI owns presentation, not driver implementation or registration.
//!
//! M001 establishes reusable machinery only: trusted executable resolution,
//! binary identity/version probing, argv-only command execution with bounded
//! capture and cancellation, raw-output artifact helpers, and a versioned
//! parser contract. It deliberately ships no oha/h2load/iperf3 adapter and
//! no EggServe/Eggfetch/Gregg integration semantics.

#![forbid(unsafe_code)]

mod catalog;
pub mod external;

pub use catalog::{DriverCatalog, production_catalog};
