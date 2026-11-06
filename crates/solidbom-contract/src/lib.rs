//! SolidBOM's portable contract: the wire types the desktop app, the calculator and the service
//! share. It has no UI, file access or database code, and it builds for the browser too.
//!
//! Money, rates and measurements use [`rust_decimal::Decimal`], serialized as strings. A missing
//! value is `None`; an explicit zero is `Some(Decimal::ZERO)`. Geometry becomes a decimal once,
//! through [`decimal_from_f64`], and never again from a displayed rounding.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod error;
pub mod types;

pub use error::{ContractError, Result, check_sha256, decimal_from_f64, is_sha256};
pub use types::*;

/// The schema version written into manifests and checked on the way in.
pub const SCHEMA_VERSION: u32 = types::SCHEMA_VERSION;
