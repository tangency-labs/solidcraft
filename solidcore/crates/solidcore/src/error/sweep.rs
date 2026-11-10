//! Why a sweep or loft refuses, in solidcore's terms (#3140).
//!
//! Upstream: none. A railed loft ([`crate::sweep::loft_rails`]) certifies each guided edge
//! within the caller's tolerance of its rail, adding stations where the bound is above it; past
//! the caller's most wires it refuses and names the bound reached.
//!
//! | Rust                                  | Status difftest compares |
//! |---------------------------------------|--------------------------|
//! | [`SweepFailure::RailPastStationCap`]  | `SM_ERR`                 |
//!
//! Deviations: not upstream.

use super::{ErrorKind, SM_ERR};

/// Why a sweep or loft refuses.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum SweepFailure {
    /// A railed loft's guided edges were asked within `asked` of their rails, and at the most
    /// wires asked, `wires`, the bound proved was still `reached`.
    RailPastStationCap {
        /// The tolerance asked.
        asked: f64,
        /// The largest bound proved between a rail and the skin at the cap.
        reached: f64,
        /// The wires (sections and stations) at the cap.
        wires: usize,
    },
}

impl SweepFailure {
    /// The status the C++ reports for it (a comparison detail): `SM_ERR`.
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::RailPastStationCap { .. } => SM_ERR,
        }
    }

    /// Its stable [`ErrorKind`]: the result is not within the tolerance asked.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::RailPastStationCap { .. } => ErrorKind::NotWithinTolerance,
        }
    }
}

impl core::fmt::Display for SweepFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Self::RailPastStationCap {
                asked,
                reached,
                wires,
            } => write!(
                f,
                "a railed loft's guided edges were asked within {asked:e} of their rails; at {wires} wires the bound proved is {reached:e}"
            ),
        }
    }
}
