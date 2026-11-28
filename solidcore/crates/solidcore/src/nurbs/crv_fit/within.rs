//! A curve refitted to a tolerance: the clamped spline of a given degree, through the source's
//! two end points and leaving each given end along its direction, with the fewest spans a
//! halving search finds whose certified distance from the source is within the tolerance.
//!
//! Upstream: none (not upstream; solidcore-native).
//!
//! | Rust               | Upstream |
//! |--------------------|----------|
//! | [`WithinFit`]      | none     |
//! | [`refit_within`]   | none     |
//!
//! The search starts with one span (two even spans for degree 2 with both directions given:
//! one span of degree 2 has a single inner control point, which cannot lie on both end
//! lines). Each round samples the source, fits the samples by least squares at their
//! parameters ([`approx_end_directions`]), and certifies the fit span by span
//! ([`certified_distance_by_span`]). While a span of the fit is past the tolerance, the span
//! with the largest bound is halved (a new simple knot at its middle); while the start's
//! reach is not positive the first span is halved instead, and the last while the end's is
//! not (the fit turns back at that end: Schneider's fit of one Bézier piece splits there
//! too). One knot a round keeps the spans few: a span next to the worst one often comes
//! within the tolerance once the worst is halved. Which span is halved does not depend on
//! the tolerance, so a looser tolerance stops on a fit that a tighter one passes through:
//! its knots are among the tighter one's. The search stops when every span is within the
//! tolerance and every reach is positive, or when the next knots would need more control
//! points than allowed, or a middle has no floating-point room inside its span. It makes one
//! fit and one certificate for every span it adds. A tolerance below what the certificate's
//! rounding terms come to for the source ([`certified_distance_floor`]) is one no span's
//! bound can come within: the search then stops after its first fit.
//!
//! A held end's leg is formed along its direction, `P_0 + s T_0`, and stored in floating
//! point: each coordinate of the second control point rounds once. On a leg many such
//! roundings long the stored leg is the direction to within nothing that matters; on a leg a
//! few roundings long (a short leg far from the origin) it is not, and no fit can make it so.
//! The result therefore says how far each stored leg is from its direction
//! ([`WithinFit::start_turn`], [`WithinFit::end_turn`]), and [`WithinFit::ends_held`] is the
//! question a caller asks before it relies on the directions.
//!
//! The samples: the source's two end points, and in every span of the fit `2 (p + 1)`
//! parameters at even steps strictly inside it and its right break. Every span then holds at
//! least `p + 1` distinct parameters, so the samples fix the fit: a spline of degree `p` that
//! vanishes at `p + 1` distinct parameters of a span is zero on that span, and one that is
//! zero on every span is zero. The normal equations are therefore regular (up to their
//! rounding, which the fit checks against its pivot tolerance).
//!
//! Numerics: the parameters and the middles are formed in plain floating point
//! (`a (1 - s) + b s`, `a / 2 + b / 2`: neither overflows on a finite domain), each checked to
//! lie strictly inside its span; the fit is a least-squares solve in plain floating point
//! (not a certificate); the bound is the certificate's, every rounding included.
//!
//! Deviations: none (no upstream counterpart).

use crate::math::det;
use std::f64::consts::PI;

use crate::error::query::QueryFailure;
use crate::error::{Result, SolidError};
use crate::limits;
use crate::math::{Vec3, EFF_ZERO_RAD};
use crate::nurbs::crv_basic::Curve;
use crate::nurbs::crv_deviation::{
    certified_distance_by_span, certified_distance_floor, SpanDistance,
};
use crate::nurbs::crv_fit::end_directions::approx_end_directions;
use crate::nurbs::knots::{KnotVector, Side, MAX_DEGREE};
use crate::nurbs::math::CPoint;

const OP: &str = "nurbs::crv_fit::within::refit_within";

/// A curve refitted to a tolerance ([`refit_within`]).
#[derive(Clone, Debug, PartialEq)]
pub struct WithinFit {
    /// The last fit made.
    pub curve: Curve,
    /// The certified bound of how far `curve` is from the source at every parameter.
    pub bound: f64,
    /// How many fits were made.
    pub fits: usize,
    /// The start's reach, where a start direction was given.
    pub start_reach: Option<f64>,
    /// The end's reach, where an end direction was given.
    pub end_reach: Option<f64>,
    /// The angle, in radians, between the curve's first leg as it is stored (from its first
    /// control point to its second) and the start's direction, where one was given. The leg
    /// is formed along the direction and each coordinate of the second control point rounds
    /// once, so the angle is that rounding's: next to nothing on a leg many roundings long,
    /// and as much as the direction is lost on a leg a few roundings long (a short leg far
    /// from the origin). π for a leg of no length.
    pub start_turn: Option<f64>,
    /// The same for the curve's last leg (from its last control point but one to its last)
    /// and the end's direction.
    pub end_turn: Option<f64>,
}

impl WithinFit {
    /// Whether every end that was given a direction leaves along it (or is reached along
    /// it): its reach positive, and its stored leg within [`EFF_ZERO_RAD`] of the direction.
    /// Two curves held to one direction at a point they share then meet there within twice
    /// that angle.
    #[must_use]
    pub fn ends_held(&self) -> bool {
        let held = |reach: Option<f64>, turn: Option<f64>| match (reach, turn) {
            (None, None) => true,
            (Some(r), Some(t)) => positive(r) && t < EFF_ZERO_RAD,
            _ => false,
        };
        held(self.start_reach, self.start_turn) && held(self.end_reach, self.end_turn)
    }
}

/// `source` refitted as a clamped spline of degree `p` on the same domain, through the
/// source's two end points, leaving the start along `start` and reaching the end along `end`
/// where they are given, its interior knots simple, with the fewest spans the halving search
/// of the module docs finds within `tolerance` of the source at every parameter.
///
/// `source` is a clamped, non-rational 3D B-spline curve on any domain. The result is always
/// the last fit made, with its certified bound (the largest of its spans') and the count of
/// fits made. The caller compares `bound` with its tolerance and asks
/// [`WithinFit::ends_held`]. The bound is within and every reach positive unless the search
/// stopped because the next knots would need more than `most` control points (or more samples
/// than the kernel allocates), a middle had no floating-point room inside its span, or the
/// tolerance is below [`certified_distance_floor`] of the source (one fit is made). An end is
/// not held, with its reach positive, where its leg is too short against the rounding of its
/// end point's coordinates to lie along its direction.
///
/// # Errors
///
/// `InvalidInput` for a tolerance not finite or not positive, a degree of 0 or past
/// [`MAX_DEGREE`], `most` below `p + 1` (or below the four control points the first fit of
/// degree 2 with both directions needs), a direction given on a degree below 2, a direction
/// not finite or of no length, a direction against the source's own tangent at that end (the
/// curve cannot leave or reach the end along it), or a source whose domain is not finite or
/// has no length (or too short to hold the first samples); those of the evaluation, the fit
/// ([`approx_end_directions`]) and the certificate ([`certified_distance_by_span`]: a
/// rational, 2D or unclamped source).
pub fn refit_within(
    source: &Curve,
    p: usize,
    tolerance: f64,
    start: Option<Vec3>,
    end: Option<Vec3>,
    most: usize,
) -> Result<WithinFit> {
    if !positive(tolerance) {
        return Err(SolidError::invalid_input(
            OP,
            "a tolerance not finite and positive",
        ));
    }
    if p == 0 || p > MAX_DEGREE {
        return Err(SolidError::invalid_input(
            OP,
            "a degree of 0 or past the most",
        ));
    }
    if (start.is_some() || end.is_some()) && p < 2 {
        return Err(SolidError::invalid_input(
            OP,
            "a direction given on a degree below 2",
        ));
    }
    if [start, end]
        .into_iter()
        .flatten()
        .any(|d| d.length_and_unit().is_none())
    {
        return Err(SolidError::invalid_input(
            OP,
            "a direction not finite or of no length",
        ));
    }
    let (Some(&lo), Some(&hi)) = (source.knots().first(), source.knots().last()) else {
        return Err(SolidError::invalid_input(OP, "a source without knots"));
    };
    if !(lo.is_finite() && hi.is_finite() && lo < hi) {
        return Err(SolidError::invalid_input(
            OP,
            "a source whose domain is not finite or has no length",
        ));
    }
    // The unit directions (each has one: checked above).
    let unit = |d: Option<Vec3>| d.and_then(Vec3::length_and_unit).map(|(_, t)| t);
    let (start_unit, end_unit) = (unit(start), unit(end));
    // A direction against the source's own tangent at that end cannot be held: the reach
    // would stay negative however short the end span became, and the search would only stop
    // at its limit. Unit against unit: a product of the raw lengths can overflow to
    // infinities of both signs, whose sum is not a number and compares as not against.
    let against = |d: Option<Vec3>, u: f64, side: Side| -> Result<bool> {
        let Some(d) = d else {
            return Ok(false);
        };
        let derivs = source.derivs(u, side, 1)?;
        let Some(tangent) = derivs.get(1) else {
            return Err(SolidError::query(
                OP,
                QueryFailure::Defect {
                    why: "the first derivative was asked for, so the evaluation returns it",
                },
            ));
        };
        Ok(tangent
            .length_and_unit()
            .is_some_and(|(_, t)| t.dot(d) < 0.0))
    };
    if against(start_unit, lo, Side::Right)? {
        return Err(SolidError::invalid_input(
            OP,
            "a start direction against the source's own",
        ));
    }
    if against(end_unit, hi, Side::Left)? {
        return Err(SolidError::invalid_input(
            OP,
            "an end direction against the source's own",
        ));
    }
    // What no span's bound against the source can be below: a tolerance under it is one no
    // refinement reaches, so one fit is made and given back with its bound.
    let floor = certified_distance_floor(source)?;
    // one span of degree 2 has a single inner control point, which cannot lie on both lines
    let both_on_quadratic = p == 2 && start.is_some() && end.is_some();
    let first_spans: usize = if both_on_quadratic { 2 } else { 1 };
    if most < p.saturating_add(first_spans) {
        return Err(SolidError::invalid_input(
            OP,
            "fewer control points allowed than the first fit needs",
        ));
    }
    let too_short = || SolidError::invalid_input(OP, "a source domain too short to sample");
    let mut breaks = if both_on_quadratic {
        let mid = middle(lo, hi).ok_or_else(too_short)?;
        vec![lo, mid, hi]
    } else {
        vec![lo, hi]
    };
    let mut params = samples(&breaks, p).ok_or_else(too_short)?;
    let mut fits = 0_usize;
    // Every round that goes on adds one break and keeps the control points within `most`, so
    // there are at most `most` rounds.
    loop {
        let knots = knots_of(&breaks, p).ok_or_else(|| {
            SolidError::query(
                OP,
                QueryFailure::Defect {
                    why: "the breaks of the search always hold the domain's two ends",
                },
            )
        })?;
        // control points: the spans plus the degree, `breaks.len() - 1 + p`; `n` the last index
        let n = breaks.len().saturating_sub(2).saturating_add(p);
        let data = params
            .iter()
            .map(|&t| source.eval(t, Side::Left))
            .collect::<Result<Vec<Vec3>>>()?;
        let fit = approx_end_directions(
            &data,
            &params,
            &KnotVector::from_slice(&knots)?,
            n,
            p,
            start,
            end,
        )?;
        fits = fits.saturating_add(1);
        let worst = per_fit_span(&breaks, &certified_distance_by_span(source, &fit.curve)?)?;
        let bound = worst.iter().copied().fold(0.0_f64, f64::max);
        let start_short = fit.start_reach.is_some_and(|r| !positive(r));
        let end_short = fit.end_reach.is_some_and(|r| !positive(r));
        // The end legs as the fit stores them, against their directions.
        let cpts = fit.curve.cpts();
        let before_last = cpts.len().checked_sub(2).and_then(|i| cpts.get(i));
        let start_turn = start_unit.map(|t| leg_turn(cpts.first(), cpts.get(1), t));
        let end_turn = end_unit.map(|t| leg_turn(before_last, cpts.last(), t));
        let done = WithinFit {
            curve: fit.curve,
            bound,
            fits,
            start_reach: fit.start_reach,
            end_reach: fit.end_reach,
            start_turn,
            end_turn,
        };
        // Within the tolerance with every reach positive: the search is over. A leg that the
        // rounding of its end point's coordinates has taken off its direction stays as it is
        // (`ends_held` tells): more knots would only shorten it.
        if !start_short && !end_short && worst.iter().all(|&w| w <= tolerance) {
            return Ok(done);
        }
        // Under the floor of the source's own rounding no span's bound comes within the
        // tolerance, whatever is halved.
        if floor > tolerance {
            return Ok(done);
        }
        // The span to halve: the first when the start's reach is not positive, else the last
        // when the end's is not, else the span with the largest bound (the first of equals).
        // The choice does not look at the tolerance, so the knots of a looser tolerance are
        // the first knots a tighter one takes.
        let last = worst.len().saturating_sub(1);
        let mut at = 0_usize;
        if end_short && !start_short {
            at = last;
        } else if !start_short {
            let mut largest = f64::NEG_INFINITY;
            for (j, &d) in worst.iter().enumerate() {
                if d > largest {
                    largest = d;
                    at = j;
                }
            }
        }
        let mut next = Vec::new();
        for (j, w) in breaks.windows(2).enumerate() {
            let &[a, b] = w else {
                return Err(SolidError::query(
                    OP,
                    QueryFailure::Defect {
                        why: "windows of two of the breaks hold two breaks",
                    },
                ));
            };
            next.push(a);
            if j == at {
                let Some(mid) = middle(a, b) else {
                    return Ok(done);
                };
                next.push(mid);
            }
        }
        next.push(hi);
        // control points of the next fit: its spans plus the degree; its samples: one start
        // point and `2 (p + 1) + 1` per span
        let spans = next.len().saturating_sub(1);
        let count = spans.saturating_add(p);
        let per_span = p.saturating_mul(2).saturating_add(3);
        let sampled = spans.saturating_mul(per_span).saturating_add(1);
        if count > most || !limits::fits::<Vec3>(sampled) {
            return Ok(done);
        }
        let Some(u) = samples(&next, p) else {
            return Ok(done);
        };
        breaks = next;
        params = u;
    }
}

/// Whether `x` is finite and positive.
fn positive(x: f64) -> bool {
    x.is_finite() && x > 0.0
}

/// The angle, in radians, between the leg from `from` to `to` (two control points as they are
/// stored) and the unit direction `t`: the arc tangent of the cross product's length over the
/// dot product, exact near 0 where the arc cosine of a dot product loses half its digits. π for
/// a leg of no length or with a coordinate that is not finite, and for a missing point: such a
/// leg is along no direction.
fn leg_turn(from: Option<&CPoint>, to: Option<&CPoint>, t: Vec3) -> f64 {
    let (Some(a), Some(b)) = (from, to) else {
        return PI;
    };
    match Vec3::new(b.x - a.x, b.y - a.y, b.z - a.z).length_and_unit() {
        Some((_, leg)) => det::atan2(leg.cross(t).length(), leg.dot(t)),
        None => PI,
    }
}

/// The middle of `[a, b]`, `None` when it is not strictly inside (no floating-point room).
fn middle(a: f64, b: f64) -> Option<f64> {
    let m = 0.5 * a + 0.5 * b;
    (a < m && m < b).then_some(m)
}

/// The clamped knots of degree `p` on `breaks`: the first and last `p + 1` times each, every
/// interior break once. `None` for fewer than two breaks.
fn knots_of(breaks: &[f64], p: usize) -> Option<Vec<f64>> {
    let (&lo, &hi) = (breaks.first()?, breaks.last()?);
    let inner = breaks.get(1..breaks.len().checked_sub(1)?)?;
    let ends = p.checked_add(1)?;
    let mut knots = Vec::new();
    knots.extend(std::iter::repeat_n(lo, ends));
    knots.extend_from_slice(inner);
    knots.extend(std::iter::repeat_n(hi, ends));
    Some(knots)
}

/// The sample parameters on `breaks` (module docs): the first break, then in every span
/// `2 (p + 1)` parameters at even steps strictly inside it and its right break. `None` when a
/// parameter is not strictly past the one before it and strictly before its span's end (a span
/// too short for its samples in floating point), or for no span.
fn samples(breaks: &[f64], p: usize) -> Option<Vec<f64>> {
    let inside = p.checked_add(1)?.checked_mul(2)?;
    let steps = inside.checked_add(1)? as f64;
    let &first = breaks.first()?;
    if breaks.len() < 2 {
        return None;
    }
    let mut u = vec![first];
    let mut last = first;
    for w in breaks.windows(2) {
        let &[a, b] = w else {
            return None;
        };
        for i in 1..=inside {
            let s = i as f64 / steps;
            let t = a * (1.0 - s) + b * s;
            if !(t > last && t < b) {
                return None;
            }
            u.push(t);
            last = t;
        }
        // `b` is past the last parameter, which is below it
        u.push(b);
        last = b;
    }
    Some(u)
}

/// The largest bound of the common spans `by_span` inside each span of the fit on `breaks`,
/// in order. The common spans are cut at every break of the fit, so each lies in one span of
/// the fit, in the same order.
fn per_fit_span(breaks: &[f64], by_span: &[SpanDistance]) -> Result<Vec<f64>> {
    let defect = || {
        SolidError::query(
            OP,
            QueryFailure::Defect {
                why: "the certificate cuts at every break of the fit, so its spans nest in them",
            },
        )
    };
    let mut rest = by_span.iter().peekable();
    let mut worst = Vec::new();
    for w in breaks.windows(2) {
        let &[a, b] = w else {
            return Err(defect());
        };
        let mut largest: Option<f64> = None;
        while let Some(s) = rest.next_if(|s| s.from >= a && s.to <= b) {
            largest = Some(largest.map_or(s.bound, |m| m.max(s.bound)));
        }
        worst.push(largest.ok_or_else(defect)?);
    }
    if rest.next().is_some() {
        return Err(defect());
    }
    Ok(worst)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nurbs::crv_fit::p1::ParamMethod;
    use crate::nurbs::crv_fit::p2::interp;

    fn v(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3::new(x, y, z)
    }

    /// The clamped curve of degree `p` on `knots` with control points `points`.
    fn curve(points: &[Vec3], p: usize, knots: Vec<f64>) -> Curve {
        let pw = points.iter().map(|&q| CPoint::from_point(q)).collect();
        Curve::new(pw, p, knots).unwrap()
    }

    /// The cubic through `count + 1` points of `f` at even steps of `t` over `[t0, t1]`.
    fn through(f: impl Fn(f64) -> Vec3, t0: f64, t1: f64, count: u32) -> Curve {
        let points: Vec<Vec3> = (0..=count)
            .map(|i| f(t0 + (t1 - t0) * f64::from(i) / f64::from(count)))
            .collect();
        interp(&points, 3, ParamMethod::ChordLength).unwrap()
    }

    fn involute(t: f64) -> Vec3 {
        let r = 10.0;
        v(
            r * (det::cos(t) + t * det::sin(t)),
            r * (det::sin(t) - t * det::cos(t)),
            0.0,
        )
    }

    fn arc(t: f64) -> Vec3 {
        v(10.0 * det::cos(t), 10.0 * det::sin(t), 0.0)
    }

    fn parabola(t: f64) -> Vec3 {
        v(t, 0.25 * t * t, 0.0)
    }

    fn sources() -> Vec<Curve> {
        vec![
            through(involute, 0.0, 1.5, 30),
            through(arc, 0.0, 2.0, 30),
            through(parabola, -3.0, 3.0, 30),
        ]
    }

    /// The direction of the start's first leg and the end's last leg of `c`.
    fn end_legs(c: &Curve) -> (Vec3, Vec3) {
        let pw = c.cpts();
        let n = pw.len();
        let at = |i: usize| v(pw[i].x, pw[i].y, pw[i].z);
        (at(1) - at(0), at(n - 1) - at(n - 2))
    }

    /// The largest absolute coordinate of `c`'s control points.
    fn size(c: &Curve) -> f64 {
        c.cpts().iter().fold(0.0_f64, |m, q| {
            m.max(q.x.abs()).max(q.y.abs()).max(q.z.abs())
        })
    }

    /// Checks the certified bound against the gap between `source` and `fit` sampled inside
    /// every span of the fit, and that `fit` is clamped on the source's domain with simple
    /// interior knots of degree `p`.
    fn holds(source: &Curve, fit: &WithinFit, p: usize) {
        let knots = fit.curve.knots();
        assert_eq!(fit.curve.degree(), p);
        assert_eq!(knots.len(), fit.curve.cpts().len() + p + 1);
        let (lo, hi) = (source.knots()[0], *source.knots().last().unwrap());
        let tail = knots.len() - p - 1;
        assert!(knots[..=p].iter().all(|&k| k == lo), "{knots:?}");
        assert!(knots[tail..].iter().all(|&k| k == hi), "{knots:?}");
        let inner = &knots[p + 1..tail];
        assert!(inner.windows(2).all(|w| w[0] < w[1]), "{knots:?}");
        let mut breaks = knots.to_vec();
        breaks.dedup();
        for w in breaks.windows(2) {
            for k in 1..8 {
                let t = w[0] + (w[1] - w[0]) * f64::from(k) / 8.0;
                let a = source.eval(t, Side::Left).unwrap();
                let b = fit.curve.eval(t, Side::Left).unwrap();
                let gap = a.distance(b);
                assert!(gap <= fit.bound, "at {t}: {gap} past {}", fit.bound);
            }
        }
        // through the source's two end points
        let (a0, b0) = (
            source.eval(lo, Side::Left).unwrap(),
            fit.curve.eval(lo, Side::Left).unwrap(),
        );
        let (a1, b1) = (
            source.eval(hi, Side::Left).unwrap(),
            fit.curve.eval(hi, Side::Left).unwrap(),
        );
        assert!(a0.distance(b0) <= fit.bound && a1.distance(b1) <= fit.bound);
    }

    /// A source of one cubic span (on [2, 5], not [0, 1]) is in the fit's own space: the first
    /// fit gives it back, within the rounding of the solve and the certificate. The allowance
    /// is a million roundings of the curve's size: the normal equations of the eight samples
    /// on one Bézier span are well conditioned (the cubic Bernstein basis), so the solve's
    /// error is a few hundred roundings at most, and the certificate counts some tens of
    /// `γ_1` of the size.
    #[test]
    fn a_source_of_one_cubic_span_is_given_back_in_one_fit() {
        let bezier = [
            v(0.0, 0.0, 0.0),
            v(1.0, 3.0, 0.0),
            v(4.0, 3.0, 1.0),
            v(5.0, 0.0, 0.0),
        ];
        let knots = vec![2.0, 2.0, 2.0, 2.0, 5.0, 5.0, 5.0, 5.0];
        let source = curve(&bezier, 3, knots.clone());
        let rounding = 1e6 * f64::EPSILON * size(&source);
        let free = refit_within(&source, 3, rounding, None, None, 64).unwrap();
        assert_eq!(free.fits, 1);
        assert!(free.bound <= rounding, "{}", free.bound);
        assert_eq!((free.start_reach, free.end_reach), (None, None));
        assert_eq!(free.curve.knots(), &knots[..]);
        holds(&source, &free, 3);
        // its own end directions: the legs P1 - P0 and P3 - P2, their reaches their lengths
        let (s, e) = (bezier[1] - bezier[0], bezier[3] - bezier[2]);
        let held = refit_within(&source, 3, rounding, Some(s), Some(e), 64).unwrap();
        assert_eq!(held.fits, 1);
        assert!(held.bound <= rounding, "{}", held.bound);
        let (rs, re) = (held.start_reach.unwrap(), held.end_reach.unwrap());
        assert!((rs - s.length()).abs() <= rounding, "{rs}");
        assert!((re - e.length()).abs() <= rounding, "{re}");
        holds(&source, &held, 3);
        // either direction alone
        let start = refit_within(&source, 3, rounding, Some(s), None, 64).unwrap();
        assert_eq!((start.fits, start.end_reach), (1, None));
        assert!(start.bound <= rounding, "{}", start.bound);
        let end = refit_within(&source, 3, rounding, None, Some(e), 64).unwrap();
        assert_eq!((end.fits, end.start_reach), (1, None));
        assert!(end.bound <= rounding, "{}", end.bound);
    }

    /// Sources through points of an involute, a circle arc and a parabola, refitted for a
    /// ladder of tolerances, free and with their own end directions: each fit is within its
    /// tolerance, its bound holds the sampled gap, and a looser tolerance takes no more control
    /// points than a tighter one. The last is a property of the search: which span is halved
    /// does not depend on the tolerance, so the looser tolerance stops on a fit the tighter
    /// one passes through, and its knots are among the tighter one's.
    #[test]
    fn closed_form_sources_are_refitted_within_a_ladder_of_tolerances() {
        for source in sources() {
            let (s, e) = end_legs(&source);
            for (start, end) in [(None, None), (Some(s), Some(e))] {
                let mut counts = Vec::new();
                let mut knots: Vec<Vec<f64>> = Vec::new();
                for tolerance in [1e-2, 1e-4, 1e-6] {
                    let fit = refit_within(&source, 3, tolerance, start, end, 4096).unwrap();
                    assert!(fit.bound <= tolerance, "{} past {tolerance}", fit.bound);
                    assert!(fit.fits >= 1);
                    assert!(fit.start_reach.is_none_or(|r| r > 0.0), "{fit:?}");
                    assert!(fit.end_reach.is_none_or(|r| r > 0.0), "{fit:?}");
                    assert_eq!(fit.start_reach.is_some(), start.is_some());
                    assert_eq!(fit.end_reach.is_some(), end.is_some());
                    // each stored leg is along its direction, and the fit says so
                    assert_eq!(fit.start_turn.is_some(), start.is_some());
                    assert_eq!(fit.end_turn.is_some(), end.is_some());
                    assert!(fit.ends_held(), "{fit:?}");
                    holds(&source, &fit, 3);
                    counts.push(fit.curve.cpts().len());
                    knots.push(fit.curve.knots().to_vec());
                }
                assert!(counts.windows(2).all(|w| w[0] <= w[1]), "{counts:?}");
                // every knot of the looser fit is a knot of the tighter one, to the bit
                for w in knots.windows(2) {
                    assert!(w[0].iter().all(|k| w[1].contains(k)), "{w:?}");
                }
            }
        }
    }

    /// The sine of the angle between `a` and `b`.
    fn sine(a: Vec3, b: Vec3) -> f64 {
        a.cross(b).length() / (a.length() * b.length())
    }

    /// One arc cut in two at 45 degrees, each half interpolated through its own points (the
    /// join point the same bits in both). Refitted with the arc's tangent at the join given to
    /// both, the fits' legs at the join are parallel within rounding: each is its reach along
    /// the one unit direction both fits form from the same direction, plus two roundings per
    /// coordinate (forming `P_n - e T`, subtracting it back) at the size of the join point and
    /// the reach, `4 ε (size + reach) / reach` of sine per leg, and the cross product and
    /// the norms add a few `ε` more (16 allowed). Refitted free, the same measure is larger.
    #[test]
    fn two_halves_of_an_arc_refitted_with_the_shared_tangent_meet_smoothly() {
        let join = std::f64::consts::FRAC_PI_4;
        let first = through(arc, 0.0, join, 8);
        let second = through(arc, join, 2.0 * join, 8);
        let tangent = v(-det::sin(join), det::cos(join), 0.0);
        let tolerance = 1e-4;
        let a = refit_within(&first, 3, tolerance, None, Some(tangent), 4096).unwrap();
        let b = refit_within(&second, 3, tolerance, Some(tangent), None, 4096).unwrap();
        assert!(a.bound <= tolerance && b.bound <= tolerance, "{a:?} {b:?}");
        let (_, la) = end_legs(&a.curve);
        let (lb, _) = end_legs(&b.curve);
        let (ea, sb) = (a.end_reach.unwrap(), b.start_reach.unwrap());
        assert!(ea > 0.0 && sb > 0.0, "{ea} {sb}");
        let at_join = size(&first).max(size(&second));
        let allowed =
            4.0 * f64::EPSILON * ((at_join + ea) / ea + (at_join + sb) / sb) + 16.0 * f64::EPSILON;
        let held = sine(la, lb);
        assert!(held <= allowed, "{held} past {allowed}");
        // each fit says its end is held, so the two meet within twice the angle that means
        assert!(a.ends_held() && b.ends_held(), "{a:?} {b:?}");
        assert!(held <= 2.0 * EFF_ZERO_RAD);

        let a = refit_within(&first, 3, tolerance, None, None, 4096).unwrap();
        let b = refit_within(&second, 3, tolerance, None, None, 4096).unwrap();
        let (_, la) = end_legs(&a.curve);
        let (lb, _) = end_legs(&b.curve);
        let free = sine(la, lb);
        assert!(free > held, "{free} not above {held}");
    }

    /// Too few control points allowed for the tolerance: the search stops at the last fit
    /// that `most` holds, past the tolerance. Four control points of a cubic make one span and
    /// a second span needs five, so `most = 4` makes one fit; the involute is no cubic, so one
    /// span (or a handful) is not within 1e-9 of it.
    #[test]
    fn too_few_control_points_give_the_last_fit_past_the_tolerance() {
        let source = through(involute, 0.0, 1.5, 30);
        let tolerance = 1e-9;
        let one = refit_within(&source, 3, tolerance, None, None, 4).unwrap();
        assert_eq!(one.fits, 1);
        assert_eq!(one.curve.cpts().len(), 4);
        assert!(one.bound > tolerance, "{}", one.bound);
        let few = refit_within(&source, 3, tolerance, None, None, 9).unwrap();
        assert!(few.curve.cpts().len() <= 9);
        assert!(few.bound > tolerance, "{}", few.bound);
        holds(&source, &few, 3);
    }

    /// Degree 2 with both directions starts on two spans: it is refitted, its reaches positive.
    #[test]
    fn a_quadratic_with_both_directions_starts_on_two_spans() {
        let source = through(arc, 0.0, 2.0, 30);
        let (s, e) = end_legs(&source);
        let fit = refit_within(&source, 2, 1e-3, Some(s), Some(e), 4096).unwrap();
        assert!(fit.bound <= 1e-3, "{}", fit.bound);
        assert!(fit.curve.cpts().len() >= 4);
        assert!(fit.start_reach.unwrap() > 0.0 && fit.end_reach.unwrap() > 0.0);
        holds(&source, &fit, 2);
        // the first fit needs four control points
        assert!(matches!(
            refit_within(&source, 2, 1e-3, Some(s), Some(e), 3),
            Err(SolidError::InvalidInput { .. })
        ));
    }

    /// A direction of any finite length is read as its unit. The source leaves its start along
    /// (1, -2, 0) and reaches its end along (2, 1, 0). The largest direction there is, against
    /// either, is refused before the search like any direction against it: with the raw
    /// vectors the products overflow to infinities of both signs, and their sum compares as
    /// not against. Along the tangents, directions of that size give the fit of their units,
    /// to the bit.
    #[test]
    fn a_direction_of_any_finite_length_is_read_as_its_unit() {
        let source = curve(
            &[
                v(0.0, 0.0, 0.0),
                v(1.0, -2.0, 0.0),
                v(3.0, -3.0, 0.0),
                v(5.0, -2.0, 0.0),
            ],
            3,
            vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        );
        let invalid = |r: Result<WithinFit>| matches!(r, Err(SolidError::InvalidInput { .. }));
        let big = f64::MAX;
        let against_start = Some(v(big, big, 0.0));
        let against_end = Some(v(-big, big, 0.0));
        assert!(invalid(refit_within(
            &source,
            3,
            1e-3,
            against_start,
            None,
            64
        )));
        assert!(invalid(refit_within(
            &source,
            3,
            1e-3,
            None,
            against_end,
            64
        )));
        let (s, e) = (v(1.0, -2.0, 0.0), v(2.0, 1.0, 0.0));
        let small = refit_within(&source, 3, 1e-3, Some(s), Some(e), 64).unwrap();
        let (s, e) = (v(0.5 * big, -big, 0.0), v(big, 0.5 * big, 0.0));
        let large = refit_within(&source, 3, 1e-3, Some(s), Some(e), 64).unwrap();
        assert_eq!(small, large);
        assert!(small.bound <= 1e-3 && small.ends_held(), "{small:?}");
    }

    /// A tolerance below what the certificate's rounding comes to for the source is one no
    /// span's bound can come within. The search stops after its first fit, where it would
    /// halve a span a round up to its limit, and gives that fit back with its bound, which is
    /// not below the floor.
    #[test]
    fn a_tolerance_below_the_floor_of_the_certificate_stops_after_one_fit() {
        for source in sources() {
            let floor = certified_distance_floor(&source).unwrap();
            assert!(floor > 0.0);
            for tolerance in [f64::MIN_POSITIVE, 1e-300, 0.5 * floor] {
                let fit = refit_within(&source, 3, tolerance, None, None, 4096).unwrap();
                assert_eq!(fit.fits, 1, "{tolerance}");
                assert!(fit.bound >= floor, "{} below {floor}", fit.bound);
                assert!(fit.bound > tolerance, "{} within {tolerance}", fit.bound);
                holds(&source, &fit, 3);
            }
        }
    }

    /// A leg a few roundings long is not called along its direction. The source is a cubic of
    /// one span whose control points lie on the grid of the floats near 2^29 (steps of
    /// `g = 2^-23`), forty steps across; its first leg is the step (14, 3). The direction
    /// asked for at the start has the slope `sqrt(2) - 1.2`, which no step of the grid of up
    /// to 64 either way has: the search over every such step below finds the nearest (the
    /// multiples of (14, 3)) 7e-5 radians off. The fit follows the source within the
    /// tolerance with a positive reach; its stored leg is a step of the grid, so it cannot
    /// lie along the direction. The turn says how far off it is, and `ends_held` says no.
    /// The same steps at the origin, a unit each, hold the direction.
    #[test]
    fn a_leg_a_few_roundings_long_is_not_called_along_its_direction() {
        use std::f64::consts::TAU;
        let g = det::powi(2.0_f64, -23);
        let base = det::powi(2.0_f64, 29);
        let steps = [(0.0, 0.0), (14.0, 3.0), (28.0, 12.0), (40.0, 40.0)];
        let knots = vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
        let d = v(1.0, 2.0_f64.sqrt() - 1.2, 0.0);
        let along = det::atan2(d.y, d.x);
        // the angle between a step of the grid and the direction
        let off = |i: f64, j: f64| {
            let a = (det::atan2(j, i) - along).abs();
            a.min(TAU - a)
        };
        let mut nearest = f64::INFINITY;
        for i in -64_i32..=64 {
            for j in -64_i32..=64 {
                if (i, j) != (0, 0) {
                    nearest = nearest.min(off(f64::from(i), f64::from(j)));
                }
            }
        }
        assert!(nearest > 10.0 * EFF_ZERO_RAD, "{nearest}");

        let far: Vec<Vec3> = steps
            .iter()
            .map(|&(x, y)| v(base + x * g, base + y * g, 0.0))
            .collect();
        let source = curve(&far, 3, knots.clone());
        let tolerance = 8.0 * g;
        let fit = refit_within(&source, 3, tolerance, Some(d), None, 64).unwrap();
        assert!(fit.bound <= tolerance, "{}", fit.bound);
        assert!(fit.start_reach.unwrap() > 0.0, "{:?}", fit.start_reach);
        // the stored leg in steps of the grid: whole numbers, among those searched above
        let (leg, _) = end_legs(&fit.curve);
        let (i, j) = (leg.x / g, leg.y / g);
        assert!(i == i.round() && j == j.round() && leg.z == 0.0, "{leg:?}");
        assert!(i.abs() <= 64.0 && j.abs() <= 64.0, "{i} {j}");
        assert!(i != 0.0 || j != 0.0, "{i} {j}");
        // the turn is the leg's angle from the direction
        let turn = fit.start_turn.unwrap();
        assert!(
            (turn - off(i, j)).abs() < 1e-12,
            "{turn} against {}",
            off(i, j)
        );
        assert!(turn >= nearest, "{turn} against {nearest}");
        assert!(!fit.ends_held());
        assert_eq!(fit.end_turn, None);

        let near: Vec<Vec3> = steps.iter().map(|&(x, y)| v(x, y, 0.0)).collect();
        let source = curve(&near, 3, knots);
        let fit = refit_within(&source, 3, 1e-2, Some(d), None, 64).unwrap();
        assert!(fit.bound <= 1e-2 && fit.ends_held(), "{fit:?}");
        assert!(fit.start_turn.unwrap() < 1e-12, "{:?}", fit.start_turn);
    }

    /// The samples: the first break, then `2 (p + 1)` parameters strictly inside each span and
    /// its right break, strictly increasing; no room inside a span of two neighbouring floats.
    #[test]
    fn the_samples_hold_every_span_and_need_room() {
        let breaks = [2.0, 2.5, 4.0];
        let u = samples(&breaks, 3).unwrap();
        // 1 + 2 spans of (8 inside + 1 break)
        assert_eq!(u.len(), 1 + 2 * 9);
        assert_eq!((u[0], u[9], u[18]), (2.0, 2.5, 4.0));
        assert!(u.windows(2).all(|w| w[0] < w[1]), "{u:?}");
        assert_eq!(
            knots_of(&breaks, 3).unwrap(),
            vec![2.0, 2.0, 2.0, 2.0, 2.5, 4.0, 4.0, 4.0, 4.0]
        );
        // the float after 1
        let next = f64::from_bits(1.0_f64.to_bits() + 1);
        assert_eq!(middle(1.0, next), None);
        assert_eq!(samples(&[1.0, next], 1), None);
        assert_eq!(middle(-f64::MAX, f64::MAX), Some(0.0));
        assert_eq!(samples(&[1.0], 3), None);
    }

    #[test]
    fn hostile_input_is_refused_or_answered_never_a_panic() {
        let source = through(arc, 0.0, 2.0, 30);
        let d = v(0.0, 1.0, 0.0);
        let invalid = |r: Result<WithinFit>| matches!(r, Err(SolidError::InvalidInput { .. }));
        for tolerance in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(invalid(refit_within(&source, 3, tolerance, None, None, 64)));
        }
        assert!(invalid(refit_within(&source, 0, 1e-3, None, None, 64)));
        assert!(invalid(refit_within(
            &source,
            MAX_DEGREE + 1,
            1e-3,
            None,
            None,
            64
        )));
        assert!(invalid(refit_within(
            &source,
            usize::MAX,
            1e-3,
            None,
            None,
            64
        )));
        for most in [0, 1, 3] {
            assert!(invalid(refit_within(&source, 3, 1e-3, None, None, most)));
        }
        assert!(invalid(refit_within(&source, 1, 1e-3, Some(d), None, 64)));
        assert!(invalid(refit_within(&source, 1, 1e-3, None, Some(d), 64)));
        for bad in [
            Vec3::ZERO,
            v(f64::NAN, 0.0, 0.0),
            v(0.0, f64::INFINITY, 0.0),
            v(0.0, 0.0, f64::NEG_INFINITY),
        ] {
            assert!(invalid(refit_within(&source, 3, 1e-3, Some(bad), None, 64)));
            assert!(invalid(refit_within(&source, 3, 1e-3, None, Some(bad), 64)));
        }
        // a direction against the source's own tangent at that end: the end legs of the
        // source's control polygon run along its end tangents, so their opposites run against
        let (s, e) = end_legs(&source);
        assert!(invalid(refit_within(
            &source,
            3,
            1e-3,
            Some(s * -1.0),
            None,
            64
        )));
        assert!(invalid(refit_within(
            &source,
            3,
            1e-3,
            None,
            Some(e * -1.0),
            64
        )));
        // a domain of no length, not finite, or reversed
        let points = [
            v(0.0, 0.0, 0.0),
            v(1.0, 1.0, 0.0),
            v(2.0, 0.0, 0.0),
            v(3.0, 1.0, 0.0),
        ];
        let clamped = |lo: f64, hi: f64| vec![lo, lo, lo, lo, hi, hi, hi, hi];
        for knots in [
            clamped(1.0, 1.0),
            clamped(0.0, f64::NAN),
            clamped(0.0, f64::INFINITY),
            clamped(f64::NEG_INFINITY, 0.0),
            clamped(1.0, 0.0),
        ] {
            let c = curve(&points, 3, knots);
            assert!(invalid(refit_within(&c, 3, 1e-3, None, None, 64)));
        }
        // rational, 2D, unclamped or not finite sources: the evaluation, the fit or the
        // certificate refuses
        let mut rational = source.clone();
        rational.non_rat_to_rat();
        assert!(refit_within(&rational, 3, 1e-3, None, None, 64).is_err());
        let mut flat = source.clone();
        flat.to_2d();
        assert!(refit_within(&flat, 3, 1e-3, None, None, 64).is_err());
        let uniform = vec![-3.0, -2.0, -1.0, 0.0, 1.0, 2.0, 3.0, 4.0];
        let unclamped = curve(&points, 3, uniform);
        assert!(refit_within(&unclamped, 3, 1e-3, None, None, 64).is_err());
        let mut wild = points;
        wild[1].y = f64::NAN;
        let wild = curve(&wild, 3, clamped(0.0, 1.0));
        assert!(refit_within(&wild, 3, 1e-3, None, None, 64).is_err());
        // numbers at the ends of the range are answered: any bound is within the largest
        // tolerance, so one fit; the smallest is out of reach, so the last fit `most` holds
        let loose = refit_within(&source, 3, f64::MAX, None, None, usize::MAX).unwrap();
        assert_eq!(loose.fits, 1);
        let tight = refit_within(&source, 3, f64::MIN_POSITIVE, None, None, 64).unwrap();
        assert!(tight.curve.cpts().len() <= 64);
        assert!(tight.bound > f64::MIN_POSITIVE);
    }
}
