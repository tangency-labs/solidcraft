//! B-splines: a smooth curve from a few control points and a row of knots, as CAD kernels hold
//! them.
//!
//! A spline of degree `p` with `n` control points has `n + p + 1` knots, none smaller than the one
//! before it, and runs over the parameters from knot `p` to knot `n`. This module evaluates such a
//! curve; [`crate::teeth::Piece::Spline`] is a piece of an outline given this way.

use crate::teeth::outline::Point;
use crate::teeth::{Error, MAX_CURVE_POINTS};

/// The highest degree accepted.
pub const MAX_DEGREE: usize = 9;

const NOT_A_SPLINE: Error = Error::Invalid {
    why: "the knots and control points do not describe a spline",
};

/// The parameters a spline runs over, first to last, if `degree`, `knots` and `control` describe
/// one: a degree from 1 to [`MAX_DEGREE`], more control points than the degree, as many knots as
/// control points plus the degree plus one, all numbers finite, the knots in order, the range not
/// empty, and no knot inside the range repeated more often than the degree (the curve would break
/// there).
pub fn range(degree: usize, knots: &[f64], control: &[Point]) -> Result<(f64, f64), Error> {
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
pub fn point(degree: usize, knots: &[f64], control: &[Point], t: f64) -> Result<Point, Error> {
    let (from, to) = range(degree, knots, control)?;
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
) -> Result<Point, Error> {
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
    ) -> Result<Checked<'a>, Error> {
        let (from, to) = range(degree, knots, control)?;
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
    pub(super) fn point(&self, t: f64) -> Result<Point, Error> {
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
pub fn breaks(degree: usize, knots: &[f64], control: &[Point]) -> Result<Vec<f64>, Error> {
    let (from, to) = range(degree, knots, control)?;
    let mut out = vec![from];
    for &k in knots {
        if k > from && k < to && out.last().is_some_and(|&last| k > last) {
            out.push(k);
        }
    }
    out.push(to);
    Ok(out)
}
