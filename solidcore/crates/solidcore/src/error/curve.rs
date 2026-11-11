//! Why a curve query refuses, in solidcore's terms (#3184).
//!
//! Upstream: none. A curve's certified length ([`crate::curve::curve::Curve::length_bound`])
//! is a bracket whose width cannot fall below the rounding of the pieces it is summed over:
//! asked for a narrower one, it refuses and names the narrowest it proves. Asked for one
//! the curve would need more than the cap of pieces for, it refuses and names the width
//! reached at the cap.
//!
//! | Rust                                 | Status difftest compares |
//! |--------------------------------------|--------------------------|
//! | [`CurveFailure::LengthBelowFloor`]   | `SM_ERR`                 |
//! | [`CurveFailure::LengthPastPieceCap`] | `SM_ERR`                 |
//!
//! Deviations: not upstream.

use super::{ErrorKind, SM_ERR};

/// Why a curve query refuses.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum CurveFailure {
    /// A length bracket was asked `asked` wide, below `floor`, the sum over the `pieces` the
    /// curve was halved into of each piece's rounding bound: halving further only widens it.
    LengthBelowFloor {
        /// The width asked.
        asked: f64,
        /// The narrowest width the bracket proves: the pieces' rounding, summed.
        floor: f64,
        /// The pieces the halving used.
        pieces: usize,
    },
    /// A length bracket was asked `asked` wide, and the curve was halved into its most
    /// pieces, `pieces`, with its bracket still `reached` wide: it could narrow further but
    /// is not halved past the cap.
    LengthPastPieceCap {
        /// The width asked.
        asked: f64,
        /// The bracket's width at the cap.
        reached: f64,
        /// The pieces at the cap.
        pieces: usize,
    },
}

impl CurveFailure {
    /// The status the C++ reports for it (a comparison detail): `SM_ERR`.
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::LengthBelowFloor { .. } | Self::LengthPastPieceCap { .. } => SM_ERR,
        }
    }

    /// Its stable [`ErrorKind`]: the accuracy asked is finer than the computation resolves.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::LengthBelowFloor { .. } | Self::LengthPastPieceCap { .. } => {
                ErrorKind::NotWithinTolerance
            }
        }
    }
}

impl core::fmt::Display for CurveFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Self::LengthBelowFloor {
                asked,
                floor,
                pieces,
            } => write!(
                f,
                "a length bracket {asked:e} wide was asked, below the {floor:e} its rounding over {pieces} pieces allows"
            ),
            Self::LengthPastPieceCap {
                asked,
                reached,
                pieces,
            } => write!(
                f,
                "a length bracket {asked:e} wide was asked; at its cap of {pieces} pieces it is {reached:e} wide"
            ),
        }
    }
}
