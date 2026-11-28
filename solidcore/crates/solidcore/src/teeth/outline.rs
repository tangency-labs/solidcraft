//! The outline of a toothed part: one tooth as a chain of pieces, repeated around the axis.

use crate::math::det;
use std::f64::consts::TAU;

use crate::blend::fillet_section::interval::Ivl;
use crate::error::teeth::{TeethFailure, Why, CHORD};
use crate::geom::certified::{gamma, next_up};
use crate::nurbs::math::UNIT;
use crate::teeth::bound::{self, Bend};
use crate::teeth::{bspline, in_range, MAX_CURVE_POINTS, MAX_LENGTH, MAX_OUTLINE_POINTS};
use crate::SolidError;

/// A point of the plane of the teeth, in millimetres. The part's axis is at the origin.
pub type Point = [f64; 2];

/// What a piece of an outline is on the splined shaft or hub.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// A working flank: the side of a tooth that carries the turning load between shaft and hub.
    Flank,
    /// The round between a flank and the root.
    Fillet,
    /// The top of a tooth, and its corners.
    Tip,
    /// The bottom of the space between two teeth.
    Root,
}

/// One piece of an outline. The pieces of a tooth follow each other counter-clockwise around the
/// axis.
#[derive(Clone, Debug, PartialEq)]
pub enum Piece {
    /// A circular arc, exact: it starts at angle `start` (radians, from the x axis) on the circle
    /// of `centre` and `radius`, and turns through `sweep` (counter-clockwise when positive).
    Arc {
        /// The centre of the circle.
        centre: Point,
        /// Its radius.
        radius: f64,
        /// The angle of the first point, seen from the centre.
        start: f64,
        /// The angle turned through, signed.
        sweep: f64,
        /// What the arc is on the part.
        role: Role,
    },
    /// A straight piece, exact.
    Line {
        /// Where it starts.
        from: Point,
        /// Where it ends.
        to: Point,
        /// What the line is on the part.
        role: Role,
    },
    /// A curve that is neither a line nor an arc (an involute, a trochoid), as points on the true
    /// curve in order. The chords between the points stay within the outline's tolerance of the
    /// curve.
    Curve {
        /// At least two points of the curve.
        points: Vec<Point>,
        /// What the curve is on the part.
        role: Role,
    },
    /// A curve that is neither a line nor an arc, as one smooth B-spline that stays within the
    /// outline's tolerance of the true curve (see [`crate::teeth::bspline`]): what a CAD kernel can
    /// hold as a single edge.
    Spline {
        /// The degree of the spline.
        degree: usize,
        /// Its knots: as many as control points plus the degree plus one.
        knots: Vec<f64>,
        /// Its control points.
        control: Vec<Point>,
        /// What the curve is on the part.
        role: Role,
    },
}

impl Piece {
    /// What the piece is on the part.
    pub fn role(&self) -> Role {
        match self {
            Piece::Arc { role, .. }
            | Piece::Line { role, .. }
            | Piece::Curve { role, .. }
            | Piece::Spline { role, .. } => *role,
        }
    }

    /// The first point of the piece (none for a curve without points or a spline that is not one,
    /// which no function of these modules makes).
    pub fn start(&self) -> Option<Point> {
        match self {
            Piece::Arc {
                centre,
                radius,
                start,
                ..
            } => Some(on_circle(*centre, *radius, *start)),
            Piece::Line { from, .. } => Some(*from),
            Piece::Curve { points, .. } => points.first().copied(),
            Piece::Spline {
                degree,
                knots,
                control,
                ..
            } => {
                let (from, _) = bspline::range_typed(*degree, knots, control).ok()?;
                bspline::point_typed(*degree, knots, control, from).ok()
            }
        }
    }

    /// The last point of the piece.
    pub fn end(&self) -> Option<Point> {
        match self {
            Piece::Arc {
                centre,
                radius,
                start,
                sweep,
                ..
            } => Some(on_circle(*centre, *radius, start + sweep)),
            Piece::Line { to, .. } => Some(*to),
            Piece::Curve { points, .. } => points.last().copied(),
            Piece::Spline {
                degree,
                knots,
                control,
                ..
            } => {
                let (_, to) = bspline::range_typed(*degree, knots, control).ok()?;
                bspline::point_typed(*degree, knots, control, to).ok()
            }
        }
    }

    /// The piece as points, all but its last (the next piece starts there): an arc or a spline cut
    /// into chords proved within `tolerance` of it, a line its start, a curve its own points (whose
    /// chords the outline cannot prove again: see [`curve_tolerance`]).
    pub(super) fn chain(&self, tolerance: f64) -> Result<Vec<Point>, TeethFailure> {
        const TOO_FINE: TeethFailure = TeethFailure::Invalid(Why::CurveTooFine);
        match self {
            Piece::Arc {
                centre,
                radius,
                start,
                sweep,
                ..
            } => {
                let steps = arc_steps(*centre, *radius, *start, *sweep, tolerance)?;
                if steps > MAX_CURVE_POINTS {
                    return Err(TOO_FINE);
                }
                // Each point is computed with the platform's sine and cosine and then measured
                // against the enclosure of the arc's true point at its angle: it must lie within
                // the allowance the count of chords was proved with.
                let allowed = arc_point_allowance(*centre, *radius);
                let mut out = Vec::new();
                for i in 0..steps {
                    let angle = start + sweep * (i as f64) / (steps as f64);
                    let p = on_circle(*centre, *radius, angle);
                    let e = bound::off(p, circle_point(*centre, *radius, angle));
                    if !(e <= allowed) {
                        return Err(not_proved(tolerance, e));
                    }
                    out.push(p);
                }
                Ok(out)
            }
            Piece::Line { from, .. } => Ok(vec![*from]),
            Piece::Curve { points, .. } => Ok(points
                .iter()
                .take(points.len().saturating_sub(1))
                .copied()
                .collect()),
            Piece::Spline {
                degree,
                knots,
                control,
                ..
            } => {
                // Each stretch between two knots is one polynomial: the Bezier points of a part of
                // it bound its bends there (`stretch_bends`), so every chord is proved. The spline
                // is checked once, not at every point.
                let spline = bspline::Checked::new(*degree, knots, control)?;
                let at = |t: f64| spline.point(t).unwrap_or([f64::NAN, f64::NAN]);
                let mut out = Vec::new();
                for stretch in bspline::breaks_typed(*degree, knots, control)?.windows(2) {
                    if let [from, to] = stretch {
                        let near = stretch_bends(*degree, knots, control, *from);
                        // The kernel's evaluation is sums, products and quotients alone, so the
                        // rounding of a point is counted, not measured.
                        let rounding = break_rounding(*degree, knots, control, *from, *to);
                        let points = sampled(&at, &|_, _| rounding, &near, *from, *to, tolerance)?;
                        out.extend(points.iter().take(points.len().saturating_sub(1)));
                        if out.len() > MAX_CURVE_POINTS {
                            return Err(TOO_FINE);
                        }
                    }
                }
                Ok(out)
            }
        }
    }
}

impl Piece {
    /// The smallest and the largest distance of the piece from the origin: exact for an arc and a
    /// line, and of its points (`chain`, with the piece's end) for another curve.
    fn radii(&self, chain: &[Point]) -> (f64, f64) {
        let of = |points: &mut dyn Iterator<Item = Point>| {
            points.fold((f64::INFINITY, 0.0_f64), |(lo, hi), [x, y]| {
                (lo.min(det::hypot(x, y)), hi.max(det::hypot(x, y)))
            })
        };
        let ends = [self.start(), self.end()].into_iter().flatten();
        match self {
            Piece::Arc {
                centre,
                radius,
                start,
                sweep,
                ..
            } => {
                let (low, high) = of(&mut ends.clone());
                let (away, off) = (
                    det::atan2(centre[1], centre[0]),
                    det::hypot(centre[0], centre[1]),
                );
                // The circle's farthest point from the origin lies away from it, the nearest
                // towards it.
                let within =
                    |angle: f64| ((angle - start) * sweep.signum()).rem_euclid(TAU) <= sweep.abs();
                let high = if off > 0.0 && within(away) {
                    high.max(off + radius)
                } else {
                    high
                };
                let low = if off > 0.0 && within(away + std::f64::consts::PI) {
                    low.min((off - radius).abs())
                } else {
                    low
                };
                (low, high)
            }
            Piece::Line { from, to, .. } => {
                (off_chord([0.0, 0.0], *from, *to), of(&mut ends.clone()).1)
            }
            Piece::Curve { .. } | Piece::Spline { .. } => {
                of(&mut chain.iter().copied().chain(ends))
            }
        }
    }
}

/// The point of a circle at an angle.
pub(super) fn on_circle(centre: Point, radius: f64, angle: f64) -> Point {
    let [cx, cy] = centre;
    [cx + radius * det::cos(angle), cy + radius * det::sin(angle)]
}

/// Into how many chords an arc is cut, proved: each chord between two of the points
/// [`Piece::chain`] computes (`on_circle` at `start + sweep i / n`) stays within `tolerance` of the
/// arc, the rounding of those points counted. More than [`MAX_CURVE_POINTS`] means the tolerance
/// is too fine for the arc.
///
/// The count starts from the angle whose sagitta `2 r sin^2(step / 4)` is the tolerance (written
/// with `asin`, which loses no digits where `1 - ratio` would), and is then verified, at most
/// `ARC_ROUNDS` times, each failed round adding `1 + n / 16` chords:
///
/// - the angles `start + sweep i / n` are within `gamma(3) (|start| + |sweep|)` of exact (three
///   operations), so the widest step taken is `delta = |sweep| / n + 2 gamma(3) (..)`, in `Ivl`;
/// - the sagitta of that step, `2 r sin^2(delta / 4)`, with the sine enclosed by
///   [`bound::sin_cos`] (from its series) and the square by [`bound::sqr`];
/// - a point `centre + r (cos a, sin a)` is allowed `E` from the exact one at its angle
///   ([`arc_point_allowance`]). The points are computed with the platform's sine and cosine, whose
///   precision nothing promises, so `E` is not taken on trust: [`Piece::chain`] measures every
///   point it computes against an enclosure of the true one and refuses an arc with a point
///   farther off than `E`.
///
/// A chord within the sagitta of its arc and points within `E` of it give the bound
/// `sagitta + E`. Refused with `NotProved` where `E` alone is not below the tolerance, and after
/// `ARC_ROUNDS` rounds that prove nothing; a count past [`MAX_CURVE_POINTS`] is returned as
/// `MAX_CURVE_POINTS + 1` for the caller to refuse.
pub(super) fn arc_steps(
    centre: Point,
    radius: f64,
    start: f64,
    sweep: f64,
    tolerance: f64,
) -> Result<usize, TeethFailure> {
    const ARC_ROUNDS: u32 = 64;
    const CAP: usize = MAX_CURVE_POINTS + 1;
    // An arc whose numbers are not numbers, or whose radius is negative, has no proof.
    if !(radius >= 0.0 && radius.is_finite() && start.is_finite() && sweep.is_finite()) {
        return Err(not_proved(tolerance, f64::INFINITY));
    }
    // A point, or no turn at all: its one point is the whole of it.
    if radius == 0.0 || sweep == 0.0 {
        return Ok(1);
    }
    let ratio = (tolerance / radius).clamp(1e-15, 1.0);
    // The angle of a chord that is this far from its arc in the middle: no cancellation.
    let step = 4.0 * det::asin((0.5 * ratio).sqrt());
    let steps = if step > 0.0 {
        (sweep.abs() / step).ceil()
    } else {
        f64::INFINITY
    };
    // A count in range, so the cast loses nothing.
    let mut n = if steps >= CAP as f64 {
        CAP
    } else if steps >= 1.0 {
        steps as usize
    } else {
        1
    };
    let e = arc_point_allowance(centre, radius);
    if !(e < tolerance) {
        return Err(not_proved(tolerance, e));
    }
    let angles = bound::exact(start.abs()) + bound::exact(sweep.abs());
    let turn = (bound::exact(gamma(3)) * angles).hi;
    let mut proved = f64::INFINITY;
    for _ in 0..ARC_ROUNDS {
        let delta = bound::exact(sweep.abs()) / bound::exact(n as f64) + bound::exact(2.0 * turn);
        let (sine, _) = bound::sin_cos(delta * bound::exact(0.25));
        let sag = bound::exact(2.0 * radius) * bound::sqr(sine);
        proved = (sag + bound::exact(e)).hi;
        if proved <= tolerance {
            return Ok(n);
        }
        if n >= CAP {
            return Ok(CAP);
        }
        n = (n + 1 + n / 16).min(CAP);
    }
    Err(not_proved(tolerance, proved))
}

/// How far a computed point of an arc is allowed to lie from the arc's true point at its angle,
/// in the proof of [`arc_steps`]: `2 (256 u r + gamma(2) (|cx| + |cy| + 2 r))`. It is an
/// allowance, not a claim about the platform: each point [`Piece::chain`] computes is measured
/// against it ([`bound::off`] of the point from [`circle_point`]), and what is measured is the
/// platform's own error together with the width of the enclosure of the true point, which grows
/// with the quarter turns in the angle. The allowance holds both for any angle a tooth has, and is
/// a few parts in 1e14 of the radius.
fn arc_point_allowance(centre: Point, radius: f64) -> f64 {
    let [cx, cy] = centre;
    let size = bound::exact(cx.abs()) + bound::exact(cy.abs()) + bound::exact(2.0 * radius);
    let placed = bound::exact(256.0 * UNIT) * bound::exact(radius) + bound::exact(gamma(2)) * size;
    let e = (bound::exact(2.0) * placed).hi;
    if e.is_nan() {
        f64::INFINITY
    } else {
        e
    }
}

/// The true point of a circle at an angle, enclosed: `centre + r (cos a, sin a)` with the sine and
/// cosine from their series ([`bound::sin_cos_at`]), every operation an `Ivl` one.
fn circle_point(centre: Point, radius: f64, angle: f64) -> [Ivl; 2] {
    let ([cx, cy], r) = (centre, bound::exact(radius));
    let (sine, cosine) = bound::sin_cos_at(angle);
    [bound::exact(cx) + r * cosine, bound::exact(cy) + r * sine]
}

/// The refusal of a chord that could not be proved within `asked`: `reached` is the best bound
/// proved (infinite where it is not a number).
fn not_proved(asked: f64, reached: f64) -> TeethFailure {
    TeethFailure::NotProved {
        what: CHORD,
        asked,
        reached: if reached.is_nan() {
            f64::INFINITY
        } else {
            reached
        },
    }
}

/// The tolerance left for the chords of a piece when one of them has an end `gap` away from the
/// piece itself. A chain drops each piece's last point, so the chord that closes a piece runs to
/// the next piece's start, and pieces need only join within the join allowance; a run of pieces
/// likewise starts a piece from the end of the one before it. A chord within `tolerance - gap` of
/// its curve stays within `tolerance` of it when one end moves by at most `gap` (every point of the
/// moved chord is within `gap` of the chord), so the gap is taken off the tolerance and not left
/// out of the bound. No gap leaves the tolerance as it is.
///
/// `gap` is an upper bound of the gap, as [`bound::apart`] gives it (with nothing taken from the
/// platform's `hypot`). Counted: the difference is an `Ivl` operation and its lower end is taken.
/// Refused with `NotProved` where the gap alone is not below the tolerance.
pub(super) fn less_gap(tolerance: f64, gap: f64) -> Result<f64, TeethFailure> {
    if gap == 0.0 {
        return Ok(tolerance);
    }
    let left = (bound::exact(tolerance) - bound::exact(gap)).lo;
    if gap > 0.0 && left > 0.0 {
        Ok(left)
    } else {
        Err(not_proved(tolerance, gap))
    }
}

/// The tolerance the chords of a curve are proved to when it is to stand in an outline of
/// `tolerance` as points ([`Piece::Curve`]): sixty-three sixty-fourths of it, rounded down.
///
/// An outline cannot prove the chords of a curve piece again: the points are all it has of the
/// curve. And the chord that closes a curve piece does not end on the piece's own last point but
/// on the next piece's start, which may lie a gap away. So the tooth modules prove their curves'
/// chords to this much, which leaves a sixty-fourth of the tolerance for that gap, and an outline
/// refuses a curve piece whose gap is wider than what is left ([`curve_gap_fits`]): the closing
/// chord then stays within the whole tolerance of the curve, as every other chord does.
pub(super) fn curve_tolerance(tolerance: f64) -> f64 {
    (bound::exact(tolerance) * bound::exact(63.0 / 64.0)).lo
}

/// Whether the gap that a curve piece's closing chord bridges fits in what the curve's own chords
/// leave of the tolerance ([`curve_tolerance`]): the two together, rounded up in `Ivl`, must not be
/// above the tolerance. Refused with `NotProved` naming what they come to.
fn curve_gap_fits(tolerance: f64, gap: f64) -> Result<(), TeethFailure> {
    if gap == 0.0 {
        return Ok(());
    }
    let used = (bound::exact(curve_tolerance(tolerance)) + bound::exact(gap)).hi;
    if gap > 0.0 && used <= tolerance {
        Ok(())
    } else {
        Err(not_proved(tolerance, used))
    }
}

/// For each piece of a tooth, the gap its closing chord has to bridge ([`less_gap`]): from the
/// piece's end to the point the chord really ends on, which is the next piece's start, and for the
/// last piece the first piece's start turned onto the next tooth. Each is an upper bound of the
/// gap ([`bound::apart`]). Zero where no gap is counted: an end that is missing or not finite, and
/// a gap wider than twice the join allowance `1e-9 (1 + size)` (such pieces do not join, which
/// [`Outline::new`] refuses in its own words; twice, so that a gap at the allowance itself is
/// counted whichever way its last digit falls).
fn closing_gaps(teeth: u32, tooth: &[Piece]) -> Vec<f64> {
    let reach = tooth
        .iter()
        .flat_map(|piece| [piece.start(), piece.end()])
        .flatten()
        .flatten()
        .fold(0.0_f64, |most, v| {
            if v.is_finite() {
                most.max(v.abs())
            } else {
                most
            }
        });
    let allowed = 1e-9 * (1.0 + reach);
    let turned = tooth
        .first()
        .and_then(Piece::start)
        .map(|p| rotated(p, TAU / f64::from(teeth)));
    let onto = tooth
        .iter()
        .skip(1)
        .map(Piece::start)
        .chain(std::iter::once(turned));
    tooth
        .iter()
        .zip(onto)
        .map(|(piece, next)| {
            let gap = piece
                .end()
                .zip(next)
                .map_or(0.0, |(end, start)| bound::apart(end, start));
            if gap <= 2.0 * allowed {
                gap
            } else {
                0.0
            }
        })
        .collect()
}

/// The counted rounding of a point [`Piece::chain`] computes on the break stretch `[from, to]` of
/// a spline that [`bspline::range`] accepted: that of the span `from` lies in
/// ([`bspline::span_rounding`]), raised to cover the end point at `to`, which `Checked::point`
/// evaluates in a later span at an interior break knot (or further on, past a repeated knot), from
/// control points the first span's rounding does not cover. In effect `2 gamma(12 p + 4)` times
/// the largest coordinate of the control points from the first span's first to the end span's
/// last; constant over the stretch. The kernel's evaluation is sums, products and quotients
/// alone, which IEEE 754 rounds correctly, so this is counted and needs no measuring.
pub(super) fn break_rounding(
    degree: usize,
    knots: &[f64],
    control: &[Point],
    from: f64,
    to: f64,
) -> f64 {
    let of = |t: f64| bspline::span_rounding(degree, control, bspline::span_of(degree, knots, t));
    let (first, end) = (of(from), of(to));
    if first.is_nan() || end.is_nan() {
        f64::INFINITY
    } else {
        first.max(end)
    }
}

/// What [`Piece::chain`] proves the chords of the break stretch that starts at `from` of a spline
/// with: for a part `[lo, hi]` of the stretch, the enclosures of that part's own Bezier points
/// ([`bspline::stretch_bend`]). The whole span's hodographs alone would bound a short part of a
/// spline of a high degree many times too loosely, and the chain would be that much too fine.
pub(super) fn stretch_bends<'a>(
    degree: usize,
    knots: &'a [f64],
    control: &'a [Point],
    from: f64,
) -> impl Fn(f64, f64) -> Bend + 'a {
    let span = bspline::span_of(degree, knots, from);
    move |lo: f64, hi: f64| bspline::stretch_bend(degree, knots, control, span, lo, hi)
}

/// A point turned about the origin.
pub fn rotated(p: Point, angle: f64) -> Point {
    let (s, c) = det::sin_cos(angle);
    let [x, y] = p;
    [x * c - y * s, x * s + y * c]
}

/// The distance between two points.
pub(super) fn distance(a: Point, b: Point) -> f64 {
    let ([ax, ay], [bx, by]) = (a, b);
    det::hypot(ax - bx, ay - by)
}

/// How far `p` is from the chord between `a` and `b`.
fn off_chord(p: Point, a: Point, b: Point) -> f64 {
    let (ex, ey) = (b[0] - a[0], b[1] - a[1]);
    let length2 = ex * ex + ey * ey;
    if length2 > 0.0 {
        let t = (((p[0] - a[0]) * ex + (p[1] - a[1]) * ey) / length2).clamp(0.0, 1.0);
        distance(p, [a[0] + t * ex, a[1] + t * ey])
    } else {
        distance(p, a)
    }
}

/// How far `of` lies to the left of the line from `from` to `to`, times the length of that piece.
fn side(from: Point, to: Point, of: Point) -> f64 {
    (to[0] - from[0]) * (of[1] - from[1]) - (to[1] - from[1]) * (of[0] - from[0])
}

/// A closed chain of points made of one tooth turned by whole pitches about the origin: `chain` is
/// one tooth without its last point (the next tooth starts there), and the whole goes around
/// `teeth` times. A body that does not repeat is a ring of one tooth.
pub(super) struct Ring<'a> {
    /// One tooth's points.
    pub(super) chain: &'a [Point],
    /// How many times it repeats.
    pub(super) teeth: u32,
}

/// One node of the tree of a tooth's chords: a run of chords that follow each other (from `first`
/// up to, not including, `end`) and a circle that holds them all.
#[derive(Clone, Copy, Debug)]
struct Node {
    centre: Point,
    radius: f64,
    first: usize,
    end: usize,
}

/// How many chords a leaf of the tree holds.
const LEAF: usize = 4;

/// The circle that holds two circles.
fn holding(a: Node, b: Node) -> Node {
    let d = distance(a.centre, b.centre);
    let (first, end) = (a.first.min(b.first), a.end.max(b.end));
    if d + b.radius <= a.radius {
        return Node { first, end, ..a };
    }
    if d + a.radius <= b.radius {
        return Node { first, end, ..b };
    }
    let radius = (d + a.radius + b.radius) / 2.0;
    let share = if d > 0.0 {
        (radius - a.radius) / d
    } else {
        0.0
    };
    let centre = [
        a.centre[0] + (b.centre[0] - a.centre[0]) * share,
        a.centre[1] + (b.centre[1] - a.centre[1]) * share,
    ];
    // A little more, so that rounding never leaves a chord's end outside.
    Node {
        centre,
        radius: radius * (1.0 + 1e-12),
        first,
        end,
    }
}

impl Ring<'_> {
    fn pitch(&self) -> f64 {
        TAU / f64::from(self.teeth.max(1))
    }

    /// The point `p` of the first tooth moved onto the tooth `turns` pitches on (none at all for a
    /// whole number of turns of the ring, so that a ring of one tooth closes onto exactly its own
    /// first point).
    fn turned(&self, p: Point, turns: i64) -> Point {
        let turns = turns
            .checked_rem_euclid(i64::from(self.teeth.max(1)))
            .unwrap_or(0);
        if turns == 0 {
            p
        } else {
            rotated(p, self.pitch() * turns as f64)
        }
    }

    /// Twice the area the ring runs around, positive when it runs counter-clockwise.
    pub(super) fn doubled_area(&self) -> f64 {
        let next = self.chain.first().map(|&p| self.turned(p, 1));
        let tooth: f64 = self
            .chain
            .iter()
            .zip(self.chain.iter().skip(1).copied().chain(next))
            .map(|(a, b)| a[0] * b[1] - b[0] * a[1])
            .sum();
        tooth * f64::from(self.teeth)
    }

    /// The points of the tooth `turns` pitches on, with the point before it and the point after it:
    /// index 0 is the last point of the tooth before, index `m + 1` the first point of the tooth
    /// after.
    fn tooth_with_neighbours(&self, turns: i64) -> Vec<Point> {
        let mut out = Vec::new();
        out.extend(self.chain.last().map(|&p| self.turned(p, turns - 1)));
        out.extend(self.chain.iter().map(|&p| self.turned(p, turns)));
        out.extend(self.chain.first().map(|&p| self.turned(p, turns + 1)));
        out
    }

    /// The tree of the first tooth's chords, its leaves first and its root last.
    fn tree(&self, points: &[Point]) -> Vec<Vec<Node>> {
        let m = self.chain.len();
        // Chord i runs from point i to point i + 1 of the tooth (with the neighbours, points i + 1
        // and i + 2).
        let mut level: Vec<Node> = (0..m / LEAF + usize::from(m % LEAF != 0))
            .map(|j| {
                let (first, end) = (j * LEAF, ((j + 1) * LEAF).min(m));
                let ends = points.get(first + 1..=end + 1).unwrap_or(&[]);
                let fold = |pick: fn(f64, f64) -> f64, start: f64| {
                    ends.iter()
                        .fold([start, start], |[x, y], p| [pick(x, p[0]), pick(y, p[1])])
                };
                let (low, high) = (
                    fold(f64::min, f64::INFINITY),
                    fold(f64::max, f64::NEG_INFINITY),
                );
                let centre = [(low[0] + high[0]) / 2.0, (low[1] + high[1]) / 2.0];
                let radius = ends
                    .iter()
                    .map(|&p| distance(p, centre))
                    .fold(0.0, f64::max)
                    * (1.0 + 1e-12);
                Node {
                    centre,
                    radius,
                    first,
                    end,
                }
            })
            .collect();
        let mut levels = Vec::new();
        // Each level has half as many nodes as the one below it, so this ends after about 64 rounds
        // at most.
        for _ in 0..80 {
            let next: Vec<Node> = level
                .chunks(2)
                .filter_map(|pair| match pair {
                    [a, b] => Some(holding(*a, *b)),
                    [a] => Some(*a),
                    _ => None,
                })
                .collect();
            let done = level.len() <= 1;
            levels.push(level);
            if done {
                break;
            }
            level = next;
        }
        levels
    }

    /// Whether the ring crosses itself anywhere: a chord passing from one side of another to the
    /// other side, or the ring passing through a point of itself from one side to the other. Pieces
    /// that meet at their ends, ends that touch, and stretches that only touch do not cross; nor
    /// does anything within `eps` of touching. Each tooth is looked at against itself and against
    /// the teeth near enough to reach it, never all against all.
    pub(super) fn crosses_itself(&self, eps: f64) -> Result<bool, TeethFailure> {
        const INTRICATE: TeethFailure = TeethFailure::Invalid(Why::CrossingCheckTooIntricate);
        let m = self.chain.len();
        if m < 2 {
            return Ok(false);
        }
        let first = self.tooth_with_neighbours(0);
        let levels = self.tree(&first);
        let Some(&root) = levels.last().and_then(|top| top.first()) else {
            return Ok(false);
        };
        // The work allowed: plenty for any outline that runs as outlines do, bounded for one that
        // does not.
        let mut work: u64 = 0;
        let allowed = 400 * (m as u64) * u64::from(self.teeth) + 10_000_000;
        let reach = 2.0 * root.radius + eps;
        for turns in 0..=self.teeth / 2 {
            // The teeth whose circles cannot meet the first tooth's are passed by.
            let (s, c) = det::sin_cos(self.pitch() * f64::from(turns));
            let moved = |p: Point| [p[0] * c - p[1] * s, p[0] * s + p[1] * c];
            if turns > 0 && distance(root.centre, moved(root.centre)) > reach {
                continue;
            }
            let other = if turns == 0 {
                first.clone()
            } else {
                self.tooth_with_neighbours(i64::from(turns))
            };
            let top = levels.len().saturating_sub(1);
            let mut waiting: Vec<(usize, usize, usize, usize)> = vec![(top, 0, top, 0)];
            while let Some((la, ia, lb, ib)) = waiting.pop() {
                work += 1;
                if work > allowed {
                    return Err(INTRICATE);
                }
                let node =
                    |l: usize, i: usize| levels.get(l).and_then(|level| level.get(i)).copied();
                let (Some(a), Some(b)) = (node(la, ia), node(lb, ib)) else {
                    continue;
                };
                let itself = turns == 0 && la == lb && ia == ib;
                if !itself && distance(a.centre, moved(b.centre)) > a.radius + b.radius + eps {
                    continue;
                }
                let children = |l: usize, i: usize| -> Vec<(usize, usize)> {
                    if l == 0 {
                        return Vec::new();
                    }
                    [2 * i, 2 * i + 1]
                        .into_iter()
                        .filter(|&j| node(l - 1, j).is_some())
                        .map(|j| (l - 1, j))
                        .collect()
                };
                if itself {
                    if la == 0 {
                        for i in a.first..a.end {
                            for j in i + 1..a.end {
                                work += 1;
                                if chords_cross(&first, &other, i, j, turns == 0, eps) {
                                    return Ok(true);
                                }
                            }
                        }
                    } else {
                        let below = children(la, ia);
                        for (x, &(l, i)) in below.iter().enumerate() {
                            for &(k, j) in below.iter().skip(x) {
                                waiting.push((l, i, k, j));
                            }
                        }
                    }
                    continue;
                }
                if la == 0 && lb == 0 {
                    for i in a.first..a.end {
                        for j in b.first..b.end {
                            work += 1;
                            if chords_cross(&first, &other, i, j, turns == 0, eps) {
                                return Ok(true);
                            }
                        }
                    }
                } else if lb == 0 || (la > 0 && a.radius >= b.radius) {
                    waiting.extend(children(la, ia).into_iter().map(|(l, i)| (l, i, lb, ib)));
                } else {
                    waiting.extend(children(lb, ib).into_iter().map(|(l, i)| (la, ia, l, i)));
                }
            }
        }
        Ok(false)
    }
}

/// Whether chord `i` of the tooth `a` and chord `j` of the tooth `b` (each given with its
/// neighbouring points, as [`Ring::tooth_with_neighbours`] makes them) cross: through each other,
/// or where the start of one lies on the other or on the other's start and the ring passes from one
/// side to the other there. `same` says the two teeth are one, so that a point is not taken for
/// another that is itself.
fn chords_cross(a: &[Point], b: &[Point], i: usize, j: usize, same: bool, eps: f64) -> bool {
    let at = |points: &[Point], k: usize| points.get(k).copied().unwrap_or([f64::NAN, f64::NAN]);
    // With the neighbours in front, chord i runs from point i + 1 to point i + 2.
    let (o, p, q) = (at(a, i), at(a, i + 1), at(a, i + 2));
    let (t, u, v) = (at(b, j), at(b, j + 1), at(b, j + 2));
    if same && i == j {
        return false;
    }
    let (lp, lu) = (distance(p, q), distance(u, v));
    if lp > eps && lu > eps {
        let (dp, dq) = (side(u, v, p) / lu, side(u, v, q) / lu);
        let (du, dv) = (side(p, q, u) / lp, side(p, q, v) / lp);
        if dp * dq < 0.0
            && du * dv < 0.0
            && dp.abs().min(dq.abs()).min(du.abs()).min(dv.abs()) > eps
        {
            return true;
        }
    }
    // The start of one chord on the middle of the other, the ring passing through the other there.
    let through = |before: Point, at: Point, after: Point, from: Point, to: Point| {
        let length = distance(from, to);
        if !(length > eps) {
            return false;
        }
        let off = side(from, to, at) / length;
        let along = ((at[0] - from[0]) * (to[0] - from[0]) + (at[1] - from[1]) * (to[1] - from[1]))
            / length;
        if !(off.abs() <= eps && along > 2.0 * eps && along < length - 2.0 * eps) {
            return false;
        }
        let (was, will) = (
            side(from, to, before) / length,
            side(from, to, after) / length,
        );
        was * will < 0.0 && was.abs() > eps && will.abs() > eps
    };
    if through(o, p, q, u, v) || through(t, u, v, p, q) {
        return true;
    }
    // The two starts at one place, the ring passing through it twice: it crosses there if one of
    // the second pass's neighbours lies on each side of the first pass.
    if distance(p, u) <= 2.0 * eps {
        let directions = [o, q, t, v].map(|x| [x[0] - p[0], x[1] - p[1]]);
        if directions
            .iter()
            .any(|d| !(det::hypot(d[0], d[1]) > 2.0 * eps))
        {
            return false;
        }
        let [back, ahead, other_back, other_ahead] = directions;
        // The angle counter-clockwise from `back` to a direction, from 0 to a whole turn.
        let turn_to = |d: Point| {
            let cross = back[0] * d[1] - back[1] * d[0];
            let dot = back[0] * d[0] + back[1] * d[1];
            det::atan2(cross, dot).rem_euclid(TAU)
        };
        let (sector, x, y) = (turn_to(ahead), turn_to(other_back), turn_to(other_ahead));
        let close =
            |angle: f64, to: f64| (angle - to).abs() < 1e-9 || (angle - to).abs() > TAU - 1e-9;
        if [x, y]
            .iter()
            .any(|&angle| close(angle, 0.0) || close(angle, sector))
        {
            return false;
        }
        return (x < sector) != (y < sector);
    }
    false
}

/// The checked tolerance of an outline: above nothing, at most [`MAX_LENGTH`].
pub(super) fn checked_tolerance(tolerance: f64) -> Result<f64, TeethFailure> {
    in_range("tolerance", tolerance, 0.0, MAX_LENGTH)
}

/// A computed point of a sampled curve: its parameter, the point, and the proved bound of its
/// distance from the true point of the curve there.
type Placed = (f64, Point, f64);

/// Points of the curve `at` from `from` to `to`, both ends included, as few as keep the chords
/// within `tolerance` of the curve, each chord PROVED by [`bound::chord_bound`]. `bend(lo, hi)`
/// (`lo <= hi`) encloses the curve's derivatives over `[lo, hi]`, and `error(t, p)` is a proved
/// bound of the distance of the computed point `p = at(t)` from the true point of the curve at
/// `t`: for a curve computed with the platform's functions, the point measured against an
/// enclosure of the true one ([`bound::off`]), so that nothing rests on how well the platform
/// computes a sine.
///
/// The probes only nominate where to split; a chord is accepted only when `chord_bound` proves it
/// (bound.rs), with the larger of the proved errors of its two ends. A stretch that cannot be
/// halved and is not proved is refused.
///
/// The curve is first refused at once where an end of it is not a finite point, or is not proved
/// within the tolerance of the curve. Then, from eight pieces, each stretch is probed at a
/// quarter, the middle and three quarters: one whose probes lie farther than the tolerance from
/// its chord is halved; another is accepted if `chord_bound` over it (its parameter width rounded
/// up, `bend` over it) is within the tolerance, halved if it is not and can be, and refused with
/// `NotProved` if it cannot. So the points are closest together where the curve bends most, and
/// every chord is within the tolerance of the curve.
pub(super) fn sampled(
    at: &dyn Fn(f64) -> Point,
    error: &dyn Fn(f64, Point) -> f64,
    bend: &dyn Fn(f64, f64) -> Bend,
    from: f64,
    to: f64,
    tolerance: f64,
) -> Result<Vec<Point>, TeethFailure> {
    const TOO_FINE: TeethFailure = TeethFailure::Invalid(Why::CurveTooFine);
    const NOT_FINITE: TeethFailure = TeethFailure::Invalid(Why::OutlineNotFinite);
    const START: u32 = 8; // pieces to begin with, so that no bend is overlooked
    let placed = |t: f64| -> Placed {
        let p = at(t);
        (t, p, error(t, p))
    };
    for (_, p, e) in [placed(from), placed(to)] {
        if !p.iter().all(|v| v.is_finite()) {
            return Err(NOT_FINITE);
        }
        if !(e < tolerance) {
            return Err(not_proved(tolerance, e));
        }
    }
    // The last place is `to` itself, not a rounded sum that only comes close to it: the proof then
    // covers the curve to the very parameter the next stretch of a piece starts from.
    let place = |i: u32| {
        if i >= START {
            to
        } else {
            from + (to - from) * f64::from(i) / f64::from(START)
        }
    };
    let mut out = vec![at(from)];
    // The stretches still to settle, the next one on top.
    let mut waiting: Vec<(Placed, Placed)> = (0..START)
        .rev()
        .map(|i| (placed(place(i)), placed(place(i + 1))))
        .collect();
    while let Some(((t0, p0, e0), (t1, p1, e1))) = waiting.pop() {
        let (quarter, middle, three_quarters) = (
            0.75 * t0 + 0.25 * t1,
            0.5 * (t0 + t1),
            0.25 * t0 + 0.75 * t1,
        );
        let at_middle = placed(middle);
        // The largest of the three, and not a number if any of them is not (`f64::max` would drop
        // it, and the curve would pass for finite).
        let probe = [at(quarter), at_middle.1, at(three_quarters)]
            .into_iter()
            .map(|p| off_chord(p, p0, p1))
            .fold(
                0.0,
                |most: f64, d| if d.is_nan() || d > most { d } else { most },
            );
        // A stretch that cannot be halved any more is as fine as numbers allow.
        let halvable = middle > t0.min(t1) && middle < t0.max(t1);
        if probe.is_nan() {
            return Err(NOT_FINITE);
        }
        let split = if probe > tolerance && halvable {
            true
        } else {
            let width = next_up((t1 - t0).abs());
            let near = bend(t0.min(t1), t0.max(t1));
            // The larger of the two ends' proved errors; not a number proves nothing.
            let e = if e0.is_nan() || e1.is_nan() {
                f64::INFINITY
            } else {
                e0.max(e1)
            };
            let proved = bound::chord_bound(width, p0, p1, e, &near);
            if proved <= tolerance {
                out.push(p1);
                false
            } else if halvable {
                true
            } else {
                return Err(not_proved(tolerance, proved));
            }
        };
        if split {
            if out.len() + waiting.len() >= MAX_CURVE_POINTS {
                return Err(TOO_FINE);
            }
            waiting.push((at_middle, (t1, p1, e1)));
            waiting.push(((t0, p0, e0), at_middle));
        }
    }
    Ok(out)
}

/// The outline of a toothed part about its axis: one tooth, and how many times it repeats around
/// the axis.
///
/// The pieces of the tooth go counter-clockwise, each starting where the one before it ends, and
/// the last ends where the first piece of the next tooth starts. For a part with its material
/// inside the outline (a splined shaft) that is the usual way round; for a part with its material
/// outside (the hub that goes on it) the outline is the boundary of its opening.
#[derive(Clone, Debug, PartialEq)]
pub struct Outline {
    teeth: u32,
    tooth: Vec<Piece>,
    tolerance: f64,
    /// One tooth as points, without the last point.
    chain: Vec<Point>,
    /// The smallest and the largest distance of the outline from the axis.
    radii: (f64, f64),
}

impl Outline {
    /// An outline from one tooth, refused unless its pieces join, the tooth closes onto the next
    /// one, the tolerance can be kept with a reasonable count of points, and the whole outline (as
    /// its points) runs counter-clockwise around an area without crossing itself. Pieces that meet
    /// at a corner or run into each other smoothly, and teeth that only touch their neighbours, do
    /// not cross; nor does anything within a thousand-millionth of the outline's size of touching.
    ///
    /// `teeth` is how many times the tooth repeats around the axis; a shape that does not repeat (a
    /// keyed bore, a spline with a master tooth) is an outline of one tooth that closes onto
    /// itself.
    pub fn new(teeth: u32, tooth: Vec<Piece>, tolerance: f64) -> crate::Result<Outline> {
        Self::new_typed(teeth, tooth, tolerance)
            .map_err(|e| SolidError::teeth("teeth::outline::Outline::new", e))
    }

    /// [`Outline::new`] with its failure as a [`TeethFailure`], for the tooth module's own callers.
    pub(crate) fn new_typed(
        teeth: u32,
        tooth: Vec<Piece>,
        tolerance: f64,
    ) -> Result<Outline, TeethFailure> {
        const OPEN: TeethFailure = TeethFailure::Invalid(Why::PiecesDoNotJoin);
        const TOO_FINE: TeethFailure = TeethFailure::Invalid(Why::OutlineTooFine);
        const NOT_FINITE: TeethFailure = TeethFailure::Invalid(Why::OutlineNotFinite);
        let tolerance = checked_tolerance(tolerance)?;
        if teeth == 0 || teeth > crate::teeth::MAX_TEETH {
            let (value, up_to) = (f64::from(teeth), f64::from(crate::teeth::MAX_TEETH));
            return Err(TeethFailure::OutOfRange {
                what: "teeth",
                value,
                above: 0.0,
                up_to,
            });
        }
        let mut size = 0.0_f64;
        let mut chain = Vec::new();
        let mut radii = (f64::INFINITY, 0.0_f64);
        // The chord that closes a piece ends on the next piece's start: a gap there, within the
        // join allowance, is counted in that piece's tolerance.
        let gaps = closing_gaps(teeth, &tooth);
        for (piece, gap) in tooth.iter().zip(&gaps) {
            for p in [piece.start(), piece.end()] {
                let [x, y] = p.ok_or(OPEN)?;
                if !(x.is_finite()
                    && y.is_finite()
                    && x.abs() <= MAX_LENGTH
                    && y.abs() <= MAX_LENGTH)
                {
                    return Err(NOT_FINITE);
                }
                size = size.max(x.abs()).max(y.abs());
            }
            if let Piece::Curve { points, .. } = piece {
                if points.len() < 2
                    || points.len() > MAX_CURVE_POINTS
                    || points.iter().flatten().any(|v| !v.is_finite())
                {
                    return Err(NOT_FINITE);
                }
            }
            if let Piece::Arc {
                centre,
                radius,
                start,
                sweep,
                ..
            } = piece
            {
                if !(centre
                    .iter()
                    .chain([radius, start, sweep])
                    .all(|v| v.is_finite())
                    && *radius >= 0.0
                    && sweep.abs() <= TAU)
                {
                    return Err(NOT_FINITE);
                }
            }
            let within = less_gap(tolerance, *gap)?;
            if matches!(piece, Piece::Curve { .. }) {
                curve_gap_fits(tolerance, *gap)?;
            }
            let of_piece = piece.chain(within)?;
            let (low, high) = piece.radii(&of_piece);
            radii = (radii.0.min(low), radii.1.max(high));
            chain.extend(of_piece);
            if chain.len().saturating_mul(teeth as usize) > MAX_OUTLINE_POINTS {
                return Err(TOO_FINE);
            }
        }
        if chain.iter().flatten().any(|v| !v.is_finite()) {
            return Err(NOT_FINITE);
        }
        let join = 1e-9 * (1.0 + size);
        for pair in tooth.windows(2) {
            if let [a, b] = pair {
                if distance(a.end().ok_or(OPEN)?, b.start().ok_or(OPEN)?) > join {
                    return Err(OPEN);
                }
            }
        }
        let first = tooth.first().and_then(Piece::start).ok_or(OPEN)?;
        let last = tooth.last().and_then(Piece::end).ok_or(OPEN)?;
        if distance(last, rotated(first, TAU / f64::from(teeth))) > join {
            return Err(OPEN);
        }
        if chain.len().saturating_mul(teeth as usize) < 3 {
            return Err(TeethFailure::Invalid(Why::TooCoarse));
        }
        let ring = Ring {
            chain: &chain,
            teeth,
        };
        if !(ring.doubled_area() > 0.0) {
            return Err(TeethFailure::Invalid(Why::NotCounterClockwise));
        }
        if ring.crosses_itself(1e-9 * (1.0 + size.max(radii.1)))? {
            return Err(TeethFailure::Invalid(Why::CrossesItself));
        }
        Ok(Outline {
            teeth,
            tooth,
            tolerance,
            chain,
            radii,
        })
    }

    /// How many teeth the part has.
    pub fn teeth(&self) -> u32 {
        self.teeth
    }

    /// The pieces of one tooth, counter-clockwise. The other teeth are this one turned by whole
    /// pitches.
    pub fn tooth(&self) -> &[Piece] {
        &self.tooth
    }

    /// The tolerance the outline was asked for: how far a chord between two of its points may be
    /// from the curve.
    pub fn tolerance(&self) -> f64 {
        self.tolerance
    }

    /// The angle from one tooth to the next, in radians.
    pub fn pitch_angle(&self) -> f64 {
        TAU / f64::from(self.teeth)
    }

    /// The whole outline as a closed chain of points, counter-clockwise, without repeating the
    /// first point.
    pub fn points(&self) -> Vec<Point> {
        let mut out = Vec::new();
        for k in 0..self.teeth {
            let angle = self.pitch_angle() * f64::from(k);
            out.extend(self.chain.iter().map(|&p| rotated(p, angle)));
        }
        out
    }

    /// The smallest and the largest distance of the outline from the axis: exact for arcs and
    /// lines, and of the points for other curves.
    pub fn radii(&self) -> (f64, f64) {
        self.radii
    }
}

/// The whole outline of `teeth` teeth as a closed chain of points, counter-clockwise, without
/// repeating the first point, from the pieces of one `tooth`: what [`Outline::points`] gives for
/// the outline they were taken from, with chords within `tolerance` of each curve, the whole turned
/// about the axis by `turn` (radians). For a caller that keeps an outline it was given as data.
/// Refused: a count of teeth no outline has, a tolerance that is not accepted, a turn or a piece
/// that is not finite, and more points than [`MAX_OUTLINE_POINTS`].
pub fn points_of(
    teeth: u32,
    tooth: &[Piece],
    tolerance: f64,
    turn: f64,
) -> crate::Result<Vec<Point>> {
    points_of_typed(teeth, tooth, tolerance, turn)
        .map_err(|e| SolidError::teeth("teeth::outline::points_of", e))
}

/// [`points_of`] with its failure as a [`TeethFailure`], for the tooth module's own callers.
pub(crate) fn points_of_typed(
    teeth: u32,
    tooth: &[Piece],
    tolerance: f64,
    turn: f64,
) -> Result<Vec<Point>, TeethFailure> {
    const TOO_FINE: TeethFailure = TeethFailure::Invalid(Why::OutlineTooFine);
    const NOT_FINITE: TeethFailure = TeethFailure::Invalid(Why::OutlineNotFinite);
    let tolerance = checked_tolerance(tolerance)?;
    if teeth == 0 || teeth > crate::teeth::MAX_TEETH {
        let (value, up_to) = (f64::from(teeth), f64::from(crate::teeth::MAX_TEETH));
        return Err(TeethFailure::OutOfRange {
            what: "teeth",
            value,
            above: 0.0,
            up_to,
        });
    }
    let mut chain: Vec<Point> = Vec::new();
    // As in `Outline::new`: the gap a piece's closing chord bridges is counted in its tolerance.
    let gaps = closing_gaps(teeth, tooth);
    for (piece, gap) in tooth.iter().zip(&gaps) {
        let within = less_gap(tolerance, *gap)?;
        if matches!(piece, Piece::Curve { .. }) {
            curve_gap_fits(tolerance, *gap)?;
        }
        chain.extend(piece.chain(within)?);
        if chain.len().saturating_mul(teeth as usize) > MAX_OUTLINE_POINTS {
            return Err(TOO_FINE);
        }
    }
    if !turn.is_finite() || chain.iter().flatten().any(|v| !v.is_finite()) {
        return Err(NOT_FINITE);
    }
    let pitch = TAU / f64::from(teeth);
    let mut out = Vec::new();
    for k in 0..teeth {
        let angle = turn + pitch * f64::from(k);
        out.extend(chain.iter().map(|&p| rotated(p, angle)));
    }
    Ok(out)
}

// -------------------------------------------------------------------------------------------------
// The section of one tooth space
// -------------------------------------------------------------------------------------------------

/// The region of one tooth space of a part with its material inside its outline (a shaft), closed
/// above by a circle about the axis: what a cutter takes out between two teeth, with room above the
/// tops of the teeth.
#[derive(Clone, Debug, PartialEq)]
pub struct SpaceSection {
    /// The boundary of the region, closed and counter-clockwise: from a point on the top of the
    /// next tooth, back down that tooth's side, along the bottom of the space and up the side of
    /// the tooth before, to a point on its top; out to the circle, along it, and in again. Every
    /// piece is one of the outline's own, turned to its place and run the other way, but for the
    /// two pieces of the tops (cut where the region starts and ends), the two straight pieces out
    /// to the circle and the arc of the circle.
    pub pieces: Vec<Piece>,
    /// The outline's own pieces between the tops of the two teeth, in the order and direction of
    /// `pieces`: from the corner of the next tooth down its side, along the bottom of the space and
    /// up the side of the tooth before, to its corner. The pieces of the tops are not in it.
    pub floor: Vec<Piece>,
    /// Where `floor` starts, the corner of the tooth after the space, and where it ends, the corner
    /// of the tooth before it.
    pub corners: (Point, Point),
    /// The angle (radians, counter-clockwise from the x axis) halfway between `from` and `to`: the
    /// middle of the space.
    pub middle: f64,
    /// The angle of the point on the top of the tooth before the space where the region starts.
    pub from: f64,
    /// The angle of the point on the top of the tooth after the space where the region ends, more
    /// than `from` and less than a whole turn on from it.
    pub to: f64,
    /// Whether the region stays clear of the regions of the neighbouring spaces. It does when the
    /// teeth have tops (arcs about the axis on the outline's largest circle): the region starts and
    /// ends inside them. A tooth that comes to a point leaves its two neighbouring regions meeting
    /// at that point.
    pub apart: bool,
}

/// An arc about the axis on the circle of radius `top`, as its start and its sweep
/// (counter-clockwise).
fn top_arc(piece: &Piece, top: f64) -> Option<(f64, f64, f64)> {
    match piece {
        Piece::Arc {
            centre,
            radius,
            start,
            sweep,
            ..
        } if det::hypot(centre[0], centre[1]) <= 1e-9 * (1.0 + top)
            && (radius - top).abs() <= 1e-6 * (1.0 + top)
            && *sweep > 1e-12 =>
        {
            Some((*radius, *start, *sweep))
        }
        _ => None,
    }
}

/// A piece run the other way: the same curve from its end to its start.
pub(super) fn reversed_piece(piece: &Piece) -> Piece {
    match piece {
        Piece::Arc {
            centre,
            radius,
            start,
            sweep,
            role,
        } => Piece::Arc {
            centre: *centre,
            radius: *radius,
            start: start + sweep,
            sweep: -sweep,
            role: *role,
        },
        Piece::Line { from, to, role } => Piece::Line {
            from: *to,
            to: *from,
            role: *role,
        },
        Piece::Curve { points, role } => Piece::Curve {
            points: points.iter().rev().copied().collect(),
            role: *role,
        },
        Piece::Spline {
            degree,
            knots,
            control,
            role,
        } => {
            let (first, last) = (
                knots.first().copied().unwrap_or(0.0),
                knots.last().copied().unwrap_or(1.0),
            );
            Piece::Spline {
                degree: *degree,
                knots: knots.iter().rev().map(|&k| first + last - k).collect(),
                control: control.iter().rev().copied().collect(),
                role: *role,
            }
        }
    }
}

/// The angle `angle` brought to within half a turn of `near`.
fn near_angle(angle: f64, near: f64) -> f64 {
    near + (angle - near + std::f64::consts::PI).rem_euclid(TAU) - std::f64::consts::PI
}

/// The section of the tooth space after the tooth whose top lies nearest the angle `tooth_at`
/// (radians), closed above by the circle of radius `out_to` about the axis, for an outline of a
/// part with its material inside (a shaft). See [`SpaceSection`].
///
/// The region runs from the last top of the run of tops that the tooth's top begins (a master
/// spline's wide tooth is a run of arcs on the largest circle) to the next top round the axis
/// counter-clockwise; it starts three quarters of the way along the one top and ends a quarter of
/// the way along the other, so that the regions of neighbouring spaces are apart. Where the outline
/// has no tops (teeth that come to a point), the region starts at the point of the outline on its
/// largest circle nearest `tooth_at` and ends at the next such point.
///
/// Refused: an angle that is not a number, a radius that is not above the outline's largest radius
/// (and at most [`MAX_LENGTH`]), an outline with no tooth top or point near the angle, and two tops
/// with nothing between them.
pub fn space_section(outline: &Outline, tooth_at: f64, out_to: f64) -> crate::Result<SpaceSection> {
    space_section_typed(outline, tooth_at, out_to)
        .map_err(|e| SolidError::teeth("teeth::outline::space_section", e))
}

/// [`space_section`] with its failure as a [`TeethFailure`], for the tooth module's own callers.
pub(crate) fn space_section_typed(
    outline: &Outline,
    tooth_at: f64,
    out_to: f64,
) -> Result<SpaceSection, TeethFailure> {
    space_section_of_typed(
        outline.teeth(),
        outline.tooth(),
        outline.radii().1,
        tooth_at,
        out_to,
    )
}

/// [`space_section`] for an outline held as its plain parts: the count of `teeth`, the pieces of
/// one `tooth` and the outline's largest radius `top`. For a caller that keeps an outline it was
/// given as data: the outline was checked when it was made, and this reads no more of it than these
/// three. Refused as [`space_section`] refuses, and for a count of teeth or a largest radius that
/// no outline has.
pub fn space_section_of(
    teeth: u32,
    tooth: &[Piece],
    top: f64,
    tooth_at: f64,
    out_to: f64,
) -> crate::Result<SpaceSection> {
    space_section_of_typed(teeth, tooth, top, tooth_at, out_to)
        .map_err(|e| SolidError::teeth("teeth::outline::space_section_of", e))
}

/// [`space_section_of`] with its failure as a [`TeethFailure`], for the tooth module's own
/// callers.
pub(crate) fn space_section_of_typed(
    teeth: u32,
    tooth: &[Piece],
    top: f64,
    tooth_at: f64,
    out_to: f64,
) -> Result<SpaceSection, TeethFailure> {
    const NO_TOP: TeethFailure = TeethFailure::Invalid(Why::NoTopNearAngle);
    const NO_SPACE: TeethFailure = TeethFailure::Invalid(Why::TopsMeet);
    if !tooth_at.is_finite() {
        return Err(TeethFailure::Invalid(Why::ToothAngleNotANumber));
    }
    if teeth == 0 || teeth > crate::teeth::MAX_TEETH {
        let (value, up_to) = (f64::from(teeth), f64::from(crate::teeth::MAX_TEETH));
        return Err(TeethFailure::OutOfRange {
            what: "teeth",
            value,
            above: 0.0,
            up_to,
        });
    }
    let top = in_range("largest radius", top, 0.0, MAX_LENGTH)?;
    let out_to = in_range("radius", out_to, top, MAX_LENGTH)?;
    let count = tooth.len();
    let pitch = TAU / f64::from(teeth);
    // Piece `k` of the outline counted round from the first tooth, past the end and round again.
    let piece_at = |k: usize| -> Option<Piece> {
        let piece = tooth.get(k.checked_rem(count)?)?;
        let turns = k.checked_div(count)? as f64; // a count of pieces, far below 2^52
        Some(crate::teeth::master_spline::turned_piece(
            piece,
            pitch * turns,
        ))
    };
    let total = count.saturating_mul(teeth as usize);
    if total == 0 {
        return Err(NO_TOP);
    }
    // The top nearest the angle, by the angle of its middle.
    let nearest = (0..total)
        .filter_map(|k| {
            let (_, start, sweep) = top_arc(&piece_at(k)?, top)?;
            let middle = start + sweep / 2.0;
            Some((k, (near_angle(middle, tooth_at) - tooth_at).abs()))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1));
    let near_point = |p: Point| (det::hypot(p[0], p[1]) - top).abs() <= 1e-6 * (1.0 + top);
    let (inner, from, to, apart) = if let Some((first, _)) = nearest {
        // The last top of the run this one begins.
        let mut k = first;
        while k + 1 < first + total && piece_at(k + 1).is_some_and(|p| top_arc(&p, top).is_some()) {
            k += 1;
        }
        let start_piece = piece_at(k).ok_or(NO_TOP)?;
        let (r0, s0, w0) = top_arc(&start_piece, top).ok_or(NO_TOP)?;
        let from = s0 + 0.75 * w0;
        let role = start_piece.role();
        let mut inner = vec![Piece::Arc {
            centre: [0.0, 0.0],
            radius: r0,
            start: from,
            sweep: s0 + w0 - from,
            role,
        }];
        let mut j = k + 1;
        let end = loop {
            if j > k + total {
                return Err(NO_TOP);
            }
            let piece = piece_at(j).ok_or(NO_TOP)?;
            if let Some((r1, s1, w1)) = top_arc(&piece, top) {
                break (r1, s1, w1, piece.role());
            }
            inner.push(piece);
            j += 1;
        };
        if inner.len() < 2 {
            return Err(NO_SPACE);
        }
        let (r1, s1, w1, role) = end;
        inner.push(Piece::Arc {
            centre: [0.0, 0.0],
            radius: r1,
            start: s1,
            sweep: 0.25 * w1,
            role,
        });
        (inner, from, s1 + 0.25 * w1, true)
    } else {
        // Teeth that come to a point: from the point nearest the angle to the next point.
        let point_start = (0..total)
            .filter_map(|k| {
                let p = piece_at(k)?.start()?;
                near_point(p).then(|| {
                    (
                        k,
                        (near_angle(det::atan2(p[1], p[0]), tooth_at) - tooth_at).abs(),
                    )
                })
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let (k, _) = point_start.ok_or(NO_TOP)?;
        let first = piece_at(k).ok_or(NO_TOP)?;
        let p0 = first.start().ok_or(NO_TOP)?;
        let mut inner = Vec::new();
        let mut j = k;
        loop {
            if j >= k + total {
                return Err(NO_TOP);
            }
            let piece = piece_at(j).ok_or(NO_TOP)?;
            let ends_on_top = piece.end().is_some_and(near_point);
            inner.push(piece);
            j += 1;
            if ends_on_top {
                break;
            }
        }
        let p1 = inner.last().and_then(Piece::end).ok_or(NO_TOP)?;
        (
            inner,
            det::atan2(p0[1], p0[0]),
            det::atan2(p1[1], p1[0]),
            false,
        )
    };
    // The end a little less than a whole turn on from the start.
    let to = from + (to - from).rem_euclid(TAU);
    if !(to > from) {
        return Err(NO_SPACE);
    }
    let low_start = inner.first().and_then(Piece::start).ok_or(NO_TOP)?;
    let low_end = inner.last().and_then(Piece::end).ok_or(NO_TOP)?;
    let (high_start, high_end) = (
        on_circle([0.0, 0.0], out_to, from),
        on_circle([0.0, 0.0], out_to, to),
    );
    // The pieces between the tops (where the teeth have tops, the first and the last gathered are
    // pieces of them).
    let between = if apart {
        inner.get(1..inner.len().saturating_sub(1)).unwrap_or(&[])
    } else {
        inner.as_slice()
    };
    let floor: Vec<Piece> = between.iter().rev().map(reversed_piece).collect();
    let corners = (
        floor.first().and_then(Piece::start).ok_or(NO_SPACE)?,
        floor.last().and_then(Piece::end).ok_or(NO_SPACE)?,
    );
    // Clockwise as gathered: along the outline, out, back along the circle, in. Then run the other
    // way.
    let mut clockwise = inner;
    clockwise.push(Piece::Line {
        from: low_end,
        to: high_end,
        role: Role::Tip,
    });
    clockwise.push(Piece::Arc {
        centre: [0.0, 0.0],
        radius: out_to,
        start: to,
        sweep: from - to,
        role: Role::Tip,
    });
    clockwise.push(Piece::Line {
        from: high_start,
        to: low_start,
        role: Role::Tip,
    });
    let pieces = clockwise.iter().rev().map(reversed_piece).collect();
    Ok(SpaceSection {
        pieces,
        floor,
        corners,
        middle: 0.5 * (from + to),
        from,
        to,
        apart,
    })
}
