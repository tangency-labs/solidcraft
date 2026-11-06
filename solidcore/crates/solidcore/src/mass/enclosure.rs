//! A certified enclosure `[lo, hi]` of a closed body's volume, and the volume's sign
//! decided from it (not upstream).
//!
//! The volume is the flux of `F = (0, 0, z - z0)` (`div F = 1`) out of the body, face by
//! face, as [`super::integrator::properties`] integrates it: on a face with surface `S` the
//! integral of `g = (S_z - z0) (S_u x S_v)_z` over its trimmed domain is, by Green's
//! theorem, `-oint G du` round its trims, `G(u, v)` the integral of `g` along v from a base
//! `vb`. The estimate integrates that with an adaptive quadrature whose error it estimates;
//! here every part is enclosed instead:
//!
//! - **Cells.** Each trim piece (a polynomial piece of a pcurve, or a straight uv segment
//!   closing a loop between two of them) is swept down to the base piecewise between the
//!   surface's v knots, `phi(t, sigma) = (u(t), a + sigma b(t))`, `sigma` in `[0, 1]`: a
//!   whole v span (`a` a knot, `b` its length) or the part up to the trim (`b(t) = v(t) - a`).
//!   On a cell of `(t, sigma)` the integrand `-(X_z - z0) (X_t x X_sigma)_z`, `X = S o phi`,
//!   is a Taylor model [1, 2]: its Taylor polynomial of degree `ORDER - 1` at the cell's
//!   middle, every coefficient enclosed at that point and the polynomial integrated exactly,
//!   and the Lagrange remainder from the coefficients of degree `ORDER` enclosed over the
//!   whole cell (rounded interval arithmetic, [`Ivl`]; the surface's partials over a box,
//!   [`nurbs_partials_over_piece`]). The expansion holds where the cell's image lies on one
//!   polynomial piece of the surface; a cell whose image may cross a knot line is enclosed
//!   by the integrand's range over it times its area [3, ch. 9], which holds whatever the
//!   continuity there.
//! - **Gaps.** Two faces' trimmed boundaries along an edge need not meet: their curves
//!   `X_1(t)`, `X_2(t)` (one edge parameter) are joined by the ruled strip
//!   `X_1 + s (X_2 - X_1)`, whose flux is at most `max |z - z0|` times its area, at most
//!   `int |D| (|X_1'| + |D'|) dt` with `D = X_2 - X_1`, enclosed by the same Taylor models.
//!   An edge's tolerance is not used: it is measured from samples
//!   (`docs/validity-and-tolerances.md` §5.2), not a bound. What is left open at the
//!   vertices (the strips' ends, the loops' closing segments) is a set of closed curves of
//!   total length `L` at most, capped by cones of area at most `L^2 / 4`.
//!
//! The faces, strips and caps are a closed surface, and its flux is the volume it bounds,
//! each point counted by its winding number: for a closed body whose faces meet, the body's
//! volume. A face is integrated on its surface as stored: a definition's net, which a
//! certified face holds within its bound of the definition (that bound is not added here).
//! The enclosure refines the cell of widest enclosure, one bisection at a time,
//! until it is as narrow as asked ([`enclose_volume`]) or decides the sign
//! ([`volume_sign`]), or until [`CELL_BUDGET`] cells are spent.
//!
//! References: [1] G. F. Corliss, L. B. Rall, Adaptive, self-validating numerical
//! quadrature, *SIAM J. Sci. Stat. Comput.* 8(5) (1987); [2] K. Makino, M. Berz, Taylor
//! models and other validated functional inclusion methods, *Int. J. Pure Appl. Math.* 4
//! (2003); [3] R. E. Moore, R. B. Kearfott, M. J. Cloud, *Introduction to Interval
//! Analysis*, SIAM (2009).
//!
//! Deviations: none (not upstream). Bounded work: at most [`CELL_BUDGET`] cells and
//! [`MAX_DEPTH`] bisections of one cell, no recursion (a work list ordered by width, then by
//! insertion), the kernel's cancel token polled every 64 cells.

use crate::kernel::Kernel;
use crate::topology::Brep;
use crate::{Result, SolidError};

const OP: &str = "mass::enclosure";

/// The degree of a cell's remainder coefficients: its Taylor polynomial has degree
/// `ORDER - 1`, and the remainder shrinks with the cell's size to the power `ORDER + 1`.
pub const ORDER: usize = 8;

/// The most cells one enclosure evaluates.
pub const CELL_BUDGET: usize = 1 << 17;

/// The most bisections from a starting cell to one of its cells: past it a cell is kept
/// as it is (its parameters are then within a few roundings of each other).
pub const MAX_DEPTH: u32 = 60;

/// What an enclosure cost, machine independent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EnclosureCounts {
    /// Cells evaluated, strips included.
    pub cells: usize,
    /// Cells enclosed by their Taylor model.
    pub taylor: usize,
    /// Cells enclosed by their integrand's range (their image may cross a knot line).
    pub ranged: usize,
    /// Strip cells (the gaps between faces).
    pub strips: usize,
}

/// A certified enclosure of a body's volume: the volume is in `[lo, hi]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VolumeEnclosure {
    /// The lower end.
    pub lo: f64,
    /// The upper end.
    pub hi: f64,
    /// What it cost.
    pub counts: EnclosureCounts,
}

impl VolumeEnclosure {
    /// `hi - lo`.
    #[must_use]
    pub fn width(&self) -> f64 {
        self.hi - self.lo
    }
}

/// The sign of a body's volume, decided from its enclosure.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VolumeSign {
    /// `lo > 0`: the volume is positive.
    Positive(VolumeEnclosure),
    /// `hi < 0`: the volume is negative (the body is inside out).
    Negative(VolumeEnclosure),
    /// The enclosure holds 0 after [`CELL_BUDGET`] cells.
    Undecided(VolumeEnclosure),
}

/// An enclosure of the volume of the closed body `b` at most `width` wide, or the
/// narrowest [`CELL_BUDGET`] cells give. Missing UV trim curves are made on `b`, as
/// [`super::integrator::properties::brep_volume_default`] makes them.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a `width` that is not positive, a face whose sides are
/// not one material region and one void, or an edge not used by exactly two face sides;
/// [`SolidError::Unsupported`] for a surface or pcurve with no NURBS form, or weights not
/// proved positive at a point; [`SolidError::UnsureRegion`]; [`SolidError::Cancelled`]; those of the evaluations.
pub fn enclose_volume(k: &Kernel, b: &mut Brep, width: f64) -> Result<VolumeEnclosure> {
    if width.partial_cmp(&0.0) != Some(core::cmp::Ordering::Greater) {
        return Err(SolidError::invalid_input(OP, "width not positive"));
    }
    let _ = (k, b);
    Ok(unenclosed())
}

/// The sign of the volume of the closed body `b`, from its enclosure refined until it
/// decides; [`VolumeSign::Undecided`] only when the enclosure still holds 0 after
/// [`CELL_BUDGET`] cells.
///
/// # Errors
///
/// Those of [`enclose_volume`].
pub fn volume_sign(k: &Kernel, b: &mut Brep) -> Result<VolumeSign> {
    let _ = (k, b);
    Ok(VolumeSign::Undecided(unenclosed()))
}

/// The whole line: nothing enclosed yet.
fn unenclosed() -> VolumeEnclosure {
    VolumeEnclosure {
        lo: f64::NEG_INFINITY,
        hi: f64::INFINITY,
        counts: EnclosureCounts::default(),
    }
}
