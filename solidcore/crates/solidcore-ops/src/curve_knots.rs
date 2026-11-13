//! Knots and degree of a free curve: a knot inserted or removed, the degree raised or lowered,
//! the curve split in two at a parameter. The curve is the record's ([`Curve`], a clamped
//! spline or rational spline); every step is the kernel's ([`NurbCurve::insert_knot`],
//! [`NurbCurve::remove_knot`], [`NurbCurve::elevate_degree`], [`NurbCurve::reduce_degree_once`],
//! [`NurbCurve::split`]: The NURBS Book's algorithms A5.1, A5.8, A5.9 and A5.11, and knot
//! insertion to full multiplicity), and a change that is not exact is kept only within a
//! tolerance of the kernel's certified bound ([`certified_distance`]). Ops checks the inputs
//! and converts.
//! A closed periodic curve (unclamped knots, as `curve_fit::closed_cv_curve` makes) is refused
//! for now: the kernel's knot routines take clamped knots.

use solidcore::error::nurbs::NurbsFailure;
use solidcore::nurbs::crv_basic::Curve as NurbCurve;
use solidcore::nurbs::crv_deviation::certified_distance;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore_record::brep::{check_curve, Budget, Curve, Natural, MAX_SPLINE_DEGREE};

use crate::session::run;
use crate::{Error, Result};

/// `curve` with the knot `t` inserted once: the same curve, one control point more.
///
/// # Errors
/// [`Error::Argument`] for a curve that is not a valid clamped spline or rational spline, a `t`
/// not strictly inside its domain, or a knot already as many times as the degree (one more would
/// break the curve); the kernel's status otherwise.
pub fn insert_knot(curve: &Curve, t: f64) -> Result<Curve> {
    let nurb = nurb_of(curve)?;
    if !interior(curve, t) || multiplicity(nurb.knots(), t) >= nurb.degree() {
        return Err(Error::Argument);
    }
    run("curve_knots::insert_knot", |_| {
        Ok(record_of(&nurb.insert_knot(t, 1)?))
    })?
}

/// `curve` with one copy of its interior knot `t` removed (The NURBS Book, A5.8; a knot of
/// multiplicity `m` keeps `m - 1`), and the kernel's certified bound of how far the result is
/// from `curve` ([`solidcore::nurbs::crv_deviation::certified_distance`], measured against
/// the curve as it was, never the removal's own estimate), kept when within `tol`.
///
/// # Errors
/// [`Error::Argument`] for a curve that is not a valid clamped spline (a rational spline, whose
/// bound the kernel does not certify yet, and a closed periodic one are refused), a `t` that is
/// not one of its interior knots, or a `tol` not finite or negative;
/// [`Error::PastTolerance`] with the bound when it is past `tol`; the kernel's status otherwise.
pub fn remove_knot(curve: &Curve, t: f64, tol: f64) -> Result<(Curve, f64)> {
    let nurb = nurb_of(curve)?;
    if !matches!(curve, Curve::Spline { .. })
        || !interior(curve, t)
        || multiplicity(nurb.knots(), t) == 0
        || !tol.is_finite()
        || tol < 0.0
    {
        return Err(Error::Argument);
    }
    run("curve_knots::remove_knot", |_| {
        // removed whatever it moves; the certified bound decides
        let removal = nurb.remove_knot(t, 1, f64::MAX)?;
        if removal.removed == 0 {
            return Ok(Err(Error::NoEffect));
        }
        within(&nurb, &removal.curve, tol)
    })?
}

/// `curve` of degree `degree`, and the kernel's certified bound of how far it is from `curve`
/// ([`solidcore::nurbs::crv_deviation::certified_distance`]). Raising it is exact in exact
/// arithmetic (the same curve, more control points; `tol` is not used), its bound the
/// rounding; the same degree is the curve as it is, bound 0; lowering it (The NURBS Book,
/// A5.11, once per degree) is kept when the bound is within `tol`. A rational spline is raised
/// with no bound (`None`): the kernel's bound does not take rational curves yet.
///
/// # Errors
/// [`Error::Argument`] for a curve that is not a valid clamped spline or rational spline (a
/// closed periodic one is refused), a degree of 0 or past the record's most
/// ([`MAX_SPLINE_DEGREE`]), a `tol` not finite or negative, or a lower degree of a rational
/// spline (whose bound the kernel does not certify yet); [`Error::PastTolerance`] with the
/// bound when it is past `tol`; [`Error::NoEffect`] when the kernel does not lower it; the
/// kernel's status otherwise.
pub fn set_degree(curve: &Curve, degree: usize, tol: f64) -> Result<(Curve, Option<f64>)> {
    let nurb = nurb_of(curve)?;
    if degree == 0 || degree > MAX_SPLINE_DEGREE || !tol.is_finite() || tol < 0.0 {
        return Err(Error::Argument);
    }
    if let Some(raise) = degree.checked_sub(nurb.degree()) {
        if raise == 0 {
            return Ok((curve.clone(), Some(0.0)));
        }
        let rational = !matches!(curve, Curve::Spline { .. });
        return run("curve_knots::set_degree", |_| {
            let raised = nurb.elevate_degree(raise)?;
            if rational {
                return Ok(record_of(&raised).map(|c| (c, None)));
            }
            let bound = certified_distance(&nurb, &raised)?;
            Ok(record_of(&raised).map(|c| (c, Some(bound))))
        })?;
    }
    if !matches!(curve, Curve::Spline { .. }) {
        return Err(Error::Argument);
    }
    run("curve_knots::set_degree", |_| {
        let mut lowered = nurb.clone();
        while lowered.degree() > degree {
            // lowered whatever it moves; the certified bound decides
            match lowered.reduce_degree_once(f64::MAX)?.curve {
                Some(c) => lowered = c,
                None => return Ok(Err(Error::NoEffect)),
            }
        }
        Ok(within(&nurb, &lowered, tol)?.map(|(c, bound)| (c, Some(bound))))
    })?
}

/// `curve` cut at `t`, strictly inside its domain: the piece before (over `[start, t]`) and
/// the piece after (over `[t, end]`), each a clamped curve of `curve`'s degree, rational where
/// `curve` is, together the same shape ([`NurbCurve::split`]: `t` inserted to full
/// multiplicity and the control points shared out). A `t` within `1e-6` of the domain's length
/// of a knot is moved onto that knot (the kernel's snap), so the pieces meet on it.
///
/// # Errors
/// [`Error::Argument`] for a curve that is not a valid clamped spline or rational spline (a
/// closed periodic one is refused: cut it open at its seam first), or a `t` not strictly inside
/// its domain or within the kernel's snap of an end; the kernel's status otherwise.
pub fn split_at(curve: &Curve, t: f64) -> Result<(Curve, Curve)> {
    let nurb = nurb_of(curve)?;
    if !interior(curve, t) {
        return Err(Error::Argument);
    }
    run("curve_knots::split_at", |_| {
        let (before, after) = match nurb.split(t) {
            Ok(pieces) => pieces,
            Err(e) if e.nurbs_failure() == Some(&NurbsFailure::Parameter) => {
                return Ok(Err(Error::Argument))
            }
            Err(e) => return Err(e),
        };
        Ok(record_of(&before).and_then(|b| record_of(&after).map(|a| (b, a))))
    })?
}

/// The record of `changed` and the kernel's certified bound of its distance from `original`,
/// when that is within `tol`, else [`Error::PastTolerance`] with the bound.
fn within(
    original: &NurbCurve,
    changed: &NurbCurve,
    tol: f64,
) -> solidcore::Result<Result<(Curve, f64)>> {
    let bound = certified_distance(original, changed)?;
    Ok(if bound <= tol {
        record_of(changed).map(|c| (c, bound))
    } else {
        Err(Error::PastTolerance { bound })
    })
}

/// The kernel's curve of a record spline or rational spline that is valid and clamped (its
/// first and last `degree + 1` knots equal), as the kernel's knot routines take it.
///
/// # Errors
/// [`Error::Argument`] for another curve, one the record refuses, or knots not clamped.
pub(crate) fn nurb_of(curve: &Curve) -> Result<NurbCurve> {
    let nurb = net_of(curve)?;
    let (knots, degree) = (nurb.knots(), nurb.degree());
    let flat =
        |ends: Option<&[f64]>| ends.is_some_and(|e| e.windows(2).all(|w| w.first() == w.get(1)));
    let last = knots.len().checked_sub(degree + 1);
    if !flat(knots.get(..=degree)) || !flat(last.and_then(|s| knots.get(s..))) {
        return Err(Error::Argument);
    }
    Ok(nurb)
}

/// The kernel's curve of a record spline or rational spline that is valid, on any knots the
/// record takes (clamped, or a closed curve's periodic ones): its control points and their
/// weights, its degree and knots as they are.
///
/// # Errors
/// [`Error::Argument`] for another curve, or one the record refuses.
pub(crate) fn net_of(curve: &Curve) -> Result<NurbCurve> {
    check_curve(curve, &mut Budget::input()).map_err(|_| Error::Argument)?;
    let (knots, pw): (&[f64], Vec<CPoint>) = match curve {
        Curve::Spline { knots, points } => (
            knots,
            points
                .iter()
                .map(|&[x, y, z]| CPoint::new(x, y, z, NOW))
                .collect(),
        ),
        Curve::RationalSpline { knots, homogeneous } => (
            knots,
            homogeneous
                .iter()
                .map(|&[x, y, z, w]| CPoint::new(x, y, z, w))
                .collect(),
        ),
        _ => return Err(Error::Argument),
    };
    let degree = knots
        .len()
        .checked_sub(pw.len() + 1)
        .ok_or(Error::Argument)?;
    NurbCurve::new(pw, degree, knots.to_vec()).map_err(|_| Error::Argument)
}

/// The record of the kernel's curve `nurb`: a spline where no control point carries a weight,
/// a rational spline where every one does.
///
/// # Errors
/// [`Error::Argument`] for a curve mixing the two, or one the record refuses (more control
/// points or a higher degree than a record holds).
pub(crate) fn record_of(nurb: &NurbCurve) -> Result<Curve> {
    let pw = nurb.cpts();
    let knots = nurb.knots().to_vec();
    let curve = if pw.iter().all(|p| p.w == NOW) {
        Curve::Spline {
            knots,
            points: pw.iter().map(|p| [p.x, p.y, p.z]).collect(),
        }
    } else if pw.iter().all(|p| p.w != NOW) {
        Curve::RationalSpline {
            knots,
            homogeneous: pw.iter().map(|p| [p.x, p.y, p.z, p.w]).collect(),
        }
    } else {
        return Err(Error::Argument);
    };
    check_curve(&curve, &mut Budget::input()).map_err(|_| Error::Argument)?;
    Ok(curve)
}

/// Whether `t` lies strictly inside the domain of the (valid) spline `curve`.
fn interior(curve: &Curve, t: f64) -> bool {
    matches!(curve.natural(), Natural::Range(a, b) if a < t && t < b)
}

/// How many times `t` appears among `knots`.
fn multiplicity(knots: &[f64], t: f64) -> usize {
    knots.iter().filter(|k| **k == t).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::cv_curve;
    use solidcore::nurbs::knots::Side;

    /// Six control points no cubic with fewer knots passes through.
    const WAVE: [[f64; 3]; 6] = [
        [0.0, 0.0, 0.0],
        [1.0, 2.0, 0.0],
        [2.0, -1.0, 0.0],
        [3.0, 3.0, 0.0],
        [4.0, 0.0, 1.0],
        [5.0, 1.0, 0.0],
    ];

    /// The point of `c` at `t`, by the kernel's evaluator.
    fn at(c: &Curve, t: f64) -> [f64; 3] {
        let p = nurb_of(c).unwrap().eval(t, Side::Left).unwrap();
        [p.x, p.y, p.z]
    }

    /// `a` and `b` are the same curve: the same point at 101 parameters across `[0, 1]`.
    fn same_shape(a: &Curve, b: &Curve, tol: f64) {
        for i in 0..=100 {
            let t = f64::from(i) / 100.0;
            let (p, q) = (at(a, t), at(b, t));
            assert!(
                p.iter().zip(q).all(|(x, y)| (x - y).abs() <= tol),
                "at {t}: {p:?} and {q:?}"
            );
        }
    }

    /// A spline's knots and control point count.
    fn parts(c: &Curve) -> (&[f64], usize) {
        match c {
            Curve::Spline { knots, points } => (knots, points.len()),
            Curve::RationalSpline { knots, homogeneous } => (knots, homogeneous.len()),
            _ => panic!("a spline: {c:?}"),
        }
    }

    #[test]
    fn an_inserted_knot_adds_a_point_and_keeps_the_shape() {
        let c = cv_curve(&WAVE, 3).unwrap();
        let more = insert_knot(&c, 0.3).unwrap();
        let (knots, count) = parts(&more);
        assert_eq!(count, WAVE.len() + 1);
        assert_eq!(multiplicity(knots, 0.3), 1);
        assert_eq!(knots.len(), parts(&c).0.len() + 1);
        same_shape(&c, &more, 1e-12);

        // Up to the degree at one value; one more is refused.
        let mut c3 = c.clone();
        for _ in 0..3 {
            c3 = insert_knot(&c3, 0.5).unwrap();
        }
        assert_eq!(multiplicity(parts(&c3).0, 0.5), 3);
        same_shape(&c, &c3, 1e-12);
        assert_eq!(insert_knot(&c3, 0.5), Err(Error::Argument));
    }

    #[test]
    fn a_knot_outside_the_domain_or_on_a_curve_without_knots_is_refused() {
        let c = cv_curve(&WAVE, 3).unwrap();
        for t in [0.0, 1.0, -0.5, 1.5, f64::NAN, f64::INFINITY] {
            assert_eq!(insert_knot(&c, t), Err(Error::Argument), "{t}");
        }
        let line = Curve::Line {
            start: [0.0; 3],
            end: [1.0; 3],
        };
        assert_eq!(insert_knot(&line, 0.5), Err(Error::Argument));
        assert_eq!(remove_knot(&line, 0.5, 1.0), Err(Error::Argument));
        assert_eq!(set_degree(&line, 3, 0.0), Err(Error::Argument));
    }

    #[test]
    fn a_knot_inserted_then_removed_gives_the_curve_back() {
        let c = cv_curve(&WAVE, 3).unwrap();
        let more = insert_knot(&c, 0.3).unwrap();
        let (back, bound) = remove_knot(&more, 0.3, 1e-9).unwrap();
        assert!(bound < 1e-12, "{bound}");
        let (knots, count) = parts(&back);
        assert_eq!(knots, parts(&c).0);
        assert_eq!(count, WAVE.len());
        same_shape(&c, &back, 1e-9);
    }

    #[test]
    fn an_inserted_knot_moves_the_curve_by_no_more_than_rounding() {
        let c = cv_curve(&WAVE, 3).unwrap();
        let more = insert_knot(&c, 0.3).unwrap();
        let bound = certified_distance(&nurb_of(&c).unwrap(), &nurb_of(&more).unwrap()).unwrap();
        assert!(bound < 1e-12, "{bound}");
        for i in 0..=200 {
            let t = f64::from(i) / 200.0;
            let (p, q) = (at(&c, t), at(&more, t));
            let gap2: f64 = p.iter().zip(q).map(|(a, b)| (a - b) * (a - b)).sum();
            assert!(gap2 <= bound * bound, "at {t}: {gap2} past {bound}²");
        }
    }

    #[test]
    fn a_repeated_knot_is_removed_one_copy_at_a_time() {
        let c = cv_curve(&WAVE, 3).unwrap();
        let twice = insert_knot(&insert_knot(&c, 0.5).unwrap(), 0.5).unwrap();
        assert_eq!(multiplicity(parts(&twice).0, 0.5), 2);
        let (once, _) = remove_knot(&twice, 0.5, 1e-9).unwrap();
        assert_eq!(multiplicity(parts(&once).0, 0.5), 1);
        assert_eq!(parts(&once).1, WAVE.len() + 1);
        let (none, bound) = remove_knot(&once, 0.5, 1e-9).unwrap();
        assert_eq!(multiplicity(parts(&none).0, 0.5), 0);
        assert_eq!(parts(&none).0, parts(&c).0);
        // measured against the curve it started from, within rounding of it
        assert!(bound < 1e-9, "{bound}");
        same_shape(&c, &none, 1e-9);
    }

    #[test]
    fn a_knot_the_shape_needs_is_not_removed() {
        let c = cv_curve(&WAVE, 3).unwrap();
        // The interior knots of six points' cubic are 1/3 and 2/3.
        let first = parts(&c).0[4];
        assert!(first > 0.0 && first < 1.0, "{first}");
        assert!(
            matches!(remove_knot(&c, first, 1e-6), Err(Error::PastTolerance { bound }) if bound > 1e-6)
        );
        // Not a knot, an end knot, or a bad tolerance.
        assert_eq!(remove_knot(&c, 0.25, 1.0), Err(Error::Argument));
        assert_eq!(remove_knot(&c, 0.0, 1.0), Err(Error::Argument));
        assert_eq!(remove_knot(&c, first, -1.0), Err(Error::Argument));
        assert_eq!(remove_knot(&c, first, f64::NAN), Err(Error::Argument));
    }

    #[test]
    fn a_raised_degree_keeps_the_shape() {
        let c = cv_curve(&WAVE, 3).unwrap();
        let (quintic, bound) = set_degree(&c, 5, 0.0).unwrap();
        assert!(bound.is_some_and(|b| b < 1e-12), "{bound:?}");
        let (knots, count) = parts(&quintic);
        assert_eq!(knots.len() - count - 1, 5);
        assert_eq!(knots.first(), Some(&0.0));
        assert_eq!(knots.last(), Some(&1.0));
        same_shape(&c, &quintic, 1e-12);
        assert_eq!(set_degree(&c, 3, 0.0), Ok((c.clone(), Some(0.0))));
        assert_eq!(set_degree(&c, 0, 0.0), Err(Error::Argument));
        assert_eq!(
            set_degree(&c, MAX_SPLINE_DEGREE + 1, 0.0),
            Err(Error::Argument)
        );
        assert_eq!(set_degree(&c, 5, f64::NAN), Err(Error::Argument));
    }

    #[test]
    fn a_degree_is_lowered_only_within_the_certified_tolerance() {
        let c = cv_curve(&WAVE, 3).unwrap();
        // a raised curve comes back down to its own degree, within rounding
        let (quintic, bound) = set_degree(&c, 5, 0.0).unwrap();
        assert!(bound.is_some_and(|b| b < 1e-12), "{bound:?}");
        let (back, bound) = set_degree(&quintic, 3, 1e-9).unwrap();
        assert!(bound.is_some_and(|b| b <= 1e-9), "{bound:?}");
        let (knots, count) = parts(&back);
        assert_eq!(knots.len() - count - 1, 3);
        same_shape(&c, &back, 1e-9);
        // the wave is no quadratic
        assert!(
            matches!(set_degree(&c, 2, 1e-6), Err(Error::PastTolerance { bound }) if bound > 1e-6)
        );
    }

    #[test]
    fn a_raised_arc_stays_on_its_circle() {
        // A quarter circle of radius 10 about the origin, as a rational quadratic.
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let arc = Curve::RationalSpline {
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            homogeneous: vec![
                [10.0, 0.0, 0.0, 1.0],
                [10.0 * w, 10.0 * w, 0.0, w],
                [0.0, 10.0, 0.0, 1.0],
            ],
        };
        let (cubic, bound) = set_degree(&arc, 3, 0.0).unwrap();
        assert_eq!(bound, None);
        assert!(matches!(cubic, Curve::RationalSpline { .. }));
        assert_eq!(parts(&cubic).1, 4);
        same_shape(&arc, &cubic, 1e-12);
        for i in 0..=20 {
            let p = at(&cubic, f64::from(i) / 20.0);
            let r2 = p[0] * p[0] + p[1] * p[1];
            assert!((r2 - 100.0).abs() < 1e-9, "r² = {r2}");
        }
        // a rational curve's removal and lowering wait for a certified rational bound
        assert_eq!(set_degree(&cubic, 2, 1.0), Err(Error::Argument));
        let more = insert_knot(&arc, 0.5).unwrap();
        assert_eq!(remove_knot(&more, 0.5, 1.0), Err(Error::Argument));
    }

    #[test]
    fn unclamped_knots_are_refused() {
        let floating = Curve::Spline {
            knots: (0..10).map(f64::from).collect(),
            points: WAVE.to_vec(),
        };
        assert_eq!(nurb_of(&floating).err(), Some(Error::Argument));
        assert_eq!(insert_knot(&floating, 4.5), Err(Error::Argument));
    }

    /// The degree and domain of a clamped spline.
    fn degree_and_domain(c: &Curve) -> (usize, [f64; 2]) {
        let (knots, count) = parts(c);
        (knots.len() - count - 1, [knots[0], knots[knots.len() - 1]])
    }

    /// `piece` is `whole` over the piece's domain: the same point at 101 parameters.
    fn same_over(piece: &Curve, whole: &Curve, tol: f64) {
        let (_, [a, b]) = degree_and_domain(piece);
        for i in 0..=100 {
            let t = a + (b - a) * f64::from(i) / 100.0;
            let (p, q) = (at(piece, t), at(whole, t));
            assert!(
                p.iter().zip(q).all(|(x, y)| (x - y).abs() <= tol),
                "at {t}: {p:?} and {q:?}"
            );
        }
    }

    #[test]
    fn the_two_pieces_of_a_split_are_the_curve_on_each_side() {
        let c = cv_curve(&WAVE, 3).unwrap();
        let (before, after) = split_at(&c, 0.4).unwrap();
        assert_eq!(degree_and_domain(&before), (3, [0.0, 0.4]));
        assert_eq!(degree_and_domain(&after), (3, [0.4, 1.0]));
        same_over(&before, &c, 1e-12);
        same_over(&after, &c, 1e-12);
        // clamped: the pieces start and end on their end points, and meet
        let (Curve::Spline { points: p0, .. }, Curve::Spline { points: p1, .. }) =
            (&before, &after)
        else {
            panic!("splines: {before:?} {after:?}")
        };
        assert_eq!(p0[0], WAVE[0]);
        assert_eq!(p1[p1.len() - 1], WAVE[5]);
        let (m0, m1) = (p0[p0.len() - 1], p1[0]);
        assert!(m0.iter().zip(m1).all(|(a, b)| (a - b).abs() < 1e-12));
        assert!(m0
            .iter()
            .zip(at(&c, 0.4))
            .all(|(a, b)| (a - b).abs() < 1e-12));

        // at an interior knot (1/3 for six points' cubic)
        let knot = parts(&c).0[4];
        let (before, after) = split_at(&c, knot).unwrap();
        assert_eq!(degree_and_domain(&before).1, [0.0, knot]);
        same_over(&before, &c, 1e-12);
        same_over(&after, &c, 1e-12);
    }

    #[test]
    fn a_split_arc_stays_rational_on_its_circle() {
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let arc = Curve::RationalSpline {
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            homogeneous: vec![
                [10.0, 0.0, 0.0, 1.0],
                [10.0 * w, 10.0 * w, 0.0, w],
                [0.0, 10.0, 0.0, 1.0],
            ],
        };
        let (before, after) = split_at(&arc, 0.5).unwrap();
        for piece in [&before, &after] {
            assert!(matches!(piece, Curve::RationalSpline { .. }), "{piece:?}");
            assert_eq!(degree_and_domain(piece).0, 2);
            same_over(piece, &arc, 1e-12);
            let (_, [a, b]) = degree_and_domain(piece);
            for i in 0..=20 {
                let p = at(piece, a + (b - a) * f64::from(i) / 20.0);
                let r2 = p[0] * p[0] + p[1] * p[1];
                assert!((r2 - 100.0).abs() < 1e-9, "r² = {r2}");
            }
        }
    }

    #[test]
    fn a_split_at_an_end_outside_or_of_a_closed_curve_is_refused() {
        let c = cv_curve(&WAVE, 3).unwrap();
        for t in [0.0, 1.0, -0.5, 1.5, f64::NAN, f64::INFINITY] {
            assert_eq!(split_at(&c, t), Err(Error::Argument), "{t}");
        }
        // within the kernel's snap of an end: moved onto it, so no piece is left
        assert_eq!(split_at(&c, 1e-8), Err(Error::Argument));
        assert_eq!(split_at(&c, 1.0 - 1e-8), Err(Error::Argument));
        let closed = crate::curve_fit::closed_cv_curve(&WAVE, 3).unwrap();
        assert_eq!(split_at(&closed, 0.5), Err(Error::Argument));
        let line = Curve::Line {
            start: [0.0; 3],
            end: [1.0; 3],
        };
        assert_eq!(split_at(&line, 0.5), Err(Error::Argument));
    }
}
