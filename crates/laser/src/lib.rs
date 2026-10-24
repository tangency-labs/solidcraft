//! Laser mark domain: machine profiles and LightBurn handoff packages.
//!
//! v1 ships geometry + profile metadata for operator import into LightBurn.
//! Direct device I/O (Serial/USB/BLE providers) is out of scope here.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod package;
pub mod profile;

pub use package::{LaserJobMeta, LaserLayer, LaserPackage, Polyline2, write_ascii_dxf, write_svg};
pub use profile::{LaserProfile, ProfileId, builtin_profiles, profile};

use thiserror::Error;

/// Hard error from laser package or profile helpers.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum LaserError {
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, LaserError>;

pub(crate) fn invalid(msg: impl Into<String>) -> LaserError {
    LaserError::Invalid(msg.into())
}
