//! B-splines: a smooth curve from a few control points and a row of knots, as CAD kernels hold
//! them.
//!
//! A spline of degree `p` with `n` control points has `n + p + 1` knots, none smaller than the one
//! before it, and runs over the parameters from knot `p` to knot `n`. This module evaluates such a
//! curve; [`crate::teeth::Piece::Spline`] is a piece of an outline given this way.

use crate::blend::fillet_section::interval::Ivl;
use crate::error::teeth::{TeethFailure, Why};
use crate::geom::certified::gamma;
use crate::teeth::bound::{self, Bend};
use crate::teeth::outline::Point;
use crate::teeth::MAX_CURVE_POINTS;
use crate::SolidError;

/// The highest degree accepted.
pub const MAX_DEGREE: usize = 9;

const NOT_A_SPLINE: TeethFailure = TeethFailure::Invalid(Why::NotASpline);

/// The parameters a spline runs over, first to last, if `degree`, `knots` and `control` describe
/// one: a degree from 1 to [`MAX_DEGREE`], more control points than the degree, as many knots as
/// control points plus the degree plus one, all numbers finite, the knots in order, the range not
/// empty, and no knot inside the range repeated more often than the degree (the curve would break
/// there).
pub fn range(degree: usize, knots: &[f64], control: &[Point]) -> crate::Result<(f64, f64)> {
    range_typed(degree, knots, control).map_err(|e| SolidError::teeth("teeth::bspline::range", e))
}

/// [`range`] with its failure as a [`TeethFailure`], for the tooth module's own callers.
pub(crate) fn range_typed(
    degree: usize,
    knots: &[f64],
    control: &[Point],
) -> Result<(f64, f64), TeethFailure> {
    visited(knots.len().saturating_add(control.len()));
    let n = control.len();
    if !(1..=MAX_DEGREE).contains(&degree)
        || n <= degree
        || n > MAX_CURVE_POINTS
        || knots.len() != n + degree + 1
    {
        return Err(NOT_A_SPLINE);
    }
    if knots
        .iter()
        .chain(control.iter().flatten())
        .any(|v| !v.is_finite())
        || knots.windows(2).any(|w| matches!(w, [a, b] if b < a))
    {
        return Err(NOT_A_SPLINE);
    }
    let (Some(&from), Some(&to)) = (knots.get(degree), knots.get(n)) else {
        return Err(NOT_A_SPLINE);
    };
    if !(from < to) {
        return Err(NOT_A_SPLINE);
    }
    // A knot inside the range that stands again `degree` places on is repeated too often.
    let breaks = knots
        .iter()
        .zip(knots.iter().skip(degree))
        .any(|(&a, &b)| a == b && a > from && a < to);
    if breaks {
        return Err(NOT_A_SPLINE);
    }
    Ok((from, to))
}

/// The point of a spline at the parameter `t`, which is taken into the spline's range (de Boor's
/// steps).
pub fn point(degree: usize, knots: &[f64], control: &[Point], t: f64) -> crate::Result<Point> {
    point_typed(degree, knots, control, t)
        .map_err(|e| SolidError::teeth("teeth::bspline::point", e))
}

/// [`point`] with its failure as a [`TeethFailure`], for the tooth module's own callers.
pub(crate) fn point_typed(
    degree: usize,
    knots: &[f64],
    control: &[Point],
    t: f64,
) -> Result<Point, TeethFailure> {
    let (from, to) = range_typed(degree, knots, control)?;
    if t.is_nan() {
        return Err(NOT_A_SPLINE);
    }
    let t = t.max(from).min(to);
    // The span of `t`: the last knot at or before it, among those that start a span of the range.
    let last_span = control.len() - 1;
    let mut span = degree;
    while span < last_span && knots.get(span + 1).is_some_and(|&next| next <= t) {
        span += 1;
    }
    visited(span - degree + 1);
    de_boor(degree, knots, control, span, t)
}

/// The point at `t` (already in the range) of a spline that [`range`] accepted, on the span `span`
/// (de Boor's steps).
fn de_boor(
    degree: usize,
    knots: &[f64],
    control: &[Point],
    span: usize,
    t: f64,
) -> Result<Point, TeethFailure> {
    visited((degree + 1) * (degree + 2) / 2);
    let first = span.checked_sub(degree).ok_or(NOT_A_SPLINE)?;
    let mut d: Vec<Point> = control.get(first..=span).ok_or(NOT_A_SPLINE)?.to_vec();
    for round in 1..=degree {
        for j in (round..=degree).rev() {
            let i = j + first;
            let (lo, hi) = (
                *knots.get(i).ok_or(NOT_A_SPLINE)?,
                *knots.get(i + 1 + degree - round).ok_or(NOT_A_SPLINE)?,
            );
            let share = if hi > lo { (t - lo) / (hi - lo) } else { 0.0 };
            let (a, b) = (
                *d.get(j - 1).ok_or(NOT_A_SPLINE)?,
                *d.get(j).ok_or(NOT_A_SPLINE)?,
            );
            let mixed = [
                (1.0 - share) * a[0] + share * b[0],
                (1.0 - share) * a[1] + share * b[1],
            ];
            *d.get_mut(j).ok_or(NOT_A_SPLINE)? = mixed;
        }
    }
    d.get(degree).copied().ok_or(NOT_A_SPLINE)
}

/// A spline that [`range`] has accepted, to be evaluated at many parameters without checking it
/// again: [`Checked::point`] gives exactly what [`point`] gives, and finds the span of a parameter
/// by halving instead of walking the knots from the start.
pub(super) struct Checked<'a> {
    degree: usize,
    knots: &'a [f64],
    control: &'a [Point],
    from: f64,
    to: f64,
}

impl<'a> Checked<'a> {
    /// The spline, if `degree`, `knots` and `control` describe one (see [`range`]).
    pub(super) fn new(
        degree: usize,
        knots: &'a [f64],
        control: &'a [Point],
    ) -> Result<Checked<'a>, TeethFailure> {
        let (from, to) = range_typed(degree, knots, control)?;
        Ok(Checked {
            degree,
            knots,
            control,
            from,
            to,
        })
    }

    /// The point at the parameter `t`, which is taken into the spline's range, as [`point`] gives
    /// it.
    pub(super) fn point(&self, t: f64) -> Result<Point, TeethFailure> {
        if t.is_nan() {
            return Err(NOT_A_SPLINE);
        }
        let t = t.max(self.from).min(self.to);
        // The span of `t`: the last knot at or before it among those that start a span of the
        // range, as `point` finds it. The knots are in order, so halving finds the same one.
        let last_span = self.control.len().saturating_sub(1);
        let (mut low, mut high) = (self.degree, last_span);
        let mut halvings = 1;
        while low < high {
            halvings += 1;
            // The upper of the two middles, so that `low` moves up with every step.
            let between = high - low;
            let middle = low + between / 2 + between % 2;
            if self.knots.get(middle).is_some_and(|&k| k <= t) {
                low = middle;
            } else {
                high = middle - 1;
            }
        }
        visited(halvings);
        de_boor(self.degree, self.knots, self.control, low, t)
    }
}

/// The span [`Checked::point`] evaluates a parameter `t` on, for a spline that [`range`] accepted
/// with `count` control points: the last index from `degree` to `count - 1` whose knot is at or
/// before `t` (`degree` if none is), found by the same halving. `t` is taken as it is: the caller
/// passes a parameter of the spline's range.
pub(super) fn span_of(degree: usize, knots: &[f64], count: usize, t: f64) -> usize {
    let (mut low, mut high) = (degree, count.saturating_sub(1));
    while low < high {
        let between = high - low;
        let middle = low + between / 2 + between % 2;
        if knots.get(middle).is_some_and(|&k| k <= t) {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    low
}

/// Counts the knots and control points looked at, for the tests that bound the work of sampling a
/// spline.
#[cfg(test)]
fn visited(count: usize) {
    let count = u64::try_from(count).unwrap_or(u64::MAX);
    VISITED.with(|v| v.set(v.get().saturating_add(count)));
}

/// Outside the tests nothing is counted.
#[cfg(not(test))]
fn visited(_count: usize) {}

#[cfg(test)]
thread_local! {
    /// How many knots and control points the spline functions have looked at on this thread.
    pub(super) static VISITED: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// The knots inside a spline's range where one polynomial piece ends and the next begins, with the
/// two ends of the range: each value once, in order.
pub fn breaks(degree: usize, knots: &[f64], control: &[Point]) -> crate::Result<Vec<f64>> {
    breaks_typed(degree, knots, control).map_err(|e| SolidError::teeth("teeth::bspline::breaks", e))
}

/// [`breaks`] with its failure as a [`TeethFailure`], for the tooth module's own callers.
pub(crate) fn breaks_typed(
    degree: usize,
    knots: &[f64],
    control: &[Point],
) -> Result<Vec<f64>, TeethFailure> {
    let (from, to) = range_typed(degree, knots, control)?;
    let mut out = vec![from];
    for &k in knots {
        if k > from && k < to && out.last().is_some_and(|&last| k > last) {
            out.push(k);
        }
    }
    out.push(to);
    Ok(out)
}

/// The enclosures of a spline over its knot span `span` (`knots[span] <= t < knots[span + 1]`,
/// `degree <= span < control.len()`) from the control points of its first and second hodograph
/// that are active there, computed in `Ivl`, and the counted rounding of a de Boor point there. A
/// degree 1 span has `second = [0, 0]` and `second_size` 0. An index outside the spline, or a
/// degree outside 1 to [`MAX_DEGREE`], gives `Bend::unknown()`. Its `rounding` covers points
/// evaluated in span `span` only; `Piece::chain` raises it for a break stretch whose end point
/// `Checked::point` evaluates in a later span.
///
/// With `p` the degree, `s` the span, `P` the control points and `t` the knots, the active control
/// points are `P[s-p] ..= P[s]`, and:
///
/// - the first hodograph `Q[j] = p (P[j+1] - P[j]) / (t[j+p+1] - t[j+1])`, `j = s-p ..= s-1`;
/// - the second `R[j] = (p-1) (Q[j+1] - Q[j]) / (t[j+p+1] - t[j+2])`, `j = s-p ..= s-2`.
///
/// The derivative of the spline on the span is a spline of degree `p - 1` whose active control
/// points there are those `Q[j]`, so it lies in their convex hull, and the second derivative in
/// the hull of the `R[j]`; `first` and `second` are those hulls, coordinate by coordinate. A
/// denominator that is exactly 0 (the knots equal) drops that `Q[j]`, whose basis function is
/// identically 0, and every `R[j]` that uses it; one whose interval holds 0 otherwise makes that
/// hodograph point the whole line. Every value is computed in `Ivl` from the doubles of the
/// spline, so the operations are counted outward.
///
/// `rounding = 2 gamma(12 p + 4) max |coordinate of P[s-p ..= s]|` (rounded up in `Ivl`): the
/// kernel's count for de Boor's convex combinations (docs/definitions-and-certificates.md 4.4,
/// `eval::collapse`), which is what `Checked::point` computes, and the factor 2 covers both
/// coordinates of the distance.
pub(crate) fn span_bend(degree: usize, knots: &[f64], control: &[Point], span: usize) -> Bend {
    if !(1..=MAX_DEGREE).contains(&degree) || span >= control.len() {
        return Bend::unknown();
    }
    let Some(first_index) = span.checked_sub(degree) else {
        return Bend::unknown();
    };
    let Some(active) = control.get(first_index..=span) else {
        return Bend::unknown();
    };
    let p = bound::exact(degree as f64);
    // The first hodograph's active points, in order; None where the basis function is 0.
    let mut q: Vec<Option<[Ivl; 2]>> = Vec::new();
    for (k, pair) in active.windows(2).enumerate() {
        let j = first_index + k;
        let [a, b] = pair else {
            return Bend::unknown();
        };
        let ([a0, a1], [b0, b1]) = (*a, *b);
        let (Some(&high), Some(&low)) = (knots.get(j + degree + 1), knots.get(j + 1)) else {
            return Bend::unknown();
        };
        if high == low {
            q.push(None);
            continue;
        }
        let gap = bound::exact(high) - bound::exact(low);
        q.push(Some([
            p * (bound::exact(b0) - bound::exact(a0)) / gap,
            p * (bound::exact(b1) - bound::exact(a1)) / gap,
        ]));
    }
    let first = hull(q.iter().flatten().copied());
    let second = if degree == 1 {
        [Ivl::new(0.0, 0.0), Ivl::new(0.0, 0.0)]
    } else {
        let lower = bound::exact((degree - 1) as f64);
        let mut r: Vec<[Ivl; 2]> = Vec::new();
        for (k, pair) in q.windows(2).enumerate() {
            let j = first_index + k;
            let (Some(&high), Some(&low)) = (knots.get(j + degree + 1), knots.get(j + 2)) else {
                return Bend::unknown();
            };
            let [Some(a), Some(b)] = pair else {
                continue;
            };
            if high == low {
                continue;
            }
            let gap = bound::exact(high) - bound::exact(low);
            let ([a0, a1], [b0, b1]) = (*a, *b);
            r.push([lower * (b0 - a0) / gap, lower * (b1 - a1) / gap]);
        }
        hull(r.into_iter())
    };
    let largest = active
        .iter()
        .flatten()
        .try_fold(0.0_f64, |most, v| {
            if v.is_finite() {
                Some(most.max(v.abs()))
            } else {
                None
            }
        })
        .unwrap_or(f64::INFINITY);
    let count = degree.saturating_mul(12).saturating_add(4);
    let rounding = (bound::exact(2.0) * bound::exact(gamma(count)) * bound::exact(largest)).hi;
    Bend {
        first,
        second,
        second_size: if degree == 1 {
            0.0
        } else {
            bound::magnitude(second)
        },
        rounding: if rounding.is_nan() {
            f64::INFINITY
        } else {
            rounding
        },
    }
}

/// The hull, coordinate by coordinate, of some boxes; the whole plane if there are none.
fn hull(boxes: impl Iterator<Item = [Ivl; 2]>) -> [Ivl; 2] {
    let mut out: Option<[Ivl; 2]> = None;
    for [b0, b1] in boxes {
        out = Some(match out {
            Some([o0, o1]) => [o0.hull(b0), o1.hull(b1)],
            None => [b0, b1],
        });
    }
    out.unwrap_or([Ivl::whole(), Ivl::whole()])
}

/// The enclosures of a spline over the part `[lo, hi]` of its knot span `span`
/// (`knots[span] <= lo < hi <= knots[span + 1]`): those of the Bezier control points of that part,
/// which follow the curve far more closely over a short part than the whole span's hodographs
/// ([`span_bend`]) do.
///
/// With `p` the degree, the Bezier points of the part are the blossom of the span's polynomial at
/// `lo` taken `p - i` times and `hi` taken `i` times, `i` from 0 to `p` ([`blossom`]). With those
/// points `c[i]` and the width `w = hi - lo`, the first derivative on the part is a Bezier curve of
/// control points `p (c[i+1] - c[i]) / w`, so it lies in their hull, and the second in the hull of
/// `p - 1` times their differences over `w`; its size is at most the largest of those points' sizes
/// (a convex combination is no longer than its longest term).
///
/// Counted: every operation is an `Ivl` one from the doubles of the spline, so the rounding goes
/// outward. The second differences lose digits as `1 / w^2` over a very short part, so each
/// enclosure is also cut to the whole span's, which holds the part too: a part too short for its
/// own differences proves no less than its span does. The rounding of a point is the span's.
/// A part that is not inside its span, or has no width, gives `Bend::unknown()`.
pub(crate) fn stretch_bend(
    degree: usize,
    knots: &[f64],
    control: &[Point],
    span: usize,
    lo: f64,
    hi: f64,
) -> Bend {
    if !(1..=MAX_DEGREE).contains(&degree) {
        return Bend::unknown();
    }
    let whole = span_bend(degree, knots, control, span);
    let (Some(&from), Some(&to)) = (knots.get(span), knots.get(span + 1)) else {
        return Bend::unknown();
    };
    if !(from <= lo && lo < hi && hi <= to) {
        return Bend::unknown();
    }
    let Some(first_index) = span.checked_sub(degree) else {
        return whole;
    };
    let Some(active) = control.get(first_index..=span) else {
        return whole;
    };
    let start: Vec<[Ivl; 2]> = active
        .iter()
        .map(|&[x, y]| [bound::exact(x), bound::exact(y)])
        .collect();
    let mut bezier: Vec<[Ivl; 2]> = Vec::new();
    for i in 0..=degree {
        let Some(point) = blossom(degree, knots, first_index, &start, lo, hi, i) else {
            return whole;
        };
        bezier.push(point);
    }
    let width = bound::exact(hi) - bound::exact(lo);
    let differences = |points: &[[Ivl; 2]], factor: Ivl| -> Vec<[Ivl; 2]> {
        points
            .windows(2)
            .filter_map(|pair| match pair {
                [[a0, a1], [b0, b1]] => {
                    Some([factor * (*b0 - *a0) / width, factor * (*b1 - *a1) / width])
                }
                _ => None,
            })
            .collect()
    };
    let q = differences(&bezier, bound::exact(degree as f64));
    let first = cut(hull(q.iter().copied()), whole.first);
    if degree == 1 {
        return Bend { first, ..whole };
    }
    let r = differences(&q, bound::exact((degree - 1) as f64));
    let size = r
        .iter()
        .map(|&v| bound::magnitude(v))
        .fold(0.0_f64, f64::max);
    Bend {
        first,
        second: cut(hull(r.into_iter()), whole.second),
        second_size: if size < whole.second_size {
            size
        } else {
            whole.second_size
        },
        rounding: whole.rounding,
    }
}

/// The blossom of the polynomial of a spline's knot span at `lo` taken `degree - i` times and `hi`
/// taken `i` times: de Boor's steps from the span's active control points `start` (the first of
/// them is the control point `first_index`), each level with its own argument, in `Ivl`. The
/// blossom is the same whatever the order of its arguments. `None` where a knot or a point is
/// missing.
fn blossom(
    degree: usize,
    knots: &[f64],
    first_index: usize,
    start: &[[Ivl; 2]],
    lo: f64,
    hi: f64,
    i: usize,
) -> Option<[Ivl; 2]> {
    let mut d: Vec<[Ivl; 2]> = start.to_vec();
    for level in 1..=degree {
        let u = bound::exact(if level + i <= degree { lo } else { hi });
        for k in (level..=degree).rev() {
            let low = bound::exact(*knots.get(first_index + k)?);
            let high = bound::exact(*knots.get(first_index + k + degree + 1 - level)?);
            let share = (u - low) / (high - low);
            let [a0, a1] = *d.get(k - 1)?;
            let [b0, b1] = *d.get(k)?;
            *d.get_mut(k)? = [a0 + share * (b0 - a0), a1 + share * (b1 - a1)];
        }
    }
    d.get(degree).copied()
}

/// The part two enclosures of the same vector have in common, coordinate by coordinate; `other`
/// where they would have none (which two true enclosures never do).
fn cut(one: [Ivl; 2], other: [Ivl; 2]) -> [Ivl; 2] {
    let both = |a: Ivl, b: Ivl| {
        let (lo, hi) = (a.lo.max(b.lo), a.hi.min(b.hi));
        if lo <= hi {
            Ivl::new(lo, hi)
        } else {
            b
        }
    };
    let ([a0, a1], [b0, b1]) = (one, other);
    [both(a0, b0), both(a1, b1)]
}
