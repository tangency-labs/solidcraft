//! Smooth curves for CAD kernels: the point curves of an outline turned into single B-splines.
//!
//! An outline gives the curves that are neither lines nor arcs (involute flanks, root fillets) as
//! points on the true curve ([`Piece::Curve`]). A CAD kernel wants each as one smooth edge.
//! [`fitted`] replaces every such piece by one B-spline ([`Piece::Spline`]) that stays within a
//! stated tolerance of the true curve, with as few control points as the greedy way below finds:
//!
//! - The points of the source are on the true curve and its chords within the source's own
//!   tolerance of it. The source must be made at a fortieth of the tolerance asked of the splines
//!   or finer, so that what is left (the tolerance less the source's) is the allowance the spline
//!   may be away from the points and chords.
//! - A spline starts with as few control points as its degree allows, is fitted by least squares,
//!   and its distance from the source is then measured quickly: from every source point and the
//!   middle of every chord to the spline, and from the spline halfway between the feet of two
//!   points and at each knot between to the chord between them. Where the distance is largest, the
//!   stretch between two knots is halved by a new knot, and the spline is fitted again. Once the
//!   quick measure is within the allowance, the spline's distance from the source's points and
//!   chords is proved, every rounding counted; the spline is accepted only when the proved bound
//!   is within the allowance, and otherwise the stretch where the bound is largest is halved in
//!   turn. A stretch that holds few of the source's points (a nearly straight flank has few) is
//!   given more to fit, on the source's chords, so that how far the fit can go does not hang on
//!   how many points the source happens to have; the added points only help fix the spline, and
//!   it is always measured against the source's own points and chords. A spline that would need
//!   more than [`MAX_CONTROL_POINTS`] is refused.
//! - Whether the points fix the spline is decided on the data before each least-squares solve,
//!   never by trying a solve and running again after a failure.
//! - The ends of each spline are exactly the ends of the piece it replaces, so the outline still
//!   closes.
//! - Where the source runs smoothly from one piece into the next (a root fillet into a flank, a
//!   flank into a round), the spline leaves its ends in the direction of the neighbour, so no
//!   corner appears; where the source has a corner (an undercut tooth, the edge of a tip) the
//!   corner stays. A join counts as smooth when the directions of the two sides, estimated from the
//!   source, differ by less than [`SMOOTH_JOIN`].
//! - A piece that is the mirror image of an earlier piece across the x axis (the two flanks of a
//!   tooth) is not fitted again: its spline is the earlier spline mirrored, when its joins ask the
//!   same of its ends as the earlier piece's joins, mirrored, asked of that piece's ends (a smooth
//!   join where the other had a corner is fitted on its own, so that it stays smooth). A piece that
//!   is its own mirror image (a tooth that is one curve from root to root) is fitted to the points
//!   of both halves with knots placed alike on both, and comes out as its own mirror image.
//! - Arcs, lines and splines are kept as they are. The result is the same on every run.
//!
//! [`bezier_pieces`] and [`cubic_beziers`] give a spline as the polynomial pieces between its
//! knots, each with its own control points, for programs that want Bezier segments.

use crate::error::teeth::{TeethFailure, Why, FIT_DISTANCE};
use crate::geom::certified::{gamma, next_up};
use crate::teeth::bound::{self, exact};
use crate::teeth::bspline::{self, MAX_DEGREE};
use crate::teeth::outline::{distance, less_gap, rotated, Outline, Piece, Point};
use crate::teeth::{in_range, MAX_CURVE_POINTS, MAX_LENGTH};
use crate::SolidError;

/// The degree [`fitted`] uses: cubic, as CAD kernels most often hold their curves.
pub const DEFAULT_DEGREE: usize = 3;

/// The most control points one fitted spline may have. A curve that would need more is refused.
pub const MAX_CONTROL_POINTS: usize = 200;

/// The source's tolerance may be at most this share of the tolerance asked of the splines: a
/// fortieth.
pub const SOURCE_SHARE: f64 = 0.025;

/// The largest angle (radians) between the directions on either side of a join, as estimated from
/// the source, at which the join is taken to be smooth. A finer source tells smooth joins and
/// corners apart more surely.
pub const SMOOTH_JOIN: f64 = 0.02;

/// The most source points one least-squares fit uses while knots are being placed; the final
/// measurement always uses all of them.
pub(super) const WORKING_POINTS: usize = 3_000;

/// How many times a fit is done again with the points moved to their feet on the curve found.
pub(super) const CORRECTIONS: usize = 2;

/// Into how many pieces one source chord may be cut for the proof of a spline's distance.
const MAX_CHORD_SPLIT: usize = 64;

/// How many pieces of source chords one proof of a spline's distance may look at.
const CERTIFICATE_WORK: usize = 2_000_000;

/// The data do not fix the curve.
const NOT_SOLVABLE: TeethFailure = TeethFailure::Invalid(Why::FitNotSolvable);

/// An outline whose point curves have been replaced by splines, and how close each spline is to the
/// true curve.
#[derive(Clone, Debug, PartialEq)]
pub struct Fitted {
    /// The outline: the same pieces, with each point curve now one spline. Its tolerance is the
    /// tolerance asked of the splines, and [`Outline::points`] cuts the splines into chords within
    /// it.
    pub outline: Outline,
    /// For each piece of one tooth, how far its spline is from the true curve at most, in
    /// millimetres: a bound proved against the source's polyline (its points and chords), plus the
    /// source's own tolerance as the outline states it. None for a piece kept as it was.
    pub within: Vec<Option<f64>>,
    /// The tolerance of the outline the splines were fitted to.
    pub source_tolerance: f64,
}

/// The outline with every point curve replaced by one cubic spline within `tolerance` (millimetres)
/// of the true curve. See [`fit`].
pub fn fitted(outline: &Outline, tolerance: f64) -> crate::Result<Outline> {
    fitted_typed(outline, tolerance).map_err(|e| SolidError::teeth("teeth::fit::fitted", e))
}

/// [`fitted`] with its failure as a [`TeethFailure`], for the tooth module's own callers.
pub(crate) fn fitted_typed(outline: &Outline, tolerance: f64) -> Result<Outline, TeethFailure> {
    Ok(fit_typed(outline, tolerance, DEFAULT_DEGREE)?.outline)
}

/// The outline with every point curve replaced by one spline of `degree` (2 to [`MAX_DEGREE`])
/// within `tolerance` (millimetres) of the true curve, and the proved closeness of each.
///
/// Refused: a tolerance that is not a positive length, an outline with point curves made at more
/// than a fortieth of `tolerance` ([`SOURCE_SHARE`]), a tolerance too fine for the size of the
/// outline to be held in numbers, a curve that would need more than [`MAX_CONTROL_POINTS`], and a
/// curve whose distance from its spline cannot be proved within the work allowed.
pub fn fit(outline: &Outline, tolerance: f64, degree: usize) -> crate::Result<Fitted> {
    fit_typed(outline, tolerance, degree).map_err(|e| SolidError::teeth("teeth::fit::fit", e))
}

/// [`fit`] with its failure as a [`TeethFailure`], for the tooth module's own callers.
pub(crate) fn fit_typed(
    outline: &Outline,
    tolerance: f64,
    degree: usize,
) -> Result<Fitted, TeethFailure> {
    let tolerance = in_range("tolerance", tolerance, 0.0, MAX_LENGTH)?;
    if !(2..=MAX_DEGREE).contains(&degree) {
        return Err(TeethFailure::OutOfRange {
            what: "degree",
            value: degree as f64,
            above: 1.0,
            up_to: MAX_DEGREE as f64,
        });
    }
    let tooth = outline.tooth();
    let source = outline.tolerance();
    let has_curves = tooth
        .iter()
        .any(|piece| matches!(piece, Piece::Curve { .. }));
    if has_curves && !(source <= tolerance * SOURCE_SHARE) {
        return Err(TeethFailure::Invalid(Why::SourceTooCoarse));
    }
    let size = tooth
        .iter()
        .flat_map(|piece| [piece.start(), piece.end()])
        .flatten()
        .flatten()
        .fold(0.0, |s, v| {
            if v.is_finite() {
                f64::max(s, v.abs())
            } else {
                f64::INFINITY
            }
        });
    if !(size.is_finite() && tolerance >= 1e-10 * (1.0 + size)) {
        return Err(TeethFailure::Invalid(Why::ToleranceTooFineForSize));
    }
    // Rounded down, so that a distance within it plus the source's tolerance is within the
    // tolerance exactly.
    let source = if has_curves { source } else { 0.0 };
    let allowance = (exact(tolerance) - exact(source)).lo;
    let joins = join_directions(outline);
    let starting = |i: usize| -> Option<Point> {
        if i == 0 {
            joins
                .last()
                .copied()
                .flatten()
                .map(|d| rotated(d, -outline.pitch_angle()))
        } else {
            joins.get(i - 1).copied().flatten()
        }
    };
    let same = 1e-12 * (1.0 + size);
    let mut pieces: Vec<Piece> = Vec::new();
    let mut within: Vec<Option<f64>> = Vec::new();
    // For each fitted piece, the parameters near which its points' feet on the spline lie.
    let mut feet: Vec<Option<Vec<f64>>> = Vec::new();
    for (i, piece) in tooth.iter().enumerate() {
        let Piece::Curve { points, role } = piece else {
            pieces.push(piece.clone());
            within.push(None);
            feet.push(None);
            continue;
        };
        // The mirror image of a piece fitted before it: its spline mirrored, if the directions in
        // which the mirrored spline would leave its ends are those this piece's own joins ask for
        // (a mirrored end that was free at a corner, put where this piece joins smoothly, would
        // leave a corner the source does not have).
        let ends_of = |k: usize| (starting(k), joins.get(k).copied().flatten());
        let original = tooth.iter().take(i).position(|earlier| match earlier {
            Piece::Curve { points: theirs, .. } => is_mirror_of(points, theirs, same),
            _ => false,
        });
        let original = original.filter(|&k| {
            let (start, end) = ends_of(k);
            let reflected = |d: Option<Point>| d.map(|d| mirror([-d[0], -d[1]]));
            same_direction(ends_of(i).0, reflected(end))
                && same_direction(ends_of(i).1, reflected(start))
        });
        let mirrored = original.and_then(|k| match pieces.get(k) {
            Some(Piece::Spline {
                degree,
                knots,
                control,
                ..
            }) => Some(mirrored_spline(*degree, knots, control)),
            _ => None,
        });
        let own = cleaned(points);
        // The knots, control points and feet of the spline, and its proved distance from the
        // source when the fit proved it.
        let (knots, control, starts, proved) = match (
            mirrored,
            original.and_then(|k| feet.get(k).cloned().flatten()),
        ) {
            (Some((knots, control)), Some(theirs)) if theirs.len() == own.len() => (
                knots,
                control,
                theirs.iter().rev().map(|u| 1.0 - u).collect(),
                None,
            ),
            _ => {
                let symmetric = is_own_mirror(points, 2.0 * source + 1e-9 * (1.0 + size));
                let ends = (starting(i), joins.get(i).copied().flatten());
                let spline = fit_curve(points, allowance, degree, ends, symmetric)?;
                (
                    spline.knots,
                    spline.control,
                    spline.starts,
                    Some(spline.bound),
                )
            }
        };
        let curve = Spline::new(degree, knots, control)?;
        let bound = match proved {
            Some(bound) => bound,
            None => certificate(&curve, &own, &starts, allowance)?.0,
        };
        feet.push(Some(starts));
        if !(bound <= allowance) {
            return Err(TeethFailure::Invalid(Why::SplineNotWithin));
        }
        // The bound plus the source's tolerance, rounded up; within the tolerance, since the bound
        // is within the allowance.
        within.push(Some((exact(bound) + exact(source)).hi.min(tolerance)));
        pieces.push(Piece::Spline {
            degree,
            knots: curve.knots,
            control: curve.control,
            role: *role,
        });
    }
    Ok(Fitted {
        outline: Outline::new_typed(outline.teeth(), pieces, tolerance)?,
        within,
        source_tolerance: outline.tolerance(),
    })
}

// -------------------------------------------------------------------------------------------------
// Joins and mirror images
// -------------------------------------------------------------------------------------------------

/// A point mirrored across the x axis.
pub(super) fn mirror([x, y]: Point) -> Point {
    [x, -y]
}

/// Whether two end directions ask the same of a spline: both free, or both given and the same to
/// rounding.
pub(super) fn same_direction(a: Option<Point>, b: Option<Point>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a[0] * b[0] + a[1] * b[1] > 0.0 && angle_between(a, b) < 1e-9,
        _ => false,
    }
}

/// A direction of length one, if it has a finite length above nothing.
pub(super) fn unit([x, y]: Point) -> Option<Point> {
    let length = x.hypot(y);
    if length > 0.0 && length.is_finite() {
        Some([x / length, y / length])
    } else {
        None
    }
}

/// `a - b`.
pub(super) fn difference(a: Point, b: Point) -> Point {
    [a[0] - b[0], a[1] - b[1]]
}

/// The angle between two directions, 0 to pi.
pub(super) fn angle_between(a: Point, b: Point) -> f64 {
    (a[0] * b[1] - a[1] * b[0])
        .atan2(a[0] * b[0] + a[1] * b[1])
        .abs()
}

/// One side of a join: the direction of travel there, whether it is exact, the join's point, and
/// for a point curve the next point away from the join.
#[derive(Clone, Copy)]
pub(super) struct Side {
    /// The direction of travel at the join.
    pub(super) direction: Point,
    /// Whether the direction is exact (an arc, a line, a spline) or estimated from points.
    pub(super) exact: bool,
    /// The join's point.
    pub(super) at: Point,
    /// For a point curve, the next point away from the join.
    pub(super) next: Option<Point>,
}

impl Side {
    /// The side turned about the axis by `angle`.
    pub(super) fn turned(self, angle: f64) -> Side {
        Side {
            direction: rotated(self.direction, angle),
            exact: self.exact,
            at: rotated(self.at, angle),
            next: self.next.map(|p| rotated(p, angle)),
        }
    }
}

/// The direction of travel at the start of a run of points `p0, p1, p2`, from the parabola through
/// them.
pub(super) fn leaving(p0: Point, p1: Point, p2: Option<Point>) -> Option<Point> {
    let h1 = distance(p0, p1);
    let Some(p2) = p2 else {
        return unit(difference(p1, p0));
    };
    let h2 = distance(p1, p2);
    if !(h1 > 0.0 && h2 > 0.0) {
        return unit(difference(p1, p0));
    }
    let (a, b, c) = (
        -(2.0 * h1 + h2) / (h1 * (h1 + h2)),
        (h1 + h2) / (h1 * h2),
        -h1 / (h2 * (h1 + h2)),
    );
    unit([
        a * p0[0] + b * p1[0] + c * p2[0],
        a * p0[1] + b * p1[1] + c * p2[1],
    ])
}

/// The side of `piece` at its start or its end, the direction always the direction of travel.
pub(super) fn side(piece: &Piece, at_end: bool) -> Option<Side> {
    let exact = |direction: Point, at: Point| {
        Some(Side {
            direction,
            exact: true,
            at,
            next: None,
        })
    };
    match piece {
        Piece::Arc {
            radius,
            start,
            sweep,
            ..
        } => {
            let angle = if at_end { start + sweep } else { *start };
            let s = sweep.signum();
            let at = if at_end { piece.end()? } else { piece.start()? };
            if *radius > 0.0 && *sweep != 0.0 {
                exact([-angle.sin() * s, angle.cos() * s], at)
            } else {
                None
            }
        }
        Piece::Line { from, to, .. } => exact(
            unit(difference(*to, *from))?,
            if at_end { *to } else { *from },
        ),
        Piece::Spline { control, .. } => {
            let (first, rest): (Point, Vec<Point>) = if at_end {
                (
                    *control.last()?,
                    control.iter().rev().skip(1).copied().collect(),
                )
            } else {
                (*control.first()?, control.iter().skip(1).copied().collect())
            };
            let other = rest.into_iter().find(|&p| p != first)?;
            let away = unit(difference(other, first))?;
            exact(if at_end { [-away[0], -away[1]] } else { away }, first)
        }
        Piece::Curve { points, .. } => {
            let run: Vec<Point> = if at_end {
                points.iter().rev().take(3).copied().collect()
            } else {
                points.iter().take(3).copied().collect()
            };
            let (p0, p1) = (*run.first()?, *run.get(1)?);
            let away = leaving(p0, p1, run.get(2).copied())?;
            let direction = if at_end { [-away[0], -away[1]] } else { away };
            Some(Side {
                direction,
                exact: false,
                at: p0,
                next: Some(p1),
            })
        }
    }
}

/// For each piece of a tooth, the direction of travel at the join with the next piece (the last
/// piece with the next tooth's first) if the join is smooth; none at a corner.
pub(super) fn join_directions(outline: &Outline) -> Vec<Option<Point>> {
    let tooth = outline.tooth();
    let count = tooth.len();
    (0..count)
        .map(|i| {
            let before = side(tooth.get(i)?, true)?;
            let after = if i + 1 < count {
                side(tooth.get(i + 1)?, false)?
            } else {
                side(tooth.first()?, false)?.turned(outline.pitch_angle())
            };
            if !(angle_between(before.direction, after.direction) < SMOOTH_JOIN) {
                return None;
            }
            if before.exact {
                return Some(before.direction);
            }
            if after.exact {
                return Some(after.direction);
            }
            // Two point curves: the direction from the points on both sides, weighted by their
            // distances.
            let (Some(p_before), Some(p_after)) = (before.next, after.next) else {
                return Some(before.direction);
            };
            let (h0, h1) = (distance(p_before, before.at), distance(before.at, p_after));
            if !(h0 > 0.0 && h1 > 0.0) {
                return Some(before.direction);
            }
            let (w0, w1) = (h1 / (h0 + h1) / h0, h0 / (h0 + h1) / h1);
            let d = [
                w0 * (before.at[0] - p_before[0]) + w1 * (p_after[0] - before.at[0]),
                w0 * (before.at[1] - p_before[1]) + w1 * (p_after[1] - before.at[1]),
            ];
            unit(d).or(Some(before.direction))
        })
        .collect()
}

/// Whether `points` are `theirs` mirrored across the x axis and run the other way, point for point
/// within `same`.
pub(super) fn is_mirror_of(points: &[Point], theirs: &[Point], same: f64) -> bool {
    points.len() == theirs.len()
        && points
            .iter()
            .zip(theirs.iter().rev())
            .all(|(&p, &q)| distance(p, mirror(q)) <= same)
}

/// Whether a run of points is its own mirror image across the x axis: its mirrored points, taken
/// the other way, lie within `within` of its chords.
pub(super) fn is_own_mirror(points: &[Point], within: f64) -> bool {
    let (Some(&first), Some(&last)) = (points.first(), points.last()) else {
        return false;
    };
    if points.len() < 3 || distance(mirror(first), last) > within {
        return false;
    }
    let mut j = 0usize;
    for &p in points.iter().rev() {
        let q = mirror(p);
        // The chords near the last one found, forward along the run.
        let (mut best, mut best_j) = (f64::INFINITY, j);
        for k in j..(j + 8).min(points.len().saturating_sub(1)) {
            if let (Some(&a), Some(&b)) = (points.get(k), points.get(k + 1)) {
                let d = off_chord(q, a, b);
                if d < best {
                    (best, best_j) = (d, k);
                }
            }
        }
        if !(best <= within) {
            return false;
        }
        j = best_j;
    }
    true
}

/// A spline mirrored across the x axis and run the other way.
pub(super) fn mirrored_spline(
    degree: usize,
    knots: &[f64],
    control: &[Point],
) -> (Vec<f64>, Vec<Point>) {
    let (from, to) = bspline::range_typed(degree, knots, control).unwrap_or((0.0, 1.0));
    let knots = knots.iter().rev().map(|&k| from + to - k).collect();
    let control = control.iter().rev().map(|&p| mirror(p)).collect();
    (knots, control)
}

/// How far `p` is from the chord between `a` and `b`.
pub(super) fn off_chord(p: Point, a: Point, b: Point) -> f64 {
    let (ex, ey) = (b[0] - a[0], b[1] - a[1]);
    let length2 = ex * ex + ey * ey;
    if length2 > 0.0 {
        let t = (((p[0] - a[0]) * ex + (p[1] - a[1]) * ey) / length2).clamp(0.0, 1.0);
        distance(p, [a[0] + t * ex, a[1] + t * ey])
    } else {
        distance(p, a)
    }
}

// -------------------------------------------------------------------------------------------------
// One spline, evaluated with its slope
// -------------------------------------------------------------------------------------------------

/// The most basis functions that are not nothing at one parameter.
pub(super) const ORDER: usize = MAX_DEGREE + 1;

/// Knot `i`, or NaN past the end.
pub(super) fn knot(knots: &[f64], i: usize) -> f64 {
    knots.get(i).copied().unwrap_or(f64::NAN)
}

/// Value `i`, or nothing past the end.
pub(super) fn cell(values: &[f64], i: usize) -> f64 {
    values.get(i).copied().unwrap_or(0.0)
}

/// Sets value `i`, if there is one.
pub(super) fn put(values: &mut [f64], i: usize, value: f64) {
    if let Some(v) = values.get_mut(i) {
        *v = value;
    }
}

/// The stretch between knots that holds `u`: the largest `s` from `degree` to `count - 1` with knot
/// `s` at or before `u`.
pub(super) fn span_of(degree: usize, knots: &[f64], count: usize, u: f64) -> usize {
    let (mut low, mut high) = (degree, count.saturating_sub(1).max(degree));
    while low < high {
        let middle = (low + high) / 2 + (low + high) % 2;
        if knot(knots, middle) <= u {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    low
}

/// The values at `u` of the `degree + 1` basis functions that are not nothing on stretch `span`.
pub(super) fn basis(degree: usize, knots: &[f64], span: usize, u: f64) -> [f64; ORDER] {
    let (mut n, mut left, mut right) = ([0.0; ORDER], [0.0; ORDER], [0.0; ORDER]);
    put(&mut n, 0, 1.0);
    for j in 1..=degree.min(MAX_DEGREE) {
        put(&mut left, j, u - knot(knots, (span + 1).saturating_sub(j)));
        put(&mut right, j, knot(knots, span + j) - u);
        let mut saved = 0.0;
        for r in 0..j {
            let below = cell(&right, r + 1) + cell(&left, j - r);
            let share = if below == 0.0 {
                0.0
            } else {
                cell(&n, r) / below
            };
            put(&mut n, r, saved + cell(&right, r + 1) * share);
            saved = cell(&left, j - r) * share;
        }
        put(&mut n, j, saved);
    }
    n
}

/// The point of a spline at `u`: the basis functions of `u`'s stretch (Cox-de Boor) weighting the
/// control points.
pub(super) fn sum_at(degree: usize, knots: &[f64], control: &[Point], u: f64) -> Point {
    let span = span_of(degree, knots, control.len(), u);
    let n = basis(degree, knots, span, u);
    let mut p = [0.0, 0.0];
    for (r, weight) in n.iter().take(degree + 1).enumerate() {
        let c = control
            .get((span + r).wrapping_sub(degree))
            .copied()
            .unwrap_or([f64::NAN, f64::NAN]);
        p = [p[0] + weight * c[0], p[1] + weight * c[1]];
    }
    p
}

/// A spline over the parameters 0 to 1, with its derivative.
pub(super) struct Spline {
    /// Its degree.
    pub(super) degree: usize,
    /// Its knots.
    pub(super) knots: Vec<f64>,
    /// Its control points.
    pub(super) control: Vec<Point>,
    /// The control points of its derivative, a spline of one degree less.
    pub(super) slope_control: Vec<Point>,
}

const NOT_FINITE: TeethFailure = TeethFailure::Invalid(Why::FittedNotFinite);

impl Spline {
    /// The spline, if `degree`, `knots` and `control` describe one.
    pub(super) fn new(
        degree: usize,
        knots: Vec<f64>,
        control: Vec<Point>,
    ) -> Result<Spline, TeethFailure> {
        bspline::range_typed(degree, &knots, &control)?;
        let p = degree as f64;
        let slope_control = control
            .windows(2)
            .enumerate()
            .map(|(i, w)| match w {
                [a, b] => {
                    let gap = knot(&knots, i + degree + 1) - knot(&knots, i + 1);
                    if gap > 0.0 {
                        [p * (b[0] - a[0]) / gap, p * (b[1] - a[1]) / gap]
                    } else {
                        [0.0, 0.0]
                    }
                }
                _ => [0.0, 0.0],
            })
            .collect();
        Ok(Spline {
            degree,
            knots,
            control,
            slope_control,
        })
    }

    /// The point at `u`.
    pub(super) fn at(&self, u: f64) -> Point {
        sum_at(self.degree, &self.knots, &self.control, u)
    }

    /// The derivative at `u`.
    pub(super) fn slope(&self, u: f64) -> Point {
        let inner = self
            .knots
            .get(1..self.knots.len().saturating_sub(1))
            .unwrap_or(&[]);
        sum_at(self.degree.saturating_sub(1), inner, &self.slope_control, u)
    }

    /// The distance from `q` to the spline, looking from the parameter `u`: the closest of the
    /// points the steps towards the foot of the perpendicular reach (never closer than the true
    /// distance), and its parameter.
    pub(super) fn foot(&self, q: Point, u: f64) -> (f64, f64) {
        let mut u = u.clamp(0.0, 1.0);
        let (mut best, mut best_u) = (distance(self.at(u), q), u);
        for _ in 0..6 {
            let (p, d) = (self.at(u), self.slope(u));
            let (along, length2) = (
                (p[0] - q[0]) * d[0] + (p[1] - q[1]) * d[1],
                d[0] * d[0] + d[1] * d[1],
            );
            if !(length2 > 0.0) {
                break;
            }
            let next = (u - along / length2).clamp(0.0, 1.0);
            let off = distance(self.at(next), q);
            if off < best {
                (best, best_u) = (off, next);
            }
            if (next - u).abs() < 1e-16 {
                break;
            }
            u = next;
        }
        (best, best_u)
    }

    /// The quick measure of the distance between the spline and the run of `points` (at
    /// parameters `u`), each distance with the parameter where it is given to `note`: from each
    /// point and from the middle of each chord to the spline, and from the spline halfway between
    /// the feet of two points and at the knots between to their chord or a neighbouring one. It
    /// guides the placing of knots and nominates a spline for the proof; a spline is never accepted
    /// or claimed by it (see `certificate`).
    pub(super) fn measure(&self, points: &[Point], u: &[f64], note: &mut dyn FnMut(f64, f64)) {
        let feet: Vec<(f64, f64)> = points
            .iter()
            .zip(u)
            .map(|(&q, &u)| self.foot(q, u))
            .collect();
        for &(d, at) in &feet {
            note(d, at);
        }
        for (k, (pair, feet_pair)) in points.windows(2).zip(feet.windows(2)).enumerate() {
            let ([a, b], [(_, fa), (_, fb)]) = (pair, feet_pair) else {
                continue;
            };
            let middle = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
            let between = 0.5 * (fa + fb);
            note(self.foot(middle, between).0, between);
            // The spline between the feet, to this chord or a neighbouring one: halfway between
            // each two knots between the feet, and at each knot.
            let to_chords = |t: f64| {
                let s = self.at(t);
                let mut off = off_chord(s, *a, *b);
                for j in [k.checked_sub(1), Some(k + 1)].into_iter().flatten() {
                    if let (Some(&c), Some(&e)) = (points.get(j), points.get(j + 1)) {
                        off = off.min(off_chord(s, c, e));
                    }
                }
                off
            };
            note(to_chords(between), between);
            let (low, high) = (fa.min(*fb), fa.max(*fb));
            let inside = self.knots.partition_point(|&v| v <= low)
                ..self.knots.partition_point(|&v| v < high);
            let mut previous = low;
            for v in self
                .knots
                .get(inside)
                .unwrap_or(&[])
                .iter()
                .copied()
                .chain([high])
            {
                if v > previous {
                    let t = 0.5 * (previous + v);
                    note(to_chords(t), t);
                    if v < high {
                        note(to_chords(v), v);
                    }
                    previous = v;
                }
            }
        }
    }
}

// -------------------------------------------------------------------------------------------------
// The proof of a spline's distance from its source
// -------------------------------------------------------------------------------------------------

/// The knot spans of a spline that are not empty, in order, each with a bound of the size of the
/// spline's second derivative on it; read by stretches of parameter whose starts never go back.
struct Bends {
    /// Each span's first and last parameter and the bound of `|S''|` on it (infinite where none is
    /// finite).
    spans: Vec<(f64, f64, f64)>,
    /// The first span that the stretches still to come can meet.
    next: usize,
}

impl Bends {
    /// The spans of `spline`, each bounded by `bspline::span_bend` (the hull of the active control
    /// points of the second hodograph, in `Ivl`).
    fn of(spline: &Spline) -> Bends {
        let (degree, knots, control) = (spline.degree, &spline.knots, &spline.control);
        let spans = (degree..control.len())
            .filter_map(|s| {
                let (lo, hi) = (knot(knots, s), knot(knots, s + 1));
                if !(hi > lo) {
                    return None;
                }
                let second = bound::magnitude(bspline::span_bend(degree, knots, control, s).second);
                Some((
                    lo,
                    hi,
                    if second.is_finite() {
                        second
                    } else {
                        f64::INFINITY
                    },
                ))
            })
            .collect();
        Bends { spans, next: 0 }
    }

    /// The largest bound of `|S''|` over the spans that meet `[a, b]` (infinite if none does or `a`
    /// or `b` is not a number). `a` is never below the `a` of an earlier call, so the spans before
    /// it are passed over once.
    fn most(&mut self, a: f64, b: f64) -> f64 {
        while self.next + 1 < self.spans.len()
            && self.spans.get(self.next).is_some_and(|&(_, hi, _)| hi < a)
        {
            self.next += 1;
        }
        let (mut most, mut met) = (0.0_f64, false);
        for &(lo, hi, second) in self.spans.iter().skip(self.next) {
            if lo > b {
                break;
            }
            if hi >= a {
                met = true;
                if !(second <= most) {
                    most = second;
                }
            }
        }
        if met {
            most
        } else {
            f64::INFINITY
        }
    }
}

/// An upper bound of the exact `b - a` for two numbers `a <= b`: the computed difference moved up
/// one place (the exact difference is within half a place of it); 0 where `b` is not above `a`.
fn gap(a: f64, b: f64) -> f64 {
    if b > a {
        next_up(b - a)
    } else {
        0.0
    }
}

/// `h^2 / 8 * m` rounded up (`Ivl`), the sag bound of a stretch of parameter width at most `h` on
/// which the second derivative is at most `m` in size; 0 for a stretch of no width, infinite where
/// the product is not a number.
fn sag(h: f64, m: f64) -> f64 {
    if !(h > 0.0) {
        return 0.0;
    }
    up((exact(h) * exact(h) * exact(0.125) * exact(m)).hi)
}

/// `x`, or infinite where it is not a number (an upper bound proves nothing then).
fn up(x: f64) -> f64 {
    if x.is_nan() {
        f64::INFINITY
    } else {
        x
    }
}

/// Whether the proof of [`certificate`] applies to the spline. Its parameters must run from
/// exactly 0 to exactly 1: the pairing takes the first point at 0 and the last at 1 as the whole
/// spline. And no knot inside may be repeated `degree` times: the first derivative would jump
/// there, and the chord bound `du^2/8 max |S''|` needs it continuous. Every spline the fit makes
/// is such a spline; this keeps the proof from being trusted for one that is not.
pub(super) fn certifiable(spline: &Spline) -> bool {
    let (degree, count) = (spline.degree, spline.control.len());
    let whole = knot(&spline.knots, degree) == 0.0 && knot(&spline.knots, count) == 1.0;
    let inside = || {
        spline
            .knots
            .iter()
            .skip(degree + 1)
            .take(count.saturating_sub(degree + 1))
    };
    let cornered = inside()
        .zip(inside().skip(degree.saturating_sub(1)))
        .any(|(a, b)| a == b);
    whole && degree >= 1 && !cornered
}

/// The proved distance between `spline` and the polyline `points` (the source's own points), and
/// the distances per knot span that guide where to add a knot.
///
/// The proof. Every point `i` gets a parameter `u_i` on the spline: 0 for the first, 1 for the
/// last, the foot of the point (found from `starts`) for the others, then the running largest, so
/// that the parameters never go back (any parameters that never go back give a sound bound; the
/// feet only make it tight). For each source chord `i` and `s` from 0 to 1, the spline at
/// `u_i + s du` and the source chord at `s` are paired. The spline's own chord over `[u_i,
/// u_{i+1}]` is within `du^2/8 max |S''|` of the spline (Peano's kernel of linear interpolation),
/// and that chord and the source chord, both straight, are within the larger of the distances
/// between their ends. Where that bend term is above a quarter of the allowance, the source chord
/// is cut into `k` (at most `MAX_CHORD_SPLIT`) pieces at points on the chord itself, each with its
/// own parameter (the foot, kept between the chord's and never going back), and the same holds
/// for each piece. The pairing is onto both ways, so the Hausdorff distance between the spline
/// and the polyline is at most the largest piece's bound, and between the spline and the true
/// curve at most that plus the source's tolerance (proved for every outline the tooth modules
/// make; for a caller's own polyline, the tolerance the caller stated). Points are added on the
/// source's chords, not knots: the source is refined, as `filled` refines it.
///
/// The counted operations. The feet only choose parameters and are not counted. The spline's
/// point `Spline::at` is `sum_at`, Cox-de Boor `basis` and a weighted sum, not de Boor's convex
/// combinations, so its count is derived here: for `u` in its span (`span_of` puts it there)
/// `left = u - t` and `right = t - u` are not negative, and each level of `basis` takes left (1),
/// right (1), `below = right + left` (1), `share = n / below` (1), `right * share` and its sum
/// with `saved` (2), all on terms that are not negative, so after `p` levels each computed `N_j`
/// is `N_j (1 + theta)`, `|theta| <= gamma(6p)`; the weights' error moves the sum by at most
/// `gamma(6p) sum N_j |c_j| <= gamma(6p) max |c|` (the exact `N_j` are not negative and sum to
/// 1); the sum over the `p + 1` terms (one product and at most `p` additions per term) adds
/// `gamma(p + 2) sum |computed N_j| |c_j| <= gamma(p + 2) (1 + gamma(6p)) max |c|`. Per
/// coordinate that is at most `(gamma(6p) + gamma(p + 2) + gamma(p + 2) gamma(6p)) max |c| <=
/// gamma(7p + 2) max |c|`, taken as `gamma(12p + 4) max |c|`, and twice that for the two
/// coordinates of a distance: `E_S`. The distance of a point from the spline's point: the two
/// differences and `hypot` (the library's, two units in the last place), the factor
/// `1 + gamma(6)`. A point inside a chord, `a + (b - a) j / k`: four operations per coordinate
/// (the difference, the product by `j`, the quotient by `k`, the sum with `a`), within `gamma(4)
/// (|a_c| + |b_c - a_c|) <= 3 gamma(4) m` per coordinate, `m` the largest coordinate of the two
/// ends, and `6 gamma(4) m` for both. Parameter widths are rounded up (`next_up` of the computed
/// difference). The bounds of `|S''|` are `bspline::span_bend`'s, and every product and sum of
/// the bound is an `Ivl` operation, rounded outward and taken at its upper end.
///
/// A chord whose bend term stays above the allowance when cut into `MAX_CHORD_SPLIT` pieces is
/// not refused here: its term is in the bound returned, which is then above the allowance, and
/// the caller goes on as for any spline not yet within it (another knot where the bound is
/// largest, or the next degree) and ends in its own refusal if nothing is. A spline that is far
/// from the curve at one count of knots bends more than the next one will.
///
/// Refused (`NotProved` naming [`FIT_DISTANCE`]): a spline the proof does not apply to
/// ([`certifiable`]), and more than `CERTIFICATE_WORK` pieces.
pub(super) fn certificate(
    spline: &Spline,
    points: &[Point],
    starts: &[f64],
    allowance: f64,
) -> Result<(f64, Vec<f64>), TeethFailure> {
    let count = spline.control.len();
    let degree = spline.degree;
    let last = points.len().saturating_sub(1);
    if last == 0 {
        return Err(TeethFailure::Invalid(Why::CurvePointCount));
    }
    if !certifiable(spline) {
        return Err(TeethFailure::NotProved {
            what: FIT_DISTANCE,
            asked: allowance,
            reached: f64::INFINITY,
        });
    }
    // The parameter of each point on the spline, never going back.
    let mut u: Vec<f64> = Vec::new();
    let mut previous = 0.0_f64;
    for (i, &q) in points.iter().enumerate() {
        let foot = if i == 0 {
            0.0
        } else if i == last {
            1.0
        } else {
            let start = starts.get(i).copied().unwrap_or(previous);
            spline.foot(q, start).1
        };
        // A foot that is not a number, or behind the one before, takes the one before.
        if foot > previous {
            previous = foot.min(1.0);
        }
        u.push(previous);
    }
    let largest = spline
        .control
        .iter()
        .flatten()
        .fold(0.0_f64, |m, v| m.max(v.abs()));
    let e_spline = up((exact(2.0) * exact(gamma(12 * degree + 4)) * exact(largest)).hi);
    let hypot_factor = exact(1.0) + exact(gamma(6));
    // How far a point on the source chord is from the spline's point at `v`, rounded up, with
    // `extra` for the rounding of the point itself.
    let end_distance = |w: Point, v: f64, extra: f64| -> f64 {
        let s = spline.at(v);
        let h = (w[0] - s[0]).hypot(w[1] - s[1]);
        up((exact(h) * hypot_factor + exact(e_spline) + exact(extra)).hi)
    };
    let mut bends = Bends::of(spline);
    let mut by_span: Vec<f64> = spline.control.iter().map(|_| 0.0).collect();
    let mut bound = 0.0_f64;
    let mut work = 0usize;
    for (pair, params) in points.windows(2).zip(u.windows(2)) {
        let ([a, b], [ui, ui1]) = (pair, params) else {
            continue;
        };
        let (a, b, ui, ui1) = (*a, *b, *ui, *ui1);
        let du = gap(ui, ui1);
        let most = bends.most(ui, ui1);
        // Into how many pieces the chord is cut: one where its bend term is within a quarter of
        // the allowance, else as many as bring each piece's term near half the allowance.
        let pieces = if sag(du, most) <= 0.25 * allowance {
            1
        } else {
            let wanted = (du * (most / (2.0 * allowance)).sqrt()).ceil();
            if wanted.is_finite() && wanted >= 1.0 {
                wanted.min(MAX_CHORD_SPLIT as f64) as usize
            } else {
                1
            }
        };
        let m = a[0].abs().max(a[1].abs()).max(b[0].abs()).max(b[1].abs());
        let e_point = up((exact(6.0) * exact(gamma(4)) * exact(m)).hi);
        let k = pieces as f64;
        let (mut v_before, mut e_before) = (ui, end_distance(a, ui, 0.0));
        let mut chord_bound = 0.0_f64;
        for j in 1..=pieces {
            let (w, v, extra) = if j == pieces {
                (b, ui1, 0.0)
            } else {
                let jf = j as f64;
                let w = [a[0] + (b[0] - a[0]) * jf / k, a[1] + (b[1] - a[1]) * jf / k];
                let guess = ui + (ui1 - ui) * jf / k;
                let v = spline.foot(w, guess).1.max(ui).min(ui1).max(v_before);
                (w, v, e_point)
            };
            work += 1;
            if work > CERTIFICATE_WORK {
                return Err(TeethFailure::NotProved {
                    what: FIT_DISTANCE,
                    asked: allowance,
                    reached: f64::INFINITY,
                });
            }
            let e = end_distance(w, v, extra);
            let term = sag(gap(v_before, v), bends.most(v_before, v));
            let d = up((exact(e_before.max(e)) + exact(term)).hi);
            if !(d <= chord_bound) {
                chord_bound = d;
            }
            (v_before, e_before) = (v, e);
        }
        if !(chord_bound <= bound) {
            bound = chord_bound;
        }
        let span = span_of(degree, &spline.knots, count, 0.5 * (ui + ui1));
        if let Some(e) = by_span.get_mut(span) {
            if !(chord_bound <= *e) {
                *e = chord_bound;
            }
        }
    }
    Ok((bound, by_span))
}

// -------------------------------------------------------------------------------------------------
// The fit of one curve
// -------------------------------------------------------------------------------------------------

/// The points without repeats of the same point one after the other.
pub(super) fn cleaned(points: &[Point]) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::new();
    for &p in points {
        if out.last().is_none_or(|&last| last != p) {
            out.push(p);
        }
    }
    out
}

/// The parameter of each point: its distance along the chords from the first, over the whole
/// length.
pub(super) fn parameters(points: &[Point]) -> Result<Vec<f64>, TeethFailure> {
    let mut along = Vec::new();
    let mut total = 0.0;
    along.push(0.0);
    for w in points.windows(2) {
        if let [a, b] = w {
            total += distance(*a, *b);
            along.push(total);
        }
    }
    if !(total > 0.0 && total.is_finite()) {
        return Err(TeethFailure::Invalid(Why::CurveHasNoLength));
    }
    let mut u: Vec<f64> = along.iter().map(|s| s / total).collect();
    if let Some(last) = u.last_mut() {
        *last = 1.0;
    }
    Ok(u)
}

/// A point to fit: its parameter, where it is, its share of the curve, and its parameter along the
/// chords of the source (which stays when `u` is moved to the spline's own pace).
#[derive(Clone, Copy)]
pub(super) struct Datum {
    /// Its parameter on the spline.
    pub(super) u: f64,
    /// Where it is.
    pub(super) q: Point,
    /// Its share of the curve: its weight in the least-squares fit.
    pub(super) weight: f64,
    /// Its parameter along the chords of the source.
    pub(super) chord: f64,
}

/// What a control point is in the least-squares fit.
#[derive(Clone, Copy)]
pub(super) enum Slot {
    /// Fixed: an end of the curve.
    Fixed(Point),
    /// On the line from `from` along `along`, the distance being the unknown of this number.
    Along {
        /// The end the line starts from.
        from: Point,
        /// The direction of the line.
        along: Point,
        /// The number of the unknown.
        unknown: usize,
    },
    /// Free: its two coordinates are the unknowns of these numbers.
    Free(usize, usize),
}

/// The control points of a solve, and the distances along the lines from the ends.
pub(super) type Solved = (Vec<Point>, f64, f64);

/// The control points that fit `data` best for these knots, with the two ends fixed and, where a
/// direction is given, the second and the last but one on the line from the end in that direction;
/// and the distances along those lines (infinite where there is no such line).
pub(super) fn solve(
    degree: usize,
    knots: &[f64],
    data: &[Datum],
    ends: (Point, Point),
    directions: (Option<Point>, Option<Point>),
) -> Result<Solved, TeethFailure> {
    let count = knots.len().saturating_sub(degree + 1);
    let mut slots = Vec::new();
    let mut unknowns = 0usize;
    for i in 0..count {
        let slot = if i == 0 {
            Slot::Fixed(ends.0)
        } else if i + 1 == count {
            Slot::Fixed(ends.1)
        } else if let (1, Some(d)) = (i, directions.0) {
            unknowns += 1;
            Slot::Along {
                from: ends.0,
                along: d,
                unknown: unknowns - 1,
            }
        } else if let (true, Some(d)) = (i + 2 == count, directions.1) {
            unknowns += 1;
            Slot::Along {
                from: ends.1,
                along: [-d[0], -d[1]],
                unknown: unknowns - 1,
            }
        } else {
            unknowns += 2;
            Slot::Free(unknowns - 2, unknowns - 1)
        };
        slots.push(slot);
    }
    let mut matrix = vec![0.0; unknowns * unknowns];
    let mut right = vec![0.0; unknowns];
    let mut row: Vec<(usize, f64)> = Vec::new();
    for datum in data {
        let span = span_of(degree, knots, count, datum.u);
        let n = basis(degree, knots, span, datum.u);
        for axis in 0..2 {
            row.clear();
            let mut known = 0.0;
            for (r, &weight) in n.iter().take(degree + 1).enumerate() {
                match slots.get((span + r).wrapping_sub(degree)) {
                    Some(Slot::Fixed(p)) => known += weight * cell(p, axis),
                    Some(Slot::Along {
                        from,
                        along,
                        unknown,
                    }) => {
                        known += weight * cell(from, axis);
                        row.push((*unknown, weight * cell(along, axis)));
                    }
                    Some(Slot::Free(x, y)) => row.push((if axis == 0 { *x } else { *y }, weight)),
                    None => known = f64::NAN,
                }
            }
            let target = cell(&datum.q, axis) - known;
            for &(a, ca) in &row {
                if let Some(v) = right.get_mut(a) {
                    *v += datum.weight * ca * target;
                }
                for &(b, cb) in &row {
                    if let Some(v) = matrix.get_mut(a * unknowns + b) {
                        *v += datum.weight * ca * cb;
                    }
                }
            }
        }
    }
    let solution = solve_symmetric_typed(unknowns, &matrix, &right)?;
    let value = |i: usize| solution.get(i).copied().unwrap_or(f64::NAN);
    let (mut first_along, mut last_along) = (f64::INFINITY, f64::INFINITY);
    let control: Vec<Point> = slots
        .iter()
        .enumerate()
        .map(|(i, slot)| match *slot {
            Slot::Fixed(p) => p,
            Slot::Along {
                from,
                along,
                unknown,
            } => {
                let t = value(unknown);
                if i == 1 {
                    first_along = t;
                } else {
                    last_along = t;
                }
                [from[0] + t * along[0], from[1] + t * along[1]]
            }
            Slot::Free(x, y) => [value(x), value(y)],
        })
        .collect();
    if control.iter().flatten().any(|v| !v.is_finite()) {
        return Err(NOT_FINITE);
    }
    Ok((control, first_along, last_along))
}

/// The solution of `matrix * x = right` for a symmetric matrix that must be positive definite (as
/// the normal equations of a least-squares fit are), given row by row with `size` rows. Only the
/// band of the matrix that is not nothing is worked through.
///
/// Refused when the matrix is not `size` by `size`, holds numbers that are not finite, or is not
/// positive definite to within the rounding of its numbers: the points do not fix the unknowns.
pub fn solve_symmetric(size: usize, matrix: &[f64], right: &[f64]) -> crate::Result<Vec<f64>> {
    solve_symmetric_typed(size, matrix, right)
        .map_err(|e| SolidError::teeth("teeth::fit::solve_symmetric", e))
}

/// [`solve_symmetric`] with its failure as a [`TeethFailure`], for the tooth module's own callers.
pub(crate) fn solve_symmetric_typed(
    size: usize,
    matrix: &[f64],
    right: &[f64],
) -> Result<Vec<f64>, TeethFailure> {
    if size.checked_mul(size) != Some(matrix.len()) || right.len() != size {
        return Err(TeethFailure::Invalid(Why::SystemNotSquare));
    }
    let solution = solved_symmetric(size, matrix, right);
    if solution.is_none() {
        failed_solve();
    }
    solution.ok_or(NOT_SOLVABLE)
}

/// Counts the systems that had no solution, for the tests that show the fit never needs one to
/// fail: the data are checked before each solve.
#[cfg(test)]
fn failed_solve() {
    FAILED_SOLVES.with(|v| v.set(v.get().saturating_add(1)));
}

/// Outside the tests nothing is counted.
#[cfg(not(test))]
fn failed_solve() {}

#[cfg(test)]
thread_local! {
    /// How many systems [`solve_symmetric`] found without a solution on this thread.
    pub(super) static FAILED_SOLVES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// The solution of a square system that [`solve_symmetric`] has sized, or `None` where it holds
/// numbers that are not finite or is not positive definite to within the rounding of its numbers.
fn solved_symmetric(size: usize, matrix: &[f64], right: &[f64]) -> Option<Vec<f64>> {
    if matrix.iter().chain(right).any(|v| !v.is_finite()) {
        return None;
    }
    let at = |m: &[f64], i: usize, j: usize| m.get(i * size + j).copied().unwrap_or(0.0);
    // How far from the diagonal the matrix is not nothing.
    let mut band = 0usize;
    for i in 0..size {
        for j in 0..i {
            if at(matrix, i, j) != 0.0 || at(matrix, j, i) != 0.0 {
                band = band.max(i - j);
                break;
            }
        }
    }
    let largest = (0..size)
        .map(|i| at(matrix, i, i).abs())
        .fold(0.0, f64::max);
    let mut lower = vec![0.0; matrix.len()];
    for i in 0..size {
        for j in i.saturating_sub(band)..=i {
            let mut sum = at(matrix, i, j);
            for k in i.saturating_sub(band).max(j.saturating_sub(band))..j {
                sum -= at(&lower, i, k) * at(&lower, j, k);
            }
            let value = if i == j {
                if !(sum > 1e-14 * largest) {
                    return None;
                }
                sum.sqrt()
            } else {
                sum / at(&lower, j, j)
            };
            if let Some(v) = lower.get_mut(i * size + j) {
                *v = value;
            }
        }
    }
    let mut y = vec![0.0; size];
    for i in 0..size {
        let mut sum = cell(right, i);
        for k in i.saturating_sub(band)..i {
            sum -= at(&lower, i, k) * cell(&y, k);
        }
        put(&mut y, i, sum / at(&lower, i, i));
    }
    let mut x = vec![0.0; size];
    for i in (0..size).rev() {
        let mut sum = cell(&y, i);
        for k in i + 1..(i + band + 1).min(size) {
            sum -= at(&lower, k, i) * cell(&x, k);
        }
        put(&mut x, i, sum / at(&lower, i, i));
    }
    if x.iter().any(|v| !v.is_finite()) {
        return None;
    }
    Some(x)
}

/// Whether the data fix every unknown control point (Schoenberg-Whitney on the reduced unknowns):
/// the control points 1 ..= count - 2 can each be given a datum of positive weight at which its
/// basis function is not 0, the data taken in strictly increasing parameter. A necessary condition
/// for the normal equations to be positive definite, decided on the data before solving.
///
/// The ends are fixed and are not unknowns; a control point on the line from an end (`Along`) is
/// one unknown and needs its datum as a free one does. Takes `data` in order of `u`. The data are
/// walked once: each control point takes the first datum after the one before's that lies on its
/// support with its basis function above nothing (the supports run in order, so the first such
/// datum is the best choice for the points still to come).
fn interlaced(degree: usize, knots: &[f64], data: &[Datum]) -> bool {
    let count = knots.len().saturating_sub(degree + 1);
    if count <= 2 {
        return true;
    }
    let mut next = data.iter();
    let mut used = f64::NEG_INFINITY;
    for j in 1..=count - 2 {
        loop {
            let Some(d) = next.next() else {
                return false;
            };
            if !(d.weight > 0.0 && d.u > used) {
                continue;
            }
            let span = span_of(degree, knots, count, d.u);
            let first = span.saturating_sub(degree);
            if first > j {
                // Past the support of control point j.
                return false;
            }
            if span >= j
                && basis(degree, knots, span, d.u)
                    .get(j - first)
                    .is_some_and(|&n| n > 0.0)
            {
                used = d.u;
                break;
            }
        }
    }
    true
}

/// Whether two runs of data are the same: the same parameters and points, exactly.
fn same_data(a: &[Datum], b: &[Datum]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.u == y.u && x.q == y.q)
}

/// The corrections of one fit as decided before each solve (step 2): each round moves the points
/// of `working` to their feet on the spline just solved; if the moved data are in order and fix
/// the spline (`interlaced`), they are solved; otherwise that round uses the chord-length data
/// `chords`: no solve where the working data are already those (the solve would give the same
/// spline bit for bit, so the corrections end), a refusal where `chords` do not fix the spline,
/// and else their solve. A failed solve refuses; there is no second run.
fn corrected(
    degree: usize,
    knots: &[f64],
    chords: &[Datum],
    working: &mut Vec<Datum>,
    mut solved: Solved,
    ends: (Point, Point),
    directions: (Option<Point>, Option<Point>),
    corrections: usize,
) -> Result<Solved, TeethFailure> {
    for _ in 0..corrections {
        let spline = Spline::new(degree, knots.to_vec(), solved.0.clone())?;
        let mut moved = working.clone();
        for d in &mut moved {
            if d.u > 0.0 && d.u < 1.0 {
                d.u = spline.foot(d.q, d.u).1.clamp(0.0, 1.0);
            }
        }
        let in_order = !moved.windows(2).any(|w| matches!(w, [a, b] if b.u < a.u));
        if in_order && interlaced(degree, knots, &moved) {
            solved = solve(degree, knots, &moved, ends, directions)?;
            *working = moved;
        } else if same_data(working, chords) {
            break;
        } else if interlaced(degree, knots, chords) {
            solved = solve(degree, knots, chords, ends, directions)?;
            *working = chords.to_vec();
        } else {
            return Err(NOT_SOLVABLE);
        }
    }
    Ok(solved)
}

/// Every `step`-th of `all`, and the last, with their shares grown to match.
pub(super) fn thinned(all: &[Datum], most: usize) -> Vec<Datum> {
    // The count of `all` over `most`, rounded up.
    let most = most.max(1);
    let step = (all.len().saturating_add(most - 1) / most.max(1)).max(1);
    if step == 1 {
        return all.to_vec();
    }
    let mut out: Vec<Datum> = all
        .iter()
        .enumerate()
        .filter(|(i, _)| i % step.max(1) == 0)
        .map(|(_, d)| Datum {
            weight: d.weight * step as f64,
            ..*d
        })
        .collect();
    if let (Some(last), Some(kept)) = (all.last(), out.last()) {
        if kept.u != last.u {
            out.push(*last);
        }
    }
    out
}

/// The parameters `u` (of the chords) carried over to the pace of a spline, from points whose chord
/// parameters `from` have come to `to`: in a straight line between them.
pub(super) fn carried(u: &[f64], from: &[f64], to: &[f64]) -> Vec<f64> {
    let mut j = 0usize;
    u.iter()
        .map(|&v| {
            while j + 2 < from.len() && cell(from, j + 1) <= v {
                j += 1;
            }
            let (f0, f1, t0, t1) = (
                cell(from, j),
                cell(from, j + 1),
                cell(to, j),
                cell(to, j + 1),
            );
            if f1 > f0 {
                (t0 + (v - f0) / (f1 - f0) * (t1 - t0)).clamp(0.0, 1.0)
            } else {
                v
            }
        })
        .collect()
}

/// A fitted spline: its knots, control points, the parameters its points' feet are found from, and
/// its proved distance from the source.
pub(super) struct Fit {
    /// Its knots.
    pub(super) knots: Vec<f64>,
    /// Its control points.
    pub(super) control: Vec<Point>,
    /// For each of the (cleaned) points, the parameter from which its foot on the spline is found.
    pub(super) starts: Vec<f64>,
    /// The proved distance between the spline and the source's points and chords.
    pub(super) bound: f64,
}

/// One spline of `degree` through the ends of `points` and within `allowance` of them and their
/// chords, leaving its ends in the directions given; with knots placed alike on both halves when
/// the run is its own mirror image.
fn fit_curve(
    points: &[Point],
    allowance: f64,
    degree: usize,
    directions: (Option<Point>, Option<Point>),
    symmetric: bool,
) -> Result<Fit, TeethFailure> {
    // With the points moved to the spline's own pace as it is fitted, as the data decide.
    fit_curve_moving(
        points,
        allowance,
        degree,
        directions,
        symmetric,
        CORRECTIONS,
    )
}

/// `fit_curve` with `corrections` rounds of moving the points to the spline's own pace.
///
/// Before each least-squares solve the data decide: a round's first solve uses the working data
/// if they fix the spline (`interlaced`) and else the chord-length data; the corrections are
/// `corrected`'s. A failed solve refuses: nothing is solved a second time.
fn fit_curve_moving(
    points: &[Point],
    allowance: f64,
    degree: usize,
    directions: (Option<Point>, Option<Point>),
    symmetric: bool,
    corrections: usize,
) -> Result<Fit, TeethFailure> {
    const TOO_MANY: TeethFailure = TeethFailure::Invalid(Why::TooManyControlPoints);
    const POINT_COUNT: TeethFailure = TeethFailure::Invalid(Why::CurvePointCount);
    let points = cleaned(points);
    if points.len() < 2 || points.len() > MAX_CURVE_POINTS {
        return Err(POINT_COUNT);
    }
    let u = parameters(&points)?;
    let (Some(&first), Some(&last)) = (points.first(), points.last()) else {
        return Err(POINT_COUNT);
    };
    let shares: Vec<f64> = (0..points.len())
        .map(|k| {
            let before = if k > 0 {
                cell(&u, k) - cell(&u, k - 1)
            } else {
                0.0
            };
            let after = if k + 1 < u.len() {
                cell(&u, k + 1) - cell(&u, k)
            } else {
                0.0
            };
            0.5 * (before + after)
        })
        .collect();
    let mut data: Vec<Datum> = points
        .iter()
        .zip(&u)
        .zip(&shares)
        .map(|((&q, &u), &weight)| Datum {
            u,
            q,
            weight,
            chord: u,
        })
        .collect();
    if symmetric {
        let mirrored: Vec<Datum> = data
            .iter()
            .rev()
            .map(|d| Datum {
                u: 1.0 - d.u,
                q: mirror(d.q),
                chord: 1.0 - d.chord,
                ..*d
            })
            .collect();
        data.extend(mirrored);
        data.sort_by(|a, b| a.u.total_cmp(&b.u));
    }
    let by_chords = thinned(&data, WORKING_POINTS);
    let mut working = by_chords.clone();
    // Each stretch between knots holds at least this many points to fit, so that halving a stretch
    // never waits on how many points the source happens to have there.
    let need = degree + 2;
    let own: Vec<Datum> = points
        .iter()
        .zip(&u)
        .map(|(&q, &u)| Datum {
            u,
            q,
            weight: 1.0,
            chord: u,
        })
        .collect();
    let checking = thinned(&own, WORKING_POINTS);
    let (check_points, check_chords): (Vec<Point>, Vec<f64>) =
        checking.iter().map(|d| (d.q, d.u)).unzip();
    // Where the checked points' feet are on the latest spline: the start for finding them on the
    // next.
    let mut check_u = check_chords.clone();

    // Begin with as few control points as the degree and the two directions allow.
    let both = directions.0.is_some() && directions.1.is_some();
    let start_count = if both {
        (degree + 1).max(4)
    } else {
        degree + 1
    };
    let inner = start_count - (degree + 1);
    let mut knots: Vec<f64> = std::iter::repeat_n(0.0, degree + 1)
        .chain((1..=inner).map(|i| i as f64 / (inner + 1) as f64))
        .chain(std::iter::repeat_n(1.0, degree + 1))
        .collect();
    filled(&mut working, &data, degree, &knots, need);
    for _ in 0..=MAX_CONTROL_POINTS {
        let count = knots.len() - degree - 1;
        // Fitted, and fitted again with each point moved to the parameter of its foot on the curve
        // just found: the spline's own pace along the curve, not the chords'.
        // The chord-length data for these knots: what a round uses where the data it holds do not
        // fix the spline.
        let mut chords = by_chords.clone();
        filled(&mut chords, &data, degree, &knots, need);
        if !interlaced(degree, &knots, &working) {
            working.clone_from(&chords);
            // Nor do the chord-length data fix the spline: decided here, not by a solve that
            // fails.
            if !interlaced(degree, &knots, &working) {
                return Err(NOT_SOLVABLE);
            }
        }
        let solved = solve(degree, &knots, &working, (first, last), directions)?;
        let solved = corrected(
            degree,
            &knots,
            &chords,
            &mut working,
            solved,
            (first, last),
            directions,
            corrections,
        )?;
        let (mut control, mut first_along, mut last_along) = solved;
        if symmetric {
            // Exactly its own mirror image: each control point with the mirror of its partner, ends
            // as they are.
            let partners: Vec<Point> = control.iter().rev().map(|&p| mirror(p)).collect();
            for (i, (c, p)) in control.iter_mut().zip(partners).enumerate() {
                if i > 0 && i + 1 < count {
                    *c = [0.5 * (c[0] + p[0]), 0.5 * (c[1] + p[1])];
                }
            }
            if directions.0.is_some() && directions.1.is_some() {
                let along = 0.5 * (first_along + last_along);
                (first_along, last_along) = (along, along);
                if let (Some(d0), Some(d1)) = directions {
                    if let Some(c) = control.get_mut(1) {
                        *c = [first[0] + along * d0[0], first[1] + along * d0[1]];
                    }
                    if let Some(c) = control.get_mut(count.saturating_sub(2)) {
                        *c = [last[0] - along * d1[0], last[1] - along * d1[1]];
                    }
                }
            }
        }
        let spline = Spline::new(degree, knots.clone(), control)?;
        // The largest distance on each stretch between knots; a spline that would leave an end
        // backwards has a loop there, which counts as the worst of all.
        let mut by_span: Vec<f64> = (0..count).map(|_| 0.0).collect();
        let mut record = |d: f64, at: f64| {
            let d = if d.is_nan() { f64::INFINITY } else { d };
            if let Some(e) = by_span.get_mut(span_of(degree, &knots, count, at)) {
                *e = e.max(d);
            }
        };
        if !(first_along > 0.0) {
            record(f64::INFINITY, 0.0);
        } else if !(last_along > 0.0) {
            record(f64::INFINITY, 1.0);
        } else {
            let mut error = 0.0_f64;
            for (u, &q) in check_u.iter_mut().zip(&check_points) {
                if *u > 0.0 && *u < 1.0 {
                    *u = spline.foot(q, *u).1;
                }
            }
            spline.measure(&check_points, &check_u, &mut |d, at| {
                error = if d <= error { error } else { d };
                record(d, at);
            });
            if error <= allowance {
                // The quick measure nominates the spline; only the proof accepts it.
                let starts = carried(&u, &check_chords, &check_u);
                let (bound, by) = certificate(&spline, &points, &starts, allowance)?;
                if bound <= allowance {
                    return Ok(Fit {
                        knots,
                        control: spline.control,
                        starts,
                        bound,
                    });
                }
                for (e, &b) in by_span.iter_mut().zip(&by) {
                    if !(b <= *e) {
                        *e = b;
                    }
                }
            }
        }
        if count + if symmetric { 2 } else { 1 } > MAX_CONTROL_POINTS {
            return Err(TOO_MANY);
        }
        // The stretch farthest out is halved; points on the source's chords are added where a half
        // would hold too few to fix it.
        let span = (degree..count)
            .filter(|&s| cell(&by_span, s) > allowance && knot(&knots, s + 1) > knot(&knots, s))
            .max_by(|&a, &b| {
                let score =
                    |s: usize| cell(&by_span, s) * (knot(&knots, s + 1) - knot(&knots, s)).sqrt();
                score(a).total_cmp(&score(b))
            })
            .ok_or(TOO_MANY)?;
        let middle = 0.5 * (knot(&knots, span) + knot(&knots, span + 1));
        let mut new = vec![middle];
        if symmetric && (middle - 0.5).abs() > 1e-12 {
            new.push(1.0 - middle);
        }
        for value in new {
            if !(value > 0.0 && value < 1.0) || knots.contains(&value) {
                return Err(TOO_MANY);
            }
            let at = knots.iter().position(|&k| k > value).unwrap_or(knots.len());
            knots = with_value_at(&knots, at, value);
        }
        filled(&mut working, &data, degree, &knots, need);
    }
    Err(TOO_MANY)
}

/// The chord parameter at the spline parameter `u`, or the spline parameter at the chord parameter
/// `u` (`back`), in a straight line between the two points of `working` (in order of both) on
/// either side of it.
pub(super) fn between_paces(working: &[Datum], u: f64, back: bool) -> f64 {
    let from = |d: &Datum| if back { d.chord } else { d.u };
    let to = |d: &Datum| if back { d.u } else { d.chord };
    let k = working.partition_point(|d| from(d) < u);
    match (
        k.checked_sub(1).and_then(|j| working.get(j)),
        working.get(k),
    ) {
        (Some(a), Some(b)) if from(b) > from(a) => {
            to(a) + (u - from(a)) / (from(b) - from(a)) * (to(b) - to(a))
        }
        (_, Some(b)) => to(b),
        (Some(a), None) => to(a),
        (None, None) => u,
    }
}

/// The point of the source's chords at the chord parameter `c`: `source` in order along the chords.
pub(super) fn on_chords(source: &[Datum], c: f64) -> Point {
    let k = source.partition_point(|d| d.chord < c);
    match (k.checked_sub(1).and_then(|j| source.get(j)), source.get(k)) {
        (Some(a), Some(b)) if b.chord > a.chord => {
            let t = (c - a.chord) / (b.chord - a.chord);
            [
                a.q[0] + t * (b.q[0] - a.q[0]),
                a.q[1] + t * (b.q[1] - a.q[1]),
            ]
        }
        (_, Some(b)) => b.q,
        (Some(a), None) => a.q,
        (None, None) => [f64::NAN, f64::NAN],
    }
}

/// Points on the source's chords added to `working` in each stretch between knots that holds fewer
/// than `need`, evenly along the chords there, and the shares of all points worked out again from
/// their places along the chords. The added points are within the source's tolerance of the true
/// curve, as its chords are; they help fix the spline and are never what it is measured against.
pub(super) fn filled(
    working: &mut Vec<Datum>,
    source: &[Datum],
    degree: usize,
    knots: &[f64],
    need: usize,
) {
    let count = knots.len().saturating_sub(degree + 1);
    let mut held: Vec<usize> = (0..count).map(|_| 0).collect();
    for d in working.iter() {
        if let Some(h) = held.get_mut(span_of(degree, knots, count, d.u)) {
            *h += 1;
        }
    }
    let mut added = Vec::new();
    for s in degree..count {
        let (a, b) = (knot(knots, s), knot(knots, s + 1));
        if !(b > a) || cell_count(&held, s) >= need {
            continue;
        }
        let (ca, cb) = (
            between_paces(working, a, false),
            between_paces(working, b, false),
        );
        for j in 1..=need {
            let c = ca + (cb - ca) * j as f64 / (need + 1) as f64;
            let u = between_paces(working, c, true);
            if u > a && u < b {
                added.push(Datum {
                    u,
                    q: on_chords(source, c),
                    weight: 0.0,
                    chord: c,
                });
            }
        }
    }
    if added.is_empty() {
        return;
    }
    working.extend(added);
    working.sort_by(|x, y| x.u.total_cmp(&y.u).then(x.chord.total_cmp(&y.chord)));
    let places: Vec<f64> = working.iter().map(|d| d.chord).collect();
    for (k, d) in working.iter_mut().enumerate() {
        let before = k
            .checked_sub(1)
            .and_then(|j| places.get(j))
            .copied()
            .unwrap_or(d.chord);
        let after = places.get(k + 1).copied().unwrap_or(d.chord);
        d.weight = 0.5 * (after - before).max(0.0);
    }
}

/// Count `i`, or none past the end.
pub(super) fn cell_count(values: &[usize], i: usize) -> usize {
    values.get(i).copied().unwrap_or(0)
}

// -------------------------------------------------------------------------------------------------
// Bezier segments
// -------------------------------------------------------------------------------------------------

/// The knots and control points do not describe a spline.
const NOT_A_SPLINE: TeethFailure = TeethFailure::Invalid(Why::NotASpline);

/// A spline with one more knot, `u`, and the same curve (one more control point).
pub(super) fn with_knot(
    degree: usize,
    knots: &[f64],
    control: &[Point],
    u: f64,
) -> Result<(Vec<f64>, Vec<Point>), TeethFailure> {
    // The last knot at or before `u`, and how often `u` is a knot already.
    let k = knots.iter().rposition(|&v| v <= u).ok_or(NOT_A_SPLINE)?;
    let already = knots.iter().filter(|&&v| v == u).count();
    if k < degree || k < already || k - already >= control.len() {
        return Err(NOT_A_SPLINE);
    }
    let mut out = Vec::new();
    for i in 0..=control.len() {
        let q = if i + degree <= k {
            *control.get(i).ok_or(NOT_A_SPLINE)?
        } else if i <= k - already {
            let (low, high) = (knot(knots, i), knot(knots, i + degree));
            let a = if high > low {
                (u - low) / (high - low)
            } else {
                0.0
            };
            let (p0, p1) = (
                *control.get(i - 1).ok_or(NOT_A_SPLINE)?,
                *control.get(i).ok_or(NOT_A_SPLINE)?,
            );
            [(1.0 - a) * p0[0] + a * p1[0], (1.0 - a) * p0[1] + a * p1[1]]
        } else {
            *control.get(i - 1).ok_or(NOT_A_SPLINE)?
        };
        out.push(q);
    }
    Ok((with_value_at(knots, k + 1, u), out))
}

/// `values` with `value` put in at the place `at`: before the value that stood there, or at the
/// end where `at` is past it.
pub(super) fn with_value_at(values: &[f64], at: usize, value: f64) -> Vec<f64> {
    let before = values.iter().take(at);
    let after = values.iter().skip(at);
    before
        .chain(std::iter::once(&value))
        .chain(after)
        .copied()
        .collect()
}

/// The spline as its polynomial pieces between knots, in order, each as `degree + 1` control points
/// of a Bezier segment: the first and last are the ends of the piece. A piece starts at the very
/// point the piece before it ends at, bit for bit, so a program that joins pieces by their shared
/// ends finds them joined.
pub fn bezier_pieces(
    degree: usize,
    knots: &[f64],
    control: &[Point],
) -> crate::Result<Vec<Vec<Point>>> {
    bezier_pieces_typed(degree, knots, control)
        .map_err(|e| SolidError::teeth("teeth::fit::bezier_pieces", e))
}

/// [`bezier_pieces`] with its failure as a [`TeethFailure`], for the tooth module's own callers.
pub(crate) fn bezier_pieces_typed(
    degree: usize,
    knots: &[f64],
    control: &[Point],
) -> Result<Vec<Vec<Point>>, TeethFailure> {
    let breaks = bspline::breaks_typed(degree, knots, control)?;
    let mut out = Vec::new();
    for stretch in breaks.windows(2) {
        let [from, to] = stretch else { continue };
        // The piece depends on the control points of its stretch and the knots around it only.
        let span = span_of(degree, knots, control.len(), *from);
        let mut local_knots = knots
            .get(span - degree..=span + degree + 1)
            .ok_or(NOT_A_SPLINE)?
            .to_vec();
        let mut local_control = control
            .get(span - degree..=span)
            .ok_or(NOT_A_SPLINE)?
            .to_vec();
        for end in [*from, *to] {
            for _ in 0..degree {
                if local_knots.iter().filter(|&&v| v == end).count() >= degree {
                    break;
                }
                (local_knots, local_control) =
                    with_knot(degree, &local_knots, &local_control, end)?;
            }
        }
        let at = local_knots
            .iter()
            .rposition(|&v| v <= *from)
            .ok_or(NOT_A_SPLINE)?;
        let mut segment = local_control
            .get(at.checked_sub(degree).ok_or(NOT_A_SPLINE)?..=at)
            .ok_or(NOT_A_SPLINE)?
            .to_vec();
        if segment.iter().flatten().any(|v| !v.is_finite()) {
            return Err(NOT_A_SPLINE);
        }
        // Each piece is worked out on its own, so the point two pieces share can come out a last
        // digit apart (rounding); the later piece then takes the earlier one's. A spline that
        // really breaks there keeps both.
        if let (Some(start), Some(&end)) = (
            segment.first_mut(),
            out.last().and_then(|before: &Vec<Point>| before.last()),
        ) {
            let close = |a: f64, b: f64| (a - b).abs() <= 1e-12 * (1.0 + a.abs().max(b.abs()));
            if close(start[0], end[0]) && close(start[1], end[1]) {
                *start = end;
            }
        }
        out.push(segment);
    }
    Ok(out)
}

// -------------------------------------------------------------------------------------------------
// A run of pieces as one Bezier piece
// -------------------------------------------------------------------------------------------------

/// The lowest degree [`one_piece`] tries.
pub const ONE_PIECE_LEAST_DEGREE: usize = 3;

/// A run of pieces that follow each other (one side of a tooth space: its flank and its fillet
/// together) as one Bezier piece, a [`Piece::Spline`] of a single span: of the lowest degree from
/// [`ONE_PIECE_LEAST_DEGREE`] to [`MAX_DEGREE`] that keeps within `tolerance` (millimetres) of the
/// pieces it replaces, with the same two ends, leaving each end in the direction the run leaves it.
/// With it, how far it is from the run at most: a bound proved against points of the run (and the
/// chords between them) made at a fortieth of `tolerance` ([`SOURCE_SHARE`]), plus that fortieth.
/// The piece takes the role of the run's first piece.
///
/// The run's own pieces are within their outline's tolerance of the true curve, so the one piece is
/// within `tolerance` plus that of the true curve.
///
/// Refused: a tolerance that is not a positive length, an empty run, a run whose pieces do not join
/// (each must start within a thousand-millionth of the run's size of where the one before ends), a
/// run too long to be held in points, a run no Bezier piece up to [`MAX_DEGREE`] keeps within
/// `tolerance` of, and a run whose distance from a piece cannot be proved within the work allowed.
pub fn one_piece(run: &[Piece], tolerance: f64) -> crate::Result<(Piece, f64)> {
    one_piece_typed(run, tolerance).map_err(|e| SolidError::teeth("teeth::fit::one_piece", e))
}

/// [`one_piece`] with its failure as a [`TeethFailure`], for the tooth module's own callers.
///
/// The run as points: a curve's own points; a line as `MAX_DEGREE + 2` points evenly along it (at
/// least two more than any degree tried, and a line's chords add no sag); an arc or a spline as
/// its chain at the source's tolerance (`Piece::chain`, each chord proved) and its end. For each
/// degree: a degree whose unknowns the data do not fix (`interlaced`) is passed over before
/// solving; the corrections are `corrected`'s, with the run's own data (which carry the chord
/// parameters) as the chord-length data; a piece that would leave an end backwards is passed over
/// (it has a loop there); a piece is taken when its distance from the run is proved within the
/// allowance, and otherwise the next degree is tried. Every other failure refuses: nothing is
/// solved a second time.
pub(crate) fn one_piece_typed(run: &[Piece], tolerance: f64) -> Result<(Piece, f64), TeethFailure> {
    const FAR: TeethFailure = TeethFailure::Invalid(Why::RunNotOnePiece);
    const NOT_JOINED: TeethFailure = TeethFailure::Invalid(Why::RunNotJoined);
    const NOT_FINITE_RUN: TeethFailure = TeethFailure::Invalid(Why::RunNotFinite);
    let tolerance = in_range("tolerance", tolerance, 0.0, MAX_LENGTH)?;
    let (Some(first), Some(last)) = (run.first(), run.last()) else {
        return Err(TeethFailure::Invalid(Why::RunEmpty));
    };
    let source = tolerance * SOURCE_SHARE;
    // Rounded down, so that a distance within it plus the source's tolerance is within the
    // tolerance exactly.
    let allowance = (exact(tolerance) - exact(source)).lo;
    // The run as points: each piece's own, at the source's tolerance, joined where the pieces meet.
    let size = run
        .iter()
        .flat_map(|piece| [piece.start(), piece.end()])
        .flatten()
        .flatten()
        .fold(0.0, |s, v| {
            if v.is_finite() {
                f64::max(s, v.abs())
            } else {
                f64::INFINITY
            }
        });
    if !size.is_finite() {
        return Err(NOT_FINITE_RUN);
    }
    let join = 1e-9 * (1.0 + size);
    let mut points: Vec<Point> = Vec::new();
    for piece in run {
        let start = piece.start().ok_or(NOT_JOINED)?;
        // A piece after the first is run from the end of the one before it, not from its own
        // start: that distance, within the join allowance, is counted in the piece's tolerance.
        let apart = points
            .last()
            .map_or(0.0, |&previous| distance(previous, start));
        if apart > join {
            return Err(NOT_JOINED);
        }
        let room = less_gap(source, apart)?;
        let own = match piece {
            Piece::Curve { points, .. } => points.clone(),
            Piece::Line { from, to, .. } => {
                let (a, b) = (*from, *to);
                let steps = (MAX_DEGREE + 1) as f64;
                (0..=MAX_DEGREE + 1)
                    .map(|j| {
                        let t = j as f64 / steps;
                        [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
                    })
                    .collect()
            }
            Piece::Arc { .. } | Piece::Spline { .. } => {
                let mut chain = piece.chain(room)?;
                chain.push(piece.end().ok_or(NOT_JOINED)?);
                chain
            }
        };
        let skip = usize::from(!points.is_empty());
        points.extend(own.into_iter().skip(skip));
        if points.len() > MAX_CURVE_POINTS {
            return Err(TeethFailure::Invalid(Why::RunTooLong));
        }
    }
    let points = cleaned(&points);
    if points.iter().flatten().any(|v| !v.is_finite()) {
        return Err(NOT_FINITE_RUN);
    }
    let (Some(&p0), Some(&p1)) = (points.first(), points.last()) else {
        return Err(FAR);
    };
    let u = parameters(&points)?;
    let shares: Vec<f64> = (0..points.len())
        .map(|k| {
            let before = if k > 0 {
                cell(&u, k) - cell(&u, k - 1)
            } else {
                0.0
            };
            let after = if k + 1 < u.len() {
                cell(&u, k + 1) - cell(&u, k)
            } else {
                0.0
            };
            0.5 * (before + after)
        })
        .collect();
    let data: Vec<Datum> = points
        .iter()
        .zip(&u)
        .zip(&shares)
        .map(|((&q, &u), &weight)| Datum {
            u,
            q,
            weight,
            chord: u,
        })
        .collect();
    // The directions in which the run leaves its two ends (the second as the direction of travel).
    let directions = (
        side(first, false).map(|s| s.direction),
        side(last, true).map(|s| s.direction),
    );
    let role = first.role();
    for degree in ONE_PIECE_LEAST_DEGREE..=MAX_DEGREE {
        let knots: Vec<f64> = std::iter::repeat_n(0.0, degree + 1)
            .chain(std::iter::repeat_n(1.0, degree + 1))
            .collect();
        let mut working = data.clone();
        // The degree's unknowns are not fixed by the data: decided before solving.
        if !interlaced(degree, &knots, &working) {
            continue;
        }
        let solved = solve(degree, &knots, &working, (p0, p1), directions)?;
        let solved = corrected(
            degree,
            &knots,
            &data,
            &mut working,
            solved,
            (p0, p1),
            directions,
            CORRECTIONS,
        )?;
        let (control, first_along, last_along) = solved;
        // A piece that would leave an end backwards has a loop there.
        if !(first_along > 0.0 && last_along > 0.0) {
            continue;
        }
        let spline = Spline::new(degree, knots, control)?;
        let starts: Vec<f64> = working.iter().map(|d| d.u).collect();
        let (bound, _) = certificate(&spline, &points, &starts, allowance)?;
        if bound <= allowance {
            // The bound plus the source's tolerance, rounded up; within the tolerance, since the
            // bound is within the allowance.
            let within = (exact(bound) + exact(source)).hi.min(tolerance);
            let piece = Piece::Spline {
                degree,
                knots: spline.knots,
                control: spline.control,
                role,
            };
            return Ok((piece, within));
        }
    }
    Err(FAR)
}

/// A cubic spline as cubic Bezier segments, each its four control points: start, two handles, end.
pub fn cubic_beziers(knots: &[f64], control: &[Point]) -> crate::Result<Vec<[Point; 4]>> {
    cubic_beziers_typed(knots, control)
        .map_err(|e| SolidError::teeth("teeth::fit::cubic_beziers", e))
}

/// [`cubic_beziers`] with its failure as a [`TeethFailure`], for the tooth module's own callers.
pub(crate) fn cubic_beziers_typed(
    knots: &[f64],
    control: &[Point],
) -> Result<Vec<[Point; 4]>, TeethFailure> {
    bezier_pieces_typed(3, knots, control)?
        .into_iter()
        .map(|segment| match segment.as_slice() {
            [a, b, c, d] => Ok([*a, *b, *c, *d]),
            _ => Err(NOT_A_SPLINE),
        })
        .collect()
}
