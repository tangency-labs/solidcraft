//! Planar profiles as the kernel holds them: closed loops of lines, circular arcs, conics and
//! Béziers in a frame's own coordinates, and regions (an outer loop less its holes). Plain data,
//! beside [`crate::eval::geometry`]; applications built on solidcore keep their own records of
//! these and copy them in at their boundary. The maths on them is here: a segment's points and
//! its homogeneous Bézier form, a loop's signed area with a proved bound on its rounding, the
//! even–odd hold of a point decided exactly or refused, the checks a profile must pass, the split
//! of a loop where it passes a point twice, and a segment made the kernel's curve on a frame.
//!
//! Not upstream (SolidCraft's planar regions, moved into the kernel so the layers above compute
//! no geometry, docs/layers.md §1).
//!
//! References: Piegl & Tiller, *The NURBS Book* (2nd ed.), §1.4 (de Casteljau), §7.3 (the
//! rational quadratic arc, its middle weight the cosine of half its angle); Farin, *Curves and
//! Surfaces for CAGD* (5th ed.), §5.2 (Bernstein products), §12.4 (the area of a conic arc);
//! Higham, *Accuracy and Stability of Numerical Algorithms* (2nd ed.), §3.1 (`gamma_k`).
//!
//! Deviations: none (no upstream counterpart). No recursion.

#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-refusing tests, kept as written

use std::f64::consts::{FRAC_PI_2, TAU};

use crate::curve::curve::Curve;
use crate::curve::ellipse::Ellipse;
use crate::curve::line::Line;
use crate::error::{Result, SolidError};
use crate::eval::Frame;
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::extent1d::Extent1d;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::math::{Point2, Vec2, EFF_ZERO};
use crate::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
use crate::nurbs::crv_basic::CONIC_ANGLE_TOL_DEG;
use crate::nurbs::crv_common::DEFAULT_ANGLE_TOL;
use crate::nurbs::math::MODEL_TOL;

const OP_CURVE: &str = "prism::profile::curve_on_frame";
const OP_AREA: &str = "prism::profile::Loop2::signed_area";
const OP_BIARC: &str = "prism::profile::biarc";
const OP_STRAIGHT_BIARC: &str = "prism::profile::biarc_with_straight_piece";
const OP_FIT_BIARC: &str = "prism::profile::fit_biarc";

/// Counter-clockwise angle from the ray `center`–`start` to `center`–`end`, in
/// radians modulo one turn. Equal rays give zero; this does not infer a full circle.
/// Returns `None` for a nonfinite or zero ray. The angle is measured by the
/// difference of the two polar angles, retaining tiny sweeps across atan2's seam.
#[must_use]
pub fn counterclockwise_sweep(center: Point2, start: Point2, end: Point2) -> Option<f64> {
    let (ax, ay) = (start.x - center.x, start.y - center.y);
    let (bx, by) = (end.x - center.x, end.y - center.y);
    if ![ax, ay, bx, by].iter().all(|v| v.is_finite())
        || (ax == 0.0 && ay == 0.0)
        || (bx == 0.0 && by == 0.0)
    {
        return None;
    }
    Some((by.atan2(bx) - ay.atan2(ax)).rem_euclid(TAU))
}

/// The margin (degrees) kept from the circle builder's angle windows ([`CONIC_ANGLE_TOL_DEG`],
/// [`DEFAULT_ANGLE_TOL`]): the kernel's circle builder
/// re-derives the arc's end angle (`atan2` of its end point, in degrees), which rounds by a few
/// ulps of 360 (an ulp of 360 is 5.7e-14), so a sweep this close to a window's edge could fall
/// either side of it. 1e-11 is about 175 ulps of 360.
const ARC_WINDOW_MARGIN_DEG: f64 = 1e-11;

/// How an arc is built: over `[0, deg]` degrees of its own circle, or as the closed circle.
#[derive(Clone, Copy, Debug, PartialEq)]
enum ArcBuild {
    /// An arc of this many degrees.
    Open(f64),
    /// The whole circle, `[0, 360]`.
    Closed,
}

/// The largest `|sweep|` an arc of `radius` may have and still be built at the zone tolerance
/// `tol`: `TAU + tol / radius`. Past a whole turn the arc is built as its closed circle, whose end
/// is the arc's start: it lies from the arc's end by the chord `2 radius sin(o / 2) <= radius o`
/// of the overshoot `o = |sweep| - TAU`, so within `tol` exactly while `o <= tol / radius`.
#[must_use]
pub fn largest_arc_sweep(radius: f64, tol: f64) -> f64 {
    TAU + tol / radius
}

/// The `|sweep|` an arc of `radius` must exceed to be built at the zone tolerance `tol`: the
/// larger of `tol / radius` (at or below it the arc's length `radius |sweep|`, and so the chord
/// between its ends, is within `tol`, the size below which points are one) and
/// [`CONIC_ANGLE_TOL_DEG`] plus `ARC_WINDOW_MARGIN_DEG`, in radians (below that the kernel's circle
/// builder makes the full turn, whatever the radius).
#[must_use]
pub fn smallest_arc_sweep(radius: f64, tol: f64) -> f64 {
    (tol / radius).max((CONIC_ANGLE_TOL_DEG + ARC_WINDOW_MARGIN_DEG).to_radians())
}

/// How an arc of `radius` turning `sweep` is built at the zone tolerance `tol`, or why it cannot
/// be (`radius` and `sweep` finite, `radius > 0`, `tol` finite and positive):
/// - its whole circle within `tol` (`radius TAU <= tol`): refused, whatever the sweep. Past this
///   check `tol / radius < TAU`, so [`largest_arc_sweep`] is below two turns: every sweep that
///   builds is one [`Seg2::spans`] takes and the conversion makes (at most one turn).
/// - `|sweep|` past [`largest_arc_sweep`]: refused.
/// - `|sweep|` at most [`smallest_arc_sweep`]: refused.
/// - `|sweep|` within [`DEFAULT_ANGLE_TOL`] (0.01 degrees, plus the margin) of a whole turn, or
///   past it: the kernel's circle builder (`Ellipse::make_nurb` passes that angle tolerance to
///   the quadratic arc, which closes any arc within it of a turn by moving its last control point
///   onto its first; [`CONIC_ANGLE_TOL_DEG`]'s window is inside it) makes such a sweep the full
///   turn, so it is built as the closed circle outright, whose end lies from the arc's end by the
///   chord of `| |sweep| - TAU |`, at most `radius | |sweep| - TAU |`: kept while that is within
///   `tol`, else refused (only short of a turn: past it the check above already holds it), so a
///   sweep short of a turn must stop more than the window short of it to build as an arc.
/// - otherwise the arc over `[0, |sweep|]` degrees: its ends are the closed form.
fn arc_build(radius: f64, sweep: f64, tol: f64) -> std::result::Result<ArcBuild, &'static str> {
    let a = sweep.abs();
    if radius * TAU <= tol {
        return Err("an arc on a circle whose whole length is within the zone tolerance");
    }
    if a > largest_arc_sweep(radius, tol) {
        return Err(
            "an arc past a whole turn by more than the zone tolerance at its end: the largest \
             that builds sweeps largest_arc_sweep(radius, zone_tol_3d) = TAU + zone_tol_3d / radius",
        );
    }
    if a <= smallest_arc_sweep(radius, tol) {
        return Err(
            "an arc too short to build: its |sweep| must exceed smallest_arc_sweep(radius, \
             zone_tol_3d), the larger of zone_tol_3d / radius and 1e-6 degrees",
        );
    }
    let deg = a.to_degrees();
    if deg >= 360.0 - DEFAULT_ANGLE_TOL - ARC_WINDOW_MARGIN_DEG {
        if radius * (a - TAU).abs() <= tol {
            Ok(ArcBuild::Closed)
        } else {
            Err(
                "an arc ending short of a whole turn by less than 0.01 degrees but by more than \
                 the zone tolerance at its end: the kernel builds it as the closed circle",
            )
        }
    } else {
        Ok(ArcBuild::Open(deg))
    }
}

/// Highest degree of a [`Seg2::Bezier`].
pub const MAX_BEZIER_DEGREE: usize = 9;

/// Control points a [`Seg2::Bezier`] carries: its highest degree plus one (the tail unused).
pub const MAX_BEZIER_CTRL: usize = MAX_BEZIER_DEGREE + 1;

/// The most segments one call takes over all its loops.
pub const MAX_SEGMENTS: usize = 8192;

/// One boundary segment, in a frame's coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg2 {
    /// The line from `a` to `b`.
    Line {
        /// Its start.
        a: Point2,
        /// Its end.
        b: Point2,
    },
    /// The arc about `center` from angle `start` sweeping `sweep` radians (positive:
    /// counter-clockwise).
    Arc {
        /// Its centre.
        center: Point2,
        /// Its radius.
        radius: f64,
        /// Its start angle.
        start: f64,
        /// Its signed sweep.
        sweep: f64,
    },
    /// The cubic Bézier of `p0 .. p3`.
    Cubic {
        /// Its start.
        p0: Point2,
        /// The first inner control point.
        p1: Point2,
        /// The second inner control point.
        p2: Point2,
        /// Its end.
        p3: Point2,
    },
    /// The rational quadratic Bézier from `a` to `b` with control `apex` of weight `w` (end
    /// weights 1): an exact conic arc.
    Conic {
        /// Its start.
        a: Point2,
        /// The apex, where the end tangents meet.
        apex: Point2,
        /// Its end.
        b: Point2,
        /// The apex's weight.
        w: f64,
    },
    /// The Bézier of degree `n` (1 ..= [`MAX_BEZIER_DEGREE`]) over `p[0..=n]`.
    Bezier {
        /// Its degree.
        n: u8,
        /// Its control points (the tail past `n` unused).
        p: [Point2; MAX_BEZIER_CTRL],
    },
}

/// A closed loop of segments, each starting where the one before ends.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Loop2 {
    /// The segments in order.
    pub segs: Vec<Seg2>,
}

/// A planar region: its outer loop (counter-clockwise) less its holes (clockwise).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Region2 {
    /// The outer loop.
    pub outer: Loop2,
    /// The holes.
    pub holes: Vec<Loop2>,
}

impl Region2 {
    /// The outer loop, then the holes.
    pub fn loops(&self) -> impl Iterator<Item = &Loop2> + Clone {
        std::iter::once(&self.outer).chain(&self.holes)
    }

    /// Its signed area, its loops' summed (holes running clockwise take theirs away), with a
    /// proved bound on its rounding.
    ///
    /// # Errors
    ///
    /// Those of [`Loop2::signed_area`].
    pub fn signed_area(&self) -> Result<Area> {
        let mut value = 0.0;
        let mut bound = 0.0;
        let mut size = 0.0_f64;
        for lp in self.loops() {
            let a = lp.signed_area()?;
            value += a.value;
            bound += a.bound;
            size += a.value.abs();
        }
        Ok(Area {
            value,
            bound: bound + gamma(self.holes.len() + 2) * size,
        })
    }
}

/// A homogeneous control point `[x w, y w, w]`.
pub type H2 = [f64; 3];

/// `gamma_k = k u / (1 - k u)` (Higham §3.1): the kernel's one copy.
pub(crate) use crate::nurbs::math::gamma;

pub(crate) fn cross2(a: Vec2, b: Vec2) -> f64 {
    a.x * b.y - a.y * b.x
}

fn unit(angle: f64) -> Vec2 {
    Vec2::new(angle.cos(), angle.sin())
}

fn finite2(p: Point2) -> bool {
    p.x.is_finite() && p.y.is_finite()
}

fn length2d(direction: Vec2) -> f64 {
    direction.x.hypot(direction.y)
}

/// Move a point by a fraction of a planar displacement, rounding the scale before addition.
#[inline]
#[must_use]
pub fn translate_point(point: Point2, displacement: Vec2, fraction: f64) -> Point2 {
    point + displacement * fraction
}

/// The directed displacement from `a` to `b` and its length.
#[inline]
#[must_use]
pub fn chord(a: Point2, b: Point2) -> (Vec2, f64) {
    let direction = b - a;
    (direction, length2d(direction))
}

/// A point at an unrestricted fraction along the chord from `a` to `b`.
#[inline]
#[must_use]
pub fn chord_point(a: Point2, b: Point2, fraction: f64) -> Point2 {
    translate_point(a, b - a, fraction)
}

/// Left unit normal of the directed chord, absent at or below `minimum_length` or for
/// a non-finite chord.
#[must_use]
pub fn left_unit_normal(a: Point2, b: Point2, minimum_length: f64) -> Option<Vec2> {
    let (direction, length) = chord(a, b);
    if !(length.is_finite() && length > minimum_length && length > 0.0) {
        return None;
    }
    let unit = direction / length;
    Some(Vec2::new(-unit.y, unit.x))
}

/// Place a positive tangent guide. Use the supplied vector when its tip fits and its length
/// reaches `min_length`. Otherwise try each permitted length along its direction in order.
/// A tip fits when it is distinct from `at` and each coordinate's magnitude is below `coord_limit`.
/// Its recovered unit direction must stay within half of `angle_tolerance`, measured as a chord
/// of the unit circle, reserving the other half for reading the generated circular contact.
#[must_use]
pub fn tangent_guide_tip(
    at: Point2,
    tangent: Vec2,
    min_length: f64,
    angle_tolerance: f64,
    fallback_lengths: &[f64],
    coord_limit: f64,
) -> Option<Point2> {
    if !(angle_tolerance.is_finite() && angle_tolerance > 0.0) {
        return None;
    }
    let op = "prism::profile::tangent_guide_tip";
    let direction = biarc_direction(tangent, op).ok()?;
    let usable = |tip: Point2| {
        finite2(tip)
            && tip.max_dimension() < coord_limit
            && biarc_direction(tip - at, op).is_ok_and(|placed| {
                placed.dot(direction) > 0.0 && biarc_near(placed, direction, 0.5 * angle_tolerance)
            })
    };
    let tip = at + tangent;
    if usable(tip) && length2d(tangent) >= min_length {
        return Some(tip);
    }
    fallback_lengths
        .iter()
        .copied()
        .filter(|length| length.is_finite() && *length > 0.0)
        .map(|length| translate_point(at, direction, length))
        .find(|tip| usable(*tip))
}

/// Lengthen a finite nonzero direction to `min_length` if it is shorter than that.
#[must_use]
pub fn lengthen_direction(direction: Vec2, min_length: f64) -> Vec2 {
    let length = length2d(direction);
    if length.is_finite() && length > 0.0 && length < min_length {
        direction / length * min_length
    } else {
        direction
    }
}

/// Read a stored circular arc from its center and directed endpoints. The counterclockwise
/// arc reads its radius from its first radial point and wraps a sweep at or below `EFF_ZERO`
/// through a whole turn. A clockwise arc is read in the opposite order and then reversed.
#[must_use]
pub fn stored_arc(center: Point2, start: Point2, end: Point2, reversed: bool) -> Seg2 {
    let (a, b) = if reversed { (end, start) } else { (start, end) };
    let first = a - center;
    let last = b - center;
    let start = first.y.atan2(first.x);
    let mut sweep = last.y.atan2(last.x) - start;
    // Each angle lies in [-π, π], so there can be at most two wraps. Both may
    // be needed across the branch cut at ±π with signed zero coordinates.
    for _ in 0..2 {
        if sweep <= EFF_ZERO {
            sweep += TAU;
        }
    }
    let arc = Seg2::Arc {
        center,
        radius: length2d(first),
        start,
        sweep,
    };
    if reversed {
        arc.reversed()
    } else {
        arc
    }
}

/// Whether a stored arc keeps the signed sweep and directed endpoint contacts of another arc.
/// Each endpoint tangent may move by at most half of `angle_tolerance` measured as a chord of
/// the unit circle. An arc wrapped to a whole turn fails the check.
#[must_use]
pub fn arc_contacts_match(expected: &Seg2, stored: &Seg2, angle_tolerance: f64) -> bool {
    let (
        Seg2::Arc {
            start: want_start,
            sweep: want,
            ..
        },
        Seg2::Arc {
            start: have_start,
            sweep: have,
            ..
        },
    ) = (*expected, *stored)
    else {
        return false;
    };
    let tolerance = 0.5 * angle_tolerance;
    have.abs() < TAU
        && (want - have).abs() <= angle_tolerance
        && biarc_near(
            biarc_arc_tangent(want_start, want.signum()),
            biarc_arc_tangent(have_start, have.signum()),
            tolerance,
        )
        && biarc_near(
            biarc_arc_tangent(want_start + want, want.signum()),
            biarc_arc_tangent(have_start + have, have.signum()),
            tolerance,
        )
}

/// Compare a stored point with its expected position, including a coordinate-scale rounding
/// allowance of sixteen machine epsilons in addition to `absolute_tolerance`.
#[must_use]
pub fn points_agree(actual: Point2, expected: Point2, absolute_tolerance: f64) -> bool {
    let scale = expected.max_dimension().max(1.0);
    finite2(actual)
        && finite2(expected)
        && biarc_near(
            actual,
            expected,
            absolute_tolerance + 16.0 * f64::EPSILON * scale,
        )
}

/// Curvature vector at a circular contact, pointing towards `center`. The squared radius has
/// a floor at `EFF_ZERO²` so an intermediate collapsed circle can still be evaluated.
#[must_use]
pub fn circle_curvature(center: Point2, point: Point2, radius: f64) -> Vec2 {
    let offset = center - point;
    let squared_radius = (radius * radius).max(crate::math::EFF_ZERO_SQ);
    // Component division keeps the regularised value even for off-circle solver trials.
    Vec2::new(offset.x / squared_radius, offset.y / squared_radius)
}

/// The joint and two circular arcs or line segments through the given endpoints and directed
/// tangents, meeting with the same forward tangent. `ratio` is the start's fraction of the sum of
/// the two auxiliary distances along the endpoint tangents (Scarparo, *Corner Detection
/// and Arc Approximation of Planar Data Set*, §5, <https://arxiv.org/abs/1311.5881>).
///
/// # Errors
/// Invalid input for a non-finite coordinate, coincident points, a zero tangent, or a
/// ratio outside `(0, 1)`. Unsupported when the positive-distance construction is
/// singular or its resulting segments cannot hold both contacts to numerical precision.
pub fn biarc(
    start: Point2,
    start_tangent: Vec2,
    end: Point2,
    end_tangent: Vec2,
    ratio: f64,
) -> Result<(Point2, [Seg2; 2])> {
    let contacts = BiarcContacts::new(start, start_tangent, end, end_tangent, ratio, OP_BIARC)?;
    let (joint, pieces) = contacts.construct(ratio, None, OP_BIARC)?;
    let pieces = pieces
        .try_into()
        .map_err(|_| biarc_precision_error(OP_BIARC))?;
    Ok((joint, pieces))
}

/// An equal-distance biarc for approximating a free-form span, using the supplied unit
/// tangents without renormalizing them. A forward-collinear span is one line; otherwise
/// each endpoint contributes a piece unless it is within `1e-12` of the joint. A piece
/// whose sagitta is negligible or whose radius exceeds `1e7` is represented by its chord.
///
/// The auxiliary-distance construction is shared with [`biarc`], including the choice of a
/// conditioned quadratic root. The approximation keeps the planar arc fitter's input-coordinate
/// circular parameters and piece cardinality. Use [`biarc`] to retain exact endpoint contacts
/// and shallow circular bends.
///
/// # Errors
/// Non-finite or coincident endpoints, tangents that are not unit vectors, or a span whose
/// positive-distance construction cannot be represented by finite arcs or lines.
pub fn fit_biarc(
    start: Point2,
    start_tangent: Vec2,
    end: Point2,
    end_tangent: Vec2,
) -> Result<Vec<Seg2>> {
    let contacts = BiarcContacts::from_unit_tangents(
        start,
        start_tangent,
        end,
        end_tangent,
        0.5,
        OP_FIT_BIARC,
    )?;
    let (_, pieces) = contacts.construct(0.5, Some(BiarcApproximation::FIT), OP_FIT_BIARC)?;
    Ok(pieces)
}

/// A line joined to a circular arc through the two directed contacts, in start-to-end order.
/// The line comes first when `straight_start` is true. If both tangents point along the
/// chord, the result is two lines with the joint at `ratio` along that chord.
/// Otherwise the contacts uniquely decide the joint.
///
/// # Errors
/// The input errors of [`biarc`], or an unsupported fit when the selected directed tangent
/// cannot give a positive-length line joined to a circle at model precision.
pub fn biarc_with_straight_piece(
    start: Point2,
    start_tangent: Vec2,
    end: Point2,
    end_tangent: Vec2,
    ratio: f64,
    straight_start: bool,
) -> Result<(Point2, [Seg2; 2])> {
    let op = OP_STRAIGHT_BIARC;
    let contacts = BiarcContacts::new(start, start_tangent, end, end_tangent, ratio, op)?;
    let BiarcContacts {
        chord,
        length,
        first,
        last,
        ..
    } = contacts;
    let direction = chord / length;
    let both_straight = forward_collinear(cross2(first, direction), first.dot(direction))
        && forward_collinear(cross2(last, direction), last.dot(direction));
    let (joint, chords) = if both_straight {
        let left = chord * ratio;
        (start + left, [left, chord * (ratio - 1.0)])
    } else {
        // The line shares an endpoint tangent with the arc. The arc chord is
        // perpendicular to first - last since the circle's endpoint tangents
        // reflect across its chord (Šír et al., §2.1, <https://www.ag.jku.at/pubs/2006sfj.pdf>).
        // Use the sum for nearby tangents and the perpendicular to their difference
        // near opposite tangents. In the latter case tangent × arc_axis = ±|difference|² / 2.
        let difference = first - last;
        let arc_axis = if first.dot(last) >= 0.0 {
            first + last
        } else {
            Vec2::new(-difference.y, difference.x)
        };
        let tangent = if straight_start { first } else { last };
        let denominator = cross2(tangent, arc_axis);
        if denominator.abs() <= gamma(16) {
            return Err(SolidError::Unsupported {
                operation: op,
                reason: "parallel tangents require a forward straight span",
            });
        }
        // Decompose the span chord along the straight tangent and the arc axis.
        // Solving both coefficients directly keeps a short arc's chord accurate.
        let distance = cross2(direction, arc_axis) / denominator * length;
        if !distance.is_finite() {
            return Err(biarc_precision_error(op));
        }
        if distance <= 0.0 {
            return Err(SolidError::Unsupported {
                operation: op,
                reason: "the selected tangent does not give a positive straight piece",
            });
        }
        let arc_chord = arc_axis * (cross2(tangent, direction) / denominator) * length;
        if straight_start {
            let left = first * distance;
            (start + left, [left, -arc_chord])
        } else {
            let right = -last * distance;
            (end + right, [arc_chord, right])
        }
    };
    let pieces = contacts.pieces(joint, chords, op)?;
    let straight = [
        both_straight || straight_start,
        both_straight || !straight_start,
    ];
    if pieces.map(|piece| matches!(piece, Seg2::Line { .. })) != straight {
        return Err(biarc_precision_error(op));
    }
    Ok((joint, pieces))
}

#[derive(Clone, Copy)]
struct BiarcContacts {
    start: Point2,
    end: Point2,
    chord: Vec2,
    length: f64,
    first: Vec2,
    last: Vec2,
}

/// The planar arc fitter's geometric approximation bounds.
#[derive(Clone, Copy)]
struct BiarcApproximation {
    minimum_piece: f64,
    straight_tolerance: f64,
    maximum_radius: f64,
}

impl BiarcApproximation {
    const FIT: Self = Self {
        minimum_piece: 1e-12,
        straight_tolerance: 1e-12,
        maximum_radius: 1e7,
    };
}

impl BiarcContacts {
    fn new(
        start: Point2,
        start_tangent: Vec2,
        end: Point2,
        end_tangent: Vec2,
        ratio: f64,
        op: &'static str,
    ) -> Result<Self> {
        let contacts = Self::from_unit_tangents(
            start,
            biarc_direction(start_tangent, op)?,
            end,
            biarc_direction(end_tangent, op)?,
            ratio,
            op,
        )?;
        if !(contacts.length > EFF_ZERO) {
            return Err(SolidError::invalid_input(
                op,
                "endpoints must be distinct at model precision",
            ));
        }
        Ok(contacts)
    }

    fn from_unit_tangents(
        start: Point2,
        first: Vec2,
        end: Point2,
        last: Vec2,
        ratio: f64,
        op: &'static str,
    ) -> Result<Self> {
        if !(finite2(start) && finite2(end) && 0.0 < ratio && ratio < 1.0) {
            return Err(SolidError::invalid_input(
                op,
                "endpoints must be finite and the ratio must be between 0 and 1",
            ));
        }
        let chord = end - start;
        let length = length2d(chord);
        if !(length.is_finite() && length > 0.0) {
            return Err(SolidError::invalid_input(
                op,
                "endpoints must be finite and distinct",
            ));
        }
        if ![first, last]
            .into_iter()
            .all(|direction| finite2(direction) && (length2d(direction) - 1.0).abs() <= gamma(8))
        {
            return Err(SolidError::invalid_input(
                op,
                "tangents must be finite unit vectors",
            ));
        }
        Ok(Self {
            start,
            end,
            chord,
            length,
            first,
            last,
        })
    }

    /// Both callers use one auxiliary-distance solve and weighted tangent-point joint.
    /// Only the approximation's chord representation and cardinality bounds differ.
    fn construct(
        &self,
        ratio: f64,
        approximation: Option<BiarcApproximation>,
        op: &'static str,
    ) -> Result<(Point2, Vec<Seg2>)> {
        if let Some(bounds) = approximation {
            let squared_length = self.chord.dot(self.chord);
            // The approximate fitter admits equality at the squared chord threshold.
            if squared_length < 1e-24 {
                return Err(biarc_precision_error(op));
            }
            let direction = self.chord / squared_length.sqrt();
            if cross2(self.first, direction).abs() < bounds.straight_tolerance
                && cross2(self.last, direction).abs() < bounds.straight_tolerance
                && self.first.dot(direction) > 0.0
                && self.last.dot(direction) > 0.0
            {
                return Ok((
                    self.start + self.chord * 0.5,
                    vec![Seg2::Line {
                        a: self.start,
                        b: self.end,
                    }],
                ));
            }
        }
        let total = self.auxiliary_distance(ratio, op)?;
        let rest = 1.0 - ratio;
        let first_distance = ratio * total;
        let last_distance = rest * total;
        if !(first_distance > 0.0 && last_distance > 0.0) {
            return Err(biarc_precision_error(op));
        }
        let first_contact = self.start + self.first * first_distance;
        let last_contact = self.end - self.last * last_distance;
        let [left_chord, right_chord] = self.local_chords(ratio, total);
        let weighted_joint = first_contact * rest + last_contact * ratio;
        let local_joint = self.start + left_chord;
        // The coordinate readback allowance can exceed a tiny translated span. Arithmetic
        // placement must also retain the span's own scale before any coordinate allowance.
        let placement_tolerance = self.position_tolerance().min(EFF_ZERO * self.length);
        // Tangent points can overflow or lose the endpoint separation before they are averaged.
        // Use the local placement when that error exceeds the existing position allowance or
        // loses the direction of an analytically straight piece.
        let joint = if finite2(local_joint)
            && (!biarc_near(weighted_joint, local_joint, placement_tolerance)
                || !self.keeps_straight_contacts(weighted_joint, [left_chord, right_chord]))
        {
            local_joint
        } else {
            weighted_joint
        };
        let chords = if approximation.is_some() {
            [joint - self.start, joint - self.end]
        } else {
            [left_chord, right_chord]
        };
        Ok((
            joint,
            self.construct_pieces(joint, chords, approximation, op)?,
        ))
    }

    /// Twice the mean auxiliary distance, shared by interpolation and approximation.
    ///
    /// Let u = total/2 and w = r*first + (1-r)*last. The positive-distance construction
    /// gives A*u² + 2*B*u - C = 0, with A = 8*r*(1-r)*(1-first·last),
    /// B = 2*chord·w and C = chord·chord. At r = 1/2 these are the equal-distance
    /// fitter's coefficients, retaining its arithmetic when the root meets the join bound.
    fn auxiliary_distance(&self, ratio: f64, op: &'static str) -> Result<f64> {
        let rest = 1.0 - ratio;
        let weighted = self.first * ratio + self.last * rest;
        let a = 8.0 * ratio * rest * (1.0 - self.first.dot(self.last));
        let b = 2.0 * self.chord.dot(weighted);
        let c = self.chord.dot(self.chord);
        let direct = if a < EFF_ZERO {
            (b > EFF_ZERO * c.sqrt()).then(|| c / (2.0 * b))
        } else {
            Some((-b + (b * b + a * c).max(0.0).sqrt()) / a)
        };
        let direct = direct.and_then(|half| {
            let total = 2.0 * half;
            if !(total.is_finite() && total > 0.0) {
                return None;
            }
            let residual = self.join_residual(self.local_chords(ratio, total));
            (residual <= EFF_ZERO * 0.5).then_some((total, residual))
        });
        if let Some((total, residual)) = direct {
            if residual <= gamma(64) {
                return Ok(total);
            }
        }

        // A subtractive root can lose precision near parallel tangents. Use the same
        // construction in chord-length units, with the difference-squared coefficient
        // and a same-sign root. The choice depends on the geometric residual, not the caller.
        let difference = self.first - self.last;
        let a = ratio * rest * difference.length_squared();
        let b = (self.chord / self.length).dot(weighted);
        let root = b.hypot(a.sqrt());
        let inverse = if b >= 0.0 { root + b } else { a / (root - b) };
        if !(inverse > 0.0) {
            return Err(SolidError::Unsupported {
                operation: op,
                reason: "tangents do not admit a finite biarc at this ratio",
            });
        }
        let total = self.length / inverse;
        if !(total.is_finite() && total > 0.0) {
            return Err(biarc_precision_error(op));
        }
        if let Some((candidate, _)) = direct {
            if self.centres_within_roundoff(ratio, candidate, total) {
                return Ok(candidate);
            }
        }
        Ok(total)
    }

    fn centres_within_roundoff(&self, ratio: f64, candidate: f64, reference: f64) -> bool {
        let direct_chords = self.local_chords(ratio, candidate);
        let reference_chords = self.local_chords(ratio, reference);
        // Two root/chord/radius evaluations take fewer than 256 unit roundings,
        // including two each for sqrt and hypot. This is the 128-EPSILON allowance
        // used by the analytical circle check, before curvature conditioning.
        let epsilon = gamma(256);
        let model_tolerance = self.position_tolerance().max(MODEL_TOL);
        direct_chords
            .into_iter()
            .zip(reference_chords)
            .zip([self.first, self.last])
            .all(|((direct, reference), tangent)| {
                let direct_length = length2d(direct);
                let reference_length = length2d(reference);
                if !(direct_length.is_finite()
                    && direct_length > 0.0
                    && reference_length.is_finite()
                    && reference_length > 0.0)
                {
                    return false;
                }
                let normal = Vec2::new(-tangent.y, tangent.x);
                let direct_height = normal.dot(direct);
                let reference_height = normal.dot(reference);
                let sine = reference_height.abs() / reference_length;
                if !(sine.is_finite() && sine > epsilon) {
                    return false;
                }
                let direct_radius = (0.5 * direct_length) / (direct_height / direct_length);
                let reference_radius =
                    (0.5 * reference_length) / (reference_height / reference_length);
                // R(h) = |h|²/(2 n·h). For |delta h| <= epsilon |h| and
                // sine = |n·h|/|h|, the relative radius change is bounded by
                // epsilon (2 + epsilon + 1/sine)/(1 - epsilon/sine).
                // Both centres lie on the same endpoint normal. Account for supplied
                // tangents being unit only to roundoff; bound both radius and centre.
                let allowance = (reference_radius.abs() * epsilon * (2.0 + epsilon + 1.0 / sine)
                    / (1.0 - epsilon / sine))
                    .min(model_tolerance);
                direct_radius.is_finite()
                    && reference_radius.is_finite()
                    && (direct_radius - reference_radius).abs() * length2d(tangent).max(1.0)
                        <= allowance
            })
    }

    fn local_chords(&self, ratio: f64, total: f64) -> [Vec2; 2] {
        let rest = 1.0 - ratio;
        let turn = (self.first - self.last) * (ratio * rest * total);
        [self.chord * ratio + turn, self.chord * -rest + turn]
    }

    /// Reflection across a circular piece's chord gives its other endpoint tangent.
    /// Normalization, dot products and reflection contribute fewer than 24 rounding units per
    /// side. Both sides and their cross product fit within `gamma(64)`.
    fn join_residual(&self, [left, right]: [Vec2; 2]) -> f64 {
        let left_length = length2d(left);
        let right_length = length2d(right);
        if !(left_length.is_finite()
            && left_length > 0.0
            && right_length.is_finite()
            && right_length > 0.0)
        {
            return f64::INFINITY;
        }
        let left = left / left_length;
        let right = right / right_length;
        let from_left = left * (2.0 * self.first.dot(left)) - self.first;
        let from_right = right * (2.0 * self.last.dot(right)) - self.last;
        if from_left.dot(from_right) > 0.0 {
            cross2(from_left, from_right).abs()
        } else {
            f64::INFINITY
        }
    }

    /// Coordinate-rounding allowance shared by joint placement and exact piece validation.
    fn position_tolerance(&self) -> f64 {
        EFF_ZERO * self.length.max(1.0)
            + gamma(4) * self.start.max_dimension().max(self.end.max_dimension())
    }

    fn keeps_straight_contacts(&self, joint: Point2, [left, right]: [Vec2; 2]) -> bool {
        [
            (self.start, self.first, left),
            (self.end, -self.last, right),
        ]
        .into_iter()
        .all(|(start, tangent, chord)| {
            let length = length2d(chord);
            if !(length.is_finite() && length > 0.0) {
                return true;
            }
            let unit = chord / length;
            if !forward_collinear(cross2(tangent, unit), tangent.dot(unit)) {
                return true;
            }
            let placed_chord = joint - start;
            let placed_length = length2d(placed_chord);
            placed_length.is_finite()
                && placed_length > 0.0
                && same_biarc_direction(placed_chord / placed_length, tangent)
        })
    }

    fn pieces(&self, joint: Point2, chords: [Vec2; 2], op: &'static str) -> Result<[Seg2; 2]> {
        self.construct_pieces(joint, chords, None, op)?
            .try_into()
            .map_err(|_| biarc_precision_error(op))
    }

    fn construct_pieces(
        &self,
        joint: Point2,
        [left_chord, right_chord]: [Vec2; 2],
        approximation: Option<BiarcApproximation>,
        op: &'static str,
    ) -> Result<Vec<Seg2>> {
        let tol =
            approximation.map_or_else(|| self.position_tolerance(), |bounds| bounds.minimum_piece);
        let mut built = Vec::new();
        for (start, tangent, chord, reverse) in [
            (self.start, self.first, left_chord, false),
            (self.end, -self.last, right_chord, true),
        ] {
            if approximation.is_some_and(|bounds| !(length2d(joint - start) > bounds.minimum_piece))
            {
                continue;
            }
            let (piece, tangent, piece_tol) =
                biarc_piece(start, tangent, joint, chord, tol, approximation, op)?;
            built.push((
                if reverse { piece.reversed() } else { piece },
                tangent,
                piece_tol,
            ));
        }
        if approximation.is_none() {
            let [(left, from_left, left_tol), (right, from_right, right_tol)] = built.as_slice()
            else {
                return Err(biarc_precision_error(op));
            };
            let join_tol = (left_tol + right_tol).min(tol.max(MODEL_TOL));
            if !same_biarc_direction(*from_left, -*from_right)
                || !biarc_near(left.end(), right.start(), join_tol)
                || !biarc_near(right.end(), self.end, *right_tol)
            {
                return Err(biarc_precision_error(op));
            }
        }
        if built.is_empty() {
            return Err(biarc_precision_error(op));
        }
        Ok(built.into_iter().map(|(piece, _, _)| piece).collect())
    }
}

fn biarc_direction(vector: Vec2, op: &'static str) -> Result<Vec2> {
    let scale = vector.max_dimension();
    if !(finite2(vector) && scale > 0.0) {
        return Err(SolidError::invalid_input(
            op,
            "tangents must be finite and nonzero",
        ));
    }
    let scaled = vector / scale;
    Ok(scaled / scaled.length())
}

fn biarc_piece(
    start: Point2,
    tangent: Vec2,
    end: Point2,
    chord: Vec2,
    tol: f64,
    approximation: Option<BiarcApproximation>,
    op: &'static str,
) -> Result<(Seg2, Vec2, f64)> {
    let placed_chord = end - start;
    let placed_length = length2d(placed_chord);
    let length = length2d(chord);
    if !(length.is_finite() && length > tol && placed_length.is_finite() && placed_length > tol) {
        return Err(biarc_precision_error(op));
    }
    let direction = chord / length;
    let sin_half = cross2(tangent, direction);
    let cos_half = tangent.dot(direction);
    let normal = Vec2::new(-tangent.y, tangent.x);
    let (straight, signed_radius) = if let Some(bounds) = approximation {
        let squared_length = chord.dot(chord);
        let height = normal.dot(chord);
        let signed_radius = squared_length / (2.0 * height);
        (
            height.abs() * squared_length.sqrt() < bounds.straight_tolerance * squared_length
                || signed_radius.abs() > bounds.maximum_radius,
            signed_radius,
        )
    } else {
        (
            forward_collinear(sin_half, cos_half),
            (0.5 * length) / sin_half,
        )
    };
    if straight {
        let admissible = if approximation.is_some() {
            tangent.dot(chord) > 0.0
        } else {
            same_biarc_direction(placed_chord / placed_length, tangent)
        };
        return if admissible {
            Ok((
                Seg2::Line { a: start, b: end },
                placed_chord / placed_length,
                tol,
            ))
        } else {
            Err(biarc_precision_error(op))
        };
    }
    let side = signed_radius.signum();
    let radius = signed_radius.abs();
    let center = start + normal * signed_radius;
    if !(finite2(center) && radius.is_finite()) {
        return Err(biarc_precision_error(op));
    }
    let (start_angle, sweep) = if approximation.is_some() {
        let radial = start - center;
        let start_angle = radial.y.atan2(radial.x);
        let last = end - center;
        let mut sweep = last.y.atan2(last.x) - start_angle;
        // The polar-angle difference lies in [-2π, 2π]; two wraps include signed zero.
        for _ in 0..2 {
            if (side > 0.0 && sweep <= 0.0) || (side < 0.0 && sweep >= 0.0) {
                sweep += side * TAU;
            }
        }
        (start_angle, sweep)
    } else {
        // Local tangent/chord angles retain precision on a short arc far from the origin.
        let radial = normal * -side;
        (radial.y.atan2(radial.x), 2.0 * sin_half.atan2(cos_half))
    };
    let arc = Seg2::Arc {
        center,
        radius,
        start: start_angle,
        sweep,
    };
    let readback_tol = (tol + gamma(24) * radius).min(tol.max(MODEL_TOL));
    let valid = if approximation.is_some() {
        sweep.is_finite() && sweep.abs() <= TAU
    } else {
        sweep.abs() > EFF_ZERO
            && sweep.abs() < TAU
            && biarc_near(arc.start(), start, readback_tol)
            && biarc_near(arc.end(), end, readback_tol)
            && same_biarc_direction(biarc_arc_tangent(start_angle, side), tangent)
    };
    if !valid {
        return Err(biarc_precision_error(op));
    }
    Ok((
        arc,
        biarc_arc_tangent(start_angle + sweep, side),
        readback_tol,
    ))
}

fn forward_collinear(sine: f64, cosine: f64) -> bool {
    sine.abs() <= gamma(16) && cosine > 0.0
}

fn biarc_arc_tangent(angle: f64, side: f64) -> Vec2 {
    let radial = unit(angle);
    Vec2::new(-radial.y, radial.x) * side
}

fn same_biarc_direction(a: Vec2, b: Vec2) -> bool {
    a.dot(b) > 0.0 && cross2(a, b).abs() <= EFF_ZERO
}

fn biarc_near(a: Point2, b: Point2, tol: f64) -> bool {
    let gap = a - b;
    gap.x.hypot(gap.y) <= tol
}

fn biarc_precision_error(operation: &'static str) -> SolidError {
    SolidError::Unsupported {
        operation,
        reason: "two tangent biarc segments cannot be represented at model precision",
    }
}

fn h(p: Point2, w: f64) -> H2 {
    [p.x * w, p.y * w, w]
}

/// The point of a homogeneous control point.
pub(crate) fn point_of(p: H2) -> Point2 {
    Vec2::new(p[0] / p[2], p[1] / p[2])
}

/// The control points `p[0..=n]` of a Bézier segment, or `None` for a degree out of range.
fn bezier_points(n: u8, p: &[Point2; MAX_BEZIER_CTRL]) -> Option<&[Point2]> {
    let n = usize::from(n);
    if !(1..=MAX_BEZIER_DEGREE).contains(&n) {
        return None;
    }
    p.get(..=n)
}

impl Seg2 {
    /// Whether `piece` is the restriction of this cubic Bézier to a nonempty interval of
    /// `[0, 1]`, in either direction, within control-point roundoff (at most 1e-9 mm).
    /// A coordinate root only proposes an endpoint: the four controls of the
    /// restriction must all agree, which bounds the whole curve by the convex hull property.
    #[must_use]
    pub fn cubic_contains(&self, piece: &Self) -> bool {
        let (
            Self::Cubic { p0, p1, p2, p3 },
            Self::Cubic {
                p0: a,
                p1: b,
                p2: c,
                p3: d,
            },
        ) = (*self, *piece)
        else {
            return false;
        };
        let controls = [p0, p1, p2, p3];
        let mut hull = crate::geom::extent2d::Extent2d::new();
        for p in controls {
            hull.add_point(p);
        }
        let tol =
            (128.0 * f64::EPSILON * hull.min.length().max(hull.max.length()).max(1.0)).min(1e-9);
        if !hull.contains_point(a, tol) || !hull.contains_point(d, tol) {
            return false;
        }
        let params = |p: Point2, tangent: Vec2| {
            // Invert the coordinate that changes fastest at this end of the restriction.
            // The wider coordinate of the whole span can turn here and lose the parameter
            // to rounding while the other coordinate still distinguishes nearby points.
            let direction = if tangent.x == 0.0 && tangent.y == 0.0 {
                hull.max - hull.min
            } else {
                tangent
            };
            let coord = |q: Point2| {
                if direction.x.abs() >= direction.y.abs() {
                    q.x
                } else {
                    q.y
                }
            };
            // Include the span ends: a crossing there can differ by roundoff in the chosen coordinate.
            let mut roots = vec![0.0, 1.0];
            let mut work = 128;
            crate::query::tangent_edge::bernstein_roots(
                &controls.map(|v| coord(v) - coord(p)),
                0.0,
                1.0,
                48,
                &mut work,
                &mut roots,
            );
            roots
        };
        let ends = params(d, d - c);
        // The blossom of the cubic: restricting to [u,v] gives B(u,u,u), B(u,u,v),
        // B(u,v,v), B(v,v,v), equally for a reversed interval.
        let blossom = |s, t, u| {
            let lerp = |p: Point2, q: Point2, f: f64| p + (q - p) * f;
            let ab = lerp(p0, p1, s);
            let bc = lerp(p1, p2, s);
            let cd = lerp(p2, p3, s);
            lerp(lerp(ab, bc, t), lerp(bc, cd, t), u)
        };
        params(a, b - a).into_iter().any(|u| {
            blossom(u, u, u).distance(a) <= tol
                && ends.iter().copied().any(|v| {
                    u != v
                        && blossom(v, v, v).distance(d) <= tol
                        && blossom(u, u, v).distance(b) <= tol
                        && blossom(u, v, v).distance(c) <= tol
                })
        })
    }

    /// Where it starts.
    #[must_use]
    pub fn start(&self) -> Point2 {
        match *self {
            Seg2::Line { a, .. } | Seg2::Conic { a, .. } => a,
            Seg2::Arc {
                center,
                radius,
                start,
                ..
            } => center + unit(start) * radius,
            Seg2::Cubic { p0, .. } => p0,
            Seg2::Bezier { p, .. } => p[0],
        }
    }

    /// Where it ends.
    #[must_use]
    pub fn end(&self) -> Point2 {
        match *self {
            Seg2::Line { b, .. } | Seg2::Conic { b, .. } => b,
            Seg2::Arc {
                center,
                radius,
                start,
                sweep,
            } => center + unit(start + sweep) * radius,
            Seg2::Cubic { p3, .. } => p3,
            Seg2::Bezier { n, p } => p.get(usize::from(n)).copied().unwrap_or(Vec2::UNDEF),
        }
    }

    /// The same segment run the other way.
    #[must_use]
    pub fn reversed(&self) -> Seg2 {
        match *self {
            Seg2::Line { a, b } => Seg2::Line { a: b, b: a },
            Seg2::Arc {
                center,
                radius,
                start,
                sweep,
            } => Seg2::Arc {
                center,
                radius,
                start: start + sweep,
                sweep: -sweep,
            },
            Seg2::Cubic { p0, p1, p2, p3 } => Seg2::Cubic {
                p0: p3,
                p1: p2,
                p2: p1,
                p3: p0,
            },
            Seg2::Conic { a, apex, b, w } => Seg2::Conic {
                a: b,
                apex,
                b: a,
                w,
            },
            Seg2::Bezier { n, mut p } => {
                if let Some(used) = p.get_mut(..=usize::from(n)) {
                    used.reverse();
                }
                Seg2::Bezier { n, p }
            }
        }
    }

    /// Whether it is a line or a circular arc.
    #[must_use]
    pub fn is_line_or_arc(&self) -> bool {
        matches!(self, Seg2::Line { .. } | Seg2::Arc { .. })
    }

    /// Its homogeneous Bézier spans, in the direction it runs: a line one span of degree 1, an
    /// arc rational quadratic spans of at most a quarter turn each (the middle weight the cosine
    /// of half the span's angle, Piegl & Tiller §7.3), the others their own control points.
    /// `None` for a malformed segment.
    #[must_use]
    pub fn spans(&self) -> Option<Vec<Vec<H2>>> {
        Some(match *self {
            Seg2::Line { a, b } => vec![vec![h(a, 1.0), h(b, 1.0)]],
            Seg2::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                if !(sweep.is_finite() && sweep.abs() <= 2.0 * TAU) {
                    return None;
                }
                // (at most eight quarter turns: the cast is of a small whole number)
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let n = (sweep.abs() / FRAC_PI_2).ceil().max(1.0) as usize;
                #[allow(clippy::cast_precision_loss)]
                let step = sweep / n as f64;
                let half = step * 0.5;
                let w = half.cos();
                (0..n)
                    .map(|i| {
                        #[allow(clippy::cast_precision_loss)]
                        let a = start + step * i as f64;
                        vec![
                            h(center + unit(a) * radius, 1.0),
                            h(center + unit(a + half) * (radius / w), w),
                            h(center + unit(a + step) * radius, 1.0),
                        ]
                    })
                    .collect()
            }
            Seg2::Cubic { p0, p1, p2, p3 } => {
                vec![vec![h(p0, 1.0), h(p1, 1.0), h(p2, 1.0), h(p3, 1.0)]]
            }
            Seg2::Conic { a, apex, b, w } => vec![vec![h(a, 1.0), h(apex, w), h(b, 1.0)]],
            Seg2::Bezier { n, ref p } => {
                vec![bezier_points(n, p)?.iter().map(|q| h(*q, 1.0)).collect()]
            }
        })
    }

    /// The point a fraction `t` of the way along its parameter (an arc's angle, a Bézier's own
    /// parameter): de Casteljau on the homogeneous points.
    #[must_use]
    pub fn point_at(&self, t: f64) -> Point2 {
        match *self {
            Seg2::Line { a, b } => a + (b - a) * t,
            Seg2::Arc {
                center,
                radius,
                start,
                sweep,
            } => center + unit(start + sweep * t) * radius,
            _ => match self.spans().as_deref() {
                Some([span]) => point_of(casteljau(span, t)),
                _ => Vec2::UNDEF,
            },
        }
    }

    /// Its term of Green's integral, `1/2 ∫ x dy - y dx`, and a proved bound on the term's
    /// rounding: a loop's signed area is the sum of its segments' terms.
    #[must_use]
    pub fn area_term(&self) -> Option<(f64, f64)> {
        match *self {
            Seg2::Line { a, b } => {
                let (p, q) = (a.x * b.y, a.y * b.x);
                Some(((p - q) * 0.5, gamma(3) * (p.abs() + q.abs()) * 0.5))
            }
            Seg2::Arc {
                center: c,
                radius: r,
                start,
                sweep,
            } => {
                // P = c + r u(t), P x P' = r^2 + r (c.x cos t + c.y sin t)
                let end = start + sweep;
                let (s0, c0, s1, c1) = (start.sin(), start.cos(), end.sin(), end.cos());
                let terms = [r * r * sweep, r * c.x * (s1 - s0), -(r * c.y * (c1 - c0))];
                let size = r * r * sweep.abs() + 2.0 * r * (c.x.abs() + c.y.abs());
                // libm's sin and cos within two ulps; gamma covers the products and the sums
                let v = terms.iter().sum::<f64>() * 0.5;
                Some((v, gamma(16) * size * 0.5))
            }
            Seg2::Conic { a, apex, b, w } => {
                // Green of a conic arc = the chord's term + T f(w), T the signed area of the
                // control triangle and f(w) = 2 w ∫ t^2 / W^2 (the arc's segment, see
                // `conic_factor`)
                let (u, v) = (apex - a, b - a);
                let (p, q) = (a.x * b.y, a.y * b.x);
                let tri = cross2(u, v) * 0.5;
                let (f, fb) = conic_factor(w)?;
                let value = (p - q) * 0.5 + tri * f;
                let size = (p.abs() + q.abs()) * 0.5 + tri.abs() * f;
                let tri_err = gamma(6) * (u.x * v.y).abs().max((u.y * v.x).abs());
                Some((value, gamma(8) * size + tri.abs() * fb + tri_err * f))
            }
            Seg2::Cubic { .. } | Seg2::Bezier { .. } => {
                let span = self.spans()?.pop()?;
                let x: Vec<f64> = span.iter().map(|p| p[0]).collect();
                let y: Vec<f64> = span.iter().map(|p| p[1]).collect();
                let (xy, xb) = product(&x, &derivative(&y))?;
                let (yx, yb) = product(&y, &derivative(&x))?;
                #[allow(clippy::cast_precision_loss)]
                let m = xy.len() as f64;
                let value = (xy.iter().sum::<f64>() - yx.iter().sum::<f64>()) / m * 0.5;
                let size = xy.iter().chain(&yx).map(|c| c.abs()).sum::<f64>() / m * 0.5;
                #[allow(clippy::cast_precision_loss)]
                let k = 4 * x.len() + 16;
                Some((value, gamma(k) * size + (xb + yb) / m * 0.5))
            }
        }
    }
}

/// `f(w) = 2 w ∫_0^1 t^2 / W(t)^2 dt`, `W = 1 - 2 (1 - w) t (1 - t)`: the area between a
/// rational quadratic arc of middle weight `w` and its chord, over its control triangle's
/// (Farin §12.4), with a bound on its error. Near the parabola (`|1 - w| <= 1/2`) by its series
/// `1/W^2 = Σ (k + 1) (2 e τ)^k` (`e = 1 - w`, `τ = t (1 - t) <= 1/4`), whose terms integrate to
/// `B(k + 3, k + 1)`; the tail is bounded by a geometric series. Elsewhere in closed form: with
/// `w = cos φ` (an ellipse), `w (φ - w s) / s^3`, `s = sin φ`; with `w = cosh ψ` (a hyperbola),
/// `w (w s - ψ) / s^3`, `s = sinh ψ`.
pub(super) fn conic_factor(w: f64) -> Option<(f64, f64)> {
    if !(w.is_finite() && w > 1e-9 && w < 1e9) {
        return None;
    }
    let e = 1.0 - w;
    if e.abs() <= 0.5 {
        // B(k + 3, k + 1) = (k + 2)! k! / (2k + 3)!, by its ratio from k to k + 1
        let mut beta = 1.0 / 3.0; // k = 0: 2! 0! / 3!
        let mut power = 1.0;
        let mut sum = 0.0;
        let mut size = 0.0;
        let mut term = 0.0;
        for k in 0..60u32 {
            let kf = f64::from(k);
            term = (kf + 1.0) * power * beta;
            sum += term;
            size += term.abs();
            power *= 2.0 * e;
            beta *= (kf + 3.0) * (kf + 1.0) / ((2.0 * kf + 4.0) * (2.0 * kf + 5.0));
        }
        // the ratio of successive terms is below |2 e| / 4 (k + 2)/(k + 1) <= 1/2 past k = 0
        let tail = term.abs() * 2.0;
        let f = 2.0 * w * sum;
        return Some((f, 2.0 * w * (tail + gamma(512) * size)));
    }
    let (f, terms) = if w < 1.0 {
        let phi = w.acos();
        let s = phi.sin();
        (
            w * (phi - w * s) / (s * s * s),
            w * (phi + w * s) / (s * s * s),
        )
    } else {
        let psi = w.acosh();
        let s = psi.sinh();
        (
            w * (w * s - psi) / (s * s * s),
            w * (w * s + psi) / (s * s * s),
        )
    };
    // |1 - w| > 1/2: the difference cancels at most a factor ~10; the functions within 4 ulps
    (f.is_finite()).then_some((f, gamma(64) * terms))
}

/// The Bernstein coefficients of the product of two Bernstein polynomials (Farin §5.2), and a
/// bound on their rounding.
pub(crate) fn product(a: &[f64], b: &[f64]) -> Option<(Vec<f64>, f64)> {
    let m = a.len().checked_sub(1)?;
    let n = b.len().checked_sub(1)?;
    let (cm, cn, ct) = (choose(m), choose(n), choose(m + n));
    let mut out = vec![0.0; m + n + 1];
    let mut size = 0.0_f64;
    for (i, ai) in a.iter().enumerate() {
        for (j, bj) in b.iter().enumerate() {
            let f = cm.get(i)? * cn.get(j)? / ct.get(i + j)?;
            let t = ai * bj * f;
            *out.get_mut(i + j)? += t;
            size = size.max(t.abs());
        }
    }
    let count = m + n + 2;
    out.iter()
        .all(|x| x.is_finite())
        .then_some((out, gamma(4 * count + 8) * size * (count as f64)))
}

/// `C(n, i)` for every `i`.
fn choose(n: usize) -> Vec<f64> {
    let mut v = vec![1.0; n + 1];
    for i in 1..=n {
        #[allow(clippy::cast_precision_loss)]
        let f = (n + 1 - i) as f64 / i as f64;
        let prev = v.get(i - 1).copied().unwrap_or(1.0);
        if let Some(x) = v.get_mut(i) {
            *x = prev * f;
        }
    }
    v
}

/// The Bernstein coefficients of a Bernstein polynomial's derivative.
pub(crate) fn derivative(a: &[f64]) -> Vec<f64> {
    #[allow(clippy::cast_precision_loss)]
    let n = a.len().saturating_sub(1) as f64;
    a.iter()
        .zip(a.iter().skip(1))
        .map(|(x, y)| (y - x) * n)
        .collect()
}

/// De Casteljau at `t` on homogeneous points.
pub(crate) fn casteljau(span: &[H2], t: f64) -> H2 {
    let mut q = span.to_vec();
    while q.len() > 1 {
        q = q
            .iter()
            .zip(q.iter().skip(1))
            .map(|(&[ax, ay, aw], &[bx, by, bw])| {
                [ax + (bx - ax) * t, ay + (by - ay) * t, aw + (bw - aw) * t]
            })
            .collect();
    }
    q.first().copied().unwrap_or([f64::NAN; 3])
}

/// A span split in half (de Casteljau at 1/2): the left and the right halves.
pub(crate) fn halves(c: &[H2]) -> (Vec<H2>, Vec<H2>) {
    let mut q = c.to_vec();
    let mut left: Vec<H2> = q.first().copied().into_iter().collect();
    let mut right: Vec<H2> = q.last().copied().into_iter().collect();
    while q.len() > 1 {
        q = q
            .iter()
            .zip(q.iter().skip(1))
            .map(|(&[ax, ay, aw], &[bx, by, bw])| {
                [(ax + bx) * 0.5, (ay + by) * 0.5, (aw + bw) * 0.5]
            })
            .collect();
        left.extend(q.first().copied());
        right.extend(q.last().copied());
    }
    right.reverse();
    (left, right)
}

/// A signed area with a proved bound on its error: the exact area is in
/// `[value - bound, value + bound]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    /// The area as computed.
    pub value: f64,
    /// The bound on its error.
    pub bound: f64,
}

impl Area {
    /// Its sign when the bound decides it: `Some(true)` positive, `Some(false)` negative.
    #[must_use]
    pub fn sign(&self) -> Option<bool> {
        if self.value > self.bound {
            Some(true)
        } else if self.value < -self.bound {
            Some(false)
        } else {
            None
        }
    }
}

impl Loop2 {
    /// The loop run the other way.
    #[must_use]
    pub fn reversed(&self) -> Loop2 {
        Loop2 {
            segs: self.segs.iter().rev().map(Seg2::reversed).collect(),
        }
    }

    /// Its signed area (positive counter-clockwise), the sum of its segments' Green terms, with a
    /// proved bound on its rounding.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a malformed segment.
    pub fn signed_area(&self) -> Result<Area> {
        let mut value = 0.0;
        let mut bound = 0.0;
        let mut size = 0.0_f64;
        for s in &self.segs {
            let (v, b) = s
                .area_term()
                .ok_or_else(|| SolidError::invalid_input(OP_AREA, "a malformed segment"))?;
            value += v;
            bound += b;
            size += v.abs();
        }
        bound += gamma(self.segs.len() + 1) * size;
        if !(value.is_finite() && bound.is_finite()) {
            return Err(SolidError::invalid_input(
                OP_AREA,
                "an area that is not finite",
            ));
        }
        Ok(Area { value, bound })
    }

    /// Whether each segment ends within `tol` of where the next starts, round the loop.
    #[must_use]
    pub fn is_closed(&self, tol: f64) -> bool {
        !self.segs.is_empty()
            && self
                .segs
                .iter()
                .zip(self.segs.iter().cycle().skip(1))
                .all(|(a, b)| a.end().distance(b.start()) <= tol)
    }

    /// Whether the loop holds `p` by the even–odd rule, decided exactly with the half-open
    /// crossing rule: a point is above the ray from `p` along +x when its `y > p.y`, and below
    /// it otherwise, so a stretch of the loop lying along the ray's line counts as below it and
    /// crosses nothing. Each segment's Bézier spans, and a straight join wherever a span ends
    /// away from where the next starts, are split until each is proved above, below, left of
    /// `p`, or right of it; a span right of `p` crosses the ray an odd number of times exactly
    /// when its ends lie on opposite sides (it is connected and stays right of `p`). `None` when
    /// a span cannot be decided (`p` on or within rounding of the loop).
    #[must_use]
    pub fn holds(&self, p: Point2) -> Option<bool> {
        let mut spans: Vec<Vec<H2>> = Vec::new();
        for s in &self.segs {
            spans.extend(s.spans()?);
        }
        let ends = |c: &[H2]| Some((point_of(*c.first()?), point_of(*c.last()?)));
        let mut odd = false;
        for (here, next) in spans.iter().zip(spans.iter().cycle().skip(1)) {
            odd ^= crossings_odd(here.clone(), p)?;
            let (_, end) = ends(here)?;
            let (start, _) = ends(next)?;
            if end != start {
                odd ^= crossings_odd(vec![[end.x, end.y, 1.0], [start.x, start.y, 1.0]], p)?;
            }
        }
        Some(odd)
    }
}

/// Whether the ray from `p` along +x crosses `span` an odd number of times by the half-open
/// rule ([`Loop2::holds`]): a point is above the ray when its `y > p.y`.
fn crossings_odd(span: Vec<H2>, p: Point2) -> Option<bool> {
    let mut odd = false;
    let mut todo = vec![(span, 0u32)];
    while let Some((c, depth)) = todo.pop() {
        if c.iter()
            .any(|q| !(q[2] > 0.0 && q.iter().all(|x| x.is_finite())))
        {
            return None;
        }
        let pts: Vec<Point2> = c.iter().map(|q| point_of(*q)).collect();
        let (first, last) = (*pts.first()?, *pts.last()?);
        let scale = pts.iter().fold(p.x.abs().max(p.y.abs()), |m, q| {
            m.max(q.x.abs()).max(q.y.abs())
        });
        // the rounding of the control points and of the hull they bound the span by
        let margin = gamma(32) * scale;
        // proved off the ray's line, or wholly left of p: no crossing on the ray
        if pts.iter().all(|q| q.y > p.y + margin)
            || pts.iter().all(|q| q.y < p.y - margin)
            || pts.iter().all(|q| q.x < p.x - margin)
        {
            continue;
        }
        // wholly right of p: the span is connected and never meets the line x = p.x, so its
        // crossings of the ray are its passages between above and below, odd exactly when its
        // ends are on opposite sides. The ends are the points the neighbouring spans and joins
        // share, so a level shared end is classified once and the count telescopes round the loop.
        if pts.iter().all(|q| q.x > p.x + margin) {
            odd ^= (first.y > p.y) != (last.y > p.y);
            continue;
        }
        if depth >= 48 {
            return None;
        }
        let (l, r) = halves(&c);
        todo.push((l, depth + 1));
        todo.push((r, depth + 1));
    }
    Some(odd)
}

/// Why a profile is refused ([`check_regions`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileRefusal {
    /// A loop without segments, not closed within its distance, or more segments than
    /// [`MAX_SEGMENTS`] in all.
    Budget,
    /// A line or arc not finite or of no length, or an arc of no positive radius or one that
    /// does not build at the zone tolerance given (its whole circle within it, its `|sweep|` past
    /// [`largest_arc_sweep`] or at most [`smallest_arc_sweep`], or within 0.01 degrees short of a
    /// turn by more than the zone tolerance at its end).
    LineOrArc,
    /// A free-form segment not finite or of no length, or a conic weight not positive.
    FreeForm,
}

/// The loops of `regions` checked: each closed within ten times `tol`, the segments within
/// [`MAX_SEGMENTS`], each segment finite and of some length (a line's ends apart; an arc of a
/// positive radius that builds at the zone tolerance `zone_tol` by the rule [`curve_on_frame`]
/// builds by ([`smallest_arc_sweep`], [`largest_arc_sweep`]); a conic's weight positive; a
/// Bézier's ends apart), its coordinates within 1e9. Pass the zone tolerance of the kernel that
/// will build the profile (`k.tolerances.zone_tol_3d`), so the check accepts exactly the arcs
/// [`curve_on_frame`] builds; `tol` is the loop-closure tolerance, a separate thing.
///
/// # Errors
///
/// The [`ProfileRefusal`] that applies first. A `zone_tol` that is not finite and positive
/// refuses every arc ([`ProfileRefusal::LineOrArc`]).
pub fn check_regions<'a>(
    regions: impl Iterator<Item = &'a Region2>,
    tol: f64,
    zone_tol: f64,
) -> std::result::Result<(), ProfileRefusal> {
    let zone_ok = zone_tol.is_finite() && zone_tol > 0.0;
    let mut count = 0usize;
    let finite = |p: Point2| finite2(p) && p.x.abs() < 1e9 && p.y.abs() < 1e9;
    let apart = |a: Point2, b: Point2| a.distance(b) > 1e-12;
    for lp in regions.flat_map(Region2::loops) {
        count = count.saturating_add(lp.segs.len());
        if count > MAX_SEGMENTS || lp.segs.is_empty() {
            return Err(ProfileRefusal::Budget);
        }
        for s in &lp.segs {
            let ok = match *s {
                Seg2::Line { a, b } => finite(a) && finite(b) && apart(a, b),
                Seg2::Arc {
                    center,
                    radius,
                    start,
                    sweep,
                } => {
                    finite(center)
                        && radius.is_finite()
                        && radius > 0.0
                        && radius < 1e9
                        && start.is_finite()
                        && sweep.is_finite()
                        && zone_ok
                        && arc_build(radius, sweep, zone_tol).is_ok()
                }
                Seg2::Cubic { p0, p1, p2, p3 } => {
                    [p0, p1, p2, p3].into_iter().all(finite) && apart(p0, p3)
                }
                Seg2::Conic { a, apex, b, w } => {
                    [a, apex, b].into_iter().all(finite)
                        && w.is_finite()
                        && w > 1e-9
                        && w < 1e9
                        && apart(a, b)
                }
                Seg2::Bezier { n, ref p } => bezier_points(n, p)
                    .is_some_and(|q| q.iter().copied().all(finite) && apart(s.start(), s.end())),
            };
            if !ok {
                return Err(if s.is_line_or_arc() {
                    ProfileRefusal::LineOrArc
                } else {
                    ProfileRefusal::FreeForm
                });
            }
        }
        if !lp.is_closed(tol * 10.0) {
            return Err(ProfileRefusal::Budget);
        }
    }
    Ok(())
}

/// A closed loop, each segment with a key for where it starts, split where it passes a point
/// twice (`same` keys for two of its segments) into loops that pass every point once. Each split
/// leaves two shorter loops, so the work is bounded by the segment count.
pub fn simple_loops<K: Clone>(segs: Vec<(K, Seg2)>, same: impl Fn(&K, &K) -> bool) -> Vec<Loop2> {
    let mut todo = vec![segs];
    let mut out = Vec::new();
    while let Some(segs) = todo.pop() {
        let repeat = segs.iter().enumerate().find_map(|(i, a)| {
            segs.iter()
                .enumerate()
                .skip(i + 1)
                .find(|(_, b)| same(&a.0, &b.0))
                .map(|(j, _)| (i, j))
        });
        match repeat {
            Some((i, j)) => {
                // segments i..j close a loop of their own; the rest, from j round to i, another
                let inner: Vec<(K, Seg2)> = segs.get(i..j).map(<[_]>::to_vec).unwrap_or_default();
                let rest: Vec<(K, Seg2)> = segs
                    .get(j..)
                    .into_iter()
                    .flatten()
                    .chain(segs.get(..i).into_iter().flatten())
                    .cloned()
                    .collect();
                todo.push(inner);
                todo.push(rest);
            }
            None => out.push(Loop2 {
                segs: segs.into_iter().map(|(_, s)| s).collect(),
            }),
        }
    }
    out
}

fn v3(a: [f64; 3]) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}

/// A frame's origin, x and y axes and normal (`x × y`).
pub(crate) fn axes(frame: &Frame) -> (Point3, Vec3, Vec3, Vec3) {
    let (x, y) = (v3(frame.x_axis), v3(frame.y_axis));
    (v3(frame.origin), x, y, x.cross(y))
}

/// The point `(p, level)` of `frame` in the world: `origin + p.x x + p.y y + level n`.
#[must_use]
pub fn to_world(frame: &Frame, p: Point2, level: f64) -> Point3 {
    let (o, x, y, n) = axes(frame);
    o + x * p.x + y * p.y + n * level
}

/// `q` in `frame`'s coordinates: `((q - o).x, (q - o).y)`.
#[must_use]
pub fn to_local(frame: &Frame, q: Point3) -> Point2 {
    let (o, x, y, _) = axes(frame);
    let d = q - o;
    Vec2::new(d.dot(x), d.dot(y))
}

/// Chord fraction of the point where the view ray meets the plane, projected orthogonally onto
/// the chord. The fraction is not clamped and the intersection may lie behind the ray origin.
/// A parallel ray or collapsed chord has no fraction.
#[must_use]
pub fn ray_chord_parameter(
    frame: &Frame,
    origin: Point3,
    direction: Vec3,
    [a, b]: [Point2; 2],
) -> Option<f64> {
    let (plane_origin, _, _, normal) = axes(frame);
    let denom = direction.dot(normal);
    if denom.abs() < EFF_ZERO {
        return None;
    }
    let t = (plane_origin - origin).dot(normal) / denom;
    if !t.is_finite() {
        return None;
    }
    let at = to_local(frame, origin + direction * t);
    let d = b - a;
    let ratio = (at - a).dot(d) / d.dot(d);
    ratio.is_finite().then_some(ratio)
}

/// `q`'s height above `frame`: `(q - o).n`.
#[must_use]
pub fn height(frame: &Frame, q: Point3) -> f64 {
    let (o, _, _, n) = axes(frame);
    (q - o).dot(n)
}

/// Whether `frame`'s axes are unit and square to each other, to within 1e-12.
#[must_use]
pub fn frame_is_orthonormal(frame: &Frame) -> bool {
    let (o, x, y, _) = axes(frame);
    [o, x, y]
        .iter()
        .all(|v| v.x.is_finite() && v.y.is_finite() && v.z.is_finite())
        && (x.length() - 1.0).abs() <= 1e-12
        && (y.length() - 1.0).abs() <= 1e-12
        && x.dot(y).abs() <= 1e-12
}

/// `seg` as the kernel's curve on `frame` at `level`, in the direction it runs: a line segment,
/// a circular arc (a circle of its own in the frame's plane, its angle 0 at the arc's start and
/// trimmed to the arc's sweep, so no two arcs share a curve and no trim crosses the circle's
/// seam; its net is proved on the arc where it is read back, [`super::read::planar_pieces`]; a
/// clockwise arc turns about the frame's reversed normal, so it runs as the segment does), a
/// conic or Bézier as its exact NURBS.
///
/// # Errors
///
/// `InvalidInput` for a frame whose axes are not orthonormal, a malformed segment, or an arc
/// that does not build at the kernel's `zone_tol_3d`: its `|sweep|` at most
/// [`smallest_arc_sweep`], or built as its closed circle (past a whole turn, or within 0.01
/// degrees short of one) with its end farther than `zone_tol_3d` from the arc's (past a turn,
/// [`largest_arc_sweep`]). [`check_regions`] holds the same rule at the zone tolerance it is given.
/// Those of the constructors.
pub fn curve_on_frame(k: &Kernel, frame: &Frame, level: f64, seg: &Seg2) -> Result<Curve> {
    if !frame_is_orthonormal(frame) || !level.is_finite() {
        return Err(SolidError::invalid_input(
            OP_CURVE,
            "a frame that is not orthonormal",
        ));
    }
    let at = |p: Point2| to_world(frame, p, level);
    match *seg {
        Seg2::Line { a, b } => Ok(Curve::from(Line::create_line_segment(
            3,
            at(a),
            at(b),
            None,
        )?)),
        Seg2::Arc {
            center,
            radius,
            start,
            sweep,
        } => {
            if !(start.is_finite() && sweep.is_finite() && radius.is_finite() && radius > 0.0) {
                return Err(SolidError::invalid_input(OP_CURVE, "a malformed segment"));
            }
            // An arc too short to build, or one the kernel would build as its closed circle
            // farther than the zone tolerance from the arc's end, is refused ([`arc_build`]).
            let build = arc_build(radius, sweep, k.tolerances.zone_tol_3d)
                .map_err(|reason| SolidError::invalid_input(OP_CURVE, reason))?;
            let (_, x, y, _) = axes(frame);
            let (y, s0) = if sweep >= 0.0 {
                (y, start)
            } else {
                (-y, -start)
            };
            // The arc's own circle, its angle 0 at the arc's start: two arcs of one circle are
            // two curves, so a boolean keeps the points where they meet, and the trim never
            // crosses the circle's seam nor runs past a whole turn, whatever the start.
            let (c, s) = (s0.cos(), s0.sin());
            let placement = Axis2Placement::new(at(center), x * c + y * s, y * c - x * s);
            // The closed circle is asked for outright, so the builder's own full-turn window is
            // never what decides it.
            let deg = match build {
                ArcBuild::Closed => Extent1d::from_min_max(0.0, 360.0),
                ArcBuild::Open(deg) => Extent1d::from_min_max(0.0, deg),
            };
            let e = Ellipse::create_canonical_circle(k, &placement, radius, Some(&deg), None)?;
            Ok(Curve::from(e))
        }
        _ => {
            let span = seg
                .spans()
                .and_then(|mut s| s.pop())
                .ok_or_else(|| SolidError::invalid_input(OP_CURVE, "a malformed segment"))?;
            let degree = span
                .len()
                .checked_sub(1)
                .ok_or_else(|| SolidError::invalid_input(OP_CURVE, "a malformed segment"))?;
            let mut knots = vec![0.0; degree + 1];
            knots.extend(std::iter::repeat_n(1.0, degree + 1));
            let homogeneous: Vec<[f64; 4]> = span
                .iter()
                .map(|q| {
                    let p = at(point_of(*q));
                    [p.x * q[2], p.y * q[2], p.z * q[2], q[2]]
                })
                .collect();
            rational_curve(&knots, &homogeneous)
        }
    }
}

/// A segment placed on a frame: its curve as a description, in world coordinates, for a caller
/// that keeps its own curve record ([`seg_on_frame`]).
#[derive(Clone, Debug, PartialEq)]
pub enum PlacedSeg {
    /// The line from `a` to `b`.
    Line {
        /// Its start.
        a: Point3,
        /// Its end.
        b: Point3,
    },
    /// The circular arc about `center` in the plane square to the unit `axis`, of `radius`,
    /// from angle `start` to `end` (radians, `start < end`) measured from the unit `reference`
    /// towards `axis × reference`: counter-clockwise about `axis`.
    Arc {
        /// Its centre.
        center: Point3,
        /// The normal it turns counter-clockwise about.
        axis: Vec3,
        /// Where its angles are measured from.
        reference: Vec3,
        /// Its radius.
        radius: f64,
        /// Its start angle.
        start: f64,
        /// Its end angle.
        end: f64,
    },
    /// The Bézier of `points` (`[x, y, z, weight]`, weights positive) on the clamped knots
    /// `knots`, of degree `degree`.
    Nurbs {
        /// Its degree.
        degree: usize,
        /// Its control points with their weights.
        points: Vec<[f64; 4]>,
        /// Its clamped knot vector, `0` and `1` each `degree + 1` times.
        knots: Vec<f64>,
    },
}

/// `seg` placed on `frame` at `level` as a [`PlacedSeg`], in the direction it runs: each point
/// by [`to_world`]; an arc about the frame's normal `x × y`, its angles from `x`, or, swept
/// clockwise, about the reversed normal with its angles negated so it still runs from its
/// start (the same points [`curve_on_frame`]'s arc runs through, which measures its angle
/// from the arc's start instead); a cubic, conic or Bézier with its control
/// points and weights. `None` for an arc of no sweep, a conic whose weight is not finite and
/// positive, or a Bézier whose degree is outside `1 ..=` [`MAX_BEZIER_DEGREE`].
#[must_use]
pub fn seg_on_frame(frame: &Frame, level: f64, seg: &Seg2) -> Option<PlacedSeg> {
    let at = |p: Point2| to_world(frame, p, level);
    let nurbs = |degree: usize, points: Vec<[f64; 4]>| {
        let mut knots = vec![0.0; degree + 1];
        knots.extend(std::iter::repeat_n(1.0, degree + 1));
        PlacedSeg::Nurbs {
            degree,
            points,
            knots,
        }
    };
    let weighted = |p: Point2, w: f64| {
        let q = at(p);
        [q.x, q.y, q.z, w]
    };
    match *seg {
        Seg2::Line { a, b } => Some(PlacedSeg::Line { a: at(a), b: at(b) }),
        Seg2::Arc {
            center,
            radius,
            start,
            sweep,
        } => {
            if sweep == 0.0 {
                return None;
            }
            let (_, x, _, normal) = axes(frame);
            let (axis, start, end) = if sweep < 0.0 {
                (-normal, -start, -start - sweep)
            } else {
                (normal, start, start + sweep)
            };
            Some(PlacedSeg::Arc {
                center: at(center),
                axis,
                reference: x,
                radius,
                start,
                end,
            })
        }
        Seg2::Cubic { p0, p1, p2, p3 } => Some(nurbs(
            3,
            [p0, p1, p2, p3].iter().map(|&p| weighted(p, 1.0)).collect(),
        )),
        Seg2::Conic { a, apex, b, w } => (w.is_finite() && w > 0.0).then(|| {
            nurbs(
                2,
                vec![weighted(a, 1.0), weighted(apex, w), weighted(b, 1.0)],
            )
        }),
        Seg2::Bezier { n, p } => {
            let degree = usize::from(n);
            (1..=MAX_BEZIER_DEGREE).contains(&degree).then(|| {
                nurbs(
                    degree,
                    p.iter()
                        .take(degree + 1)
                        .map(|&q| weighted(q, 1.0))
                        .collect(),
                )
            })
        }
    }
}

/// The B-spline curve of the knot vector `knots` (with its repeats) and the homogeneous control
/// points `[x w, y w, z w, w]` (all weights one: polynomial), by the kernel's canonical
/// constructor.
///
/// # Errors
///
/// `InvalidInput` for knots that do not fit the points or do not increase, a weight not
/// positive or numbers not finite; those of the constructor (a knot repeated past the degree).
pub fn rational_curve(knots: &[f64], homogeneous: &[[f64; 4]]) -> Result<Curve> {
    let bad = |reason: &'static str| SolidError::invalid_input(OP_CURVE, reason);
    let degree = knots
        .len()
        .checked_sub(
            homogeneous
                .len()
                .checked_add(1)
                .ok_or_else(|| bad("too many points"))?,
        )
        .ok_or_else(|| bad("the knots do not fit the points"))?;
    let mut unique: Vec<f64> = Vec::new();
    let mut mult: Vec<usize> = Vec::new();
    for &u in knots {
        if !u.is_finite() {
            return Err(bad("a knot is not finite"));
        }
        match (unique.last(), mult.last_mut()) {
            (Some(&prev), Some(m)) if u == prev => *m += 1,
            (Some(&prev), _) if u < prev => return Err(bad("the knots decrease")),
            _ => {
                unique.push(u);
                mult.push(1);
            }
        }
    }
    // a knot repeated past the degree inside the curve is a jump: refused, never repaired (the
    // canonical constructor would remove it and move the points)
    if mult.iter().skip(1).rev().skip(1).any(|m| *m > degree) {
        return Err(bad("an interior knot repeated past the degree"));
    }
    let mut points = Vec::new();
    let mut weights = Vec::new();
    for h in homogeneous {
        let [x, y, z, w] = *h;
        if !(w > 0.0 && w.is_finite() && x.is_finite() && y.is_finite() && z.is_finite()) {
            return Err(bad(
                "a control point is not finite or its weight not positive",
            ));
        }
        points.push(Point3::new(x / w, y / w, z / w));
        weights.push(w);
    }
    let rational = weights.iter().any(|w| *w != 1.0);
    let data = CanonicalData {
        dim: 3,
        degree,
        control_points: &points,
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &mult,
        knots: &unique,
        knot_type: KnotType::Unspecified,
        weights: rational.then_some(weights.as_slice()),
        trim: None,
    };
    Ok(Curve::from(BSplineCurve::create_canonical(&data)?))
}

/// A tapered flange's thickness at its two ends: `(at the toe, at the root)`. The flange is
/// `thickness` thick at the middle of its inside length `inside` (a channel's width less its web)
/// and its inside face slopes by `slope` degrees across that length, so each end differs from the
/// middle by half the length times the slope's tangent. No slope: `thickness` at both ends.
/// Inputs are not checked: a caller refuses a slope or a length out of its range first; a
/// non-finite input gives a non-finite answer, never a panic.
pub fn tapered_flange_ends(inside: f64, thickness: f64, slope: f64) -> (f64, f64) {
    let change = inside / 2.0 * slope.to_radians().tan();
    (thickness - change, thickness + change)
}
