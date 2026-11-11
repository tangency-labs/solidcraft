//! Knots and degree of a free curve: a knot inserted or removed, the degree raised or lowered.
//! The curve is the record's ([`Curve`], a clamped spline or rational spline); every step is the
//! kernel's ([`NurbCurve::insert_knot`], [`NurbCurve::remove_knot`] with its error bound,
//! [`NurbCurve::elevate_degree`]: The NURBS Book's algorithms A5.1, A5.8 and A5.9). Ops checks the
//! inputs and converts.

use solidcore::nurbs::crv_basic::Curve as NurbCurve;
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

/// `curve` with one copy of its interior knot `t` removed, when the kernel's bound on how far
/// that moves the curve (The NURBS Book, eq. 5.30, through the basis functions' maxima) is
/// within `tol`.
///
/// # Errors
/// [`Error::Argument`] for a curve that is not a valid clamped spline or rational spline, a `t`
/// that is not one of its interior knots, or a `tol` not finite or negative;
/// [`Error::NoEffect`] when the bound is past `tol` (the curve stays as it was); the kernel's
/// status otherwise.
pub fn remove_knot(curve: &Curve, t: f64, tol: f64) -> Result<Curve> {
    let nurb = nurb_of(curve)?;
    if !interior(curve, t) || multiplicity(nurb.knots(), t) == 0 || !tol.is_finite() || tol < 0.0 {
        return Err(Error::Argument);
    }
    run("curve_knots::remove_knot", |_| {
        let removal = nurb.remove_knot(t, 1, tol)?;
        Ok(if removal.removed == 0 {
            Err(Error::NoEffect)
        } else {
            record_of(&removal.curve)
        })
    })?
}

/// `curve` of degree `degree`. Raising it is exact (the same curve, more control points); the
/// same degree is the curve as it is.
///
/// # Errors
/// [`Error::Argument`] for a curve that is not a valid clamped spline or rational spline, or a
/// degree of 0 or past the record's most ([`MAX_SPLINE_DEGREE`]); [`Error::Unsupported`] for a
/// lower degree (the kernel has no certified bound yet of how far a reduction moves the curve);
/// the kernel's status otherwise.
pub fn set_degree(curve: &Curve, degree: usize) -> Result<Curve> {
    let nurb = nurb_of(curve)?;
    if degree == 0 || degree > MAX_SPLINE_DEGREE {
        return Err(Error::Argument);
    }
    let Some(raise) = degree.checked_sub(nurb.degree()) else {
        return Err(Error::Unsupported(
            "lowering a curve's degree needs a certified bound of how far it moves",
        ));
    };
    if raise == 0 {
        return Ok(curve.clone());
    }
    run("curve_knots::set_degree", |_| {
        Ok(record_of(&nurb.elevate_degree(raise)?))
    })?
}

/// The kernel's curve of a record spline or rational spline that is valid and clamped (its
/// first and last `degree + 1` knots equal), as the kernel's knot routines take it.
///
/// # Errors
/// [`Error::Argument`] for another curve, one the record refuses, or knots not clamped.
pub(crate) fn nurb_of(curve: &Curve) -> Result<NurbCurve> {
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
    let flat =
        |ends: Option<&[f64]>| ends.is_some_and(|e| e.windows(2).all(|w| w.first() == w.get(1)));
    let last = knots.len().checked_sub(degree + 1);
    if !flat(knots.get(..=degree)) || !flat(last.and_then(|s| knots.get(s..))) {
        return Err(Error::Argument);
    }
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
        assert_eq!(set_degree(&line, 3), Err(Error::Argument));
    }

    #[test]
    fn a_knot_inserted_then_removed_gives_the_curve_back() {
        let c = cv_curve(&WAVE, 3).unwrap();
        let more = insert_knot(&c, 0.3).unwrap();
        let back = remove_knot(&more, 0.3, 1e-9).unwrap();
        let (knots, count) = parts(&back);
        assert_eq!(knots, parts(&c).0);
        assert_eq!(count, WAVE.len());
        same_shape(&c, &back, 1e-9);
    }

    #[test]
    fn a_knot_the_shape_needs_is_not_removed() {
        let c = cv_curve(&WAVE, 3).unwrap();
        // The interior knots of six points' cubic are 1/3 and 2/3.
        let first = parts(&c).0[4];
        assert!(first > 0.0 && first < 1.0, "{first}");
        assert_eq!(remove_knot(&c, first, 1e-6), Err(Error::NoEffect));
        // Not a knot, an end knot, or a bad tolerance.
        assert_eq!(remove_knot(&c, 0.25, 1.0), Err(Error::Argument));
        assert_eq!(remove_knot(&c, 0.0, 1.0), Err(Error::Argument));
        assert_eq!(remove_knot(&c, first, -1.0), Err(Error::Argument));
        assert_eq!(remove_knot(&c, first, f64::NAN), Err(Error::Argument));
    }

    #[test]
    fn a_raised_degree_keeps_the_shape() {
        let c = cv_curve(&WAVE, 3).unwrap();
        let quintic = set_degree(&c, 5).unwrap();
        let (knots, count) = parts(&quintic);
        assert_eq!(knots.len() - count - 1, 5);
        assert_eq!(knots.first(), Some(&0.0));
        assert_eq!(knots.last(), Some(&1.0));
        same_shape(&c, &quintic, 1e-12);
        assert_eq!(set_degree(&c, 3), Ok(c.clone()));
        assert_eq!(set_degree(&c, 0), Err(Error::Argument));
        assert_eq!(set_degree(&c, MAX_SPLINE_DEGREE + 1), Err(Error::Argument));
        assert!(matches!(
            set_degree(&quintic, 3),
            Err(Error::Unsupported(_))
        ));
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
        let cubic = set_degree(&arc, 3).unwrap();
        assert!(matches!(cubic, Curve::RationalSpline { .. }));
        assert_eq!(parts(&cubic).1, 4);
        same_shape(&arc, &cubic, 1e-12);
        for i in 0..=20 {
            let p = at(&cubic, f64::from(i) / 20.0);
            let r2 = p[0] * p[0] + p[1] * p[1];
            assert!((r2 - 100.0).abs() < 1e-9, "r² = {r2}");
        }
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
}
