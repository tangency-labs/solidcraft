//! Surface/surface intersection of rational B-spline patches.
//!
//! The target is every case of the ssi-corpus (github.com/jyviko/ssi-corpus):
//! transversal, tangent and singular intersections, boundaries, coincident
//! regions, near-degenerate pairs, seams and poles.
//!
//! A plane against a sphere, or against a patch whose implicit form has
//! degree 2 and cuts it in an ellipse, is solved in closed form (`quadric`).
//! Where a patch has an implicit form (an analytic surface, or a B-spline
//! patch's exact algebraic form proved by composition, `algebraic`), the
//! other patch's pieces are traced in their own parameters against it as a
//! Bernstein field (`trace2d`): signs, monotone sweeps, critical points proved
//! one to a cell, and valleys proved on their chords give touch points,
//! crossings and tangent curves. Other pairs are subdivided with Sederberg and
//! Meyers's loop test and marched ([`intersect`]); where that cannot separate
//! them near a tangency, a pair of pieces is traced on a local field (`taylor`):
//! the separation along a fixed direction as a polynomial with proved bounds,
//! read by the same cell tests. Contacts it cannot prove are refused by name,
//! never approximated.
//!
//! No intersection point is ever chosen by thresholding distances at sample
//! points (US patent 12,229,885 claims that): points come from Newton roots and
//! bracketed roots of the field, regions are dropped only by proof.

// Comparisons are written `!(x > 0.0)` where NaN must fail the test, which
// `x <= 0.0` would pass; and index loops run over parallel arrays (Bernstein
// coefficients, control rows) whose arithmetic reads best indexed.
#![allow(clippy::neg_cmp_op_on_partial_ord, clippy::needless_range_loop)]

mod algebraic;
mod coincide;
mod densify;
pub mod field;
pub mod geom3;
pub mod intersect;
mod invert;
pub mod patch;
pub mod piece;
mod quadric;
pub mod shapes;
mod small;
pub mod surface3;
mod taylor;
mod trace2d;

pub use densify::densify;
pub use intersect::{intersect_bodies, Curve, CurvePoint, Intersection, Options, Param, Stats};
pub use patch::Patch;

use std::fmt;

/// Why an intersection was not built.
#[derive(Clone, Debug)]
pub enum Error {
    /// The surfaces touch or coincide, or a piece is degenerate right at the
    /// crossing: a contact this method has not built (Form Studio's `BOOLEAN_TANGENT`).
    Tangent {
        /// Where, when known.
        at: Option<piece::V3>,
        /// What was found there.
        detail: String,
    },
    /// A patch whose degrees, counts, knots or weights cannot be read.
    BadPatch(String),
    /// Marching could not take a step at this point.
    Stalled(piece::V3),
    /// The Newton solve onto a pair's boundary did not converge.
    ExitDiverged,
    /// An arc did not leave its pair of pieces in 100,000 steps.
    Unfinished,
}

impl Error {
    /// A short name for the refusal, as the corpus runner reports it.
    pub fn code(&self) -> &'static str {
        match self {
            Error::Tangent { .. } => "BOOLEAN_TANGENT",
            Error::BadPatch(_) => "BAD_PATCH",
            Error::Stalled(_) => "MARCHING_STALLED",
            Error::ExitDiverged => "EXIT_DIVERGED",
            Error::Unfinished => "ARC_UNFINISHED",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Tangent { detail, .. } | Error::BadPatch(detail) => write!(f, "{detail}"),
            Error::Stalled(p) => write!(
                f,
                "marching stalled at {:.6e}, {:.6e}, {:.6e}",
                p[0], p[1], p[2]
            ),
            Error::ExitDiverged => write!(f, "an exit point did not converge"),
            Error::Unfinished => write!(
                f,
                "an arc did not leave its pair of pieces in 100,000 steps"
            ),
        }
    }
}

impl std::error::Error for Error {}
