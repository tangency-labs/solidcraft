//! SolidBOM's direct-cost calculator: pure arithmetic over the portable contract, used by the
//! service only. The desktop displays the service's answer and never reaches this crate.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod calculate;

pub use calculate::{calculate, calculator_version};
