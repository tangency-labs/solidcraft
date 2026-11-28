//! A planar region extruded along its frame's normal between two levels: the prism the planar
//! extrusion of `solidcore-ops` joins its pieces from.
//!
//! Not upstream: the placement SolidCraft's extrusion of planar regions made (`crate::extrude`
//! for one region), moved into the kernel (docs/layers.md §1). The solid is the kernel's linear
//! sweep (`SmApiCreateLinearSweep`) of the region's curves.
//!
//! The levels of an extrude's ends on planes at a slant to it ([`level_on_plane`],
//! [`plane_rise`], [`plane_gap`]), and the planes and sides an extrude's tools are cut between
//! ([`plane_raised`], [`slab_sides`]), were SolidCraft's extrude From/To code, moved into the
//! kernel the same way (#21934).
//!
//! Deviations: none (no upstream counterpart). No recursion; the search over a free-form span
//! is bounded by [`MAX_HALVINGS`] and [`MAX_PIECES`] and refuses past them.

#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-refusing tests

use crate::math::det;
use std::f64::consts::{PI, TAU};

use super::profile::{axes, curve_on_frame, gamma, halves, point_of, Region2, Seg2, H2};
use crate::error::{Result, SolidError};
use crate::eval::Frame;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::math::{Point2, Vec2};
use crate::sweep::api_brep::create_linear_sweep;
use crate::topology::Brep;

const OP: &str = "prism::extrude::extrude_region";

/// The solid of `region` (in `frame`'s coordinates) from level `lo` to level `hi` above the
/// frame: its loops' curves placed on the frame at `lo` ([`curve_on_frame`]) and swept along the
/// frame's normal by `hi - lo`, capped at both ends.
///
/// # Errors
///
/// `InvalidInput` for levels not finite or not increasing; those of the curves and the sweep.
pub fn extrude_region(
    k: &mut Kernel,
    frame: &Frame,
    region: &Region2,
    lo: f64,
    hi: f64,
) -> Result<Brep> {
    if !(lo.is_finite() && hi.is_finite() && lo < hi) {
        return Err(SolidError::invalid_input(
            OP,
            "levels not finite and increasing",
        ));
    }
    let curves = region
        .loops()
        .flat_map(|lp| &lp.segs)
        .map(|s| curve_on_frame(k, frame, lo, s))
        .collect::<Result<Vec<_>>>()?;
    let refs: Vec<_> = curves.iter().map(Some).collect();
    let (_, _, _, n) = axes(frame);
    create_linear_sweep(k, &refs, n, hi - lo, true)
}

/// Positive and negative wing `(start, end)` intervals for a symmetric-offset extrude with
/// clearance `gap` on each side of the symmetry plane and `half` distance per wing.
#[must_use]
pub fn symmetric_offset_intervals(gap: f64, half: f64) -> [(f64, f64); 2] {
    let gap = gap.abs();
    [(gap, gap + half), (-gap - half, -gap)]
}

/// Probe points along both wings of a symmetric-offset extrude for auto-operation detection
/// (cut vs join), given the sketch-plane offset `start`, total extent `d`, and `near_limit`.
#[must_use]
pub fn symmetric_offset_probe_points(
    base: [f64; 3],
    n: [f64; 3],
    start: f64,
    d: f64,
    near_limit: f64,
) -> [[f64; 3]; 4] {
    let gap = start.abs();
    let half = d.abs() * 0.5;
    let near = (half * 0.5).min(near_limit);
    let mid = half * 0.5;
    let at = |s: f64| [base[0] + n[0] * s, base[1] + n[1] * s, base[2] + n[2] * s];
    [
        at(-start + gap + near),
        at(-start + gap + mid),
        at(-start - gap - near),
        at(-start - gap - mid),
    ]
}

const OP_LEVEL: &str = "prism::extrude::level_on_plane";
const OP_RISE: &str = "prism::extrude::plane_rise";
const OP_GAP: &str = "prism::extrude::plane_gap";
const OP_RAISED: &str = "prism::extrude::plane_raised";
const OP_SLAB: &str = "prism::extrude::slab_sides";

/// Halvings of a free-form span past which [`span_extremes`] refuses: at 2^-60 of the span's
/// parameter a piece's enclosure has long reached its rounding (it shrinks to about a quarter
/// with each halving), so a span still apart there is not one the search can settle.
const MAX_HALVINGS: usize = 60;

/// Pieces of one free-form span [`span_extremes`] looks at before it refuses. It keeps only the
/// pieces near an extreme, a few at each halving, so this bounds the work and is not reached by
/// a span whose enclosures settle.
const MAX_PIECES: u32 = 4096;

/// A plane's level above a frame, which changes linearly across the frame's sketch:
/// `at0 + slope · q` at sketch point `q`.
#[derive(Clone, Copy, Debug)]
struct Level {
    at0: f64,
    slope: Vec2,
}

impl Level {
    /// The level above `frame` of the point of `plane` on the line along `frame`'s normal `n`
    /// (`x × y`) through each sketch point: `(P - o)·n + t (n·n)` at `P = o + x q.x + y q.y`,
    /// where the line `P + t n` meets `plane` (`t = (p - P)·m / (n·m)`, `p` and `m` the plane's
    /// origin and normal). Whether the line meets the plane is decided only past the rounding of
    /// `n·m` ([`meeting`]), and the origins are subtracted before anything is scaled, so the
    /// rounding follows the part's size, not its distance from the world origin.
    fn of(frame: &Frame, plane: &Frame, op: &'static str) -> Result<Self> {
        let (o, x, y, n) = axes(frame);
        let (p, _, _, _) = axes(plane);
        let (across, m) = meeting(n, plane, op)?;
        let nn = n.dot(n);
        let level = Self {
            at0: (p - o).dot(m) / across * nn,
            slope: Vec2::new(
                x.dot(n) - x.dot(m) / across * nn,
                y.dot(n) - y.dot(m) / across * nn,
            ),
        };
        if !(level.at0.is_finite() && level.slope.x.is_finite() && level.slope.y.is_finite()) {
            return Err(SolidError::invalid_input(op, "a level that is not finite"));
        }
        Ok(level)
    }

    /// The level at sketch point `q`.
    fn at(self, q: Point2) -> f64 {
        self.at0 + self.slope.dot(q)
    }
}

/// `n·m` and `m`, the normal `x × y` of `plane` as computed, refused where the sign of `n·m` is
/// within its rounding: there the line along `n` may run along the plane and never meet it.
/// `n` is the direction as given. The rounding counts the dot product, `gamma(3) |n| |m|`, and
/// the cross product's own error, each component of `m` within `gamma(2) |x| |y|` of `x × y`
/// (Higham §3.1; Cauchy-Schwarz on the component's two products), so `gamma(2) |n|_1 |x| |y|`
/// through the dot product. Both terms take `gamma(5)`, which also covers the few roundings of
/// the bound itself.
fn meeting(n: Vec3, plane: &Frame, op: &'static str) -> Result<(f64, Vec3)> {
    let (_, x, y, m) = axes(plane);
    let across = n.dot(m);
    let n_1 = n.x.abs() + n.y.abs() + n.z.abs();
    let rounding = gamma(5) * (n.length() * m.length() + n_1 * x.length() * y.length());
    if !(across.abs() > rounding) {
        return Err(SolidError::invalid_input(
            op,
            "the plane runs along the extrusion",
        ));
    }
    Ok((across, m))
}

/// Where the line along `frame`'s normal through `point` meets `plane` (the plane through
/// `plane`'s origin square to its normal `x × y`), as a level along that normal measured from
/// `frame`'s origin: `(point - o)·n + t (n·n)`, the line `point + t n` meeting the plane
/// ([`Level::of`] for the rounding).
///
/// # Errors
///
/// `InvalidInput` for a plane that runs along the normal, which the line never meets, or a
/// level that is not finite.
pub fn level_on_plane(frame: &Frame, point: Point3, plane: &Frame) -> Result<f64> {
    let (o, _, _, n) = axes(frame);
    let (p, _, _, _) = axes(plane);
    let (across, m) = meeting(n, plane, OP_LEVEL)?;
    let level = (point - o).dot(n) + (p - point).dot(m) / across * n.dot(n);
    if !level.is_finite() {
        return Err(SolidError::invalid_input(
            OP_LEVEL,
            "a level that is not finite",
        ));
    }
    Ok(level)
}

/// The least and the most level of `plane` above `frame` over the outer loops of `regions` (in
/// `frame`'s coordinates): the rise of a plane at a slant to the extrusion across the profiles
/// ([`affine_extremes`]).
///
/// # Errors
///
/// `InvalidInput` for a plane along the extrusion, no outer segment or a malformed segment;
/// `NotConverged` for a free-form piece whose search does not settle.
pub fn plane_rise(frame: &Frame, plane: &Frame, regions: &[Region2]) -> Result<(f64, f64)> {
    affine_extremes(Level::of(frame, plane, OP_RISE)?, regions, OP_RISE)
}

/// The least and the most of `to`'s level above `frame` less `from`'s, over the outer loops of
/// `regions`: the gap an extrude From one plane To another runs across the profiles
/// ([`affine_extremes`]). Where the gap changes sign over the profiles the planes cross over
/// them, and part of the profiles would have no extent; how close to zero still counts as
/// touching is the caller's to choose.
///
/// # Errors
///
/// As [`plane_rise`].
pub fn plane_gap(
    frame: &Frame,
    from: &Frame,
    to: &Frame,
    regions: &[Region2],
) -> Result<(f64, f64)> {
    let (from, to) = (
        Level::of(frame, from, OP_GAP)?,
        Level::of(frame, to, OP_GAP)?,
    );
    let gap = Level {
        at0: to.at0 - from.at0,
        slope: to.slope - from.slope,
    };
    affine_extremes(gap, regions, OP_GAP)
}

/// `plane` moved by `by` along `frame`'s normal `n` (`x × y`): its origin moved to
/// `origin + by n`, its axes kept. Its level above `frame` is `by` (times `n·n`, 1 for an
/// orthonormal frame) more than `plane`'s at every sketch point: an extrude's end on a slanted
/// face moved along the extrusion.
///
/// # Errors
///
/// `InvalidInput` for a frame with no normal, axes that are not finite, or a distance or an
/// origin that is not finite.
pub fn plane_raised(frame: &Frame, plane: &Frame, by: f64) -> Result<Frame> {
    let (_, _, _, n) = axes(frame);
    let (_, x, y, _) = axes(plane);
    let finite = |v: Vec3| v.x.is_finite() && v.y.is_finite() && v.z.is_finite();
    if !(finite(n) && n.length() > 0.0 && finite(x) && finite(y)) {
        return Err(SolidError::invalid_input(
            OP_RAISED,
            "a frame with no normal or axes that are not finite",
        ));
    }
    let origin = Vec3::from(plane.origin) + n * by;
    if !(by.is_finite() && finite(origin)) {
        return Err(SolidError::invalid_input(
            OP_RAISED,
            "a distance or an origin that is not finite",
        ));
    }
    Ok(Frame {
        origin: origin.to_array(),
        ..*plane
    })
}

/// Which side of each plane holds what lies past `from` and short of `to` along `travel`: an
/// extrude's tools cut between its two ends. Each answer is `true` for the side the plane's
/// normal (`x × y`) points to: past `from` when `travel` runs with `from`'s normal, short of
/// `to` when it runs against `to`'s. A plane is refused as running along `travel` only where the
/// sign of their dot product is within its rounding ([`meeting`], as [`level_on_plane`]
/// refuses it): there the travel may never cross the plane.
///
/// # Errors
///
/// `InvalidInput` for a plane along `travel` (or a `travel` that is zero or not finite).
pub fn slab_sides(travel: Vec3, from: &Frame, to: &Frame) -> Result<(bool, bool)> {
    let past_from = meeting(travel, from, OP_SLAB)?.0 > 0.0;
    let short_of_to = meeting(travel, to, OP_SLAB)?.0 < 0.0;
    Ok((past_from, short_of_to))
}

/// The least and the most of `level` over the outer loops of `regions`. Over a line they lie at
/// its ends; over an arc at its ends or at its points farthest along and against the level's
/// slope where the arc passes them. Over a free-form piece (cubic, conic, Bézier) they are
/// searched for by [`span_extremes`], which returns values the curve takes.
fn affine_extremes(level: Level, regions: &[Region2], op: &'static str) -> Result<(f64, f64)> {
    let malformed = || SolidError::invalid_input(op, "a malformed profile segment");
    // the angle of the sketch direction along which the level rises fastest
    let rise = (level.slope.x != 0.0 || level.slope.y != 0.0)
        .then(|| det::atan2(level.slope.y, level.slope.x));
    let mut range: Option<(f64, f64)> = None;
    let mut take = |v: f64| -> Result<()> {
        if !v.is_finite() {
            return Err(malformed());
        }
        let (lo, hi) = range.unwrap_or((v, v));
        range = Some((lo.min(v), hi.max(v)));
        Ok(())
    };
    for seg in regions.iter().flat_map(|r| &r.outer.segs) {
        match *seg {
            Seg2::Line { a, b } => {
                take(level.at(a))?;
                take(level.at(b))?;
            }
            Seg2::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                take(level.at(seg.start()))?;
                take(level.at(seg.end()))?;
                for angle in rise.into_iter().flat_map(|r| [r, r + PI]) {
                    // The arc's point at angle `t` is `center + (cos t, sin t) radius`, so with a
                    // negative radius the point farthest along a direction lies at the opposite
                    // angle. Whether the arc passes that angle, going from `start` by `sweep`:
                    let toward = if radius < 0.0 { angle + PI } else { angle };
                    let turn = (toward - start) * sweep.signum();
                    if turn.rem_euclid(TAU) <= sweep.abs() {
                        take(level.at(center + Vec2::new(det::cos(toward), det::sin(toward)) * radius))?;
                    }
                }
            }
            _ => {
                for span in seg.spans().ok_or_else(malformed)? {
                    let (lo, hi) = span_extremes(level, span, op)?;
                    take(lo)?;
                    take(hi)?;
                }
            }
        }
    }
    range.ok_or_else(|| SolidError::invalid_input(op, "no outer profile segment"))
}

/// The least and the most of `level` over one homogeneous Bézier span, as values it takes at
/// points of the span. The level's values at the span's control points enclose it (the convex
/// hull property, the weights positive), so a piece is halved (de Casteljau) until its
/// enclosure lies within the rounding of those values of the extremes found at the ends of its
/// pieces, which lie on the curve.
///
/// That rounding is taken from the whole span, never from the piece: near an extreme where the
/// level is 0 a piece's coordinates shrink with its enclosure, and a rounding that shrank with
/// them would never let the search settle. Every piece's points lie in the hull of the span's,
/// so `|at0| + |slope.x| X + |slope.y| Y` (`X`, `Y` the span's largest `|q.x|`, `|q.y|`) bounds
/// each level it evaluates; that takes `gamma(8)` (a coordinate's division by its weight, then
/// a dot product of two, Higham §3.1). A halving of degree `k` averages each homogeneous
/// coordinate `k` times, and an average rounds at most once by the span's largest magnitude of
/// that coordinate, so after `d` halvings a coordinate is off by `k d u` of it, and its point
/// by `2 k d u rho` of the span's largest (`rho` the span's largest weight over its least,
/// dividing by a weight). Together the rounding at depth `d` is
/// `gamma(8 + 2 k d) rho (|at0| + |slope.x| X + |slope.y| Y)`. What is returned is a value the
/// curve takes, within twice the deepest piece's rounding of its true extreme.
///
/// # Errors
///
/// `InvalidInput` for a weight not positive or a control point not finite; `NotConverged` past
/// [`MAX_HALVINGS`] or [`MAX_PIECES`].
fn span_extremes(level: Level, span: Vec<H2>, op: &'static str) -> Result<(f64, f64)> {
    let malformed = || SolidError::invalid_input(op, "a malformed profile segment");
    if span
        .iter()
        .any(|q| !(q[2] > 0.0 && q.iter().all(|x| x.is_finite())))
    {
        return Err(malformed());
    }
    let degree = span.len().saturating_sub(1);
    let (mut x, mut y, mut w_lo, mut w_hi) = (0.0_f64, 0.0_f64, f64::INFINITY, 0.0_f64);
    for q in &span {
        let at = point_of(*q);
        x = x.max(at.x.abs());
        y = y.max(at.y.abs());
        w_lo = w_lo.min(q[2]);
        w_hi = w_hi.max(q[2]);
    }
    let size =
        (w_hi / w_lo) * (level.at0.abs() + level.slope.x.abs() * x + level.slope.y.abs() * y);
    if !size.is_finite() {
        return Err(malformed());
    }
    let ends = |c: &[H2]| -> Result<(f64, f64)> {
        let first = c.first().ok_or_else(malformed)?;
        let last = c.last().ok_or_else(malformed)?;
        let (a, b) = (level.at(point_of(*first)), level.at(point_of(*last)));
        Ok((a.min(b), a.max(b)))
    };
    let mut todo = vec![(span, 0usize)];
    let (mut least, mut most) = (f64::INFINITY, f64::NEG_INFINITY);
    let mut pieces = 0u32;
    while let Some((c, depth)) = todo.pop() {
        pieces += 1;
        if pieces > MAX_PIECES || depth > MAX_HALVINGS {
            return Err(SolidError::NotConverged {
                operation: op,
                iterations: pieces,
            });
        }
        if c.iter()
            .any(|q| !(q[2] > 0.0 && q.iter().all(|x| x.is_finite())))
        {
            return Err(malformed());
        }
        let (lo, hi) = ends(&c)?;
        least = least.min(lo);
        most = most.max(hi);
        let (mut hull_lo, mut hull_hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for q in &c {
            let v = level.at(point_of(*q));
            hull_lo = hull_lo.min(v);
            hull_hi = hull_hi.max(v);
        }
        let rounding = gamma(8 + 2 * degree * depth) * size;
        if hull_lo >= least - rounding && hull_hi <= most + rounding {
            continue;
        }
        let (l, r) = halves(&c);
        todo.push((l, depth + 1));
        todo.push((r, depth + 1));
    }
    if !(least.is_finite() && most.is_finite()) {
        return Err(malformed());
    }
    Ok((least, most))
}
