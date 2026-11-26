//! Splined shafts and the hubs that fit them, from plain numbers.
//!
//! This module makes the outlines of splined shafts and of the hubs that go on them. Every outline
//! comes from a few plain numbers (how many teeth, how large, which angle, which clearances), never
//! from a table, a series or a default of any standard, and every shaft and hub can be checked the
//! way a fitter would check them: turn the hub on the shaft and measure the smallest gap between
//! them.
//!
//! The teeth are made in two ways.
//!
//! Teeth rolled against a straight-sided rack (the involute):
//!
//! - [`involute`]: an external tooth is exactly what the rack leaves: the involute flank, the true
//!   root fillet its rounded tip cuts, and the undercut where the rack reaches too deep. An
//!   internal tooth space is formed, as a broach leaves it: the same involute flanks, with round
//!   corners at the root. [`involute::hub_for`] derives the hub that fits a shaft with stated
//!   clearances, and [`involute::spline_pair`] puts the two on one axis.
//!
//! Teeth drawn from straight lines and arcs, every piece exact:
//!
//! - [`straight`]: straight-sided splines and serrations, the hubs that fit them with stated
//!   clearances, and the turning play of a hub on its shaft.
//!
//! Other modules take finished teeth further:
//!
//! - [`master_spline`]: a splined shaft and its hub with one tooth made different, so that the hub
//!   goes on one way only: a tooth left out of the shaft and a wide tooth in the hub, or a wide
//!   tooth on the shaft and a tooth left out of the hub, placed exactly at an angle by turning the
//!   whole spline.
//! - [`fit`]: the point curves of an outline turned into single B-splines within a stated tolerance
//!   of the true curve, with as few control points as the fit finds, joining smoothly where the
//!   curve does and keeping its corners; the cubic ones also as Bezier segments, and any spline as
//!   the Bezier pieces of a profile, a degree a profile does not hold lowered within a stated
//!   tolerance. What a CAD kernel holds as one edge. The splines are the kernel's NURBS curves:
//!   evaluated, solved for, cut into pieces and bounded by the kernel's own routines
//!   ([`crate::nurbs`]).
//! - [`shaft_spline`]: a spline on a shaft and in its hub planned from one set of numbers, with
//!   every refusal naming the number to change.
//! - [`seat`]: where a spline sits on a round face of a part (the frame at its free end, angle 0
//!   from the world axes) and the measures a caller that cuts it takes of the face and the tools.
//! - [`section`]: an outline as one closed loop of profile segments, the section a cutter takes
//!   out of one tooth space, and the section of a chamfer's tool and the plane it is drawn in.
//!
//! A shaft and its hub end in a [`Pair`]: two outlines, their axes, the side each has its material
//! on, and how the second part moves when the first turns. [`Pair::through_a_cycle`] turns the pair
//! and measures the gap at each step ([`mesh`] is the measurement: the smallest distance between
//! two placed parts, looking no farther than a stated reach, negative where they would overlap).
//! The work of one measurement is bounded whatever the reach and the outlines (see [`mesh::gap`]
//! for what a very small reach means), and so is the work of turning a pair through all its steps
//! ([`pair::MOST_TURNING_WORK`]).
//!
//! An outline keeps what is exact as exact: arcs stay arcs ([`Piece::Arc`]) and lines stay lines
//! ([`Piece::Line`]). The curves that are neither are given as points on the true curve
//! ([`Piece::Curve`]), as few as keep the chords between them within the tolerance asked for and
//! closer together where the curve bends more, or as one smooth spline ([`Piece::Spline`],
//! [`bspline`]). [`Outline::new`] is the backstop of every module: it refuses pieces that do not
//! join, an outline that does not run counter-clockwise around an area, and one that crosses
//! itself, so that no module hands out an outline that cannot be made. The modules refuse such
//! numbers earlier with reasons of their own where they can.
//!
//! Every public function returns [`crate::Result`]; a refusal is a [`crate::SolidError::Failed`]
//! whose reason is a [`TeethFailure`] (read it with [`crate::SolidError::teeth_failure`]) and
//! whose operation names the function.
//!
//! ```
//! use solidcore::teeth::involute::{External, Fit, hub_for, spline_pair};
//!
//! // A splined shaft: 20 teeth of module 1.5 with 30 degree flanks, cut by a round-tipped rack.
//! let shaft = External {
//!     module: 1.5,
//!     teeth: 20,
//!     pressure_angle: 30.0,
//!     addendum: 0.5,
//!     dedendum: 0.85,
//!     shift: 0.0,
//!     tool_tip_radius: 0.4,
//!     thinning: 0.0,
//! };
//! // The hub that fits it with 0.06 mm between the flanks.
//! let fit = Fit {
//!     side_clearance: 0.06,
//!     hub_addendum: 0.5,
//!     tip_clearance: 0.2,
//!     hub_fillet_radius: 0.3,
//!     form_clearance: 0.05,
//! };
//! let hub = hub_for(&shaft, &fit)?;
//! assert_eq!(shaft.outline(0.001)?.teeth(), 20);
//! assert!(hub.major_radius > shaft.tip_radius()?);
//!
//! // The check: between the flanks the clearance appears turned by the pressure angle, half on
//! // each side.
//! let gaps = spline_pair(&shaft, &fit, 1e-6)?.through_a_cycle(4, 0.5)?;
//! let least = gaps.least.map(|g| g.distance).unwrap_or(f64::NAN);
//! assert!((least - 0.06 * 30f64.to_radians().cos() / 2.0).abs() < 1e-5);
//! # Ok::<(), solidcore::SolidError>(())
//! ```
//!
//! Not upstream: SolidCraft's tooth outlines, kernel primitives because they compute geometry any
//! client could use (docs/layers.md §1). An outline is the kernel's own planar data, lines, arcs
//! and splines about an axis; a body is made of it by the caller's sweeps. No recursion. Inputs are
//! hostile: every number is checked before it is used, and work that would grow past a stated bound
//! is refused rather than done.

#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-refusing range tests, kept as written

pub(crate) mod bound;
pub mod bspline;
pub mod fit;
pub mod involute;
pub mod master_spline;
pub mod mesh;
pub mod outline;
pub mod pair;
pub mod round_face;
pub mod seat;
pub mod section;
pub mod shaft_spline;
pub mod straight;

#[cfg(test)]
mod tests;

pub use crate::error::teeth::{PlanRefusal, ProfileWhy, TeethFailure, Why};
pub use mesh::Material;
pub use outline::{Outline, Piece, Point, Role};
pub use pair::{Gaps, Motion, Pair, Part};

/// `value` if it is finite, above `above` and at most `up_to`; otherwise
/// [`TeethFailure::OutOfRange`] naming `what`.
pub(crate) fn in_range(
    what: &'static str,
    value: f64,
    above: f64,
    up_to: f64,
) -> Result<f64, TeethFailure> {
    if value.is_finite() && value > above && value <= up_to {
        Ok(value)
    } else {
        Err(TeethFailure::OutOfRange {
            what,
            value,
            above,
            up_to,
        })
    }
}

/// `value` if it is finite, at least zero and at most `up_to`; otherwise
/// [`TeethFailure::OutOfRange`] naming `what` (above 0).
pub(crate) fn zero_up_to(what: &'static str, value: f64, up_to: f64) -> Result<f64, TeethFailure> {
    if value.is_finite() && value >= 0.0 && value <= up_to {
        Ok(value)
    } else {
        Err(TeethFailure::OutOfRange {
            what,
            value,
            above: 0.0,
            up_to,
        })
    }
}

/// The most points one curve of an outline may have. A tolerance that would need more is refused.
pub const MAX_CURVE_POINTS: usize = 50_000;

/// The most points a whole outline may have, all teeth together. A tolerance that would need more
/// is refused.
pub const MAX_OUTLINE_POINTS: usize = 8_000_000;

/// The most teeth an outline may have.
pub const MAX_TEETH: u32 = 2_000;

/// The largest length accepted, in millimetres.
pub const MAX_LENGTH: f64 = 100_000.0;
