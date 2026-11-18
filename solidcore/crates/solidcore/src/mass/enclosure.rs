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
//!   whole v span (`a` a knot, `b` its length) or the part up to the trim
//!   (`b(t) = v(t) - a`). On a cell of `(t, sigma)` the integrand `-(S_z - z0) (S_u x S_v)_z u'(t) b(t)` at
//!   `phi(t, sigma)` (`u' b` the Jacobian of `phi`; in this factored form no product cancels
//!   another, which an interval could not follow) is a Taylor model [1, 2]: its Taylor
//!   polynomial of degree `ORDER - 1` at the cell's middle, every coefficient enclosed at
//!   that point and the polynomial integrated exactly, and the Lagrange remainder from the
//!   coefficients of degree `ORDER` enclosed over the whole cell (rounded interval
//!   arithmetic, [`Ivl`]; the surface's partials over a box,
//!   [`nurbs_partials_within_span_centred`]: its rational quotient taken about the box's
//!   middle point, so a face's distance from the origin does not widen them).
//!   A cell is first evaluated at the low degree [`FIRST_ORDER`] and at `ORDER` only once it
//!   is the widest. The expansion holds where the cell's image lies on one polynomial piece
//!   of the surface; a cell whose image may cross a knot line is enclosed by the integrand's
//!   range over it times its area [3, ch. 9], which holds whatever the continuity there.
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
//! ([`volume_sign`]), or until [`CELL_BUDGET`] cells are spent or no cell is left to cut
//! ([`EnclosureStop`]); it answers the intersection of every enclosure it summed.
//!
//! **Moments and area** ([`enclose_quantity`], [`enclose_mass_properties`],
//! [`enclose_face_area`] for some of the faces). The same cells
//! enclose `int X^a Y^b Z^c dV` (`X = x - x0`, and so on, about the bounding box's middle
//! `(x0, y0, z0)`, `a + b + c <= 2`): the flux of `F = (0, 0, Phi)` with
//! `Phi = X^a Y^b Z^(c+1) / (c+1)` (`dPhi/dz` the weight), so the integrand is
//! `-Phi(S) (S_u x S_v)_z u' b` and a strip's or cap's flux is at most `max |Phi|` over it
//! times its area; the volume is `a = b = c = 0`. A face's area is `int |S_u x S_v|` over its
//! trimmed domain: the same sweep cells with the integrand `-|S_u x S_v| u' b`, its square
//! root a series `sqrt(f0) sum binom(1/2, k) h^k`, `h = (f - f0) / f0` (exact in degree for
//! a series with no constant term), where `f0` is proved positive over the cell, else the
//! cell's range. A face's cells sum to its area times the winding of its loops as walked
//! (+1 or -1 throughout), so each face's sum enters by its magnitude; the gaps between faces
//! do not enter an area.
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

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};

use crate::blend::fillet_section::certificate::{
    curve_derivs_over, nurbs_partials_within_span_centred, partials_hull, NurbsCurve,
};
use crate::blend::fillet_section::interval::Ivl;
use crate::blend::fillet_section::series::V3;
use crate::blend::fillet_section::station::{as_b_spline, knot, span, Nurbs, Partials};
use crate::kernel::Kernel;
use crate::mass::integrator::properties::region_material;
use crate::math::Point3;
use crate::nurbs::b_spline_curve::p1::BSplineCurve;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::query::brep::{self as qbrep, BoundingBoxOptions};
use crate::topology::{Brep, EdgeId, EdgeuseId, FaceId};
use crate::{Result, SolidError};

const OP: &str = "mass::enclosure";

/// The degree of a cell's remainder coefficients: its Taylor polynomial has degree
/// `ORDER - 1`, and the remainder shrinks with the cell's size to the power `ORDER + 1`.
pub const ORDER: usize = 8;

/// The remainder degree a cell is first evaluated at: low, so a body that decides at once
/// (most do) costs little; a cell is evaluated again at [`ORDER`] when it is the widest,
/// before it is cut.
pub const FIRST_ORDER: usize = 3;

/// The cells past which an enclosure is no longer refined (the starting cells, one or a
/// few per trim piece and edge, and the last bisection's two come on top). A bound on the
/// work of one decision, not fitted to a case: the rig's slowest decision takes about 10^5.
pub const CELL_BUDGET: usize = 1 << 17;

/// The most bisections from a starting cell to one of its cells: past it a cell is kept
/// as it is (its parameters are then within a few roundings of each other).
pub const MAX_DEPTH: u32 = 60;

/// A cell's series are needed one order past `ORDER`: the integrand is built from `X_t` and
/// `X_sigma`, each one order below `X`.
const XORD: usize = ORDER + 1;
/// The coefficients of a series in two variables of total degree at most `XORD`.
const LEN: usize = (XORD + 1) * (XORD + 2) / 2;

const IZ: Ivl = Ivl::new(0.0, 0.0);

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

/// Why refinement ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnclosureStop {
    /// The enclosure is as narrow as asked ([`enclose_volume`]) or decides the sign
    /// ([`volume_sign`]).
    Reached,
    /// The sign is not decided, and the settled cells and fixed terms already hold 0
    /// whatever the rest becomes ([`volume_sign`] only).
    Settled,
    /// [`CELL_BUDGET`] cells are spent.
    Budget,
    /// No cell is left that a cut narrows: every cell is settled, cut [`MAX_DEPTH`] times,
    /// or of single parameters.
    Exhausted,
}

/// A certified enclosure of a body's volume: the volume is in `[lo, hi]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VolumeEnclosure {
    /// The lower end.
    pub lo: f64,
    /// The upper end.
    pub hi: f64,
    /// The width of the part of the last enclosure summed that no cut narrows (the settled
    /// strip cells and the fixed terms), rounded up: refining does not take that one
    /// enclosure below it. The result can still be narrower, since it intersects the
    /// earlier enclosures too, and can then end at [`EnclosureStop::Reached`] for a width
    /// below the floor; otherwise a width asked below it ends at [`EnclosureStop::Budget`] or
    /// [`EnclosureStop::Exhausted`].
    pub floor: f64,
    /// Why refinement ended.
    pub stop: EnclosureStop,
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
    /// The enclosure still holds 0 when refinement ends: the cells that no cut narrows (gaps
    /// between faces) already hold it whatever the rest becomes, [`CELL_BUDGET`] cells are
    /// spent, or every cell is cut [`MAX_DEPTH`] times.
    Undecided(VolumeEnclosure),
}

/// An enclosure of the volume of the closed body `b` at most `width` wide
/// ([`EnclosureStop::Reached`]), or else the narrowest refinement gives within
/// [`CELL_BUDGET`] cells ([`EnclosureStop::Budget`]) or once no cut narrows a cell
/// ([`EnclosureStop::Exhausted`]). It is refined whatever `width` is: a width below the
/// enclosure's [`VolumeEnclosure::floor`] is not reached, and refining stops only at one of
/// those ends. The result is the intersection of every enclosure summed along the way (each
/// holds the volume), so a tighter `width` is never answered wider by further refinement.
/// Missing UV trim curves are made on `b`, as
/// [`super::integrator::properties::brep_volume_default`] makes them.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a `width` that is not positive, a face whose sides are
/// not one material region and one void, an edge whose face sides do not pair in opposite
/// senses, two pcurves of an edge that share no parameters, or a loop's end that is not
/// finite; [`SolidError::Unsupported`] for a surface or pcurve with no NURBS form, a face
/// without trim loops, weights not proved positive at a point or along a trim, or a
/// parameter box that is not finite; [`SolidError::UnsureRegion`];
/// [`SolidError::Cancelled`]; those of the evaluations.
pub fn enclose_volume(k: &Kernel, b: &mut Brep, width: f64) -> Result<VolumeEnclosure> {
    if width.partial_cmp(&0.0) != Some(core::cmp::Ordering::Greater) {
        return Err(SolidError::invalid_input(OP, "width not positive"));
    }
    let mut w = Work::new(k, b)?;
    // refined toward `width` whatever the floor: the floor only says where it ends
    w.refine(|e| e.hi - e.lo <= width, |_, _| false)
}

/// What [`enclose_quantity`] encloses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quantity {
    /// `int X^x Y^y Z^z dV` over the body, `X = x - x0` and so on about the middle
    /// `(x0, y0, z0)` of its bounding box ([`moment_centre`]), of total degree at most 2:
    /// `(0, 0, 0)` is the volume.
    Moment {
        /// The power of `X`.
        x: u8,
        /// The power of `Y`.
        y: u8,
        /// The power of `Z`.
        z: u8,
    },
    /// The total area of the body's faces.
    Area,
}

/// The point [`Quantity::Moment`] is taken about: the middle of the body's bounding box (as
/// [`super::integrator::properties::brep_mass_properties`] integrates about it).
///
/// # Errors
///
/// Those of the bounding box.
pub fn moment_centre(k: &Kernel, b: &mut Brep) -> Result<Point3> {
    Ok(qbrep::calculate_bounding_box(k, b, BoundingBoxOptions::default())?.mid())
}

/// An enclosure of `quantity` of the closed body `b` at most `width` wide, as
/// [`enclose_volume`] refines it (the [`VolumeEnclosure`] then holds that quantity, not
/// the volume, unless it is the volume). [`Quantity::Area`] also takes a sheet body: a
/// face void on both sides has an area, though it bounds no volume or moment.
///
/// # Errors
///
/// Those of [`enclose_volume`]; [`SolidError::InvalidInput`] for a moment of total degree
/// above 2.
pub fn enclose_quantity(
    k: &Kernel,
    b: &mut Brep,
    quantity: Quantity,
    width: f64,
) -> Result<VolumeEnclosure> {
    if width.partial_cmp(&0.0) != Some(core::cmp::Ordering::Greater) {
        return Err(SolidError::invalid_input(OP, "width not positive"));
    }
    if let Quantity::Moment { x, y, z } = quantity {
        if u16::from(x) + u16::from(y) + u16::from(z) > 2 {
            return Err(SolidError::invalid_input(OP, "a moment above degree 2"));
        }
    }
    let mut w = Work::with(k, b, quantity)?;
    w.refine(|e| e.hi - e.lo <= width, |_, _| false)
}

/// An enclosure of the total area of the faces `faces` of `b` at most `width` wide, as
/// [`enclose_quantity`] refines [`Quantity::Area`] (an area is its faces' alone, so any set
/// of faces has one; a face listed twice counts twice). A sheet's face, void on both sides,
/// is measured from its upward use; its area, like any face's, is positive whichever of its
/// uses is upward.
///
/// # Errors
///
/// Those of [`enclose_quantity`]; [`SolidError::InvalidInput`] for an empty `faces`.
pub fn enclose_face_area(
    k: &Kernel,
    b: &mut Brep,
    faces: &[FaceId],
    width: f64,
) -> Result<VolumeEnclosure> {
    if width.partial_cmp(&0.0) != Some(core::cmp::Ordering::Greater) {
        return Err(SolidError::invalid_input(OP, "width not positive"));
    }
    if faces.is_empty() {
        return Err(SolidError::invalid_input(OP, "no faces"));
    }
    let mut w = Work::over(k, b, Quantity::Area, Some(faces))?;
    w.refine(|e| e.hi - e.lo <= width, |_, _| false)
}

/// An enclosure of the total area of the faces `faces` of `b` at most `rel` of its own
/// magnitude wide ([`EnclosureStop::Reached`]), or else the narrowest one refinement gives
/// ([`EnclosureStop::Budget`], [`EnclosureStop::Exhausted`]), as [`enclose_face_area`]
/// refines it. The width asked follows the enclosure as it narrows, in one refinement: no
/// rough first enclosure is needed to size it, and no restart re-evaluates an earlier
/// run's cells. An unbounded enclosure never counts as reached.
///
/// # Errors
///
/// Those of [`enclose_face_area`]; [`SolidError::InvalidInput`] for a `rel` that is not
/// positive.
pub fn enclose_face_area_relative(
    k: &Kernel,
    b: &mut Brep,
    faces: &[FaceId],
    rel: f64,
) -> Result<VolumeEnclosure> {
    if rel.partial_cmp(&0.0) != Some(core::cmp::Ordering::Greater) {
        return Err(SolidError::invalid_input(OP, "rel not positive"));
    }
    if faces.is_empty() {
        return Err(SolidError::invalid_input(OP, "no faces"));
    }
    let mut w = Work::over(k, b, Quantity::Area, Some(faces))?;
    w.refine(within(rel), |_, _| false)
}

/// Whether an enclosure is finite and at most `rel` of its own magnitude wide (false for
/// infinite or NaN ends: `inf <= rel * inf` would hold, so an unbounded enclosure would
/// count as reached before any cell is cut; refinement goes on instead, to its budget).
fn within(rel: f64) -> impl Fn(Ivl) -> bool {
    move |e: Ivl| {
        let width = e.hi - e.lo;
        width.is_finite() && width <= rel * e.mag()
    }
}

/// A certified enclosure `[lo, hi]` of one mass property.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    /// The lower end.
    pub lo: f64,
    /// The upper end.
    pub hi: f64,
}

impl Bounds {
    fn of(x: Ivl) -> Self {
        Self { lo: x.lo, hi: x.hi }
    }

    /// Whether `x` lies in it.
    #[must_use]
    pub fn contains(&self, x: f64) -> bool {
        self.lo <= x && x <= self.hi
    }

    /// `hi - lo`.
    #[must_use]
    pub fn width(&self) -> f64 {
        self.hi - self.lo
    }
}

/// Certified enclosures of the mass properties
/// [`super::integrator::properties::brep_mass_properties`] estimates, at density 1 and
/// about the world origin: each value is in its bounds.
#[derive(Clone, Debug, PartialEq)]
pub struct MassEnclosure {
    /// The total area of the faces.
    pub area: Bounds,
    /// The volume (the mass at density 1).
    pub volume: Bounds,
    /// The centre of mass.
    pub centroid: [Bounds; 3],
    /// The axial inertias `int (y^2 + z^2)`, `int (z^2 + x^2)`, `int (x^2 + y^2)` about the
    /// origin.
    pub inertia: [Bounds; 3],
    /// The products `int yz`, `int zx`, `int xy` about the origin.
    pub products: [Bounds; 3],
    /// The point the moments were enclosed about ([`moment_centre`]).
    pub centre: Point3,
    /// The first enclosure that did not reach its width (its quantity and why), if any: the
    /// bounds still hold, wider than asked.
    pub short: Option<(Quantity, EnclosureStop)>,
    /// Cells evaluated over every quantity.
    pub cells: usize,
}

/// [`MassEnclosure`] of the closed body `b`: the area, the volume and each moment about
/// [`moment_centre`] enclosed ([`enclose_quantity`]) at most `rel` of its scale wide (the
/// area's, the volume's, and the volume times the box's half diagonal to the moment's
/// degree), then shifted to the origin in interval arithmetic. The area stage takes a
/// sheet's faces too ([`enclose_quantity`]); the volume stage still refuses them.
///
/// # Errors
///
/// Those of [`enclose_quantity`]; [`SolidError::InvalidInput`] for a `rel` that is not
/// positive, or a volume not proved positive.
pub fn enclose_mass_properties(k: &Kernel, b: &mut Brep, rel: f64) -> Result<MassEnclosure> {
    if rel.partial_cmp(&0.0) != Some(core::cmp::Ordering::Greater) {
        return Err(SolidError::invalid_input(OP, "rel not positive"));
    }
    let bbox = qbrep::calculate_bounding_box(k, &mut *b, BoundingBoxOptions::default())?;
    let c = bbox.mid();
    let half = 0.5 * bbox.size().length();
    let mut short = None;
    let mut cells = 0;
    // `width`: an absolute width; `None`: `rel` of the quantity's own magnitude, the width
    // asked following the enclosure as it narrows in one refinement (no rough first
    // enclosure sizes it, and no restart re-evaluates an earlier run's cells)
    let mut run = |b: &mut Brep, q: Quantity, width: Option<f64>| -> Result<(Ivl, EnclosureStop)> {
        let e = match width {
            Some(width) => enclose_quantity(k, b, q, width)?,
            None => Work::with(k, b, q)?.refine(within(rel), |_, _| false)?,
        };
        cells += e.counts.cells;
        Ok((Ivl::new(e.lo, e.hi), e.stop))
    };
    let mut note = |q: Quantity, stop: EnclosureStop| {
        if stop != EnclosureStop::Reached && short.is_none() {
            short = Some((q, stop));
        }
    };
    let (area, stop) = run(b, Quantity::Area, None)?;
    note(Quantity::Area, stop);
    let vol_q = Quantity::Moment { x: 0, y: 0, z: 0 };
    let (volume, stop) = run(b, vol_q, None)?;
    note(vol_q, stop);
    if volume.lo.partial_cmp(&0.0) != Some(core::cmp::Ordering::Greater) {
        return Err(SolidError::invalid_input(
            OP,
            "a volume not proved positive",
        ));
    }
    let scale = |d: usize| {
        let mut s = volume.hi;
        for _ in 0..d {
            s *= half;
        }
        rel * s
    };
    // the powers of X, Y, Z of a moment of the coordinates `axes`
    let powers = |axes: &[usize]| {
        let count = |a: usize| axes.iter().filter(|&&x| x == a).count() as u8;
        Quantity::Moment {
            x: count(0),
            y: count(1),
            z: count(2),
        }
    };
    let mut first = [IZ; 3];
    for (i, m) in first.iter_mut().enumerate() {
        let q = powers(&[i]);
        let (e, stop) = run(b, q, Some(scale(1)))?;
        note(q, stop);
        *m = e;
    }
    // second moments: xx, yy, zz, yz, zx, xy
    let mut second = [[IZ; 3]; 3];
    for (i, j) in [(0, 0), (1, 1), (2, 2), (1, 2), (2, 0), (0, 1)] {
        let q = powers(&[i, j]);
        let (e, stop) = run(b, q, Some(scale(2)))?;
        note(q, stop);
        set2(&mut second, i, j, e);
        set2(&mut second, j, i, e);
    }
    let cv = [Ivl::from(c.x), Ivl::from(c.y), Ivl::from(c.z)];
    let at = |v: &[Ivl; 3], i: usize| v.get(i).copied().unwrap_or(Ivl::whole());
    let at2 = |i: usize, j: usize| {
        second
            .get(i)
            .and_then(|r| r.get(j))
            .copied()
            .unwrap_or(Ivl::whole())
    };
    // about the origin: int x_i x_j = int X_i X_j + c_i int X_j + c_j int X_i + c_i c_j V
    let raw = |i: usize, j: usize| {
        at2(i, j)
            + at(&cv, i) * at(&first, j)
            + at(&cv, j) * at(&first, i)
            + at(&cv, i) * at(&cv, j) * volume
    };
    let centroid = [0, 1, 2].map(|i| Bounds::of(at(&cv, i) + at(&first, i) / volume));
    let inertia = [
        raw(1, 1) + raw(2, 2),
        raw(2, 2) + raw(0, 0),
        raw(0, 0) + raw(1, 1),
    ]
    .map(Bounds::of);
    let products = [raw(1, 2), raw(2, 0), raw(0, 1)].map(Bounds::of);
    Ok(MassEnclosure {
        area: Bounds::of(area),
        volume: Bounds::of(volume),
        centroid,
        inertia,
        products,
        centre: c,
        short,
        cells,
    })
}

/// Sets `m[i][j]` where it exists.
fn set2(m: &mut [[Ivl; 3]; 3], i: usize, j: usize, x: Ivl) {
    if let Some(v) = m.get_mut(i).and_then(|r| r.get_mut(j)) {
        *v = x;
    }
}

/// The sign of the volume of the closed body `b`, from its enclosure refined until it
/// decides; [`VolumeSign::Undecided`] only when the enclosure still holds 0 when
/// refinement ends ([`VolumeSign::Undecided`] says when).
///
/// # Errors
///
/// Those of [`enclose_volume`].
pub fn volume_sign(k: &Kernel, b: &mut Brep) -> Result<VolumeSign> {
    // each decision a call of this scope (feature `profile`; nothing without it)
    crate::profile::profile_scope!(k, "mass::enclosure::volume_sign");
    volume_sign_within(k, b, CELL_BUDGET)
}

/// [`volume_sign`] within `budget` cells.
fn volume_sign_within(k: &Kernel, b: &mut Brep, budget: usize) -> Result<VolumeSign> {
    let mut w = Work::new(k, b)?;
    w.budget = budget;
    // the settled cells and the fixed terms are symmetric about 0: once their half width is
    // at least the magnitude of everything else, 0 stays inside however the rest is refined
    let e = w.refine(
        |e| e.lo > 0.0 || e.hi < 0.0,
        |rest, still| still >= rest.mag(),
    )?;
    Ok(if e.lo > 0.0 {
        VolumeSign::Positive(e)
    } else if e.hi < 0.0 {
        VolumeSign::Negative(e)
    } else {
        VolumeSign::Undecided(e)
    })
}

// ---- series in two variables ----------------------------------------------------------

/// The index of `x^i y^j` in a [`Bs`].
const fn ix(i: usize, j: usize) -> usize {
    let d = i + j;
    d * (d + 1) / 2 + j
}

/// A truncated power series in `x` (along `t`) and `y` (along `sigma`) with enclosed
/// coefficients, of total degree at most `XORD`.
#[derive(Clone, Copy, Debug)]
struct Bs {
    c: [Ivl; LEN],
}

impl Bs {
    const fn zero() -> Self {
        Self { c: [IZ; LEN] }
    }

    fn constant(a: Ivl) -> Self {
        let mut s = Self::zero();
        s.set(0, 0, a);
        s
    }

    fn at(&self, i: usize, j: usize) -> Ivl {
        if i + j > XORD {
            return IZ;
        }
        self.c.get(ix(i, j)).copied().unwrap_or(IZ)
    }

    fn set(&mut self, i: usize, j: usize, v: Ivl) {
        if i + j > XORD {
            return;
        }
        if let Some(c) = self.c.get_mut(ix(i, j)) {
            *c = v;
        }
    }

    fn add(&self, o: &Self) -> Self {
        let mut r = *self;
        for (a, b) in r.c.iter_mut().zip(&o.c) {
            if !is_zero(*b) {
                *a = *a + *b;
            }
        }
        r
    }

    fn sub(&self, o: &Self) -> Self {
        let mut r = *self;
        for (a, b) in r.c.iter_mut().zip(&o.c) {
            if !is_zero(*b) {
                *a = *a - *b;
            }
        }
        r
    }

    fn scale(&self, s: Ivl) -> Self {
        let mut r = *self;
        for a in &mut r.c {
            if !is_zero(*a) {
                *a = *a * s;
            }
        }
        r
    }

    /// The product, truncated at total degree `n`.
    fn mul(&self, o: &Self, n: usize) -> Self {
        let mut r = Self::zero();
        for d1 in 0..=n {
            for j1 in 0..=d1 {
                let a = self.at(d1 - j1, j1);
                if is_zero(a) {
                    continue;
                }
                for d2 in 0..=(n - d1) {
                    for j2 in 0..=d2 {
                        let b = o.at(d2 - j2, j2);
                        if is_zero(b) {
                            continue;
                        }
                        let (i, j) = (d1 - j1 + d2 - j2, j1 + j2);
                        r.set(i, j, r.at(i, j) + a * b);
                    }
                }
            }
        }
        r
    }

    /// `d/dx`, of total degree `n - 1` for a series of degree `n`.
    fn dx(&self, n: usize) -> Self {
        let mut r = Self::zero();
        for d in 1..=n {
            for j in 0..d {
                let i = d - j;
                let a = self.at(i, j);
                if !is_zero(a) {
                    r.set(i - 1, j, a * Ivl::from(i as f64));
                }
            }
        }
        r
    }
}

/// The point interval 0 (an exact zero coefficient, skipped by the products).
fn is_zero(a: Ivl) -> bool {
    a.lo == 0.0 && a.hi == 0.0
}

/// `1 / n!` enclosed.
fn inverse_factorial(n: usize) -> Ivl {
    let mut f = 1.0_f64;
    for i in 2..=n {
        f *= i as f64; // exact up to 18!
    }
    Ivl::from(1.0) / Ivl::from(f)
}

/// `X = sum P_ab / (a! b!) du^a dv^b`, `a + b <= n`: the surface composed with the cell's
/// parameters, `du`, `dv` their series without constant term and `P` the surface's
/// partials at the expansion point (or, for coefficients over a cell, over the box its
/// points map into).
fn compose(p: &Partials<Ivl>, du: &Bs, dv: &Bs, n: usize) -> [Bs; 3] {
    compose_partial(p, (0, 0), du, dv, n)
}

/// [`compose`] of the surface's partial `(da, db)` (`S_u` for `(1, 0)`): its Taylor
/// coefficients are the surface's partials shifted by that order.
fn compose_partial(
    p: &Partials<Ivl>,
    (da, db): (usize, usize),
    du: &Bs,
    dv: &Bs,
    n: usize,
) -> [Bs; 3] {
    let mut dv_pow = vec![Bs::constant(Ivl::from(1.0))];
    for b in 1..=n {
        let next = dv_pow.get(b - 1).map_or(Bs::zero(), |q| q.mul(dv, n));
        dv_pow.push(next);
    }
    let mut x = [Bs::zero(); 3];
    let mut du_pow = Bs::constant(Ivl::from(1.0));
    for a in 0..=n {
        let mut inner = [Bs::zero(); 3];
        for (b, q) in dv_pow.iter().enumerate().take(n - a + 1) {
            let f = inverse_factorial(a) * inverse_factorial(b);
            let pab = p.at(a + da, b + db);
            for (acc, c) in inner.iter_mut().zip([pab.x, pab.y, pab.z]) {
                if !is_zero(c) {
                    *acc = acc.add(&q.scale(c * f));
                }
            }
        }
        for (xc, ic) in x.iter_mut().zip(&inner) {
            *xc = xc.add(&du_pow.mul(ic, n));
        }
        du_pow = du_pow.mul(du, n);
    }
    x
}

// ---- what is integrated ----------------------------------------------------------------

/// The quantity and the point its moments are about.
#[derive(Clone, Copy, Debug)]
struct Weight {
    quantity: Quantity,
    centre: [f64; 3],
}

impl Weight {
    /// The factors of `Phi = X^a Y^b Z^(c+1) / (c+1)`: each coordinate's power, and the
    /// divisor `c + 1`.
    fn powers(&self) -> ([u8; 3], u8) {
        match self.quantity {
            Quantity::Moment { x, y, z } => ([x, y, z.saturating_add(1)], z.saturating_add(1)),
            Quantity::Area => ([0, 0, 1], 1),
        }
    }

    fn is_area(&self) -> bool {
        self.quantity == Quantity::Area
    }

    /// `Phi` of the centred coordinates `c`: the product of the factors in the order X, Y,
    /// Z (the volume's `Z` alone, unscaled), then divided by `c + 1` where that is not 1.
    fn phi(&self, c: [Ivl; 3]) -> Ivl {
        let (p, div) = self.powers();
        let mut out: Option<Ivl> = None;
        for (x, n) in c.into_iter().zip(p) {
            for _ in 0..n {
                out = Some(out.map_or(x, |o| o * x));
            }
        }
        let out = out.unwrap_or_else(|| Ivl::from(1.0));
        if div > 1 {
            out / Ivl::from(f64::from(div))
        } else {
            out
        }
    }

    /// [`Weight::phi`] of series (truncated at degree `n`).
    fn phi_series(&self, c: [&Bs; 3], n: usize) -> Bs {
        let (p, div) = self.powers();
        let mut out: Option<Bs> = None;
        for (x, k) in c.into_iter().zip(p) {
            for _ in 0..k {
                out = Some(out.map_or(*x, |o| o.mul(x, n)));
            }
        }
        let out = out.unwrap_or_else(|| Bs::constant(Ivl::from(1.0)));
        if div > 1 {
            out.scale(Ivl::from(1.0) / Ivl::from(f64::from(div)))
        } else {
            out
        }
    }

    /// The coordinates of a point less the centre, enclosed.
    fn centred(&self, s: V3<Ivl>) -> [Ivl; 3] {
        let [x0, y0, z0] = self.centre;
        [
            s.x - Ivl::from(x0),
            s.y - Ivl::from(y0),
            s.z - Ivl::from(z0),
        ]
    }

    /// The integrand `g` at partials `p` (order 1): `Phi(S) (S_u x S_v)_z` of a moment,
    /// `|S_u x S_v|` of the area.
    fn density(&self, p: &Partials<Ivl>) -> Ivl {
        let (s, su, sv) = (p.at(0, 0), p.at(1, 0), p.at(0, 1));
        let nz = su.x * sv.y - su.y * sv.x;
        if self.is_area() {
            let nx = su.y * sv.z - su.z * sv.y;
            let ny = su.z * sv.x - su.x * sv.z;
            (sq(nx) + sq(ny) + sq(nz)).sqrt()
        } else {
            self.phi(self.centred(s)) * nz
        }
    }
}

/// `x^2` over the interval `x`: from its least magnitude's square to its greatest's (a
/// product `x x` takes the ends apart, below 0 where `x` holds 0).
fn sq(x: Ivl) -> Ivl {
    if !x.is_finite() {
        return x * x;
    }
    let (lo, hi) = (Ivl::from(x.mig()), Ivl::from(x.mag()));
    Ivl::new((lo * lo).lo, (hi * hi).hi)
}

/// `sqrt(f)` as a series of degree `n`: `sqrt(f0)` for `n = 0` (of the non-negative part:
/// the root is of a sum of squares); else `sqrt(f0) sum binom(1/2, k) h^k`,
/// `h = (f - f0) / f0`, where `f0` is proved positive (`None` otherwise).
fn sqrt_series(f: &Bs, n: usize) -> Option<Bs> {
    let f0 = f.at(0, 0);
    if n == 0 {
        return (f0.hi >= 0.0).then(|| Bs::constant(f0.sqrt()));
    }
    if f0.lo.partial_cmp(&0.0) != Some(core::cmp::Ordering::Greater) || !f0.is_finite() {
        return None;
    }
    let mut h = f.sub(&Bs::constant(f0)).scale(Ivl::from(1.0) / f0);
    h.set(0, 0, IZ);
    let mut sum = Bs::constant(Ivl::from(1.0));
    let mut hk = Bs::constant(Ivl::from(1.0));
    let mut c = Ivl::from(1.0);
    for k in 1..=n {
        // binom(1/2, k) = binom(1/2, k - 1) (1/2 - (k - 1)) / k
        c = c * (Ivl::from(0.5) - Ivl::from((k - 1) as f64)) / Ivl::from(k as f64);
        hk = hk.mul(&h, n);
        sum = sum.add(&hk.scale(c));
    }
    Some(sum.scale(f0.sqrt()))
}

// ---- the body's parts ------------------------------------------------------------------

/// A face's surface and the levels its trims are swept down to.
struct FaceGeo {
    cn: Nurbs,
    /// The base `vb` (at most every trim's lowest v), then the surface's v knots above it
    /// and below the trims' highest v, ascending.
    levels: Vec<f64>,
    /// -1 where the face's material side is its upward face use's (`Same`), as the
    /// integrator signs it ([`super::integrator::properties::boundary_properties`]).
    sign: f64,
}

/// A pcurve piece of one face's loop, integrated from `t.0` to `t.1` along its parameter
/// and counted `dir` times (-1: the edge use runs against its curve).
struct Piece {
    face: usize,
    cn: NurbsCurve,
    span: usize,
    dir: f64,
}

/// An edge's two boundary curves on its faces, over their common parameters.
struct StripGeo {
    sides: [(usize, NurbsCurve); 2],
}

#[derive(Clone, Copy, Debug)]
enum Kind {
    /// A sweep of piece `piece` over the v span from `levels[j]` (the whole span when
    /// `whole`, else up to the trim).
    Sweep { piece: usize, j: usize, whole: bool },
    /// The gap strip of strip `strip`, on the polynomial pieces `spans` of its curves.
    Strip { strip: usize, spans: [usize; 2] },
}

#[derive(Clone, Copy, Debug)]
struct Item {
    kind: Kind,
    t: (f64, f64),
    s: (f64, f64),
    depth: u32,
    /// The remainder degree it is evaluated at ([`FIRST_ORDER`], then [`ORDER`]).
    order: usize,
    /// A strip cell whose width is mostly a gap no cut removes ([`Work::strip_cell`]): kept
    /// as it is.
    settled: bool,
    enc: Ivl,
    split_t: bool,
}

/// The enclosure under construction.
struct Work<'a> {
    k: &'a Kernel,
    z0: f64,
    /// What is integrated.
    weight: Weight,
    faces: Vec<FaceGeo>,
    pieces: Vec<Piece>,
    strips: Vec<StripGeo>,
    items: Vec<Item>,
    alive: Vec<bool>,
    heap: BinaryHeap<(u64, Reverse<usize>)>,
    /// The caps' and the loops' rounding terms, `[-w, w]`.
    fixed: Ivl,
    /// The cells past which it is no longer refined ([`CELL_BUDGET`]).
    budget: usize,
    counts: EnclosureCounts,
}

/// The polynomial pieces of a curve: `(span, start, end)` for every non-empty knot span.
fn curve_spans(cn: &NurbsCurve) -> Vec<(usize, f64, f64)> {
    let mut out = Vec::new();
    for k in cn.p..cn.n {
        let (a, b) = (knot(&cn.u, k as isize), knot(&cn.u, k as isize + 1));
        if a < b {
            out.push((k, a, b));
        }
    }
    out
}

/// The control points' range in u and v of the curve's polynomial piece `span` (where the
/// piece lies: its weights are positive).
fn piece_hull(cn: &NurbsCurve, span: usize) -> (Ivl, Ivl) {
    let mut u = Ivl::new(f64::INFINITY, f64::NEG_INFINITY);
    let mut v = u;
    for i in (span + 1).saturating_sub(cn.p + 1)..=span {
        let at = |c: usize| cn.pw.get(4 * i + c).copied().unwrap_or(f64::NAN);
        let w = at(3);
        // a point x w / w, widened by its rounding where the weight is not 1
        let point = |x: f64| {
            if w == 1.0 {
                Ivl::from(x)
            } else {
                Ivl::from(x) / Ivl::from(w)
            }
        };
        let (p, q) = (point(at(0)), point(at(1)));
        u = u.hull(p);
        v = v.hull(q);
    }
    (u, v)
}

/// Whether `u` is constant on the curve's polynomial piece `span`, exactly: the control points
/// that define it have one weight `w` and one `u w`, so on it `u = (u w) / w` everywhere (the
/// basis functions sum to 1). Such a piece (a seam's trim, a closing segment along v) has
/// `u' = 0`, and each of its cells' integrand carries the factor `u'(t)`: its flux is exactly 0.
fn constant_u(cn: &NurbsCurve, span: usize) -> bool {
    let point = |i: usize| {
        let at = |c: usize| cn.pw.get(4 * i + c).map(|x| x.to_bits());
        (at(0), at(3))
    };
    let first = point((span + 1).saturating_sub(cn.p + 1));
    first.0.is_some()
        && first.1.is_some()
        && ((span + 1).saturating_sub(cn.p + 1)..=span).all(|i| point(i) == first)
}

/// The closed parameter range of span `k` of `knots` (degree `p`, `n` control points), the
/// end spans continued without end.
fn span_range(knots: &[f64], p: usize, n: usize, k: usize) -> (f64, f64) {
    let lo = if k <= p {
        f64::NEG_INFINITY
    } else {
        knot(knots, k as isize)
    };
    let hi = if k + 1 >= n {
        f64::INFINITY
    } else {
        knot(knots, k as isize + 1)
    };
    (lo, hi)
}

/// The surface's piece `(ku, kv)` whose closed span holds the box, if one does.
fn piece_of(cn: &Nurbs, u: Ivl, v: Ivl) -> Option<(usize, usize)> {
    let ku = span(&cn.u, cn.p, cn.nu, u.mid());
    let kv = span(&cn.v, cn.q, cn.nv, v.mid());
    let (ua, ub) = span_range(&cn.u, cn.p, cn.nu, ku);
    let (va, vb) = span_range(&cn.v, cn.q, cn.nv, kv);
    let inside = u.lo >= ua && u.hi <= ub && v.lo >= va && v.hi <= vb;
    inside.then_some((ku, kv))
}

/// Whether an interior knot of `degree + 1` or more repeats (where the surface may not be
/// continuous) lies in `x`, its ends included.
fn breaks_within(knots: &[f64], degree: usize, x: Ivl) -> bool {
    let (Some(&first), Some(&last)) = (knots.first(), knots.last()) else {
        return false;
    };
    knots.chunk_by(|a, b| a == b).any(|run| {
        run.len() > degree
            && run
                .first()
                .is_some_and(|&k| first < k && k < last && x.lo <= k && k <= x.hi)
    })
}

/// The partials of piece `(ku, kv)` over the box ([`nurbs_partials_within_span_centred`]:
/// the quotient taken about the box's middle point, not the origin, so on a rational face
/// far from the origin they are of the box's size), or `None` where its weights are not
/// proved positive over it (its one failure: a box too large for the interval evaluation,
/// which a smaller one proves; a cell answers it by refining).
fn piece_partials(
    cn: &Nurbs,
    (ku, kv): (usize, usize),
    u: Ivl,
    v: Ivl,
    lk: usize,
) -> Option<Partials<Ivl>> {
    nurbs_partials_within_span_centred(cn, ku, kv, u.lo, u.hi, v.lo, v.hi, lk).ok()
}

/// The surface's partials up to order `lk` at every point of the box, from every piece it
/// meets, hulled; `None` where a piece's weights are not proved positive over it
/// ([`piece_partials`]).
pub(crate) fn partials_over(
    cn: &Nurbs,
    u: Ivl,
    v: Ivl,
    lk: usize,
) -> Result<Option<Partials<Ivl>>> {
    let bad = || SolidError::Unsupported {
        operation: OP,
        reason: "a parameter box that is not finite",
    };
    if !(u.lo.is_finite() && u.hi.is_finite() && v.lo.is_finite() && v.hi.is_finite()) {
        return Err(bad());
    }
    let (ku0, ku1) = (
        span(&cn.u, cn.p, cn.nu, u.lo),
        span(&cn.u, cn.p, cn.nu, u.hi),
    );
    let (kv0, kv1) = (
        span(&cn.v, cn.q, cn.nv, v.lo),
        span(&cn.v, cn.q, cn.nv, v.hi),
    );
    let mut out: Option<Partials<Ivl>> = None;
    for ku in ku0..=ku1 {
        let (ua, ub) = span_range(&cn.u, cn.p, cn.nu, ku);
        let (a0, a1) = (u.lo.max(ua), u.hi.min(ub));
        if a0
            .partial_cmp(&a1)
            .is_none_or(|o| o == core::cmp::Ordering::Greater)
            || (ku > cn.p
                && ku + 1 < cn.nu
                && ua.partial_cmp(&ub) != Some(core::cmp::Ordering::Less))
        {
            continue;
        }
        for kv in kv0..=kv1 {
            let (va, vb) = span_range(&cn.v, cn.q, cn.nv, kv);
            let (b0, b1) = (v.lo.max(va), v.hi.min(vb));
            if b0
                .partial_cmp(&b1)
                .is_none_or(|o| o == core::cmp::Ordering::Greater)
                || (kv > cn.q
                    && kv + 1 < cn.nv
                    && va.partial_cmp(&vb) != Some(core::cmp::Ordering::Less))
            {
                continue;
            }
            let Some(p) = piece_partials(cn, (ku, kv), Ivl::new(a0, a1), Ivl::new(b0, b1), lk)
            else {
                return Ok(None);
            };
            out = Some(match out {
                None => p,
                Some(h) => partials_hull(h, &p, lk),
            });
        }
    }
    out.map(Some).ok_or_else(bad)
}

/// [`partials_over`] where a point's box must be proved: the box is a point's enclosure,
/// within a few roundings of it.
fn partials_at(cn: &Nurbs, u: Ivl, v: Ivl, lk: usize) -> Result<Partials<Ivl>> {
    partials_over(cn, u, v, lk)?.ok_or(SolidError::Unsupported {
        operation: OP,
        reason: "a surface whose weights are not proved positive at a point",
    })
}

/// A bound on `int |g(u, s)| ds` from `a` to `b` for every `u` in `u`, `g` the weight's
/// integrand ([`Weight::density`]: `(S_z - z0) (S_u x S_v)_z` for the volume): the interval
/// cut in halves (at most 2^16 parts) until each part's partials are proved.
fn g_integral(cn: &Nurbs, u: Ivl, a: f64, b: f64, weight: &Weight) -> Result<f64> {
    let (a, b) = (a.min(b), a.max(b));
    let mut sum = Ivl::from(0.0);
    let mut stack = vec![(a, b, 0u32)];
    while let Some((s0, s1, depth)) = stack.pop() {
        if let Some(p) = partials_over(cn, u, Ivl::new(s0, s1), 1)? {
            let g = weight.density(&p);
            sum = sum + Ivl::from(g.mag()) * (Ivl::from(s1) - Ivl::from(s0));
            continue;
        }
        let m = 0.5 * (s0 + s1);
        if depth >= 16 || !(s0 < m && m < s1) {
            return Err(SolidError::Unsupported {
                operation: OP,
                reason: "a surface whose weights are not proved positive along a trim",
            });
        }
        stack.push((m, s1, depth + 1));
        stack.push((s0, m, depth + 1));
    }
    Ok(sum.hi)
}

/// The largest magnitude of a vector enclosure's length, bounded by its components' sum.
fn mag3(v: [Ivl; 3]) -> f64 {
    v.iter()
        .fold(Ivl::from(0.0), |sum, c| sum + Ivl::from(c.mag()))
        .hi
}

/// `hi - lo` of an interval, rounded up: a bound on its width.
fn width(x: Ivl) -> f64 {
    (Ivl::from(x.hi) - Ivl::from(x.lo)).hi
}

/// `int |x|^i` over `[xl, xh]` (`xl <= 0 <= xh`), enclosed: also `int x^i` for even `i`.
fn abs_moment(xl: Ivl, xh: Ivl, i: usize) -> Ivl {
    let mut a = Ivl::from(1.0);
    let mut b = Ivl::from(1.0);
    let nl = Ivl::from(0.0) - xl;
    for _ in 0..=i {
        a = a * xh;
        b = b * nl;
    }
    (a + b) / Ivl::from((i + 1) as f64)
}

/// `int x^i` over `[xl, xh]`, enclosed.
fn moment(xl: Ivl, xh: Ivl, i: usize) -> Ivl {
    let mut a = Ivl::from(1.0);
    let mut b = Ivl::from(1.0);
    for _ in 0..=i {
        a = a * xh;
        b = b * xl;
    }
    (a - b) / Ivl::from((i + 1) as f64)
}

/// The series of a curve's parameter `c` (0: u, 1: v) about a point (`over` false, the
/// coefficients at `t`) or every point of `t` (`over` true), from its derivatives, without
/// its constant term (returned apart).
fn curve_series(d: &[V3<Ivl>], c: usize, n: usize) -> (Ivl, Bs) {
    let pick = |v: &V3<Ivl>| if c == 0 { v.x } else { v.y };
    let mut s = Bs::zero();
    for kk in 1..=n {
        if let Some(v) = d.get(kk) {
            s.set(kk, 0, pick(v) * inverse_factorial(kk));
        }
    }
    (d.first().map_or(Ivl::whole(), pick), s)
}

impl<'a> Work<'a> {
    fn new(k: &'a Kernel, b: &mut Brep) -> Result<Self> {
        Self::with(k, b, Quantity::Moment { x: 0, y: 0, z: 0 })
    }

    fn with(k: &'a Kernel, b: &mut Brep, quantity: Quantity) -> Result<Self> {
        Self::over(k, b, quantity, None)
    }

    /// The work for `quantity` over every face of `b`, or over `only` (an area's faces;
    /// the caller checks the quantity is an area).
    fn over(
        k: &'a Kernel,
        b: &mut Brep,
        quantity: Quantity,
        only: Option<&[FaceId]>,
    ) -> Result<Self> {
        let bbox = qbrep::calculate_bounding_box(k, &mut *b, BoundingBoxOptions::default())?;
        let mid = bbox.mid();
        let z0 = mid.z;
        let weight = Weight {
            quantity,
            centre: [mid.x, mid.y, mid.z],
        };
        let mut w = Self {
            k,
            z0,
            weight,
            faces: Vec::new(),
            pieces: Vec::new(),
            strips: Vec::new(),
            items: Vec::new(),
            alive: Vec::new(),
            heap: BinaryHeap::new(),
            fixed: IZ,
            budget: CELL_BUDGET,
            counts: EnclosureCounts::default(),
        };
        let mut sides: BTreeMap<EdgeId, Vec<(usize, NurbsCurve, bool)>> = BTreeMap::new();
        // the closing pieces' and rounding gaps' lengths in 3D, and the greatest distance
        // of each coordinate from the centre's near them
        let mut open_length = Ivl::from(0.0);
        let mut zcap = [0.0_f64; 3];
        let faces = match only {
            Some(only) => only.to_vec(),
            None => crate::topology::brep::p1::faces(b, None)?,
        };
        for f in faces {
            k.check_cancelled(OP)?;
            w.add_face(k, b, f, &mut sides, &mut open_length, &mut zcap)?;
        }
        if w.weight.is_area() {
            // an area is the faces' alone: no strips or caps close a surface
            return Ok(w);
        }
        // each edge's face sides in pairs of opposite sense along it, a strip for each pair
        // (two sides at a manifold edge; four where solids touch along it)
        for (_, s) in sides {
            let (forward, backward): (Vec<_>, Vec<_>) = s.into_iter().partition(|x| x.2);
            if forward.len() != backward.len() || forward.is_empty() {
                return Err(SolidError::invalid_input(
                    OP,
                    "an edge whose face sides do not pair in opposite senses",
                ));
            }
            for (a, c) in forward.into_iter().zip(backward) {
                w.add_strip((a.0, a.1), (c.0, c.1), &mut open_length, &mut zcap)?;
            }
        }
        // the caps: closed curves of total length L, each within L of the points near it
        // (each coordinate within its greatest distance plus L of the centre's: `|Phi|` at
        // most its magnitude over that box; the volume's `|z - z0| + L`)
        let l = open_length.hi;
        let reach = zcap.map(|c| {
            let r = (Ivl::from(c) + Ivl::from(l)).hi;
            Ivl::new(-r, r)
        });
        let phi = w.weight.phi(reach).mag();
        let cap = Ivl::from(phi) * Ivl::from(l) * Ivl::from(l) / Ivl::from(4.0);
        w.fixed = w.fixed + Ivl::new(-cap.hi, cap.hi);
        Ok(w)
    }

    fn add_face(
        &mut self,
        k: &Kernel,
        b: &mut Brep,
        f: FaceId,
        sides: &mut BTreeMap<EdgeId, Vec<(usize, NurbsCurve, bool)>>,
        open_length: &mut Ivl,
        zcap: &mut [f64; 3],
    ) -> Result<()> {
        use crate::topology::edgeuse::p1 as eu1;
        let up = crate::topology::face::p2::upward_faceuse(b, f)?
            .ok_or_else(|| SolidError::mass_missing(OP, "the face's upward faceuse"))?;
        let mate = b
            .faceuse(up)?
            .mate
            .ok_or_else(|| SolidError::mass_missing(OP, "the faceuse's mate"))?;
        let material = |fu| -> Result<bool> {
            match eu1::faceuse_region(b, fu)? {
                Some(r) => region_material(b, r),
                None => Ok(false),
            }
        };
        let (mu, mm) = (material(up)?, material(mate)?);
        // a sheet's face (void on both sides) has an area but bounds no material: its
        // area is measured from its upward use (the integrand `|S_u x S_v|`, with the
        // loops walked as that use's orientation gives, is positive either way)
        let sheet_area = self.weight.is_area() && !mu && !mm;
        if mu == mm && !sheet_area {
            return Err(SolidError::invalid_input(
                OP,
                "a face whose sides are not one material region and one void",
            ));
        }
        let fu = if mu || sheet_area { up } else { mate };
        let sign = if b.faceuse(fu)?.orientation == OrientType::Same {
            -1.0
        } else {
            1.0
        };
        let surface = b
            .face(f)?
            .surface
            .as_ref()
            .ok_or_else(|| SolidError::mass_missing(OP, "the face's surface"))?;
        let cn = match as_b_spline(k, surface)? {
            Some(bs) => Nurbs::of(&bs)?,
            None => {
                return Err(SolidError::Unsupported {
                    operation: OP,
                    reason: "a surface with no NURBS form",
                })
            }
        };
        // the loops in the order and direction the integrator walks them
        // (`create_oriented_trim_boundary`): the upward use's loops, each edge use's
        // pcurve forward where it is `Same`, backward where `Opposite`
        let mut loops: Vec<Vec<(EdgeId, NurbsCurve, f64)>> = Vec::new();
        for lu in eu1::faceuse_loopuses(b, up, false, None)? {
            if crate::topology::loopuse::is_vertex_loopuse(b, lu)? {
                continue;
            }
            let mut lp = Vec::new();
            for eu in eu1::loopuse_edgeuses(b, lu)? {
                let dir = if eu1::orientation(b, eu)? == OrientType::Same {
                    1.0
                } else {
                    -1.0
                };
                let e = b
                    .edgeuse(eu)?
                    .edge
                    .ok_or_else(|| SolidError::mass_missing(OP, "the edgeuse's edge"))?;
                lp.push((e, pcurve(k, b, eu)?, dir));
            }
            loops.push(lp);
        }
        let face = self.faces.len();
        // the levels: the base below every trim, then the v knots between
        let (mut vlo, mut vhi) = (f64::INFINITY, f64::NEG_INFINITY);
        for (_, c, _) in loops.iter().flatten() {
            for (s, _, _) in curve_spans(c) {
                let (_, v) = piece_hull(c, s);
                vlo = vlo.min(v.lo);
                vhi = vhi.max(v.hi);
            }
        }
        if vlo
            .partial_cmp(&vhi)
            .is_none_or(|o| o == core::cmp::Ordering::Greater)
        {
            // a face with no trim loops is its whole surface (a closed one), which the
            // sweep down to a base does not cover
            return Err(SolidError::Unsupported {
                operation: OP,
                reason: "a face without trim loops",
            });
        }
        let mut levels = vec![vlo];
        for &x in &cn.v {
            if x > vlo && x < vhi && levels.last().is_some_and(|&l| x > l) {
                levels.push(x);
            }
        }
        self.faces.push(FaceGeo { cn, levels, sign });
        for lp in loops {
            // each curve's start and end point (as walked), enclosed
            let mut ends = Vec::new();
            for (_, c, dir) in &lp {
                let (t0, t1) = (knot(&c.u, c.p as isize), knot(&c.u, c.n as isize));
                let (a, z) = if *dir > 0.0 { (t0, t1) } else { (t1, t0) };
                let pa = curve_derivs_over(c, Ivl::from(a), 0, None)?;
                let pz = curve_derivs_over(c, Ivl::from(z), 0, None)?;
                let first = |d: &[V3<Ivl>]| {
                    d.first()
                        .copied()
                        .ok_or_else(|| SolidError::mass_failed(OP, "the curve has no point"))
                };
                ends.push((first(&pa)?, first(&pz)?));
            }
            let count = lp.len();
            for (i, (e, c, dir)) in lp.into_iter().enumerate() {
                // the segment from the previous curve's end to this one's start
                let before = if i == 0 {
                    count.saturating_sub(1)
                } else {
                    i - 1
                };
                let prev = ends.get(before).map(|x| x.1);
                let this = ends.get(i).map(|x| x.0);
                if let (Some(p), Some(q)) = (prev, this) {
                    let (a, z) = ([p.x.mid(), p.y.mid()], [q.x.mid(), q.y.mid()]);
                    // the rounding gaps between the exact ends and the segment's ends
                    for end in [p, q] {
                        self.rounding_gap(face, end, open_length, zcap)?;
                    }
                    if a != z {
                        let seg = segment(a, z)?;
                        let len = self.segment_length(face, &seg)?;
                        *open_length = *open_length + len;
                        self.add_piece(face, &seg, 1.0)?;
                    }
                }
                self.add_piece(face, &c, dir)?;
                // the side's sense along the edge's curve on the closed surface: its walk
                // times the face's sign
                let forward = dir * sign > 0.0;
                sides.entry(e).or_default().push((face, c, forward));
            }
        }
        Ok(())
    }

    /// The gap between a curve's exact end (in the box `end`) and the closing segment's end
    /// (its middle): its Green term is at most its extent in u times `int |g| dv` up from
    /// the base, its 3D length at most `|S_u| du + |S_v| dv`, both over the box.
    fn rounding_gap(
        &mut self,
        face: usize,
        end: V3<Ivl>,
        open_length: &mut Ivl,
        zcap: &mut [f64; 3],
    ) -> Result<()> {
        let fg = self
            .faces
            .get(face)
            .ok_or_else(|| SolidError::mass_failed(OP, "no such face"))?;
        let p = partials_at(&fg.cn, end.x, end.y, 1)?;
        let (s, su, sv) = (p.at(0, 0), p.at(1, 0), p.at(0, 1));
        let (du, dv) = (width(end.x), width(end.y));
        for (c, x) in zcap.iter_mut().zip(self.weight.centred(s)) {
            *c = c.max(x.mag());
        }
        *open_length = *open_length
            + Ivl::from(mag3([su.x, su.y, su.z])) * Ivl::from(du)
            + Ivl::from(mag3([sv.x, sv.y, sv.z])) * Ivl::from(dv);
        let base = fg.levels.first().copied().unwrap_or(end.y.lo);
        let g = g_integral(&fg.cn, end.x, base, end.y.hi, &self.weight)?;
        let green = Ivl::from(du) * Ivl::from(g);
        self.fixed = self.fixed + Ivl::new(-green.hi, green.hi);
        Ok(())
    }

    /// A bound on the 3D length of a closing segment on face `face`: along a side of the net
    /// whose control points are nearly one point (a pole), from their spread
    /// ([`side_length`]); else from the partials over its box.
    fn segment_length(&self, face: usize, seg: &NurbsCurve) -> Result<Ivl> {
        let fg = self
            .faces
            .get(face)
            .ok_or_else(|| SolidError::mass_failed(OP, "no such face"))?;
        let (u, v) = piece_hull(seg, seg.p);
        if let Some(len) = side_length(&fg.cn, u, v) {
            return Ok(len);
        }
        let p = partials_at(&fg.cn, u, v, 1)?;
        let (su, sv) = (p.at(1, 0), p.at(0, 1));
        Ok(Ivl::from(mag3([su.x, su.y, su.z])) * Ivl::from(width(u))
            + Ivl::from(mag3([sv.x, sv.y, sv.z])) * Ivl::from(width(v)))
    }

    /// Adds the cells of one pcurve on face `face`: each polynomial piece, cut where it may
    /// cross a knot line of the surface, swept down to the base.
    fn add_piece(&mut self, face: usize, cn: &NurbsCurve, dir: f64) -> Result<()> {
        let fg = self
            .faces
            .get(face)
            .ok_or_else(|| SolidError::mass_failed(OP, "no such face"))?;
        let mut lines_u: Vec<f64> = Vec::new();
        for &x in &fg.cn.u {
            if lines_u.last() != Some(&x) {
                lines_u.push(x);
            }
        }
        let levels = fg.levels.clone();
        let lines_v: Vec<f64> = levels.iter().skip(1).copied().collect();
        for (s, a, z) in curve_spans(cn) {
            // a piece of constant u has no flux (its cells' integrand carries u' = 0): no
            // cells, and the enclosure is that of the other pieces
            if constant_u(cn, s) {
                continue;
            }
            let mut cuts = vec![a, z];
            for (c, lines) in [(0usize, &lines_u), (1, &lines_v)] {
                for &line in lines {
                    crossings(cn, s, a, z, c, line, &mut cuts)?;
                }
            }
            cuts.sort_by(f64::total_cmp);
            cuts.dedup();
            let piece = self.pieces.len();
            self.pieces.push(Piece {
                face,
                cn: cn.clone(),
                span: s,
                dir,
            });
            for pair in cuts.windows(2) {
                let &[t0, t1] = pair else {
                    continue;
                };
                // the decomposition below the trim, by where its middle lies: the whole
                // spans under it and the part of the span it is in
                let mid = curve_derivs_over(cn, Ivl::from(0.5 * (t0 + t1)), 0, Some(s))?;
                let vc = mid.first().map_or(f64::NAN, |p| p.y.mid());
                let j_top = levels.iter().rposition(|&l| l <= vc).unwrap_or(0);
                for j in 0..=j_top {
                    self.push(Item {
                        kind: Kind::Sweep {
                            piece,
                            j,
                            whole: j < j_top,
                        },
                        t: (t0, t1),
                        s: (0.0, 1.0),
                        depth: 0,
                        order: FIRST_ORDER,
                        settled: false,
                        enc: IZ,
                        split_t: true,
                    })?;
                }
            }
        }
        Ok(())
    }

    fn add_strip(
        &mut self,
        a: (usize, NurbsCurve),
        c: (usize, NurbsCurve),
        open_length: &mut Ivl,
        zcap: &mut [f64; 3],
    ) -> Result<()> {
        let range = |cn: &NurbsCurve| (knot(&cn.u, cn.p as isize), knot(&cn.u, cn.n as isize));
        let (ra, rc) = (range(&a.1), range(&c.1));
        let (t0, t1) = (ra.0.max(rc.0), ra.1.min(rc.1));
        if t0.partial_cmp(&t1) != Some(core::cmp::Ordering::Less) {
            return Err(SolidError::invalid_input(
                OP,
                "an edge whose two pcurves share no parameters",
            ));
        }
        let strip = self.strips.len();
        self.strips.push(StripGeo { sides: [a, c] });
        // the parts of either curve past the common parameters, and the strip's two ends
        for side in 0..2 {
            let (lo, hi) = if side == 0 { ra } else { rc };
            for (x0, x1) in [(lo, t0), (t1, hi)] {
                if x0 < x1 {
                    let len = self.curve_length(strip, side, x0, x1)?;
                    *open_length = *open_length + len;
                }
            }
        }
        for t in [t0, t1] {
            let (x, y) = (self.point(strip, 0, t)?, self.point(strip, 1, t)?);
            let d = [y[0] - x[0], y[1] - x[1], y[2] - x[2]];
            *open_length = *open_length + Ivl::from(mag3(d));
            for ((c, xi), ci) in zcap.iter_mut().zip(x).zip(self.weight.centre) {
                *c = c.max((xi - Ivl::from(ci)).mag());
            }
        }
        // cut at both curves' knots
        let sg = self
            .strips
            .get(strip)
            .ok_or_else(|| SolidError::mass_failed(OP, "no such strip"))?;
        let mut cuts = vec![t0, t1];
        for (_, cn) in &sg.sides {
            for &x in &cn.u {
                if x > t0 && x < t1 {
                    cuts.push(x);
                }
            }
        }
        cuts.sort_by(f64::total_cmp);
        cuts.dedup();
        let spans_at = |t: f64| -> [usize; 2] {
            let s = |cn: &NurbsCurve| span(&cn.u, cn.p, cn.n, t);
            [s(&sg.sides[0].1), s(&sg.sides[1].1)]
        };
        let mut items = Vec::new();
        for pair in cuts.windows(2) {
            if let &[x0, x1] = pair {
                items.push(Item {
                    kind: Kind::Strip {
                        strip,
                        spans: spans_at(0.5 * (x0 + x1)),
                    },
                    t: (x0, x1),
                    s: (0.0, 1.0),
                    depth: 0,
                    order: FIRST_ORDER,
                    settled: false,
                    enc: IZ,
                    split_t: true,
                });
            }
        }
        for it in items {
            self.push(it)?;
        }
        Ok(())
    }

    /// The 3D point of a strip's side `side` at `t`, enclosed.
    fn point(&self, strip: usize, side: usize, t: f64) -> Result<[Ivl; 3]> {
        let (face, cn) = self.strip_side(strip, side)?;
        let d = curve_derivs_over(cn, Ivl::from(t), 0, None)?;
        let p = d
            .first()
            .copied()
            .ok_or_else(|| SolidError::mass_failed(OP, "the curve has no point"))?;
        let s = partials_at(&face.cn, p.x, p.y, 0)?.at(0, 0);
        Ok([s.x, s.y, s.z])
    }

    /// A bound on the 3D length of a strip side's curve over `[t0, t1]`.
    fn curve_length(&self, strip: usize, side: usize, t0: f64, t1: f64) -> Result<Ivl> {
        let (face, cn) = self.strip_side(strip, side)?;
        let d = curve_derivs_over(cn, Ivl::new(t0, t1), 1, None)?;
        let (p, dp) = (
            d.first()
                .copied()
                .ok_or_else(|| SolidError::mass_failed(OP, "the curve has no point"))?,
            d.get(1)
                .copied()
                .ok_or_else(|| SolidError::mass_failed(OP, "the curve has no derivative"))?,
        );
        let s = partials_at(&face.cn, p.x, p.y, 1)?;
        let (su, sv) = (s.at(1, 0), s.at(0, 1));
        let speed = Ivl::from(mag3([su.x, su.y, su.z])) * Ivl::from(dp.x.mag())
            + Ivl::from(mag3([sv.x, sv.y, sv.z])) * Ivl::from(dp.y.mag());
        Ok(speed * (Ivl::from(t1) - Ivl::from(t0)))
    }

    fn strip_side(&self, strip: usize, side: usize) -> Result<(&FaceGeo, &NurbsCurve)> {
        let (face, cn) = self
            .strips
            .get(strip)
            .and_then(|s| s.sides.get(side))
            .ok_or_else(|| SolidError::mass_failed(OP, "no such strip side"))?;
        let fg = self
            .faces
            .get(*face)
            .ok_or_else(|| SolidError::mass_failed(OP, "no such face"))?;
        Ok((fg, cn))
    }

    /// Evaluates an item and puts it on the work list.
    fn push(&mut self, mut it: Item) -> Result<()> {
        self.counts.cells += 1;
        if self.counts.cells % 64 == 0 {
            self.k.check_cancelled(OP)?;
        }
        let (enc, split_t) = match it.kind {
            Kind::Sweep { piece, j, whole } => {
                self.sweep_cell(piece, j, whole, it.t, it.s, it.order)?
            }
            Kind::Strip { strip, spans } => {
                self.counts.strips += 1;
                let (enc, settled) = self.strip_cell(strip, spans, it.t, it.order)?;
                it.settled = settled;
                (enc, true)
            }
        };
        it.enc = enc;
        it.split_t = split_t;
        let index = self.items.len();
        let width = enc.hi - enc.lo;
        let key = if width >= 0.0 { width } else { f64::INFINITY };
        self.items.push(it);
        self.alive.push(true);
        self.heap.push((key.to_bits(), Reverse(index)));
        Ok(())
    }

    /// A sweep cell's enclosure, signed, and whether it is to be cut along `t`.
    fn sweep_cell(
        &mut self,
        piece: usize,
        j: usize,
        whole: bool,
        t: (f64, f64),
        s: (f64, f64),
        ord: usize,
    ) -> Result<(Ivl, bool)> {
        let p = self
            .pieces
            .get(piece)
            .ok_or_else(|| SolidError::mass_failed(OP, "no such piece"))?;
        let fg = self
            .faces
            .get(p.face)
            .ok_or_else(|| SolidError::mass_failed(OP, "no such face"))?;
        let sign = Ivl::from(fg.sign * p.dir);
        let a = fg
            .levels
            .get(j)
            .copied()
            .ok_or_else(|| SolidError::mass_failed(OP, "no such level"))?;
        let next = fg.levels.get(j + 1).copied();
        let z0 = Ivl::from(self.z0);
        let (tc, sc) = (0.5 * (t.0 + t.1), 0.5 * (s.0 + s.1));
        let (xl, xh) = (
            Ivl::from(t.0) - Ivl::from(tc),
            Ivl::from(t.1) - Ivl::from(tc),
        );
        let (yl, yh) = (
            Ivl::from(s.0) - Ivl::from(sc),
            Ivl::from(s.1) - Ivl::from(sc),
        );
        let (hull_u, hull_v) = piece_hull(&p.cn, p.span);
        // the series of u and of b (v = a + sigma b) at `tt` (a point or the cell's t), and
        // the box the points map into
        let series = |tt: Ivl, sg: Ivl, n: usize| -> Result<(Bs, Bs, Ivl, Ivl)> {
            let d = curve_derivs_over(&p.cn, tt, n, Some(p.span))?;
            let (u0, du) = curve_series(&d, 0, n);
            let (v0, vs) = curve_series(&d, 1, n);
            let (b0, bs) = match (whole, next) {
                (true, Some(top)) => (Ivl::from(top) - Ivl::from(a), Bs::zero()),
                (true, None) => {
                    return Err(SolidError::mass_failed(
                        OP,
                        "no level above the whole piece",
                    ))
                }
                (false, _) => (v0 - Ivl::from(a), vs),
            };
            // dv = sg (b(x) - b0) + y b(x)
            let mut dv = bs.scale(sg);
            dv.set(0, 1, b0);
            for kk in 1..n {
                dv.set(kk, 1, bs.at(kk, 0));
            }
            // where the points lie: the enclosure, cut to the curve's hull
            let u = Ivl::new(u0.lo.max(hull_u.lo), u0.hi.min(hull_u.hi));
            let vb = Ivl::from(a) + sg * b0;
            let true_v = match (whole, next) {
                (true, Some(top)) => Ivl::new(a, top),
                _ => Ivl::new(a.min(hull_v.lo), a.max(hull_v.hi)),
            };
            let v = Ivl::new(vb.lo.max(true_v.lo), vb.hi.min(true_v.hi));
            Ok((du, dv, u, v))
        };
        // the integrand -(S_z - z0) (S_u x S_v)_z u'(t) b(t) as a series of degree `n` (the
        // Jacobian of phi is u' b): in this form no product cancels another, where the
        // equal form (X_t x X_sigma)_z of the composed surface does, and an interval keeps
        // a cancellation's whole width (from partials to order n + 1, du and dv to n + 1)
        // A moment's `Phi(S)` takes the place of `S_z - z0`. The area's `|S_u x S_v|` is the
        // root of a series ([`sqrt_series`]): `None` where its value is not proved positive
        // over the cell (a pole's), and the cell is then enclosed by its range.
        let weight = self.weight;
        let [x0, y0, _] = weight.centre;
        let integrand = |p: &Partials<Ivl>, du: &Bs, dv: &Bs, n: usize| -> Option<Bs> {
            let [xx, xy, xz] = compose_partial(p, (0, 0), du, dv, n);
            let [sux, suy, suz] = compose_partial(p, (1, 0), du, dv, n);
            let [svx, svy, svz] = compose_partial(p, (0, 1), du, dv, n);
            let normal = sux.mul(&svy, n).sub(&suy.mul(&svx, n));
            let g = if weight.is_area() {
                let nx = suy.mul(&svz, n).sub(&suz.mul(&svy, n));
                let ny = suz.mul(&svx, n).sub(&sux.mul(&svz, n));
                let mut nn = nx
                    .mul(&nx, n)
                    .add(&ny.mul(&ny, n))
                    .add(&normal.mul(&normal, n));
                // the value is a sum of squares: each square enclosed as one (`sq`), not as
                // a product, so a component crossing 0 over the cell adds no negative part
                nn.set(
                    0,
                    0,
                    sq(nx.at(0, 0)) + sq(ny.at(0, 0)) + sq(normal.at(0, 0)),
                );
                sqrt_series(&nn, n)?
            } else {
                let cx = xx.sub(&Bs::constant(Ivl::from(x0)));
                let cy = xy.sub(&Bs::constant(Ivl::from(y0)));
                let cz = xz.sub(&Bs::constant(z0));
                weight.phi_series([&cx, &cy, &cz], n).mul(&normal, n)
            };
            let mut b = Bs::zero();
            for kk in 0..=n {
                b.set(kk, 0, dv.at(kk, 1));
            }
            Some(g.mul(&du.dx(n + 1), n).mul(&b, n).scale(Ivl::from(-1.0)))
        };
        let tcell = Ivl::new(t.0, t.1);
        let scell = Ivl::new(s.0, s.1);
        let (du_c, dv_c, u_c, v_c) = series(tcell, scell, ord + 1)?;
        let taylor = piece_of(&fg.cn, u_c, v_c);
        // the range over the cell times its area
        let ranged = || -> Result<(Ivl, bool)> {
            let (du1, dv1, u1, v1) = series(tcell, scell, 1)?;
            let Some(pr) = partials_over(&fg.cn, u1, v1, 1)? else {
                return Ok((Ivl::whole(), true));
            };
            let x = compose(&pr, &du1, &dv1, 1);
            let Some(f) = integrand(&pr, &du1, &dv1, 0).map(|g| g.at(0, 0)) else {
                return Ok((Ivl::whole(), true));
            };
            let area = (xh - xl) * (yh - yl);
            let [xx, xy, _] = &x;
            let wt = (xh - xl).mag() * (mag3([xx.at(1, 0), xy.at(1, 0), IZ]));
            let ws = (yh - yl).mag() * (mag3([xx.at(0, 1), xy.at(0, 1), IZ]));
            let crosses_u = piece_of(&fg.cn, u1, Ivl::from(v1.mid())).is_none();
            Ok((sign * f * area, crosses_u || wt >= ws))
        };
        let Some((ku, kv)) = taylor else {
            self.counts.ranged += 1;
            return ranged();
        };
        // the remainder: the coefficients of degree ORDER over the cell
        let (du_m, dv_m, u_m, v_m) = series(Ivl::from(tc), Ivl::from(sc), ord)?;
        let (Some(pr), Some(pm)) = (
            piece_partials(&fg.cn, (ku, kv), u_c, v_c, ord + 1),
            piece_partials(&fg.cn, (ku, kv), u_m, v_m, ord),
        ) else {
            self.counts.taylor += 1;
            return Ok((Ivl::whole(), true));
        };
        // the polynomial: the coefficients of degree < ORDER at the middle
        let (Some(fr), Some(fm)) = (
            integrand(&pr, &du_c, &dv_c, ord),
            integrand(&pm, &du_m, &dv_m, ord - 1),
        ) else {
            // an area's root not proved analytic over the cell
            self.counts.ranged += 1;
            return ranged();
        };
        self.counts.taylor += 1;
        let mx: Vec<Ivl> = (0..=ord).map(|i| moment(xl, xh, i)).collect();
        let my: Vec<Ivl> = (0..=ord).map(|i| moment(yl, yh, i)).collect();
        let ax: Vec<Ivl> = (0..=ord).map(|i| abs_moment(xl, xh, i)).collect();
        let ay: Vec<Ivl> = (0..=ord).map(|i| abs_moment(yl, yh, i)).collect();
        let at = |v: &[Ivl], i: usize| v.get(i).copied().unwrap_or(Ivl::whole());
        let mut sum = Ivl::from(0.0);
        for d in 0..ord {
            for jj in 0..=d {
                let c = fm.at(d - jj, jj);
                if !is_zero(c) {
                    sum = sum + c * at(&mx, d - jj) * at(&my, jj);
                }
            }
        }
        let (mut wt, mut ws) = (0.0_f64, 0.0_f64);
        for jj in 0..=ord {
            let i = ord - jj;
            let c = fr.at(i, jj);
            if is_zero(c) {
                continue;
            }
            let m = at(&ax, i) * at(&ay, jj);
            // x^i y^j keeps its sign over the cell where both powers are even
            let r = if i % 2 == 0 && jj % 2 == 0 {
                c * m
            } else {
                Ivl::new(-c.mag(), c.mag()) * m
            };
            if i > jj {
                wt += r.hi - r.lo;
            } else if jj > i {
                ws += r.hi - r.lo;
            }
            sum = sum + r;
        }
        let taylor_cell = (sign * sum, wt >= ws);
        if !weight.is_area() {
            return Ok(taylor_cell);
        }
        // an area's cell near a pole: the root's series divides by the least value of
        // `|S_u x S_v|^2` over the cell, near 0 there, and its remainder is wider than the
        // cell's range times its area; both hold the cell's integral, so it is enclosed by
        // their intersection (the hull, should rounding leave them apart)
        let (by_range, range_split) = ranged()?;
        let (t_cell, _) = taylor_cell;
        let (lo, hi) = (t_cell.lo.max(by_range.lo), t_cell.hi.min(by_range.hi));
        if lo > hi || lo.is_nan() || hi.is_nan() {
            return Ok((t_cell.hull(by_range), taylor_cell.1));
        }
        let split = if by_range.hi - by_range.lo < t_cell.hi - t_cell.lo {
            range_split
        } else {
            taylor_cell.1
        };
        Ok((Ivl::new(lo, hi), split))
    }

    /// A strip cell's enclosure, `[-w, w]` with `w` its flux's bound, and whether it is
    /// settled: at least half of `int |D|` is the gap's least size over the cell (at its
    /// middle, less the slope's bound times the half length) times its length, which every
    /// cut of it keeps, so cutting it can at most halve that factor. A
    /// real gap between two faces' boundaries (not their rounding) is such a cell: refining
    /// it would spend the budget without deciding.
    fn strip_cell(
        &self,
        strip: usize,
        spans: [usize; 2],
        t: (f64, f64),
        ord: usize,
    ) -> Result<(Ivl, bool)> {
        let tc = 0.5 * (t.0 + t.1);
        let (xl, xh) = (
            Ivl::from(t.0) - Ivl::from(tc),
            Ivl::from(t.1) - Ivl::from(tc),
        );
        let z0 = Ivl::from(self.z0);
        // each side's curve in 3D as a series in x: at the middle (degree < ORDER) and over
        // the cell (degree ORDER), or its range over the cell where a box may cross a knot
        // line
        let mut mid = Vec::new();
        let mut over = Vec::new();
        let mut taylor = true;
        for side in 0..2 {
            let (face, cn) = self.strip_side(strip, side)?;
            let sp = spans
                .get(side)
                .copied()
                .ok_or_else(|| SolidError::mass_failed(OP, "no such strip side"))?;
            let (hu, hv) = piece_hull(cn, sp);
            let at = |tt: Ivl, n: usize| -> Result<(Bs, Bs, Ivl, Ivl)> {
                let d = curve_derivs_over(cn, tt, n, Some(sp))?;
                let (u0, du) = curve_series(&d, 0, n);
                let (v0, dv) = curve_series(&d, 1, n);
                let u = Ivl::new(u0.lo.max(hu.lo), u0.hi.min(hu.hi));
                let v = Ivl::new(v0.lo.max(hv.lo), v0.hi.min(hv.hi));
                Ok((du, dv, u, v))
            };
            let (du, dv, u, v) = at(Ivl::new(t.0, t.1), ord)?;
            let (dum, dvm, um, vm) = at(Ivl::from(tc), ord - 1)?;
            let pieces = piece_of(&face.cn, u, v).and_then(|kk| {
                Some((
                    piece_partials(&face.cn, kk, u, v, ord)?,
                    piece_partials(&face.cn, kk, um, vm, ord - 1)?,
                ))
            });
            match pieces {
                Some((po, pm)) if taylor => {
                    over.push(compose(&po, &du, &dv, ord));
                    mid.push(compose(&pm, &dum, &dvm, ord - 1));
                }
                _ => {
                    taylor = false;
                    let (du, dv, u, v) = at(Ivl::new(t.0, t.1), 1)?;
                    let Some(p) = partials_over(&face.cn, u, v, 1)? else {
                        return Ok((Ivl::whole(), false));
                    };
                    over.push(compose(&p, &du, &dv, 1));
                }
            }
        }
        let (Some(o1), Some(o2)) = (over.first(), over.get(1)) else {
            return Err(SolidError::mass_failed(
                OP,
                "the cell has fewer than two series",
            ));
        };
        let comp = |s: &[Bs; 3], i: usize| [s[0].at(i, 0), s[1].at(i, 0), s[2].at(i, 0)];
        let diff = |a: [Ivl; 3], b: [Ivl; 3]| [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        // |X_1'| + |D'| and max |z - z0| over the cell
        let speed = mag3([
            Ivl::from(mag3(comp(o1, 1))),
            Ivl::from(mag3(diff(comp(o1, 1), comp(o2, 1)))),
            IZ,
        ]);
        // `|Phi|` over the strip: each point of it lies between its sides' points, in the
        // hull of their ranges (the volume's `max |z - z0|`)
        let hull = |i: usize| {
            let c = self.weight.centre.get(i).copied();
            let side = |o: &[Bs; 3]| match (o.get(i), c) {
                (Some(s), Some(c)) => s.at(0, 0) - Ivl::from(c),
                _ => Ivl::whole(),
            };
            side(o1).hull(side(o2))
        };
        let zmax = if self.weight.quantity == (Quantity::Moment { x: 0, y: 0, z: 0 }) {
            (o1[2].at(0, 0) - z0).mag().max((o2[2].at(0, 0) - z0).mag())
        } else {
            self.weight.phi([hull(0), hull(1), hull(2)]).mag()
        };
        // Where a side's box may cross a knot line of its surface: the gap at the middle, and
        // its derivative's bound over the cell (the hull over every piece the box meets). `D`
        // is continuous and piecewise smooth on the cell (one polynomial piece of each curve on
        // a surface continuous across the knot lines its box meets: none of full multiplicity,
        // [`breaks_within`], else the range alone), so `|D(t) - D(tc)| <= |D'| |t - tc|`
        // component by component, and `int |D|` is at most `|D(tc)| h + |D'| int |x|`. The
        // range's `|D|` bounds `X_2` and `X_1` each alone: it is as wide as `|X'| h` however
        // close the sides are, and `int |D|` from it shrinks with the cell's square. `|D'|`
        // is as wide as `|X''| h` where the sides meet, so this shrinks with the cube.
        let centred = if taylor {
            None
        } else {
            let mut middle = Vec::new();
            for side in 0..2 {
                let (face, cn) = self.strip_side(strip, side)?;
                let sp = spans
                    .get(side)
                    .copied()
                    .ok_or_else(|| SolidError::mass_failed(OP, "no such strip side"))?;
                let (hu, hv) = piece_hull(cn, sp);
                let cell = curve_derivs_over(cn, Ivl::new(t.0, t.1), 0, Some(sp))?;
                let (uc, _) = curve_series(&cell, 0, 0);
                let (vc, _) = curve_series(&cell, 1, 0);
                if breaks_within(&face.cn.u, face.cn.p, uc)
                    || breaks_within(&face.cn.v, face.cn.q, vc)
                {
                    break;
                }
                let d = curve_derivs_over(cn, Ivl::from(tc), 0, Some(sp))?;
                let (u0, du) = curve_series(&d, 0, 0);
                let (v0, dv) = curve_series(&d, 1, 0);
                let u = Ivl::new(u0.lo.max(hu.lo), u0.hi.min(hu.hi));
                let v = Ivl::new(v0.lo.max(hv.lo), v0.hi.min(hv.hi));
                let Some(p) = partials_over(&face.cn, u, v, 0)? else {
                    break;
                };
                middle.push(comp(&compose(&p, &du, &dv, 0), 0));
            }
            match (middle.first(), middle.get(1)) {
                (Some(&c1), Some(&c2)) => Some(diff(c1, c2)),
                _ => None,
            }
        };
        let slope = mag3(diff(comp(o1, 1), comp(o2, 1)));
        // int |D| over the cell
        let gap = if let (true, Some(m1), Some(m2)) = (taylor, mid.first(), mid.get(1)) {
            let mut g = Ivl::from(0.0);
            for i in 0..ord {
                let d = mag3(diff(comp(m1, i), comp(m2, i)));
                g = g + Ivl::from(d) * abs_moment(xl, xh, i);
            }
            let d = mag3(diff(comp(o1, ord), comp(o2, ord)));
            g + Ivl::from(d) * abs_moment(xl, xh, ord)
        } else {
            let range = Ivl::from(mag3(diff(comp(o1, 0), comp(o2, 0)))) * (xh - xl);
            match centred {
                Some(dm) => {
                    let mean =
                        Ivl::from(mag3(dm)) * (xh - xl) + Ivl::from(slope) * abs_moment(xl, xh, 1);
                    // both hold: the narrower
                    if mean.hi < range.hi {
                        mean
                    } else {
                        range
                    }
                }
                None => range,
            }
        };
        let w = Ivl::from(zmax) * Ivl::from(speed) * gap;
        // the gap's least size over the cell, in the norm `int |D|` is taken in (the sum of
        // the components, `mag3`): at the middle less its derivative's bound times the half
        // length, or over the whole cell where there is no middle series
        let least_sum = |d: [Ivl; 3]| {
            d.iter()
                .fold(Ivl::from(0.0), |sum, c| sum + Ivl::from(c.mig()))
                .lo
        };
        let half = xl.mag().max(xh.mag());
        let at_middle = |d: [Ivl; 3]| {
            (Ivl::from(least_sum(d)) - Ivl::from(slope) * Ivl::from(half))
                .lo
                .max(0.0)
        };
        let least = if let (true, Some(m1), Some(m2)) = (taylor, mid.first(), mid.get(1)) {
            at_middle(diff(comp(m1, 0), comp(m2, 0)))
        } else {
            let range = least_sum(diff(comp(o1, 0), comp(o2, 0)));
            centred.map_or(range, |dm| range.max(at_middle(dm)))
        };
        let kept = (Ivl::from(least) * (xh - xl)).lo;
        let settled = kept + kept >= gap.hi;
        Ok((Ivl::new(-w.hi, w.hi), settled))
    }

    /// The enclosure: the cells' sum and the fixed terms, in insertion order.
    fn total(&self) -> Ivl {
        if self.weight.is_area() {
            return self.fixed + self.area_cells();
        }
        let mut sum = self.fixed;
        for (it, alive) in self.items.iter().zip(&self.alive) {
            if *alive {
                sum = sum + it.enc;
            }
        }
        sum
    }

    /// The area's cells: each face's sum (its area times the winding of its loops, +1 or
    /// -1), by its magnitude, summed in face order.
    fn area_cells(&self) -> Ivl {
        let mut per_face = vec![Ivl::from(0.0); self.faces.len()];
        for (it, alive) in self.items.iter().zip(&self.alive) {
            if !*alive {
                continue;
            }
            let Kind::Sweep { piece, .. } = it.kind else {
                continue;
            };
            let face = self.pieces.get(piece).map(|p| p.face);
            match face.and_then(|f| per_face.get_mut(f)) {
                Some(s) => *s = *s + it.enc,
                None => return Ivl::whole(),
            }
        }
        per_face
            .into_iter()
            .fold(Ivl::from(0.0), |sum, s| sum + abs_ivl(s))
    }

    /// The enclosure in two parts: the sum of the cells still refined, and the half width
    /// of the rest (the settled strip cells and the fixed terms, each `[-w, w]`), rounded
    /// up.
    fn parts(&self) -> (Ivl, f64) {
        if self.weight.is_area() {
            // no strip cells: every cell is still refined
            return (self.area_cells(), Ivl::from(self.fixed.hi).hi);
        }
        let mut rest = Ivl::from(0.0);
        let mut still = Ivl::from(self.fixed.hi);
        for (it, alive) in self.items.iter().zip(&self.alive) {
            if !*alive {
                continue;
            }
            if it.settled {
                still = still + Ivl::from(it.enc.hi);
            } else {
                rest = rest + it.enc;
            }
        }
        (rest, still.hi)
    }

    /// Refines the widest cell, one bisection at a time, until `done` holds of the
    /// enclosure ([`EnclosureStop::Reached`]), `hopeless` of its parts ([`Work::parts`]: no
    /// refining can make `done` hold; [`EnclosureStop::Settled`]), the budget is spent, or
    /// no cell is left to cut. The result is the intersection of every enclosure summed
    /// (each holds the volume): rounding that grows faster than the cells shrink does not
    /// widen it. Two enclosures that do not overlap mean one of them is not sound, which no
    /// cell's construction allows: [`MassFailure::Failed`](crate::error::mass::MassFailure::Failed)
    /// then, not an enclosure (no input is known to reach it).
    fn refine(
        &mut self,
        done: impl Fn(Ivl) -> bool,
        hopeless: impl Fn(Ivl, f64) -> bool,
    ) -> Result<VolumeEnclosure> {
        let out = |w: &Self, best: Ivl, stop: EnclosureStop| -> Result<VolumeEnclosure> {
            let e = intersect(w.total(), best)?;
            let (_, still) = w.parts();
            Ok(VolumeEnclosure {
                lo: e.lo,
                hi: e.hi,
                floor: (Ivl::from(still) + Ivl::from(still)).hi,
                stop,
                counts: w.counts,
            })
        };
        // running sums in f64 say when to sum the enclosure in intervals again: when they
        // look done, or every quarter of the work list's length
        let total = self.total();
        let (mut lo, mut hi) = (total.lo, total.hi);
        // the intersection of the enclosures summed so far
        let mut best = total;
        let mut checked = self.counts.cells;
        let mut next_check = 0usize;
        loop {
            let due = self.counts.cells >= next_check
                || (done(Ivl::new(lo, hi)) && self.counts.cells >= checked + 16);
            if due {
                let total = self.total();
                best = intersect(total, best)?;
                if done(best) {
                    return out(self, best, EnclosureStop::Reached);
                }
                let (rest, still) = self.parts();
                if hopeless(rest, still) {
                    return out(self, best, EnclosureStop::Settled);
                }
                (lo, hi) = (total.lo, total.hi);
                checked = self.counts.cells;
                next_check = self.counts.cells + (self.items.len() / 4).max(64);
            }
            if self.counts.cells >= self.budget {
                return out(self, best, EnclosureStop::Budget);
            }
            let Some((_, Reverse(i))) = self.heap.pop() else {
                return out(self, best, EnclosureStop::Exhausted);
            };
            let Some(it) = self.items.get(i).copied() else {
                continue;
            };
            if it.settled {
                continue; // a gap no cut narrows: kept as it is
            }
            if it.order < ORDER {
                // evaluated again at the full order before it is cut
                if let Some(a) = self.alive.get_mut(i) {
                    *a = false;
                }
                lo -= it.enc.lo;
                hi -= it.enc.hi;
                self.push(Item { order: ORDER, ..it })?;
                if let Some(n) = self.items.last() {
                    lo += n.enc.lo;
                    hi += n.enc.hi;
                }
                continue;
            }
            if it.depth >= MAX_DEPTH {
                continue; // kept as it is
            }
            let tm = 0.5 * (it.t.0 + it.t.1);
            let sm = 0.5 * (it.s.0 + it.s.1);
            let can_t = it.t.0 < tm && tm < it.t.1;
            let can_s = it.s.0 < sm && sm < it.s.1 && matches!(it.kind, Kind::Sweep { .. });
            // an unbounded cell (weights not proved positive over its box) is cut along t
            // and sigma in turn: its box shrinks in both, whichever its size depends on
            let split_t = if (it.enc.hi - it.enc.lo).is_finite() {
                it.split_t
            } else {
                it.depth % 2 == 0
            };
            let children = if can_t && (split_t || !can_s) {
                [(it.t.0, tm), (tm, it.t.1)].map(|t| Item {
                    t,
                    depth: it.depth + 1,
                    ..it
                })
            } else if can_s {
                [(it.s.0, sm), (sm, it.s.1)].map(|s| Item {
                    s,
                    depth: it.depth + 1,
                    ..it
                })
            } else {
                continue; // a cell of single parameters: kept as it is
            };
            if let Some(a) = self.alive.get_mut(i) {
                *a = false;
            }
            lo -= it.enc.lo;
            hi -= it.enc.hi;
            for c in children {
                self.push(c)?;
                if let Some(n) = self.items.last() {
                    lo += n.enc.lo;
                    hi += n.enc.hi;
                }
            }
        }
    }
}

/// `{|x| : x in a}`.
fn abs_ivl(a: Ivl) -> Ivl {
    if a.lo >= 0.0 {
        a
    } else if a.hi <= 0.0 {
        Ivl::new(-a.hi, -a.lo)
    } else {
        Ivl::new(0.0, a.mag())
    }
}

/// The intersection of two enclosures of the volume; a [`MassFailure::Failed`](crate::error::mass::MassFailure::Failed)
/// where they do not overlap (one of them is not sound).
fn intersect(a: Ivl, b: Ivl) -> Result<Ivl> {
    let (lo, hi) = (a.lo.max(b.lo), a.hi.min(b.hi));
    if lo <= hi {
        Ok(Ivl::new(lo, hi))
    } else {
        Err(SolidError::mass_failed(
            OP,
            "two enclosures of the volume do not overlap",
        ))
    }
}

/// The parameters in `(a, z)` where the curve's coordinate `c` (0: u, 1: v) on span `s`
/// crosses `line`, located by sign changes between 16 evenly spaced points and bisection
/// (where the cells are cut; a cut off the exact crossing only leaves a cell whose image
/// may cross the line, enclosed by its range).
fn crossings(
    cn: &NurbsCurve,
    s: usize,
    a: f64,
    z: f64,
    c: usize,
    line: f64,
    out: &mut Vec<f64>,
) -> Result<()> {
    const SAMPLES: usize = 16;
    let (hu, hv) = piece_hull(cn, s);
    let h = if c == 0 { hu } else { hv };
    if !(h.lo < line && line < h.hi) {
        return Ok(());
    }
    let at = |t: f64| -> Result<f64> {
        let d = curve_derivs_over(cn, Ivl::from(t), 0, Some(s))?;
        let p = d
            .first()
            .copied()
            .ok_or_else(|| SolidError::mass_failed(OP, "the curve has no point"))?;
        Ok((if c == 0 { p.x } else { p.y }).mid() - line)
    };
    let mut t0 = a;
    let mut f0 = at(t0)?;
    for i in 1..=SAMPLES {
        let t1 = a + (z - a) * (i as f64) / (SAMPLES as f64);
        let f1 = at(t1)?;
        if f0 * f1 < 0.0 {
            let (mut lo, mut hi, mut flo) = (t0, t1, f0);
            for _ in 0..64 {
                let m = 0.5 * (lo + hi);
                if !(lo < m && m < hi) {
                    break;
                }
                let fm = at(m)?;
                if fm * flo <= 0.0 {
                    hi = m;
                } else {
                    lo = m;
                    flo = fm;
                }
            }
            out.push(0.5 * (lo + hi));
        }
        t0 = t1;
        f0 = f1;
    }
    Ok(())
}

/// A bound on the 3D length of the uv box where it lies on the net's first or last knot in
/// one direction (`None` elsewhere). On that side the surface is the rational curve of the
/// side's control points `P_i`: with them in a box of diameter `delta`, its derivative
/// `sum (N_i w_i)' (P_i - C) / W` is at most `sum |N_i'| max w delta / min w`, and
/// `sum |N_i'| <= 2 p / h` on a knot span of length `h` (each `N_i'` is two terms of
/// `p / (u_(i+p) - u_i)` times a basis function of degree `p - 1`). At a pole, where the
/// points are one point to rounding, the bound is of the order of rounding. A box off the
/// side by rounding (a trim's end a few units in the last place from the knot) adds its
/// distance times the mixed partial over the box between.
fn side_length(cn: &Nurbs, u: Ivl, v: Ivl) -> Option<Ivl> {
    let (u, v, across) = on_side(cn, u, v)?;
    let along = side_speed_bound(cn, u, v)?;
    // off the side by `d` (rounding): the speed along it grows by at most `d` times the
    // mixed partial over the box between
    let between = |x: Ivl, k: f64| Ivl::new(x.lo.min(k), x.hi.max(k));
    let (bu, bv, off, extent) = match across {
        Across::V(k) => (u, between(v, k), (v, k), width(u)),
        Across::U(k) => (between(u, k), v, (u, k), width(v)),
    };
    let d = (off.0.lo - off.1).abs().max((off.0.hi - off.1).abs());
    let mixed = if d > 0.0 {
        // (`None` for a box past the finite numbers or weights not proved positive: the
        // caller then bounds the length by the partials over the box)
        let p = partials_over(cn, bu, bv, 2).ok()??.at(1, 1);
        Ivl::from(mag3([p.x, p.y, p.z])) * Ivl::from(d) * Ivl::from(extent)
    } else {
        Ivl::from(0.0)
    };
    Some(along + mixed)
}

/// Which side of the net a box lies on to within rounding (`1e-12` of the knot's size):
/// the box, and the side's knot in the direction across it.
fn on_side(cn: &Nurbs, u: Ivl, v: Ivl) -> Option<(Ivl, Ivl, Across)> {
    let near = |x: Ivl, k: f64| {
        let tol = 1e-12 * (1.0 + k.abs());
        (x.lo - k).abs() <= tol && (x.hi - k).abs() <= tol
    };
    let (v0, v1) = (knot(&cn.v, cn.q as isize), knot(&cn.v, cn.nv as isize));
    let (u0, u1) = (knot(&cn.u, cn.p as isize), knot(&cn.u, cn.nu as isize));
    let side = [(v, v0, true), (v, v1, true), (u, u0, false), (u, u1, false)]
        .into_iter()
        .find(|&(x, k, _)| near(x, k))?;
    Some((
        u,
        v,
        if side.2 {
            Across::V(side.1)
        } else {
            Across::U(side.1)
        },
    ))
}

/// The direction across a side of the net, with the side's knot.
#[derive(Clone, Copy)]
enum Across {
    /// The side is the u line at this v knot.
    V(f64),
    /// The side is the v line at this u knot.
    U(f64),
}

/// The bound of [`side_length`] on the side itself.
fn side_speed_bound(cn: &Nurbs, u: Ivl, v: Ivl) -> Option<Ivl> {
    let point = |i: usize, j: usize| -> Option<([Ivl; 3], f64)> {
        let at = |c: usize| cn.pw.get(4 * (i * cn.nv + j) + c).copied();
        let w = at(3)?;
        let d = |c: usize| at(c).map(|x| Ivl::from(x) / Ivl::from(w));
        Some(([d(0)?, d(1)?, d(2)?], w))
    };
    let (_, _, across) = on_side(cn, u, v)?;
    let (v0, v1) = (knot(&cn.v, cn.q as isize), knot(&cn.v, cn.nv as isize));
    let (u0, u1) = (knot(&cn.u, cn.p as isize), knot(&cn.u, cn.nu as isize));
    // the side's control points, and the direction along it: its knots, degree, count and
    // the box's range
    let on = |k: f64| match across {
        Across::V(x) | Across::U(x) => x == k,
    };
    let (pts, knots, p, n, along): (Vec<_>, &[f64], usize, usize, Ivl) =
        if matches!(across, Across::V(_)) && on(v0) {
            (
                (0..cn.nu).map(|i| point(i, 0)).collect(),
                &cn.u,
                cn.p,
                cn.nu,
                u,
            )
        } else if matches!(across, Across::V(_)) && on(v1) {
            (
                (0..cn.nu)
                    .map(|i| point(i, cn.nv.saturating_sub(1)))
                    .collect(),
                &cn.u,
                cn.p,
                cn.nu,
                u,
            )
        } else if matches!(across, Across::U(_)) && on(u0) {
            (
                (0..cn.nv).map(|j| point(0, j)).collect(),
                &cn.v,
                cn.q,
                cn.nv,
                v,
            )
        } else if matches!(across, Across::U(_)) && on(u1) {
            (
                (0..cn.nv)
                    .map(|j| point(cn.nu.saturating_sub(1), j))
                    .collect(),
                &cn.v,
                cn.q,
                cn.nv,
                v,
            )
        } else {
            return None;
        };
    let mut hull: Option<[Ivl; 3]> = None;
    let (mut wmin, mut wmax) = (f64::INFINITY, 0.0_f64);
    for q in pts {
        let (x, w) = q?;
        hull = Some(match hull {
            None => x,
            Some(h) => [h[0].hull(x[0]), h[1].hull(x[1]), h[2].hull(x[2])],
        });
        wmin = wmin.min(w);
        wmax = wmax.max(w);
    }
    let h = hull?;
    let delta = mag3([
        Ivl::from(width(h[0])),
        Ivl::from(width(h[1])),
        Ivl::from(width(h[2])),
    ]);
    // the shortest knot span the range meets
    let mut shortest = f64::INFINITY;
    for k in span(knots, p, n, along.lo)..=span(knots, p, n, along.hi) {
        let len = knot(knots, k as isize + 1) - knot(knots, k as isize);
        if len > 0.0 {
            shortest = shortest.min(len);
        }
    }
    let ok = wmin > 0.0 && shortest.is_finite() && delta.is_finite();
    if !ok {
        return None;
    }
    let speed = Ivl::from(2.0 * p as f64) / Ivl::from(shortest) * Ivl::from(wmax) / Ivl::from(wmin)
        * Ivl::from(delta);
    Some(speed * Ivl::from(width(along)))
}

/// The straight uv segment from `a` to `z` as a degree 1 curve on `[0, 1]`.
fn segment(a: [f64; 2], z: [f64; 2]) -> Result<NurbsCurve> {
    let ok = a.iter().chain(&z).all(|x| x.is_finite());
    if !ok {
        return Err(SolidError::invalid_input(
            OP,
            "a loop's end that is not finite",
        ));
    }
    Ok(NurbsCurve {
        p: 1,
        n: 2,
        u: vec![0.0, 0.0, 1.0, 1.0],
        pw: vec![a[0], a[1], 0.0, 1.0, z[0], z[1], 0.0, 1.0],
    })
}

/// An edge use's pcurve as NURBS data, made first where it is missing (as the integrator's
/// trim boundary makes it).
fn pcurve(k: &Kernel, b: &mut Brep, eu: EdgeuseId) -> Result<NurbsCurve> {
    let fetch =
        crate::topology::edgeuse::p3::get_or_create_uv_trim_curve(k, b, eu, false, false, true)?;
    fetch.status?;
    let c = crate::topology::edgeuse::p1::uv_trim_curve(b, eu)?
        .0
        .ok_or_else(|| SolidError::mass_missing(OP, "the edgeuse's pcurve"))?;
    let bs: &BSplineCurve = c.b_spline_base().ok_or(SolidError::Unsupported {
        operation: OP,
        reason: "a pcurve with no NURBS form",
    })?;
    NurbsCurve::of(bs)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The relative stop test holds only of a finite enclosure within `rel` of its
    /// magnitude: an unbounded one (a cell enclosed to the whole line, where `inf <= inf`)
    /// or one with a NaN end never counts as reached.
    #[test]
    fn the_relative_stop_needs_a_finite_enclosure() {
        let done = within(1e-2);
        assert!(done(Ivl::new(99.5, 100.0)));
        assert!(!done(Ivl::new(98.0, 100.0)));
        assert!(!done(Ivl::whole()));
        assert!(!done(Ivl::new(1.0, f64::INFINITY)));
        assert!(!done(Ivl::new(f64::NEG_INFINITY, -1.0)));
        assert!(!done(Ivl::new(f64::NAN, 1.0)));
    }

    /// A sheet's faces, void on both sides, have area enclosures, positive whichever use
    /// of each face is upward, face by face and over the whole sheet, though they bound no
    /// volume: the volume is still refused.
    #[test]
    fn a_sheet_face_has_an_area_enclosure() {
        use crate::primitive::api_curves::create_line_segment;
        let mut k = Kernel::default();
        // a 3 x 3 square and, apart from it, a 2 x 2 one: areas 9 and 4
        let squares = [(0.0, 3.0), (4.0, 2.0)];
        let mut curves: Vec<crate::curve::curve::Curve> = Vec::new();
        for (x0, s) in squares {
            let pts = [[x0, 0.0], [x0 + s, 0.0], [x0 + s, s], [x0, s]];
            for i in 0..4 {
                let (a, c) = (pts[i], pts[(i + 1) % 4]);
                curves.push(
                    create_line_segment(Point3::new(a[0], a[1], 0.0), Point3::new(c[0], c[1], 0.0))
                        .unwrap()
                        .into(),
                );
            }
        }
        let mut b = crate::primitive::api_trim_ops::create_planar_faces(&mut k, &curves).unwrap();
        let faces = crate::topology::brep::p1::faces(&b, None).unwrap();
        assert_eq!(faces.len(), 2);
        let holds = |e: &VolumeEnclosure, a: f64| {
            assert!(
                e.lo > 0.0 && e.lo <= a && a <= e.hi,
                "[{}, {}] for {a}",
                e.lo,
                e.hi
            );
            assert!(e.hi - e.lo <= 1e-8 * a, "[{}, {}] for {a}", e.lo, e.hi);
        };
        for flip in [None, Some(0), Some(1)] {
            if let Some(i) = flip {
                crate::topology::face::p1::flip_face_orientation(&mut b, faces[i]).unwrap();
            }
            let mut areas = [0.0; 2];
            for (i, &f) in faces.iter().enumerate() {
                let e = enclose_face_area_relative(&k, &mut b, &[f], 1e-9).unwrap();
                areas[i] = (e.lo + e.hi) / 2.0;
                let a = if (areas[i] - 9.0).abs() < 1.0 {
                    9.0
                } else {
                    4.0
                };
                holds(&e, a);
            }
            assert!((areas[0] - areas[1]).abs() > 1.0, "{areas:?}");
            let whole = enclose_quantity(&k, &mut b, Quantity::Area, 1e-8).unwrap();
            holds(&whole, 13.0);
        }
        assert!(enclose_volume(&k, &mut b, 1e-6).is_err());
    }

    /// The cylinder r 2, h 5, its side moved along x by `shift`.
    fn cylinder_side_moved(k: &mut Kernel, shift: f64) -> Brep {
        let mut b = crate::primitive::api_primitives::create_cylinder(
            k,
            Point3::new(0.0, 0.0, 0.0),
            2.0,
            5.0,
        )
        .unwrap();
        let side = crate::topology::brep::p1::faces(&b, None)
            .unwrap()
            .into_iter()
            .find(|&f| {
                let face = b.face(f).unwrap();
                let s = face.surface.as_ref().unwrap();
                s.evaluate_point(k, face.uv_domain.mid())
                    .is_ok_and(|p| (p.x * p.x + p.y * p.y - 4.0).abs() < 1e-9)
            })
            .unwrap();
        let mut by = crate::geom::axis2_placement::Axis2Placement::default();
        by.translate(crate::math::Vec3::new(shift, 0.0, 0.0));
        b.face_mut(side)
            .unwrap()
            .surface
            .as_mut()
            .unwrap()
            .transform(k, &by, None)
            .unwrap();
        b
    }

    /// Whether either side's box over the cell crosses or touches a knot line of its surface
    /// (the cell is then enclosed by its range).
    fn on_knot_line(w: &Work, strip: usize, spans: [usize; 2], t: (f64, f64)) -> bool {
        (0..2).any(|side| {
            let (face, cn) = w.strip_side(strip, side).unwrap();
            let d = curve_derivs_over(cn, Ivl::new(t.0, t.1), 0, Some(spans[side])).unwrap();
            let (u, _) = curve_series(&d, 0, 0);
            let (v, _) = curve_series(&d, 1, 0);
            piece_of(&face.cn, u, v).is_none()
        })
    }

    /// The strip cells ending on each strip's pieces' ends, `h` long.
    fn end_cells(w: &Work, h: f64) -> Vec<(usize, [usize; 2], (f64, f64))> {
        w.items
            .iter()
            .filter_map(|it| match it.kind {
                Kind::Strip { strip, spans } => Some((strip, spans, (it.t.1 - h, it.t.1))),
                Kind::Sweep { .. } => None,
            })
            .collect()
    }

    /// Where a strip's two sides meet, its cells' enclosures shrink with the cube of their
    /// length, on a knot line of a side's surface too. The cylinder's side meets its caps
    /// along circles whose pieces end on the side's knot lines (every quarter turn): a cell
    /// ending there is enclosed by its range, not its Taylor model. That range's gap
    /// (`X_2` less `X_1`, each bounded alone) shrinks only with the cell's length, so its
    /// enclosure with the square: at 0.1 long it was 8.2e-2 (`4 h^3` is 4e-3), where the
    /// Taylor cells are within 1e-12.
    #[test]
    fn strips_that_meet_shrink_with_the_cube_on_knot_lines() {
        let mut k = Kernel::default();
        let mut b = cylinder_side_moved(&mut k, 0.0);
        let w = Work::new(&k, &mut b).unwrap();
        let mut on_knots = 0;
        for h in [0.1, 0.05, 0.025] {
            for (strip, spans, t) in end_cells(&w, h) {
                let (e, _) = w.strip_cell(strip, spans, t, ORDER).unwrap();
                assert!(e.hi <= 4.0 * h * h * h, "{strip} {t:?}: {e:?}");
                on_knots += usize::from(on_knot_line(&w, strip, spans, t));
            }
        }
        assert!(on_knots >= 9, "{on_knots}");
    }

    /// Where the sides do not meet, a strip cell's enclosure still holds the strip: the side
    /// moved along x by `d` leaves at each cap a flat strip between a circle's arc and the
    /// same arc moved by `d`, whose flux of `(0, 0, z - z0)` is `(z - z0) d dy` in closed
    /// form (the sheared area `d dy` at the cap's height). Every cell ending on a piece's
    /// end, on a knot line or not, at lengths 0.4 to 1/40, encloses that.
    #[test]
    fn strips_apart_on_knot_lines_hold_their_closed_form() {
        let d = 1e-3;
        let mut k = Kernel::default();
        let mut b = cylinder_side_moved(&mut k, d);
        let w = Work::new(&k, &mut b).unwrap();
        let (mut checked, mut on_knots) = (0, 0);
        for h in [0.4, 0.1, 0.025] {
            for (strip, spans, t) in end_cells(&w, h) {
                let p = |side, at| w.point(strip, side, at).unwrap().map(Ivl::mid);
                let (a0, a1, b0) = (p(0, t.0), p(0, t.1), p(1, t.0));
                let flat = (a0[2] - a1[2]).abs() < 1e-12;
                let moved = (b0[0] - a0[0]).abs() - d;
                if !flat || moved.abs() > 1e-9 || (b0[1] - a0[1]).abs() > 1e-9 {
                    continue; // the seam: both sides on the moved face, which meet
                }
                let flux = ((a0[2] - w.z0) * d * (a1[1] - a0[1])).abs();
                let (e, _) = w.strip_cell(strip, spans, t, ORDER).unwrap();
                assert!(e.hi >= flux, "{strip} {t:?}: {e:?} < {flux}");
                checked += 1;
                on_knots += usize::from(on_knot_line(&w, strip, spans, t));
            }
        }
        assert!(checked >= 18 && on_knots >= 6, "{checked} {on_knots}");
    }

    /// A knot of full multiplicity inside the vector breaks continuity; one at the ends, or
    /// one of the degree's multiplicity, does not.
    #[test]
    fn breaks_are_interior_knots_of_full_multiplicity() {
        let k = [0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0];
        assert!(breaks_within(&k, 2, Ivl::new(0.4, 0.5)));
        assert!(!breaks_within(&k, 2, Ivl::new(0.0, 0.4)));
        assert!(!breaks_within(&k, 3, Ivl::new(0.0, 1.0)));
        let c0 = [0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0];
        assert!(!breaks_within(&c0, 2, Ivl::new(0.0, 1.0)));
    }

    /// A product of series is the product of the polynomials, truncated.
    #[test]
    fn series_multiply_and_differentiate_as_polynomials() {
        // (1 + x + y)^2 = 1 + 2x + 2y + x^2 + 2xy + y^2
        let mut a = Bs::constant(Ivl::from(1.0));
        a.set(1, 0, Ivl::from(1.0));
        a.set(0, 1, Ivl::from(1.0));
        let s = a.mul(&a, XORD);
        for (i, j, c) in [
            (0, 0, 1.0),
            (1, 0, 2.0),
            (0, 1, 2.0),
            (2, 0, 1.0),
            (1, 1, 2.0),
            (0, 2, 1.0),
        ] {
            let v = s.at(i, j);
            assert!(v.lo <= c && c <= v.hi, "{i} {j}: {v:?}");
        }
        assert!(is_zero(s.at(3, 0)));
        let d = s.dx(2);
        assert!(d.at(0, 0).lo <= 2.0 && 2.0 <= d.at(0, 0).hi);
        assert!(d.at(0, 1).lo <= 2.0 && 2.0 <= d.at(0, 1).hi);
    }

    /// The bounds' sums and widths round up: 1 + 1e-16 is not a double, and rounded to the
    /// nearest it is 1, below the true sum.
    #[test]
    fn bounds_round_up() {
        assert!(mag3([Ivl::from(1.0), Ivl::from(1e-16), IZ]) > 1.0);
        assert!(width(Ivl::new(-1e-16, 1.0)) > 1.0);
    }

    /// A budget too small to decide leaves the sign undecided, its enclosure holding the
    /// volume (the quarter shell R 1e4, t 1e-3, V 157.08, decides within some 150 cells).
    #[test]
    fn a_spent_budget_leaves_the_sign_undecided() {
        let mut k = Kernel::default();
        let mut b = crate::primitive::api_primitives::create_cylindrical_box(
            &mut k,
            Point3::new(0.0, 0.0, 0.0),
            10.0,
            1e4,
            1e4 + 1e-3,
            0.0,
            90.0,
        )
        .unwrap();
        let v = 157.0796;
        match volume_sign_within(&k, &mut b, 16).unwrap() {
            VolumeSign::Undecided(e) => assert!(e.lo < v && v < e.hi, "{e:?}"),
            other => panic!("{other:?}"),
        }
    }

    /// Enclosures intersect to their common part, and two that do not overlap (one of them
    /// unsound) are a mass failure, not an enclosure.
    #[test]
    fn enclosures_that_do_not_overlap_are_a_failure() {
        let e = intersect(Ivl::new(1.0, 3.0), Ivl::new(2.0, 4.0)).unwrap();
        assert_eq!((e.lo, e.hi), (2.0, 3.0));
        let r = intersect(Ivl::new(1.0, 2.0), Ivl::new(2.5, 4.0));
        assert!(
            matches!(
                r.as_ref().map_err(SolidError::mass_failure),
                Err(Some(crate::error::mass::MassFailure::Failed(_)))
            ),
            "{r:?}"
        );
    }

    /// The moments hold the closed forms.
    #[test]
    fn moments_hold_their_closed_forms() {
        let (xl, xh) = (Ivl::from(-0.5), Ivl::from(1.5));
        // int x^3 over [-0.5, 1.5] = (1.5^4 - 0.5^4) / 4 = 1.25
        let m = moment(xl, xh, 3);
        assert!(m.lo <= 1.25 && 1.25 <= m.hi && m.hi - m.lo < 1e-14);
        // int |x|^3 = (1.5^4 + 0.5^4) / 4 = 1.28125
        let a = abs_moment(xl, xh, 3);
        assert!(a.lo <= 1.28125 && 1.28125 <= a.hi);
    }

    /// Constant u is decided exactly, on the control points that define the piece: a segment
    /// along v (one u, weight 1) is constant, one along u is not, and one whose two weights or
    /// two `u w` differ in the last bit is not.
    #[test]
    fn constant_u_is_exact_on_the_piece_control_points() {
        assert!(constant_u(&segment([0.25, 0.0], [0.25, 1.0]).unwrap(), 1));
        assert!(!constant_u(&segment([0.0, 0.5], [1.0, 0.5]).unwrap(), 1));
        let mut seg = segment([0.25, 0.0], [0.25, 1.0]).unwrap();
        seg.pw[4] = f64::from_bits(0.25_f64.to_bits() + 1);
        assert!(!constant_u(&seg, 1));
        let mut seg = segment([0.25, 0.0], [0.25, 1.0]).unwrap();
        seg.pw[7] = 2.0;
        assert!(!constant_u(&seg, 1));
    }

    /// A piece of constant u adds no cell and leaves the enclosure as it was, bit for bit; a
    /// piece across u on the same face adds cells. A cylinder's seam is swept by no cell.
    #[test]
    fn a_piece_of_constant_u_adds_no_cell() {
        let mut k = Kernel::default();
        let mut b = crate::primitive::api_primitives::create_cylinder(
            &mut k,
            Point3::new(0.0, 0.0, 0.0),
            2.0,
            5.0,
        )
        .unwrap();
        let mut w = Work::new(&k, &mut b).unwrap();
        // every piece Work made sweeps cells: a constant-u one was left out
        assert!(w.pieces.iter().all(|p| !constant_u(&p.cn, p.span)));
        // the curved face, and its parameter box
        let (face, (u0, u1), (v0, v1)) = w
            .faces
            .iter()
            .enumerate()
            .find(|(_, f)| f.cn.p > 1 || f.cn.q > 1)
            .map(|(i, f)| {
                let ends = |k: &[f64]| (*k.first().unwrap(), *k.last().unwrap());
                (i, ends(&f.cn.u), ends(&f.cn.v))
            })
            .unwrap();
        let um = 0.5 * (u0 + u1);
        let before = (w.pieces.len(), w.items.len(), w.total());
        w.add_piece(face, &segment([um, v0], [um, v1]).unwrap(), 1.0)
            .unwrap();
        let after = (w.pieces.len(), w.items.len(), w.total());
        assert_eq!((before.0, before.1), (after.0, after.1));
        assert_eq!(
            (before.2.lo.to_bits(), before.2.hi.to_bits()),
            (after.2.lo.to_bits(), after.2.hi.to_bits())
        );
        w.add_piece(face, &segment([u0, v1], [u1, v1]).unwrap(), 1.0)
            .unwrap();
        assert!(w.items.len() > before.1);
    }

    /// The box 40 x 30 x 20 with its edge through (20, 30, 20) rounded at radius 3: the
    /// round's face is rational and lies 20 to 40 from the origin.
    fn rounded_box(k: &mut Kernel) -> Brep {
        let mut b = crate::primitive::api_primitives::create_box(
            k,
            Point3::new(0.0, 0.0, 0.0),
            40.0,
            30.0,
            20.0,
        )
        .unwrap();
        let q = Point3::new(20.0, 30.0, 20.0);
        let e = b
            .edge_list()
            .iter()
            .copied()
            .find(|&e| {
                crate::query::api_queries::edge_closest_point(k, &b, e, q)
                    .is_ok_and(|c| c.closest.distance < 1e-9)
            })
            .unwrap();
        crate::blend::fillet_executive::api::fillet_edges(k, &mut b, &[e], 3.0, 1, 1, 1.0).unwrap();
        b
    }

    /// A cell of the round's face (a rational face 20 to 40 from the origin), a sixteenth of
    /// its piece each way: the partials taken about the cell's middle ([`piece_partials`])
    /// and about the origin ([`nurbs_partials_within_span`]) both hold the face's partials
    /// at a grid of points of the cell (each point's own interval enclosure lies inside them,
    /// so its exact partial does). Taken about the cell, each component of the first partial
    /// along the direction the weights vary in is at least `NARROWER` times narrower (7.8 to
    /// 17 times, measured). A box that is one point is enclosed exactly as before (no shift).
    /// Taken about the origin, the quotient's `w_ij S` terms are as wide
    /// as the face's distance from the origin: the area enclosure's cells then never proved
    /// `|S_u x S_v|^2` positive, fell back to range times area, and spent the whole budget.
    #[test]
    fn a_far_rational_cells_partials_are_taken_about_the_cell() {
        use crate::blend::fillet_section::certificate::nurbs_partials_within_span;
        use crate::blend::fillet_section::station::nurbs_partials;
        const NARROWER: f64 = 6.0;
        let mut k = Kernel::default();
        let mut b = rounded_box(&mut k);
        let w = Work::new(&k, &mut b).unwrap();
        let weight = |cn: &Nurbs, i: usize, j: usize| cn.pw[4 * (i * cn.nv + j) + 3];
        // whether the weights vary along u (else along v), on a rational face
        let varies = |cn: &Nurbs| {
            let along_u =
                (0..cn.nv).any(|j| (1..cn.nu).any(|i| weight(cn, i, j) != weight(cn, 0, j)));
            let along_v =
                (0..cn.nu).any(|i| (1..cn.nv).any(|j| weight(cn, i, j) != weight(cn, i, 0)));
            (along_u || along_v).then_some(along_u)
        };
        let (cn, along_u) = w
            .faces
            .iter()
            .find_map(|f| varies(&f.cn).map(|d| (&f.cn, d)))
            .unwrap();
        let mid = |k: &[f64]| 0.5 * (k.first().unwrap() + k.last().unwrap());
        let (ku, kv) = (
            span(&cn.u, cn.p, cn.nu, mid(&cn.u)),
            span(&cn.v, cn.q, cn.nv, mid(&cn.v)),
        );
        // a sixteenth of the piece each way, about its middle
        let cell = |k: &[f64], s: usize| {
            let (a, b) = (knot(k, s as isize), knot(k, s as isize + 1));
            let (m, h) = (0.5 * (a + b), (b - a) / 32.0);
            (m - h, m + h)
        };
        let ((ua, ub), (va, vb)) = (cell(&cn.u, ku), cell(&cn.v, kv));
        let lk = 2;
        let new = piece_partials(cn, (ku, kv), Ivl::new(ua, ub), Ivl::new(va, vb), lk).unwrap();
        let old = nurbs_partials_within_span(cn, ku, kv, ua, ub, va, vb, lk).unwrap();
        let comps = |p: V3<Ivl>| [p.x, p.y, p.z];
        let inside = |p: Ivl, e: Ivl| e.lo <= p.lo && p.hi <= e.hi;
        // the grid's last point is the box's end exactly (a + (b - a) may round past b)
        let grid = |a: f64, b: f64, n: i32| {
            if n == 4 {
                b
            } else {
                a + (b - a) * f64::from(n) / 4.0
            }
        };
        for i in 0..=4 {
            for j in 0..=4 {
                let u = grid(ua, ub, i);
                let v = grid(va, vb, j);
                let at = nurbs_partials(cn, ku, kv, Ivl::from(u), Ivl::from(v), lk);
                for a in 0..=lk {
                    for c in 0..=(lk - a) {
                        let x = comps(at.at(a, c));
                        for (name, e) in [("about the cell", &new), ("about the origin", &old)] {
                            for (xe, ee) in x.iter().zip(comps(e.at(a, c))) {
                                assert!(
                                    inside(*xe, ee),
                                    "{name}: P[{a}][{c}] at ({u}, {v}): {xe:?} not in {ee:?}"
                                );
                            }
                        }
                    }
                }
            }
        }
        let width = |i: Ivl| i.hi - i.lo;
        let (a, c) = if along_u { (1, 0) } else { (0, 1) };
        for (n, o) in comps(new.at(a, c)).into_iter().zip(comps(old.at(a, c))) {
            assert!(
                NARROWER * width(n) <= width(o),
                "P[{a}][{c}]: {n:?} against {o:?}"
            );
        }
        // a box that is one point is enclosed as before, exactly
        let (u, v) = (0.5 * (ua + ub), 0.5 * (va + vb));
        assert_eq!(
            piece_partials(cn, (ku, kv), Ivl::from(u), Ivl::from(v), lk).unwrap(),
            nurbs_partials_within_span(cn, ku, kv, u, u, v, v, lk).unwrap()
        );
    }
}
