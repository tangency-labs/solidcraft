//! What a sweep path of lines and arcs is: one line ([`path_as_line`]), arcs about one
//! axis ([`path_as_arc`]), or lines and arcs end to end, each leaving in the direction
//! the one before arrives in ([`path_as_segments`], checked by [`segments_join`]). Each
//! comparison is certified: a quantity within its rounding bound of its tolerance
//! answers [`PathShape::Undecided`].
//!
//! A straight or round path is read on positions, against the path's own gap: every
//! vertex within it of the line from the path's start to its end, or every arc's end
//! within it of the first arc's circle where the run has turned to; so the sweep that
//! follows the reading ends where the path does.
//!
//! Not upstream: SolidCraft's sweep dispatch (`straight`, `circular`, `segmented`).
//! Deviations: none (no upstream counterpart). No recursion.

use std::f64::consts::TAU;

use crate::curve::arc::{arcs_on_one_circle, at_angle, ArcDefinition};
use crate::curve::curve::p3::compute_crvs_normal;
use crate::curve::curve::{Curve, CurveKind};
use crate::curve::ellipse::Ellipse;
use crate::error::{Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::certified::{
    distance, dot, gamma, one_minus_dot, unit, unit_error, unit_error_of, Bounded, Verdict,
};
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;

/// One piece of a path: its curve, and for an arc its definition, the curve made from
/// it ([`Ellipse::arc`]) so the two agree by construction.
#[derive(Debug, Clone)]
pub struct PathPiece {
    curve: Curve,
    arc: Option<ArcDefinition>,
}

impl PathPiece {
    /// A curve as given, read as itself (an arc's curve without its definition is no
    /// arc here).
    #[must_use]
    pub const fn of_curve(curve: Curve) -> Self {
        Self { curve, arc: None }
    }

    /// The arc of a definition, its curve made by [`Ellipse::arc`]. TODO(d0b44726f): the
    /// path queries compare the curve's ends with the definition's circle, so they take
    /// the curve on it (bound 0 by its label until the rebase onto d0b44726f).
    ///
    /// # Errors
    ///
    /// Those of [`Ellipse::arc`].
    pub fn of_arc(k: &Kernel, arc: &ArcDefinition) -> Result<Self> {
        Ok(Self {
            curve: Curve::from(Ellipse::arc(k, arc)?),
            arc: Some(*arc),
        })
    }

    /// The curve.
    #[must_use]
    pub const fn curve(&self) -> &Curve {
        &self.curve
    }

    /// The arc's definition, for a piece made as an arc.
    #[must_use]
    pub const fn arc(&self) -> Option<&ArcDefinition> {
        self.arc.as_ref()
    }
}

/// The tolerances a path is read with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PathTolerances {
    /// How far apart consecutive pieces' ends may be, and how far a straight or round
    /// path's points from the line or circle it is read as.
    pub gap: f64,
    /// How far from 1 the cosine of the turn between the directions of consecutive
    /// segments may be where they join.
    pub turn: f64,
    /// How short a line (or an arc's radius) may be and still count as one.
    pub length: f64,
    /// How far apart a path's ends must be to be open.
    pub closed: f64,
}

/// Which comparison of a path query was left undecided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathCheck {
    /// A piece starting where the one before ends, to the gap.
    Gap,
    /// A line (or the whole straight path) longer than the length tolerance, or an arc's
    /// radius.
    Length,
    /// A line going forward along the straight path's chord.
    Forward,
    /// A point within the gap of the straight path's chord.
    OffLine,
    /// An arc on the first arc's circle.
    Circle,
    /// An arc's end where the run of arcs has turned to.
    Reached,
    /// A segment leaving in the direction the one before arrives in.
    Turn,
    /// The path's ends apart (an open path).
    Open,
}

/// The first comparison a path query left undecided: its piece (index in the path) and
/// which comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathDoubt {
    /// The piece's index in the path.
    pub piece: usize,
    /// The comparison.
    pub check: PathCheck,
}

/// What a path is, as a query answers.
#[derive(Debug, Clone, PartialEq)]
pub enum PathShape<T> {
    /// Proved.
    Is(T),
    /// Proved not.
    Not,
    /// A comparison lies within its rounding bound of its tolerance: the first such.
    Undecided(PathDoubt),
}

/// A running answer: `Not` stops, the first undecided comparison is remembered.
struct Reading {
    doubt: Option<PathDoubt>,
}

impl Reading {
    const NEW: Self = Self { doubt: None };

    /// Whether the reading goes on after `v`, piece `piece`'s condition `check`.
    fn need(&mut self, v: Verdict, piece: usize, check: PathCheck) -> bool {
        match v {
            Verdict::Yes => true,
            Verdict::No => false,
            Verdict::Undecided => {
                self.doubt.get_or_insert(PathDoubt { piece, check });
                true
            }
        }
    }

    fn give<T>(&self, value: T) -> PathShape<T> {
        match self.doubt {
            Some(d) => PathShape::Undecided(d),
            None => PathShape::Is(value),
        }
    }
}

/// The unit direction from `a` to `b` given `|b - a|`, with each component's error:
/// the difference rounds once per component (`gamma_1` of the length), then
/// [`unit`]'s own rounding ([`unit_error_of`]).
fn direction(a: Point3, b: Point3, length: f64) -> Option<(Vec3, f64)> {
    Some((unit(b - a)?, unit_error_of(gamma(1) * length, length)))
}

/// The distance of `p` from the line through `a` along the unit `u` (each component
/// within `eu`), certified: `w = p - a` rounds once per component (moving the distance
/// by at most `gamma_1 |w|`), `u`'s error moves `|w x u|` by at most `2 eu |w|`, the
/// cross's two products and difference per component by `4 gamma_2 |w|` over the
/// vector, the length by `gamma_4` of itself.
fn off_line(p: Point3, a: Point3, u: Vec3, eu: f64) -> Bounded {
    let w = p - a;
    let value = w.cross(u).length();
    let wl = w.length() * (1.0 + gamma(5));
    Bounded {
        value,
        bound: (gamma(1) + 2.0 * eu + 4.0 * gamma(2)) * wl * (1.0 + gamma(4)) + gamma(4) * value,
    }
}

/// A straight path: its unit direction and its length.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinePath {
    /// The unit direction from the path's start to its end.
    pub direction: Vec3,
    /// The distance from the path's start to its end.
    pub length: f64,
}

/// Whether `path` is lines end to end along one line: the gaps within `tol.gap`, each
/// line longer than `tol.length` and going forward along the chord from the path's
/// start to its end, and every line's ends within `tol.gap` of that chord's line. Then
/// the chord's direction and length, so a sweep along them ends where the path does.
///
/// # Errors
///
/// Those of [`Curve::ends`].
pub fn path_as_line(
    k: &Kernel,
    path: &[&Curve],
    tol: &PathTolerances,
) -> Result<PathShape<LinePath>> {
    let mut reading = Reading::NEW;
    let mut ends = Vec::new();
    for (i, c) in path.iter().enumerate() {
        if !matches!(c.kind(), CurveKind::Line(_)) {
            return Ok(PathShape::Not);
        }
        let e = c.ends(k)?;
        if let Some(&(_, p)) = ends.last() {
            if !reading.need(distance(e.start, p).at_most(tol.gap), i, PathCheck::Gap) {
                return Ok(PathShape::Not);
            }
        }
        if !reading.need(
            distance(e.end, e.start).above(tol.length),
            i,
            PathCheck::Length,
        ) {
            return Ok(PathShape::Not);
        }
        ends.push((e.start, e.end));
    }
    let (Some(&(a0, _)), Some(&(_, b1))) = (ends.first(), ends.last()) else {
        return Ok(PathShape::Not);
    };
    let chord = distance(b1, a0);
    let last = ends.len().saturating_sub(1);
    if !reading.need(chord.above(tol.length), last, PathCheck::Length) {
        return Ok(PathShape::Not);
    }
    let Some((u, eu)) = direction(a0, b1, chord.value) else {
        return Ok(PathShape::Not);
    };
    for (i, &(a, b)) in ends.iter().enumerate() {
        let Some((d, ed)) = direction(a, b, (b - a).length()) else {
            return Ok(PathShape::Not);
        };
        let ok = reading.need(dot(d, u, ed, eu).above(0.0), i, PathCheck::Forward)
            && reading.need(
                off_line(a, a0, u, eu).at_most(tol.gap),
                i,
                PathCheck::OffLine,
            )
            && reading.need(
                off_line(b, a0, u, eu).at_most(tol.gap),
                i,
                PathCheck::OffLine,
            );
        if !ok {
            return Ok(PathShape::Not);
        }
    }
    Ok(reading.give(LinePath {
        direction: u,
        length: chord.value,
    }))
}

/// A path of arcs about one axis: the first's centre, its unit axis and the total turn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArcPath {
    /// The first arc's centre.
    pub centre: Point3,
    /// The first arc's unit axis, which the path turns about counter-clockwise.
    pub axis: Vec3,
    /// The sum of the arcs' sweeps (radians).
    pub angle: f64,
}

/// Whether `path` is arcs end to end about one axis: every piece an arc on the first's
/// circle to `tol.gap` ([`arcs_on_one_circle`]: centre, radius, and axis at the rim),
/// consecutive ends within `tol.gap`, every arc's end within `tol.gap` of the first's
/// circle where the run has turned to ([`at_angle`]), and the total turn at most a full
/// turn (to a relative `1e-12`).
///
/// # Errors
///
/// Those of [`Curve::ends`] and [`ArcDefinition::placement`].
pub fn path_as_arc(
    k: &Kernel,
    path: &[PathPiece],
    tol: &PathTolerances,
) -> Result<PathShape<ArcPath>> {
    let mut reading = Reading::NEW;
    let mut arcs = Vec::new();
    for piece in path {
        let Some(arc) = piece.arc() else {
            return Ok(PathShape::Not);
        };
        arcs.push(arc);
    }
    let Some(&first) = arcs.first() else {
        return Ok(PathShape::Not);
    };
    let Some(axis) = unit(first.axis) else {
        return Ok(PathShape::Not);
    };
    let (mut t, mut size, mut prev) = (first.start, first.start.abs(), None::<Point3>);
    for (i, (piece, arc)) in path.iter().zip(&arcs).enumerate() {
        let e = piece.curve().ends(k)?;
        t += arc.sweep();
        size += arc.sweep().abs();
        let n = i.saturating_add(2);
        let meets = prev.map_or(Verdict::Yes, |p| distance(e.start, p).at_most(tol.gap));
        let reached = at_angle(first, t, gamma(n) * size, e.end, tol.gap)?;
        let ok = reading.need(
            arcs_on_one_circle(first, arc, tol.gap)?.circle,
            i,
            PathCheck::Circle,
        ) && reading.need(meets, i, PathCheck::Gap)
            && reading.need(reached, i, PathCheck::Reached);
        if !ok {
            return Ok(PathShape::Not);
        }
        prev = Some(e.end);
    }
    let angle = t - first.start;
    Ok(if angle <= TAU * (1.0 + 1e-12) {
        reading.give(ArcPath {
            centre: first.centre,
            axis,
            angle,
        })
    } else {
        PathShape::Not
    })
}

/// One segment of a path of lines and arcs, from its start point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Segment {
    /// A line from `start` to `end`.
    Line {
        /// Its start.
        start: Point3,
        /// Its end.
        end: Point3,
    },
    /// An arc from `start`, turning `angle` (radians, positive) right-handed about the
    /// `axis` (any length) through `centre`.
    Arc {
        /// Its start.
        start: Point3,
        /// A point of the axis.
        centre: Point3,
        /// The axis.
        axis: Vec3,
        /// The turn.
        angle: f64,
    },
}

/// The rotation by `angle` right-handed about `axis` through `centre`, as a placement
/// ([`Axis2Placement::rotate_about_axis_at_point`]).
#[must_use]
pub fn rotation(centre: Point3, axis: Vec3, angle: f64) -> Axis2Placement {
    let mut r = Axis2Placement::IDENTITY;
    r.rotate_about_axis_at_point(angle, centre, axis);
    r
}

/// The bound on where [`rotation`] carries a point `w` from its centre, against the
/// exact rotation about the exact unit axis: the rotation's entries (the axis made
/// unit, `t a a + c` and `t a a +- s a`, within `7 e + gamma_4`, `e` the
/// [`unit_error`]), made unit twice and re-orthogonalised by two crosses (each doubling
/// it, plus their own rounding), the point's three products and sums (`gamma_4`): the
/// entries within `64 e`, so the point within `3 * 64 e |w|` (`sqrt 3 < 2`, three
/// entries a coordinate), counted as `256 e |w|`; the move to and from the centre
/// rounds once each, `2 gamma_1 (|centre| + |w|)` over the vector.
fn rotation_error(centre: Point3, w: f64) -> f64 {
    256.0 * unit_error() * w + 4.0 * gamma(1) * (centre.length() + w)
}

impl Segment {
    /// Where it starts.
    #[must_use]
    pub const fn start(&self) -> Point3 {
        match *self {
            Self::Line { start, .. } | Self::Arc { start, .. } => start,
        }
    }

    /// For an arc, its radius: the distance of its start from its axis.
    fn radius(&self) -> Option<f64> {
        match *self {
            Self::Line { .. } => None,
            Self::Arc {
                start,
                centre,
                axis,
                ..
            } => {
                let k = unit(axis)?;
                let w = start - centre;
                Some((w - k * w.dot(k)).length())
            }
        }
    }

    /// Its length along the path: a line's, an arc's radius times its turn.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for an arc whose axis has no length.
    pub fn length(&self) -> Result<f64> {
        match *self {
            Self::Line { start, end } => Ok((end - start).length()),
            Self::Arc { angle, .. } => match self.radius() {
                Some(r) => Ok(r * angle),
                None => Err(SolidError::invalid_input(
                    "curve::path::Segment::length",
                    "an arc whose axis has no length",
                )),
            },
        }
    }

    /// Where it ends, with the bound on its rounding.
    fn end(&self) -> (Point3, f64) {
        match *self {
            Self::Line { end, .. } => (end, 0.0),
            Self::Arc {
                start,
                centre,
                axis,
                angle,
            } => (
                rotation(centre, axis, angle).transform_point(start),
                rotation_error(centre, (start - centre).length()),
            ),
        }
    }

    /// The unit direction it leaves its start in and arrives at its end in, each with
    /// its components' error; `None` for a line without a length or an arc without an
    /// axis or a radius.
    fn directions(&self) -> Option<[(Vec3, f64); 2]> {
        match *self {
            Self::Line { start, end } => {
                let d = direction(start, end, (end - start).length())?;
                Some([d, d])
            }
            Self::Arc {
                start,
                centre,
                axis,
                angle,
            } => {
                // The tangent k x (p - c) made unit: p - c rounds once per component,
                // the cross takes the axis's error and its own two roundings.
                let k = unit(axis)?;
                let w = start - centre;
                let c = k.cross(w);
                let wl = w.length() * (1.0 + gamma(5));
                let ec = (2.0 * unit_error() + 2.0 * gamma(1) + gamma(2)) * wl * (1.0 + gamma(2));
                let leaving = (unit(c)?, unit_error_of(ec, c.length()));
                // Arriving: the leaving tangent turned with the arc.
                let turned = rotation(Point3::ZERO, axis, angle).transform_point(leaving.0);
                let arriving = (
                    unit(turned)?,
                    leaving.1 + rotation_error(Point3::ZERO, 1.0) + unit_error(),
                );
                Some([leaving, arriving])
            }
        }
    }
}

/// Whether `segments` make one path: each line longer than `tol.length`; each arc with
/// an axis, a radius longer than `tol.length` and a turn in `(0, 2 pi]` (to a relative
/// `1e-12`); each segment starting within `tol.gap` of where the one before ends, and
/// leaving in the direction it arrives in (`1 - cos` within `tol.turn`). Certified: `Not`
/// at the first condition proved false, `Undecided` (naming it) when one is within its rounding
/// bound of its tolerance.
#[must_use]
pub fn segments_join(segments: &[Segment], tol: &PathTolerances) -> PathShape<()> {
    let mut reading = Reading::NEW;
    if segments.is_empty() {
        return PathShape::Not;
    }
    let mut before: Option<((Point3, f64), (Vec3, f64))> = None;
    for (i, g) in segments.iter().enumerate() {
        let finite = match *g {
            Segment::Line { start, end } => [start, end]
                .iter()
                .all(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite()),
            Segment::Arc {
                start,
                centre,
                axis,
                angle,
            } => {
                [start, centre, axis]
                    .iter()
                    .all(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite())
                    && angle.is_finite()
            }
        };
        if !finite {
            return PathShape::Not;
        }
        let long = match *g {
            Segment::Line { start, end } => distance(end, start).above(tol.length),
            Segment::Arc {
                start,
                centre,
                angle,
                ..
            } => {
                if !(angle > 0.0 && angle <= TAU * (1.0 + 1e-12)) {
                    return PathShape::Not;
                }
                let Some(r) = g.radius() else {
                    return PathShape::Not;
                };
                // The radius: the difference, the projection's dot and product, the
                // difference and the length, within gamma_9 of |start - centre|, and
                // the unit axis's error (4 e of it).
                Bounded {
                    value: r,
                    bound: (gamma(9) + 4.0 * unit_error()) * (start - centre).length(),
                }
                .above(tol.length)
            }
        };
        if !reading.need(long, i, PathCheck::Length) {
            return PathShape::Not;
        }
        let Some([leaving, arriving]) = g.directions() else {
            return PathShape::Not;
        };
        if let Some(((end, end_bound), (d, ed))) = before {
            let gap = distance(g.start(), end);
            let joined = Bounded {
                value: gap.value,
                bound: gap.bound + end_bound,
            };
            let turn = one_minus_dot(leaving.0, d, leaving.1, ed).at_most(tol.turn);
            let ok = reading.need(joined.at_most(tol.gap), i, PathCheck::Gap)
                && reading.need(turn, i, PathCheck::Turn);
            if !ok {
                return PathShape::Not;
            }
        }
        before = Some((g.end(), arriving));
    }
    reading.give(())
}

/// Whether `path` is two or more lines and arcs end to end, each leaving in the
/// direction the one before arrives in ([`segments_join`]), and open (its ends more
/// than `tol.closed` apart): then its segments, from the pieces' start points.
///
/// # Errors
///
/// Those of [`Curve::ends`].
pub fn path_as_segments(
    k: &Kernel,
    path: &[PathPiece],
    tol: &PathTolerances,
) -> Result<PathShape<Vec<Segment>>> {
    if path.len() < 2 {
        return Ok(PathShape::Not);
    }
    let mut out = Vec::new();
    for piece in path {
        let e = piece.curve().ends(k)?;
        out.push(match piece.arc() {
            None if matches!(piece.curve().kind(), CurveKind::Line(_)) => Segment::Line {
                start: e.start,
                end: e.end,
            },
            Some(arc) => Segment::Arc {
                start: e.start,
                centre: arc.centre,
                axis: arc.axis,
                angle: arc.sweep(),
            },
            None => return Ok(PathShape::Not),
        });
    }
    let mut reading = Reading::NEW;
    match segments_join(&out, tol) {
        PathShape::Is(()) => {}
        PathShape::Not => return Ok(PathShape::Not),
        PathShape::Undecided(d) => reading.doubt = Some(d),
    }
    let (Some(first), Some(last)) = (out.first(), out.last()) else {
        return Ok(PathShape::Not);
    };
    let (end, end_bound) = last.end();
    let gap = distance(first.start(), end);
    let open = Bounded {
        value: gap.value,
        bound: gap.bound + end_bound,
    };
    let last = out.len().saturating_sub(1);
    if !reading.need(open.above(tol.closed), last, PathCheck::Open) {
        return Ok(PathShape::Not);
    }
    Ok(reading.give(out))
}

/// Whether `curves`, in order, run end to end round one closed loop: each curve's end
/// within `gap` of the next one's start, and the last's of the first's (certified).
/// No curves is no loop.
///
/// # Errors
///
/// Those of [`Curve::ends`].
pub fn closed_loop(k: &Kernel, curves: &[Curve], gap: f64) -> Result<Verdict> {
    let mut ends = Vec::new();
    for c in curves {
        ends.push(c.ends(k)?);
    }
    if ends.is_empty() {
        return Ok(Verdict::No);
    }
    let mut v = Verdict::Yes;
    for (i, e) in ends.iter().enumerate() {
        // The next curve round the loop (the first after the last).
        let after = if i + 1 == ends.len() { 0 } else { i + 1 };
        let Some(next) = ends.get(after) else {
            return Ok(Verdict::No);
        };
        v = v.and(distance(next.start, e.end).at_most(gap));
    }
    Ok(v)
}

/// The Euclidean control points of the profile's curves (lines, circles and B-splines),
/// every weight positive, so each curve lies in the convex hull of its own; `None` for a
/// curve of another kind or a weight not finite and positive.
fn hull(curves: &[Curve]) -> Result<Option<Vec<Point3>>> {
    let mut out = Vec::new();
    for c in curves {
        let can = match c.kind() {
            CurveKind::Line(l) => l.base_with_nurb()?.canonical()?,
            CurveKind::Ellipse(e) => e.base().canonical()?,
            CurveKind::BSpline(b) => b.canonical()?,
            _ => return Ok(None),
        };
        if !can.weights.iter().all(|w| w.is_finite() && *w > 0.0) {
            return Ok(None);
        }
        out.extend(can.control_points.iter().copied());
    }
    Ok(Some(out))
}

/// `|(p - c) . n|`, the distance of `p` from the plane through `c` square to the unit `n`
/// (each component within `en`), certified: `p - c` rounds once per component (moving
/// the distance by at most `2 gamma_1 |w|`, `w = p - c`), `n`'s error by `2 en |w|`, the
/// dot's three products and two sums by `gamma_3` of `|w|`.
fn off_plane(p: Point3, c: Point3, n: Vec3, en: f64) -> Bounded {
    let w = p - c;
    let value = w.dot(n).abs();
    Bounded {
        value,
        bound: (2.0 * gamma(1) + 2.0 * en + 2.0 * gamma(3)) * w.length() * (1.0 + gamma(5)),
    }
}

/// Whether the profile `curves` lies in the plane square to the unit `direction` (each
/// component within [`unit_error`]): every control point (so, by their convex hulls,
/// every point of the profile) within `ratio` times the profile's size (at least 1) of
/// the plane through the first control point. Certified; a profile of another kind of
/// curve, or with a weight not positive, is `No`.
///
/// # Errors
///
/// Those of the curves' nets.
pub fn profile_square_to(curves: &[Curve], direction: Vec3, ratio: f64) -> Result<Verdict> {
    let Some(points) = hull(curves)? else {
        return Ok(Verdict::No);
    };
    let Some(&c0) = points.first() else {
        return Ok(Verdict::No);
    };
    let size = points
        .iter()
        .map(|q| distance(*q, c0).value)
        .fold(0.0, f64::max);
    // The size from distances within gamma_5, the product's own rounding: rounded down.
    let floor = ratio * size.max(1.0) * (1.0 - gamma(8));
    let mut v = Verdict::Yes;
    for &q in &points {
        v = v.and(off_plane(q, c0, direction, unit_error()).at_most(floor));
    }
    Ok(v)
}

/// Whether `point` lies off the slab the planar profile `curves` lies in by more than `ratio`
/// times the profile's size (at least 1). The slab is about the plane through the profile's
/// first control point square to `n`, the profile's normal as [`compute_crvs_normal`]
/// computes it, made unit and used as a given vector (so its own error does not enter); its
/// half-width `delta` is the largest distance of a control point from that plane (so, by the
/// convex hulls, of any point of the profile), certified. The point must lie farther than
/// `delta + ratio * max(1, size)` from the plane, certified. That is what is proved: the
/// point is off the slab, not off the profile's own plane extended (a thin profile's own
/// plane may tilt within the slab by up to `2 delta` over its width). A profile of another
/// kind of curve, with a weight not positive, or of no size is `No`.
///
/// # Errors
///
/// `InvalidInput` for no curves; those of [`compute_crvs_normal`] and the curves' nets.
pub fn point_off_plane_of(
    k: &Kernel,
    curves: &[Curve],
    point: Point3,
    ratio: f64,
) -> Result<Verdict> {
    if curves.is_empty() {
        return Err(SolidError::invalid_input(
            "curve::path::point_off_plane_of",
            "no curves",
        ));
    }
    let Some(n) = unit(compute_crvs_normal(k, curves)?) else {
        return Ok(Verdict::No);
    };
    let Some(points) = hull(curves)? else {
        return Ok(Verdict::No);
    };
    let Some(&c0) = points.first() else {
        return Ok(Verdict::No);
    };
    let (mut size, mut delta) = (0.0_f64, 0.0_f64);
    for &q in &points {
        size = size.max(distance(q, c0).value * (1.0 - gamma(6)));
        let off = off_plane(q, c0, n, unit_error());
        delta = delta.max((off.value + off.bound) * (1.0 + gamma(2)));
    }
    if size.is_nan() || size <= 0.0 {
        return Ok(Verdict::No);
    }
    // The size rounded up where it scales the ratio.
    let up = size * (1.0 + 2.0 * gamma(6));
    let floor = (ratio * up.max(1.0) + delta) * (1.0 + gamma(4));
    let h = off_plane(point, c0, n, unit_error());
    let value = h.value - floor;
    Ok(Bounded {
        value,
        bound: h.bound + gamma(1) * (h.value + floor),
    }
    .above(0.0))
}

#[cfg(test)]
mod tests {
    use std::f64::consts::PI;

    use super::*;
    use crate::curve::line::Line;

    const TOL: PathTolerances = PathTolerances {
        gap: 1e-9,
        turn: 1e-12,
        length: 1e-12,
        closed: 1e-6,
    };

    fn line(a: Point3, b: Point3) -> Curve {
        Curve::from(Line::create_line_segment(3, a, b, None).unwrap())
    }

    fn arc_about_z(k: &Kernel, centre: Point3, radius: f64, start: f64, end: f64) -> PathPiece {
        let d = ArcDefinition {
            centre,
            axis: Vec3::Z,
            reference: Vec3::X,
            radius,
            start,
            end,
        };
        PathPiece::of_arc(k, &d).unwrap()
    }

    #[test]
    fn collinear_lines_are_one_line() {
        let k = Kernel::default();
        let a = line(Point3::ZERO, Point3::new(3.0, 4.0, 0.0));
        let b = line(Point3::new(3.0, 4.0, 0.0), Point3::new(6.0, 8.0, 0.0));
        let PathShape::Is(l) = path_as_line(&k, &[&a, &b], &TOL).unwrap() else {
            panic!()
        };
        assert!((l.length - 10.0).abs() < 1e-14);
        assert!((l.direction - Vec3::new(0.6, 0.8, 0.0)).length() < 1e-15);
        // Bent by 1e-3 radians: not one line.
        let c = line(Point3::new(6.0, 8.0, 0.0), Point3::new(9.0, 12.0, 0.005));
        assert_eq!(
            path_as_line(&k, &[&a, &b, &c], &TOL).unwrap(),
            PathShape::Not
        );
        // Apart.
        let d = line(Point3::new(6.0, 8.0, 1e-6), Point3::new(9.0, 12.0, 1e-6));
        assert_eq!(
            path_as_line(&k, &[&a, &b, &d], &TOL).unwrap(),
            PathShape::Not
        );
        // Ends exactly the gap apart: not decided.
        let loose = PathTolerances { gap: 1.0, ..TOL };
        let e = line(Point3::new(7.0, 8.0, 0.0), Point3::new(10.0, 12.0, 0.0));
        assert_eq!(
            path_as_line(&k, &[&b, &e], &loose).unwrap(),
            PathShape::Undecided(PathDoubt {
                piece: 1,
                check: PathCheck::Gap
            })
        );
        // Back along itself: not one line.
        let back = line(Point3::new(6.0, 8.0, 0.0), Point3::new(3.0, 4.0, 0.0));
        assert_eq!(
            path_as_line(&k, &[&a, &b, &back], &TOL).unwrap(),
            PathShape::Not
        );
    }

    #[test]
    fn a_path_bent_within_the_turn_tolerance_but_off_its_gap_is_not_a_line() {
        // Ten lines 10 long, alternating between 0 and 1.3e-6 radians: each direction
        // within 1 - cos 1e-12 of the first, but the path's points up to 6.5e-5 off
        // the line its first direction gives, far beyond the gap.
        let k = Kernel::default();
        let mut p = Point3::ZERO;
        let mut curves = Vec::new();
        for i in 0..10 {
            let a = if i % 2 == 0 { 0.0 } else { 1.3e-6_f64 };
            let q = p + Vec3::new(a.cos(), a.sin(), 0.0) * 10.0;
            curves.push(line(p, q));
            p = q;
        }
        let refs: Vec<&Curve> = curves.iter().collect();
        assert_eq!(path_as_line(&k, &refs, &TOL).unwrap(), PathShape::Not);
        // With a gap that takes the bend in, it is the chord: the sweep ends at the end.
        let loose = PathTolerances { gap: 1e-4, ..TOL };
        let PathShape::Is(l) = path_as_line(&k, &refs, &loose).unwrap() else {
            panic!()
        };
        assert!((l.direction * l.length - p).length() < 1e-12);
    }

    #[test]
    fn arcs_about_one_axis_turn_their_sum() {
        let k = Kernel::default();
        let path = [
            arc_about_z(&k, Point3::ZERO, 2.0, 0.0, 0.5 * PI),
            arc_about_z(&k, Point3::ZERO, 2.0, 0.5 * PI, 1.5 * PI),
        ];
        let PathShape::Is(a) = path_as_arc(&k, &path, &TOL).unwrap() else {
            panic!()
        };
        assert_eq!(a.axis, Vec3::Z);
        assert!((a.angle - 1.5 * PI).abs() < 1e-15);
        // Another centre.
        let off = [
            arc_about_z(&k, Point3::ZERO, 2.0, 0.0, 0.5 * PI),
            arc_about_z(&k, Point3::new(0.0, 0.0, 1e-6), 2.0, 0.5 * PI, PI),
        ];
        assert_eq!(path_as_arc(&k, &off, &TOL).unwrap(), PathShape::Not);
        // A line is no arc.
        let l = PathPiece::of_curve(line(Point3::ZERO, Point3::X));
        assert_eq!(path_as_arc(&k, &[l], &TOL).unwrap(), PathShape::Not);
    }

    #[test]
    fn arcs_whose_axes_turn_within_the_turn_tolerance_are_not_one_arc() {
        // Two quarter arcs of radius 1000, the second's axis turned 1e-6 radians about
        // its start's radius: 1 - cos is 5e-13, within 1e-12, but its end is 1e-3 off the
        // first's circle.
        let k = Kernel::default();
        let first = arc_about_z(&k, Point3::ZERO, 1000.0, 0.0, 0.5 * PI);
        let tilted = ArcDefinition {
            centre: Point3::ZERO,
            axis: Vec3::new(1e-6, 0.0, 1.0),
            reference: Vec3::Y,
            radius: 1000.0,
            start: 0.0,
            end: 0.5 * PI,
        };
        let path = [first, PathPiece::of_arc(&k, &tilted).unwrap()];
        assert_eq!(path_as_arc(&k, &path, &TOL).unwrap(), PathShape::Not);
    }

    #[test]
    fn a_line_into_a_tangent_arc_is_two_segments() {
        let k = Kernel::default();
        // Along x to (2, 0, 0), then a quarter turn about z through (2, 1, 0), radius 1.
        let l = PathPiece::of_curve(line(Point3::ZERO, Point3::new(2.0, 0.0, 0.0)));
        let d = ArcDefinition {
            centre: Point3::new(2.0, 1.0, 0.0),
            axis: Vec3::Z,
            reference: Vec3::new(0.0, -1.0, 0.0),
            radius: 1.0,
            start: 0.0,
            end: 0.5 * PI,
        };
        let path = [l.clone(), PathPiece::of_arc(&k, &d).unwrap()];
        let PathShape::Is(segs) = path_as_segments(&k, &path, &TOL).unwrap() else {
            panic!()
        };
        assert_eq!(segs.len(), 2);
        assert!((segs[0].length().unwrap() - 2.0).abs() < 1e-15);
        assert!((segs[1].length().unwrap() - 0.5 * PI).abs() < 1e-15);
        // Turned the other way: the arc leaves backwards.
        let back = ArcDefinition {
            axis: Vec3::new(0.0, 0.0, -1.0),
            ..d
        };
        let kinked = [l.clone(), PathPiece::of_arc(&k, &back).unwrap()];
        assert_eq!(path_as_segments(&k, &kinked, &TOL).unwrap(), PathShape::Not);
        // One piece is not segments.
        assert_eq!(
            path_as_segments(&k, &path[..1], &TOL).unwrap(),
            PathShape::Not
        );
    }

    #[test]
    fn segments_that_kink_part_or_turn_backwards_do_not_join() {
        let along = |a: Point3, b: Point3| Segment::Line { start: a, end: b };
        let good = [
            along(Point3::ZERO, Point3::new(4.0, 0.0, 0.0)),
            Segment::Arc {
                start: Point3::new(4.0, 0.0, 0.0),
                centre: Point3::new(4.0, 2.0, 0.0),
                axis: Vec3::Z,
                angle: 0.5 * PI,
            },
        ];
        assert_eq!(segments_join(&good, &TOL), PathShape::Is(()));
        // A kink: along x, then along y.
        let kink = [
            along(Point3::ZERO, Point3::new(4.0, 0.0, 0.0)),
            along(Point3::new(4.0, 0.0, 0.0), Point3::new(4.0, 4.0, 0.0)),
        ];
        assert_eq!(segments_join(&kink, &TOL), PathShape::Not);
        // Apart.
        let apart = [
            along(Point3::ZERO, Point3::new(4.0, 0.0, 0.0)),
            along(Point3::new(5.0, 0.0, 0.0), Point3::new(9.0, 0.0, 0.0)),
        ];
        assert_eq!(segments_join(&apart, &TOL), PathShape::Not);
        // A negative turn, a zero turn, more than a turn.
        for angle in [-0.5 * PI, 0.0, TAU + 1e-6] {
            let mut bad = good;
            bad[1] = Segment::Arc {
                start: Point3::new(4.0, 0.0, 0.0),
                centre: Point3::new(4.0, 2.0, 0.0),
                axis: Vec3::Z,
                angle,
            };
            assert_eq!(segments_join(&bad, &TOL), PathShape::Not, "{angle}");
        }
        // The arc's start where the line ends, but the arc turning the other way.
        let mut back = good;
        back[1] = Segment::Arc {
            start: Point3::new(4.0, 0.0, 0.0),
            centre: Point3::new(4.0, 2.0, 0.0),
            axis: Vec3::new(0.0, 0.0, -1.0),
            angle: 0.5 * PI,
        };
        assert_eq!(segments_join(&back, &TOL), PathShape::Not);
        assert_eq!(segments_join(&[], &TOL), PathShape::Not);
    }

    #[test]
    fn a_square_is_a_closed_loop_and_an_apex_above_it_is_off_its_plane() {
        let k = Kernel::default();
        let p = [
            Point3::ZERO,
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(2.0, 2.0, 0.0),
            Point3::new(0.0, 2.0, 0.0),
        ];
        let sq: Vec<Curve> = (0..4).map(|i| line(p[i], p[(i + 1) % 4])).collect();
        assert_eq!(closed_loop(&k, &sq, 1e-6).unwrap(), Verdict::Yes);
        assert_eq!(closed_loop(&k, &sq[..3], 1e-6).unwrap(), Verdict::No);
        assert_eq!(closed_loop(&k, &[], 1e-6).unwrap(), Verdict::No);
        // Size 2 sqrt 2 from the first corner: 1e-9 of it is 2.8e-9.
        assert_eq!(
            point_off_plane_of(&k, &sq, Point3::new(1.0, 1.0, 3.0), 1e-9).unwrap(),
            Verdict::Yes
        );
        assert_eq!(
            point_off_plane_of(&k, &sq, Point3::new(1.0, 1.0, 1e-9), 1e-9).unwrap(),
            Verdict::No
        );
        assert_eq!(
            point_off_plane_of(&k, &sq, Point3::new(5.0, 7.0, 0.0), 1e-9).unwrap(),
            Verdict::No
        );
    }

    #[test]
    fn a_closed_path_is_not_segments() {
        let k = Kernel::default();
        let path = [
            arc_about_z(&k, Point3::ZERO, 1.0, 0.0, PI),
            arc_about_z(&k, Point3::ZERO, 1.0, PI, TAU),
        ];
        assert_eq!(path_as_segments(&k, &path, &TOL).unwrap(), PathShape::Not);
    }
}
